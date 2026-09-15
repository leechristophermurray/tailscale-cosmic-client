//! `cosmic-applet-tailscale` — the panel applet.
//!
//! This is the everyday surface: an icon whose shape says whether traffic is
//! flowing and how, and a flyout for the three things people actually do from
//! the panel — toggle the tunnel, switch exit node, and grab a peer's address.
//! Anything deeper opens the full client.

pub mod icons;
pub mod message;
pub mod monitoring;
pub mod popup;

use std::sync::Arc;
use std::time::Duration;

use cosmic::app::{Core, Task};
use cosmic::applet::token::subscription::{
    TokenRequest, TokenUpdate, activation_token_subscription,
};
use cosmic::cctk::sctk::reexports::calloop;
use cosmic::iced::window::Id;
use cosmic::iced::{Rectangle, Subscription, stream};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::{Element, iced};
use futures_util::{SinkExt, StreamExt};
use tailscale_localapi::{BackendState, LocalApi, MaskedPrefs, Prefs, Status};

use crate::fl;

use message::Message;

const ID: &str = "com.system76.CosmicAppletTailscale";

/// How long "suspend" keeps the tunnel down.
const SUSPEND_DURATION: Duration = Duration::from_secs(60 * 60);

/// Re-read the peer map this often; the IPN bus covers everything else.
const POLL_INTERVAL: Duration = Duration::from_secs(15);

/// How often to ask the monitoring hub. Slower than the tunnel poll: it is a
/// network round trip to another machine, and hardware metrics do not change
/// meaningfully in fifteen seconds.
const MONITORING_INTERVAL: Duration = Duration::from_secs(60);

/// The full client, which the flyout hands off to.
const MAIN_BINARY: &str = "cosmic-tailscale";

/// Spawn a binary, passing the activation token through so the compositor
/// focuses the window it opens.
fn launch(exec: &str, token: Option<String>) {
    let mut command = std::process::Command::new(exec);

    if let Some(token) = token {
        command.env("XDG_ACTIVATION_TOKEN", &token);
        command.env("DESKTOP_STARTUP_ID", token);
    }

    if let Err(error) = command.spawn() {
        tracing::warn!(%exec, %error, "could not launch the main window");
    }
}

pub struct Applet {
    core: Core,
    popup: Option<Id>,
    api: LocalApi,
    pub status: Option<Arc<Status>>,
    pub prefs: Option<Arc<Prefs>>,
    pub backend: BackendState,
    pub reachable: bool,
    pub suspended: bool,
    /// Channel for asking the compositor for an XDG activation token.
    token_sender: Option<calloop::channel::Sender<TokenRequest>>,
    pub monitoring: monitoring::MonitoringState,
}

/// What the panel icon should convey at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconState {
    /// Tunnel down, or the daemon is not running.
    Disconnected,
    /// Coming up, or waiting on a login.
    Pending,
    /// Connected, traffic going direct over the mesh.
    Connected,
    /// Connected, with an exit node carrying internet traffic.
    ExitNode,
}

impl IconState {
    /// The panel icon for this state.
    ///
    /// Each state has a distinct silhouette rather than a distinct colour: the
    /// panel renders symbolic icons in a single tint, so shape is the only
    /// channel that actually survives.
    pub fn icon(self) -> cosmic::widget::icon::Named {
        let (name, stock) = match self {
            // Hollow rings read as "off".
            Self::Disconnected => (
                "com.system76.CosmicAppletTailscale-disconnected-symbolic",
                "network-wired-disconnected-symbolic",
            ),
            Self::Pending => (
                "com.system76.CosmicAppletTailscale-disconnected-symbolic",
                "network-wired-acquiring-symbolic",
            ),
            // The full solid mark.
            Self::Connected => (
                "com.system76.CosmicAppletTailscale-connected-symbolic",
                "network-transmit-receive-symbolic",
            ),
            // A shield says traffic is leaving through somewhere else.
            Self::ExitNode => (
                "com.system76.CosmicAppletTailscale-exitnode-symbolic",
                "security-high-symbolic",
            ),
        };

        icons::branded(name, stock)
    }

    #[must_use]
    pub fn tooltip(self, tailnet: &str) -> String {
        match self {
            Self::Disconnected => fl!("tooltip-off"),
            Self::Pending => fl!("tooltip-connecting"),
            Self::Connected => fl!("tooltip-connected", tailnet = tailnet),
            Self::ExitNode => fl!("tooltip-exit-node", tailnet = tailnet),
        }
    }
}

