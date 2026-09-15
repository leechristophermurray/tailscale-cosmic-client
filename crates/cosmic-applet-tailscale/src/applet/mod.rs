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

const ID: &str = "io.github.leechristophermurray.CosmicAppletTailscale";

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
                "io.github.leechristophermurray.CosmicAppletTailscale-disconnected-symbolic",
                "network-wired-disconnected-symbolic",
            ),
            Self::Pending => (
                "io.github.leechristophermurray.CosmicAppletTailscale-disconnected-symbolic",
                "network-wired-acquiring-symbolic",
            ),
            // The full solid mark.
            Self::Connected => (
                "io.github.leechristophermurray.CosmicAppletTailscale-connected-symbolic",
                "network-transmit-receive-symbolic",
            ),
            // A shield says traffic is leaving through somewhere else.
            Self::ExitNode => (
                "io.github.leechristophermurray.CosmicAppletTailscale-exitnode-symbolic",
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
        // The last backend state goes stale the moment the daemon disappears,
        // so an unreachable daemon is disconnected — not "still starting".
        if !self.reachable {
            return IconState::Disconnected;
        }

        if !self.backend.is_running() {
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

            Message::MonitoringLoaded(snapshot) => {
                self.monitoring.configured = snapshot.is_some();
                if let Some(snapshot) = snapshot {
                    self.monitoring.systems.clone_from(&snapshot.systems);

                    // A pinned machine the hub no longer monitors would leave a
                    // stale number in the panel forever.
                    if let Some(pinned) = self.monitoring.pinned.clone()
                        && !self.monitoring.systems.iter().any(|s| s.id == pinned)
                    {
                        self.monitoring.pinned = None;
                    }

                    // Always observed, so muting and unmuting does not replay
                    // what happened while muted.
                    let events = self.monitoring.alert_events(&snapshot);
                    if snapshot.notify {
                        let notifications = events
                            .iter()
                            .map(|event| monitoring::alert_notification(event, &snapshot.systems))
                            .collect();
                        return monitoring::notify(notifications);
                    }
                }
            }

            Message::Noop => {}

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
        let pinned_label = self
            .monitoring
            .pinned_system()
            .map(|system| format!("{:.0}%", system.info.cpu.max(system.info.memory_pct)));

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
            None => self
                .core
                .applet
                .icon_button_from_handle(state.icon().into()),
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

#[cfg(test)]
mod tests {
    //! The applet's state and panel icon, driven directly.
    //!
    //! `OpenMainWindow` (without a token) and `OpenAdminConsole` act immediately
    //! rather than returning a task, so running them here would launch an app or
    //! a browser. They are deliberately not exercised.

    use super::*;
    use cosmic::Application as _;

    /// Fluent isolates each inserted value with bidi marks, so a right-to-left
    /// machine name cannot reorder the sentence around it. Compare without them.
    fn plain(text: &str) -> String {
        text.replace(['\u{2068}', '\u{2069}'], "")
    }

    /// Whether an update started any work, such as a notification.
    fn started(task: &cosmic::app::Task<Message>) -> bool {
        task.units() > 0
    }

    fn applet() -> Applet {
        Applet {
            core: Core::default(),
            popup: None,
            api: LocalApi::with_socket("/nonexistent/tailscaled.sock"),
            status: None,
            prefs: None,
            backend: BackendState::Unknown,
            reachable: true,
            suspended: false,
            token_sender: None,
            monitoring: monitoring::MonitoringState::default(),
        }
    }

    fn status() -> Arc<Status> {
        Arc::new(
            serde_json::from_str(include_str!(
                "../../../tailscale-localapi/tests/fixtures/status.json"
            ))
            .expect("fixture decodes"),
        )
    }

    fn prefs(json: &str) -> Arc<Prefs> {
        Arc::new(serde_json::from_str(json).expect("prefs decode"))
    }

    fn systems(json: &str) -> Arc<monitoring::Snapshot> {
        snapshot(json, None, true)
    }

    fn snapshot(systems: &str, alerts: Option<&str>, notify: bool) -> Arc<monitoring::Snapshot> {
        Arc::new(monitoring::Snapshot {
            systems: serde_json::from_str(systems).expect("systems decode"),
            alerts: alerts.map(|json| serde_json::from_str(json).expect("alerts decode")),
            hub: "you@https://mon.example.com".to_string(),
            notify,
        })
    }

    fn history(resolved: &str) -> String {
        format!(
            r#"[{{"id":"h1","system":"s1","alert_id":"a1","name":"Disk","value":80,
                 "created":"2026-09-15 08:12:03.200Z","resolved":"{resolved}",
                 "expand":{{"system":{{"name":"homeforge"}}}}}}]"#
        )
    }

    // ---- the panel icon --------------------------------------------------------

    #[test]
    fn the_icon_follows_the_connection() {
        let mut applet = applet();

        applet.backend = BackendState::Stopped;
        assert_eq!(applet.icon_state(), IconState::Disconnected);

        applet.backend = BackendState::Starting;
        assert_eq!(applet.icon_state(), IconState::Pending);

        applet.backend = BackendState::NeedsLogin;
        assert_eq!(
            applet.icon_state(),
            IconState::Pending,
            "a login is waiting on the user"
        );

        applet.backend = BackendState::Running;
        applet.prefs = Some(prefs(r#"{"WantRunning":true,"ExitNodeID":""}"#));
        assert_eq!(applet.icon_state(), IconState::Connected);

        applet.prefs = Some(prefs(r#"{"WantRunning":true,"ExitNodeID":"nEXIT"}"#));
        assert_eq!(applet.icon_state(), IconState::ExitNode);
    }

    /// The last known state goes stale the moment the daemon disappears. A
    /// daemon that died while starting must not leave "connecting" showing.
    #[test]
    fn an_unreachable_daemon_shows_disconnected_whatever_it_last_said() {
        let mut applet = applet();

        for last in [
            BackendState::Running,
            BackendState::Starting,
            BackendState::NeedsLogin,
        ] {
            applet.backend = last;
            applet.reachable = false;
            assert_eq!(
                applet.icon_state(),
                IconState::Disconnected,
                "unreachable after {last:?}"
            );
        }
    }

    /// Each state needs its own silhouette: the panel draws symbolic icons in a
    /// single tint, so shape is the only thing that tells them apart.
    #[test]
    fn connected_disconnected_and_exit_node_look_different() {
        let names: std::collections::HashSet<String> = [
            IconState::Disconnected,
            IconState::Connected,
            IconState::ExitNode,
        ]
        .into_iter()
        .map(|state| format!("{:?}", state.icon()))
        .collect();

        assert_eq!(names.len(), 3);
    }

    #[test]
    fn the_tooltip_names_the_tailnet_when_connected() {
        assert!(
            IconState::Connected
                .tooltip("example.ts.net")
                .contains("example.ts.net")
        );
        assert!(
            IconState::ExitNode
                .tooltip("example.ts.net")
                .contains("example.ts.net")
        );
        assert!(
            !IconState::Disconnected
                .tooltip("example.ts.net")
                .contains("example.ts.net")
        );
    }

    // ---- daemon state ------------------------------------------------------------

    #[test]
    fn a_failed_status_poll_marks_the_daemon_unreachable() {
        let mut applet = applet();
        let _ = applet.update(Message::StatusLoaded(Some(status())));
        assert!(applet.reachable);
        assert_eq!(applet.backend, BackendState::Running);

        let _ = applet.update(Message::StatusLoaded(None));
        assert!(!applet.reachable);
        assert_eq!(applet.icon_state(), IconState::Disconnected);
    }

    #[test]
    fn a_bus_state_change_is_applied_and_rereads_the_peer_map() {
        let mut applet = applet();
        let task = applet.update(Message::DaemonEvent(Arc::new(tailscale_localapi::Notify {
            state: Some(BackendState::Stopped),
            prefs: Some(serde_json::from_str(r#"{"WantRunning":false}"#).unwrap()),
            ..Default::default()
        })));

        assert_eq!(applet.backend, BackendState::Stopped);
        assert!(!applet.want_running());
        assert!(
            task.units() > 0,
            "the exit-node list may be stale after a state change"
        );
    }

    // ---- suspend ------------------------------------------------------------------

    #[test]
    fn a_suspend_resumes_unless_the_user_reconnected_first() {
        let mut applet = applet();
        let _ = applet.update(Message::SuspendForAnHour);
        assert!(applet.suspended);

        let task = applet.update(Message::SuspendElapsed);
        assert!(task.units() > 0, "the tunnel comes back");
        assert!(!applet.suspended);

        let mut applet = self::applet();
        let _ = applet.update(Message::SuspendForAnHour);
        let _ = applet.update(Message::SetConnected(true));
        assert!(
            !applet.suspended,
            "reconnecting by hand cancels the suspend"
        );
        let task = applet.update(Message::SuspendElapsed);
        assert_eq!(task.units(), 0, "so the timer firing does nothing");
    }

    #[test]
    fn an_invalid_exit_node_index_writes_nothing() {
        let mut applet = applet();
        let _ = applet.update(Message::StatusLoaded(Some(status())));
        assert_eq!(applet.update(Message::ExitNodeSelected(42)).units(), 0);
        assert!(
            applet.update(Message::ExitNodeSelected(0)).units() > 0,
            "direct mesh is always valid"
        );
    }

    // ---- monitoring ----------------------------------------------------------------

    const TWO: &str = r#"[
      {"id":"s1","name":"homeforge","host":"100.101.0.5","status":"up","info":{"cpu":18.0,"mp":15.0,"dp":64.0}},
      {"id":"s2","name":"nas","host":"nas","status":"up","info":{"cpu":2.0,"mp":20.0,"dp":95.0,"sv":[30,0]}}
    ]"#;

    #[test]
    fn no_hub_configured_leaves_monitoring_hidden() {
        let mut applet = applet();
        let _ = applet.update(Message::MonitoringLoaded(None));
        assert!(!applet.monitoring.configured);
        assert!(applet.monitoring.pinned_system().is_none());
    }

    #[test]
    fn pinning_shows_that_machine_and_unpinning_clears_it() {
        let mut applet = applet();
        let _ = applet.update(Message::MonitoringLoaded(Some(systems(TWO))));
        assert!(applet.monitoring.configured);

        let _ = applet.update(Message::PinSystem("s2".into()));
        assert_eq!(
            applet.monitoring.pinned_system().map(|s| s.name.as_str()),
            Some("nas")
        );

        let _ = applet.update(Message::UnpinSystem);
        assert!(applet.monitoring.pinned_system().is_none());
    }

    /// A pinned machine the hub stops monitoring would otherwise leave a frozen
    /// number in the panel indefinitely.
    #[test]
    fn a_pin_the_hub_no_longer_reports_is_dropped() {
        let mut applet = applet();
        let _ = applet.update(Message::MonitoringLoaded(Some(systems(TWO))));
        let _ = applet.update(Message::PinSystem("s2".into()));

        let only_first = r#"[{"id":"s1","name":"homeforge","status":"up","info":{}}]"#;
        let _ = applet.update(Message::MonitoringLoaded(Some(systems(only_first))));

        assert!(applet.monitoring.pinned.is_none());
    }

    #[test]
    fn unhealthy_machines_are_picked_out() {
        let mut applet = applet();
        let with_down = r#"[
          {"id":"a","name":"fine","status":"up","info":{"dp":40.0,"mp":30.0}},
          {"id":"b","name":"full","status":"up","info":{"dp":93.0}},
          {"id":"c","name":"gone","status":"down","info":{}},
          {"id":"d","name":"failing","status":"up","info":{"sv":[20,2]}}
        ]"#;
        let _ = applet.update(Message::MonitoringLoaded(Some(systems(with_down))));

        let mut names: Vec<&str> = applet
            .monitoring
            .unhealthy()
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["failing", "full", "gone"]);
    }

    #[test]
    fn a_peer_is_matched_to_its_monitored_machine() {
        let mut applet = applet();
        let _ = applet.update(Message::StatusLoaded(Some(status())));
        let _ = applet.update(Message::MonitoringLoaded(Some(systems(TWO))));

        let status = applet.status.clone().expect("status");
        let forge = status
            .peer
            .values()
            .find(|p| p.display_name() == "homeforge")
            .expect("fixture has homeforge");

        assert_eq!(
            applet.monitoring.system_for(forge).map(|s| s.id.as_str()),
            Some("s1")
        );
    }

    // ---- rendering -------------------------------------------------------------------

    /// The flyout has distinct branches for an unreachable daemon, no status
    /// yet, and a populated tailnet with monitoring; none may panic.
    #[test]
    fn the_flyout_renders_in_every_state() {
        let unreachable = Applet {
            reachable: false,
            ..applet()
        };
        let _ = popup::view(&unreachable);

        let empty = applet();
        let _ = popup::view(&empty);

        let mut populated = applet();
        let _ = populated.update(Message::StatusLoaded(Some(status())));
        let _ = populated.update(Message::PrefsLoaded(Some(prefs(r#"{"WantRunning":true}"#))));
        let _ = populated.update(Message::MonitoringLoaded(Some(systems(TWO))));
        let _ = populated.update(Message::PinSystem("s1".into()));
        let _ = popup::view(&populated);

        let suspended = Applet {
            suspended: true,
            ..applet()
        };
        let _ = popup::view(&suspended);
    }

    // ---- alert notifications ------------------------------------------------

    #[test]
    fn a_new_alert_notifies_once_and_its_resolution_notifies_once() {
        let mut applet = applet();
        // First read: whatever is already firing is not news.
        assert!(!started(&applet.update(Message::MonitoringLoaded(Some(
            snapshot(TWO, Some("[]"), true)
        )))));

        let fired = history("");
        assert!(started(&applet.update(Message::MonitoringLoaded(Some(
            snapshot(TWO, Some(&fired), true)
        )))));
        assert!(!started(&applet.update(Message::MonitoringLoaded(Some(
            snapshot(TWO, Some(&fired), true)
        )))));

        let cleared = history("2026-09-15 09:00:00.000Z");
        assert!(started(&applet.update(Message::MonitoringLoaded(Some(
            snapshot(TWO, Some(&cleared), true)
        )))));
    }

    #[test]
    fn muted_alerts_are_still_tracked_so_unmuting_does_not_replay_them() {
        let mut applet = applet();
        let _ = applet.update(Message::MonitoringLoaded(Some(snapshot(
            TWO,
            Some("[]"),
            true,
        ))));

        let fired = history("");
        assert!(!started(&applet.update(Message::MonitoringLoaded(Some(
            snapshot(TWO, Some(&fired), false)
        )))));
        assert!(!started(&applet.update(Message::MonitoringLoaded(Some(
            snapshot(TWO, Some(&fired), true)
        )))));
    }

    #[test]
    fn a_hub_without_alert_history_still_shows_its_machines() {
        let mut applet = applet();
        let task = applet.update(Message::MonitoringLoaded(Some(snapshot(TWO, None, true))));
        assert!(!started(&task));
        assert_eq!(applet.monitoring.systems.len(), 2);
    }

    fn event(
        fired: bool,
        name: &str,
        value: f64,
        system_name: Option<&str>,
    ) -> beszel_client::AlertEvent {
        let mut json = serde_json::json!({
            "id": "h1", "system": "s1", "name": name, "value": value, "resolved": if fired { "" } else { "2026-09-15 09:00:00.000Z" }
        });
        if let Some(name) = system_name {
            json["expand"] = serde_json::json!({"system": {"name": name}});
        }
        let record: beszel_client::AlertHistoryRecord = serde_json::from_value(json).unwrap();
        if fired {
            beszel_client::AlertEvent::Fired(record)
        } else {
            beszel_client::AlertEvent::Resolved(record)
        }
    }

    #[test]
    fn notifications_name_the_machine_the_metric_and_the_threshold() {
        let n = monitoring::alert_notification(&event(true, "Disk", 80.0, Some("homeforge")), &[]);
        assert_eq!(plain(&n.summary), "Alert on homeforge");
        assert_eq!(plain(&n.body), "Disk usage is above 80%");
        assert!(!n.urgent);

        let n = monitoring::alert_notification(
            &event(false, "Temperature", 72.5, Some("homeforge")),
            &[],
        );
        assert_eq!(plain(&n.summary), "Alert cleared on homeforge");
        assert_eq!(plain(&n.body), "Temperature is back below 72.5°C");

        let n =
            monitoring::alert_notification(&event(true, "LoadAvg5", 4.0, Some("homeforge")), &[]);
        assert_eq!(plain(&n.body), "5-minute load average is above 4");
    }

    #[test]
    fn a_machine_going_down_is_urgent_and_battery_alerts_read_the_right_way_round() {
        let down =
            monitoring::alert_notification(&event(true, "Status", 0.0, Some("homelab")), &[]);
        assert_eq!(
            plain(&down.body),
            "homelab has stopped reporting to the hub"
        );
        assert!(down.urgent);

        let up = monitoring::alert_notification(&event(false, "Status", 0.0, Some("homelab")), &[]);
        assert!(!up.urgent);

        let battery =
            monitoring::alert_notification(&event(true, "Battery", 20.0, Some("laptop")), &[]);
        assert_eq!(plain(&battery.body), "Battery is below 20%");
    }

    #[test]
    fn without_an_expanded_name_the_machine_is_found_in_the_systems_list() {
        let systems: Vec<beszel_client::SystemRecord> = serde_json::from_str(TWO).unwrap();
        let first = systems[0].clone();
        let mut json = serde_json::json!({"id":"h1","system": first.id, "name":"CPU","value":90,"resolved":""});
        json["expand"] = serde_json::json!({});
        let record: beszel_client::AlertHistoryRecord = serde_json::from_value(json).unwrap();

        let n = monitoring::alert_notification(&beszel_client::AlertEvent::Fired(record), &systems);
        assert_eq!(plain(&n.summary), format!("Alert on {}", first.name));
    }
}