impl Applet {
    /// Which icon the panel should show right now.
    #[must_use]
    pub fn icon_state(&self) -> IconState {
        if !self.reachable || !self.backend.is_running() {
            return match self.backend {
                BackendState::Starting => IconState::Pending,
                _ if self.backend.needs_attention() => IconState::Pending,
                _ => IconState::Disconnected,
            };
        }

        if self
            .prefs
            .as_deref()
            .is_some_and(Prefs::is_exit_node_active)
        {
            IconState::ExitNode
        } else {
            IconState::Connected
        }
    }

    #[must_use]
    pub fn tailnet_name(&self) -> &str {
        self.status
            .as_deref()
            .map_or("Tailscale", Status::tailnet_name)
    }

    #[must_use]
    pub fn want_running(&self) -> bool {
        self.prefs.as_deref().is_some_and(|p| p.want_running)
    }

    #[must_use]
    pub fn popup_id(&self) -> Id {
        self.popup.unwrap_or(Id::NONE)
    }

    fn reload(&self) -> Task<Message> {
        let status_api = self.api.clone();
        let prefs_api = self.api.clone();

        Task::batch([
            cosmic::task::future(async move {
                Message::StatusLoaded(status_api.status().await.ok().map(Arc::new))
            }),
            cosmic::task::future(async move {
                Message::PrefsLoaded(prefs_api.prefs().await.ok().map(Arc::new))
            }),
        ])
    }

    fn write_prefs(&self, prefs: MaskedPrefs) -> Task<Message> {
        let api = self.api.clone();
        cosmic::task::future(async move {
            Message::PrefsLoaded(api.set_prefs(prefs).await.ok().map(Arc::new))
        })
    }
}

impl cosmic::Application for Applet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        let applet = Self {
            core,
            popup: None,
            api: LocalApi::default(),
            status: None,
            prefs: None,
            backend: BackendState::Unknown,
            reachable: true,
            suspended: false,
            token_sender: None,
            monitoring: monitoring::MonitoringState::default(),
        };

        let task = Task::batch([applet.reload(), monitoring::poll()]);
        (applet, task)
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn style(&self) -> Option<iced::theme::Style> {
        Some(cosmic::applet::style())
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            // Push updates, so the panel icon changes the moment the tunnel does.
            Subscription::run(|| {
                stream::channel(8, async |mut output| {
                    let api = LocalApi::default();
                    loop {
                        if let Ok(notifications) = api.watch().await {
                            let mut notifications = Box::pin(notifications);
                            while let Some(Ok(notify)) = notifications.next().await {
                                if notify.is_meaningful()
                                    && output
                                        .send(Message::DaemonEvent(Arc::new(notify)))
                                        .await
                                        .is_err()
                                {
                                    return;
                                }
                            }
                        }
                        if output.send(Message::Refresh).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                })
            }),
            iced::time::every(POLL_INTERVAL).map(|_| Message::Refresh),
            // The hub changes more slowly than the tunnel and is a remote
            // call, so it gets its own, gentler cadence.
            iced::time::every(MONITORING_INTERVAL).map(|_| Message::MonitoringRefresh),
            // Launching an app from a panel applet needs an activation token,
            // or the compositor has no reason to focus the new window.
            activation_token_subscription(0).map(Message::TokenUpdate),
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
            }
            Message::Surface(action) => {
                return cosmic::task::message(cosmic::Action::Surface(action));
            }

            Message::Refresh => return self.reload(),

            Message::MonitoringRefresh => return monitoring::poll(),

            Message::StatusLoaded(status) => {
                self.reachable = status.is_some();
                if let Some(status) = status {
                    if self.backend == BackendState::Unknown {
                        self.backend = status.backend_state;
                    }
                    self.status = Some(status);
                }
            }
            Message::PrefsLoaded(prefs) => {
                if let Some(prefs) = prefs {
                    self.prefs = Some(prefs);
                }
            }

            Message::DaemonEvent(notify) => {
                self.reachable = true;
                if let Some(state) = notify.state {
                    self.backend = state;
                }
                if let Some(prefs) = &notify.prefs {
                    self.prefs = Some(Arc::new(prefs.clone()));
                }
                // The peer map is not on the bus, so a state change means the
                // exit-node list we are showing may already be stale.
                if notify.state.is_some() {
                    return self.reload();
                }
            }

            Message::SetConnected(value) => {
                if value {
                    self.suspended = false;
                }
                return self.write_prefs(MaskedPrefs::new().want_running(value));
            }

            Message::ExitNodeSelected(index) => {
                let prefs = if index == 0 {
                    MaskedPrefs::new().exit_node_id("")
                } else {
                    let options = self
                        .status
                        .as_deref()
                        .map(Status::exit_node_options)
                        .unwrap_or_default();

                    match options.get(index - 1) {
                        Some(peer) => MaskedPrefs::new().exit_node_id(peer.id.clone()),
                        None => return Task::none(),
                    }
                };
                return self.write_prefs(prefs);
            }

            Message::CopyToClipboard(value) => return iced::clipboard::write(value),

            Message::OpenAdminConsole => {
                let _ = open::that_detached("https://login.tailscale.com/admin/machines");
            }

            Message::OpenMainWindow => {
                // Ask the compositor for an activation token first; the launch
                // happens when it answers. Without one the new window opens
                // unfocused, behind whatever the user was looking at.
                if let Some(sender) = self.token_sender.as_ref() {
                    let _ = sender.send(TokenRequest {
                        app_id: Self::APP_ID.to_string(),
                        exec: MAIN_BINARY.to_string(),
                    });
                } else {
                    // No token service available; launching unfocused still
                    // beats doing nothing.
                    launch(MAIN_BINARY, None);
                }
            }

            Message::TokenUpdate(update) => match update {
                TokenUpdate::Init(sender) => self.token_sender = Some(sender),
                TokenUpdate::Finished => self.token_sender = None,
                TokenUpdate::ActivationToken { token, exec } => launch(&exec, token),
            },

            Message::SuspendForAnHour => {
                self.suspended = true;
                return Task::batch([
                    self.write_prefs(MaskedPrefs::new().want_running(false)),
                    cosmic::task::future(async {
                        tokio::time::sleep(SUSPEND_DURATION).await;
                        Message::SuspendElapsed
                    }),
                ]);
            }

            Message::MonitoringLoaded(systems) => {
                self.monitoring.configured = systems.is_some();
                if let Some(systems) = systems {
                    self.monitoring.systems = (*systems).clone();

                    // A pinned machine the hub no longer monitors would leave a
                    // stale number in the panel forever.
                    if let Some(pinned) = self.monitoring.pinned.clone()
                        && !self.monitoring.systems.iter().any(|s| s.id == pinned)
                    {
                        self.monitoring.pinned = None;
                    }
                }
            }

            Message::PinSystem(id) => self.monitoring.pinned = Some(id),
            Message::UnpinSystem => self.monitoring.pinned = None,

            Message::SuspendElapsed => {
                // Respect the user having turned it back on by hand.
                if self.suspended {
                    self.suspended = false;
                    return self.write_prefs(MaskedPrefs::new().want_running(true));
                }
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let state = self.icon_state();
        let open = self.popup;

        // A pinned metric rides next to the icon, which is the point of
        // pinning: seeing a server's load without opening anything.
        let pinned_label = self.monitoring.pinned_system().map(|system| {
            format!("{:.0}%", system.info.cpu.max(system.info.memory_pct))
        });

        let button = match pinned_label {
            // Panel sizing is the applet Context's job, so the composed
            // icon-and-figure goes through `button_from_element` rather than
            // being laid out by hand at a guessed size.
            Some(label) => {
                let content = if self.core.applet.is_horizontal() {
                    Element::from(
                        cosmic::widget::Row::new()
                            .push(cosmic::widget::icon(state.icon().into()).size(16))
                            .push(self.core.applet.text(label))
                            .spacing(4)
                            .align_y(cosmic::iced::Alignment::Center),
                    )
                } else {
                    // A vertical panel has no room for a figure beside the
                    // icon, so it stacks.
                    Element::from(
                        cosmic::widget::Column::new()
                            .push(cosmic::widget::icon(state.icon().into()).size(16))
                            .push(self.core.applet.text(label))
                            .spacing(2)
                            .align_x(cosmic::iced::Alignment::Center),
                    )
                };

                self.core.applet.button_from_element(content, true)
            }
            None => self.core.applet.icon_button_from_handle(state.icon().into()),
        }
            .on_press_with_rectangle(move |offset, bounds| {
                if let Some(id) = open {
                    return Message::Surface(destroy_popup(id));
                }

                Message::Surface(app_popup::<Applet>(
                    |_| Default::default(),
                    move |applet: &mut Applet| {
                        let id = Id::unique();
                        applet.popup = Some(id);

                        let mut settings = applet.core.applet.get_popup_settings(
                            applet.core.main_window_id().unwrap(),
                            id,
                            None,
                            None,
                            None,
                        );

                        settings.positioner.anchor_rect = Rectangle {
                            x: (bounds.x - offset.x) as i32,
                            y: (bounds.y - offset.y) as i32,
                            width: bounds.width as i32,
                            height: bounds.height as i32,
                        };

                        settings
                    },
                    Some(Box::new(|applet: &Applet| {
                        Element::from(applet.core.applet.popup_container(popup::view(applet)))
                            .map(cosmic::Action::App)
                    })),
                ))
            });

        self.core
            .applet
            .applet_tooltip::<Message>(
                button,
                state.tooltip(self.tailnet_name()),
                self.popup.is_some(),
                Message::Surface,
                None,
            )
            .into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        popup::view(self)
    }
}
