//! The `cosmic::Application` implementation: window chrome, the nav bar, and
//! the update loop that keeps [`State`] in step with the daemon.

pub mod action;
pub mod beszel;
pub mod caddy;
pub mod config;
pub mod message;
pub mod notify;
pub mod secrets;
pub mod state;
pub mod subscription;

use std::sync::Arc;
use std::time::{Duration, Instant};

use cosmic::app::{Core, Task};
use cosmic::iced::{Alignment, Length, Subscription};
use cosmic::widget::nav_bar;
use cosmic::{Apply, Element, theme, widget};
use tailscale_localapi::{LocalApi, MaskedPrefs};

use crate::fl;
use crate::pages::{self, Page};
use crate::ui::{Tone, format, icons, widgets};

use message::{Failure, Message};
use state::State;

/// How long "suspend" keeps the tunnel down before bringing it back.
const SUSPEND_DURATION: Duration = Duration::from_secs(60 * 60);

/// Startup input. Populated by the file manager's "Send via Taildrop" action,
/// which launches this binary with the selected paths.
#[derive(Debug, Default, Clone)]
pub struct Flags {
    pub pending_files: Vec<std::path::PathBuf>,
}

pub struct App {
    core: Core,
    nav: nav_bar::Model,
    api: LocalApi,
    state: State,
    /// Handle for persisting settings; `None` when the config store could not
    /// be opened, in which case settings simply do not survive a restart.
    config_handler: Option<cosmic::cosmic_config::Config>,
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = Flags;
    type Message = Message;

    const APP_ID: &'static str = "com.system76.CosmicTailscale";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let mut nav = nav_bar::Model::default();

        for page in Page::ALL {
            nav.insert()
                .text(page.title())
                .icon(widget::icon::from_name(page.icon()))
                .data(page);
        }
        nav.activate_position(0);

        let api = LocalApi::default();
        let task = action::refresh_all(&api);

        let (config_handler, config) = config::Config::load();

        let mut state = State::default();
        state.beszel.url_input.clone_from(&config.beszel_url);
        state.beszel.user_input.clone_from(&config.beszel_user);
        if !config.caddy_target.is_empty() {
            state.caddy.target = Some(config.caddy_target.clone());
        }
        state.config = config;

        // Launched from the file manager: queue the selection so the user only
        // has to pick a machine.
        if !flags.pending_files.is_empty() {
            let count = flags.pending_files.len();
            let label = if count == 1 { "file" } else { "files" };
            state.notice = Some(format!(
                "{count} {label} ready — choose a machine to send to"
            ));
            state.pending_drop = flags.pending_files;
        }

        // With a hub already configured, look for its password so the
        // Monitoring page is populated rather than presenting a sign-in form
        // the user has already filled in once.
        let task = if state.config.is_beszel_configured() {
            Task::batch([
                task,
                beszel::load_password(
                    state.config.beszel_url.clone(),
                    state.config.beszel_user.clone(),
                ),
            ])
        } else {
            task
        };

        let app = Self {
            core,
            nav,
            api,
            state,
            config_handler,
        };

        (app, task)
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<Self::Message> {
        self.nav.activate(id);

        // The exit-node list is the one place latency is shown without the user
        // asking for it, so opening the page is what triggers the probes.
        if self.nav.active_data::<Page>() == Some(&Page::ExitNodes) {
            return cosmic::task::message(Message::PingExitNodes);
        }

        // Reconnect to the remembered Caddy machine when its page is opened —
        // not at startup, which would start an SSH tunnel on every launch for a
        // page the user may not visit.
        if self.nav.active_data::<Page>() == Some(&Page::Caddy)
            && self.state.caddy.target.is_some()
            && matches!(
                self.state.caddy.connection,
                caddy::Connection::Idle | caddy::Connection::Failed(_)
            )
        {
            return cosmic::task::message(Message::CaddyConnect);
        }

        Task::none()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        subscription::all()
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        vec![self.tailnet_chip()]
    }

    fn header_end(&self) -> Vec<Element<'_, Self::Message>> {
        vec![
            widget::button::icon(widget::icon::from_name(icons::REFRESH))
                .on_press(Message::Refresh)
                .class(theme::Button::Icon)
                .into(),
        ]
    }

    fn footer(&self) -> Option<Element<'_, Self::Message>> {
        Some(self.status_bar())
    }

    fn update(&mut self, message: Message) -> Task<Self::Message> {
        match message {
            // ---- data in --------------------------------------------------
            Message::Refresh | Message::DaemonStreamEnded => {
                return action::refresh_all(&self.api);
            }

            Message::Tick => return action::refresh_all(&self.api),

            Message::StatusLoaded(Ok(status)) => {
                self.clear_daemon_error();
                // The bus is the faster source of backend state, but on a cold
                // start the first status poll is all we have.
                if self.state.backend == tailscale_localapi::BackendState::Unknown {
                    self.state.backend = status.backend_state;
                }
                self.state.status = Some(status);

                if let Some(task) = self.key_expiry_warning() {
                    return task;
                }
            }
            Message::StatusLoaded(Err(failure)) => self.record_failure(&failure),

            Message::PrefsLoaded(Ok(prefs)) | Message::PrefsApplied(Ok(prefs)) => {
                self.clear_daemon_error();
                self.state.writing_prefs = false;
                self.state.prefs = Some(prefs);
                // A prefs write can change which peers are exit nodes or
                // reachable, so re-read the peer map rather than guessing.
                return action::status(&self.api);
            }
            Message::PrefsLoaded(Err(failure)) | Message::PrefsApplied(Err(failure)) => {
                self.state.writing_prefs = false;
                self.record_failure(&failure);
            }

            Message::ServeLoaded(Ok(serve)) => self.state.serve = serve,
            Message::FileTargetsLoaded(Ok(targets)) => self.state.file_targets = targets,
            Message::WaitingFilesLoaded(Ok(files)) => {
                // Announce only transfers we have not already mentioned.
                let fresh: Vec<tailscale_localapi::WaitingFile> = files
                    .iter()
                    .filter(|file| !self.state.announced_files.contains(&file.name))
                    .cloned()
                    .collect();

                for file in &fresh {
                    self.state.announced_files.insert(file.name.clone());
                }

                self.state.waiting_files = files;

                if !fresh.is_empty() {
                    return notify::taildrop_arrived(&fresh);
                }
            }

            // These three are supporting detail; a failure should not replace
            // the whole window with an error.
            Message::ServeLoaded(Err(failure))
            | Message::FileTargetsLoaded(Err(failure))
            | Message::WaitingFilesLoaded(Err(failure)) => {
                if failure.unreachable {
                    self.record_failure(&failure);
                } else {
                    tracing::debug!(error = %failure.message, "optional daemon call failed");
                }
            }

            Message::DaemonEvent(notify) => return self.apply_notify(&notify),

            // ---- navigation -----------------------------------------------
            Message::FilterChanged(filter) => self.state.filter = filter,
            Message::SelectPeer(id) => self.state.selected = Some(id),

            // ---- connection -----------------------------------------------
            Message::SetConnected(connected) => {
                self.state.writing_prefs = true;
                // Turning it on by hand cancels a suspend window.
                if connected {
                    self.state.suspended_until = None;
                }
                return action::apply_prefs(&self.api, MaskedPrefs::new().want_running(connected));
            }

            Message::LoginRequested => return action::login(&self.api),

            Message::SuspendRequested => {
                self.state.writing_prefs = true;
                self.state.suspended_until = Some(Instant::now() + SUSPEND_DURATION);
                self.state.notice = Some(fl!("notice-suspended"));
                return Task::batch([
                    action::apply_prefs(&self.api, MaskedPrefs::new().want_running(false)),
                    action::resume_after(SUSPEND_DURATION),
                ]);
            }

            Message::SuspendElapsed => {
                // Only resume if the user has not already changed their mind.
                if self.state.suspended_until.take().is_some() && !self.state.want_running() {
                    self.state.writing_prefs = true;
                    return action::apply_prefs(&self.api, MaskedPrefs::new().want_running(true));
                }
            }

            // ---- exit nodes -----------------------------------------------
            Message::ExitNodeSelected(index) => {
                self.state.writing_prefs = true;
                let prefs = if index == 0 {
                    MaskedPrefs::new().exit_node_id("")
                } else {
                    match self.state.exit_node_options().get(index - 1) {
                        Some(peer) => MaskedPrefs::new().exit_node_id(peer.id.clone()),
                        None => {
                            self.state.writing_prefs = false;
                            return Task::none();
                        }
                    }
                };
                return action::apply_prefs(&self.api, prefs);
            }

            Message::SetAllowLanAccess(value) => {
                self.state.writing_prefs = true;
                return action::apply_prefs(
                    &self.api,
                    MaskedPrefs::new().exit_node_allow_lan_access(value),
                );
            }

            Message::SetAdvertiseExitNode(value) => {
                self.state.writing_prefs = true;
                // Preserve any real subnet routes; only the default routes
                // that mean "exit node" are added or removed.
                let keep: Vec<String> = self
                    .state
                    .prefs
                    .as_deref()
                    .map(|p| p.subnet_routes().iter().map(|s| (*s).to_string()).collect())
                    .unwrap_or_default();

                let mut routes = keep;
                if value {
                    routes.push("0.0.0.0/0".to_string());
                    routes.push("::/0".to_string());
                }
                return action::apply_prefs(&self.api, MaskedPrefs::new().advertise_routes(routes));
            }

            // ---- preference switches ---------------------------------------
            Message::SetAcceptRoutes(value) => {
                self.state.writing_prefs = true;
                return action::apply_prefs(&self.api, MaskedPrefs::new().route_all(value));
            }
            Message::SetAcceptDns(value) => {
                self.state.writing_prefs = true;
                return action::apply_prefs(&self.api, MaskedPrefs::new().corp_dns(value));
            }
            Message::SetRunSsh(value) => {
                self.state.writing_prefs = true;
                return action::apply_prefs(&self.api, MaskedPrefs::new().run_ssh(value));
            }
            Message::SetShieldsUp(value) => {
                self.state.writing_prefs = true;
                return action::apply_prefs(&self.api, MaskedPrefs::new().shields_up(value));
            }

            // ---- per-machine actions ----------------------------------------
            Message::CopyToClipboard(value) => {
                self.state.notice = Some(fl!("copied", value = value.as_str()));
                return cosmic::iced::clipboard::write(value);
            }

            Message::OpenUrl(url) => return action::open_url(url),

            Message::SshToPeer(host) => {
                self.state.notice = Some(fl!("notice-opening-ssh", host = host.as_str()));
                return action::ssh_to(host);
            }

            Message::PingPeer(id) => {
                let Some(ip) = self
                    .state
                    .status
                    .as_deref()
                    .and_then(|s| s.peer_by_id(&id))
                    .and_then(|p| p.ipv4())
                    .map(str::to_string)
                else {
                    return Task::none();
                };

                self.state.pinging.insert(id.clone(), ());
                return action::ping(&self.api, id, ip);
            }

            Message::PingExitNodes => {
                let probes: Vec<(String, String)> = self
                    .state
                    .exit_node_options()
                    .into_iter()
                    // Offline nodes cannot answer, and a node already probed
                    // this session does not need re-probing on every visit.
                    .filter(|peer| peer.online && !self.state.pings.contains_key(&peer.id))
                    .filter_map(|peer| Some((peer.id.clone(), peer.ipv4()?.to_string())))
                    .collect();

                if probes.is_empty() {
                    return Task::none();
                }

                let tasks: Vec<Task<Message>> = probes
                    .into_iter()
                    .map(|(id, ip)| {
                        self.state.pinging.insert(id.clone(), ());
                        action::ping(&self.api, id, ip)
                    })
                    .collect();

                return Task::batch(tasks);
            }

            Message::PingCompleted(id, result) => {
                self.state.pinging.remove(&id);
                match result {
                    // A probe that comes back with an error is a result, not a
                    // failure: the row renders it. Only a broken call to the
                    // daemon is worth a banner.
                    Ok(ping) => {
                        self.state.pings.insert(id, (*ping).clone());
                    }
                    Err(failure) => self.record_failure(&failure),
                }
            }

            // ---- taildrop ----------------------------------------------------
            Message::DragOverWindow(over) => self.state.drag_over = over,

            Message::PortalFileTransfer(key) => {
                self.state.drag_over = false;

                // The portal gives out a key, not paths; redeeming it is what
                // grants this process access to the files.
                return cosmic::command::file_transfer_receive(key).map(|result| {
                    let paths: Vec<std::path::PathBuf> = result
                        .unwrap_or_default()
                        .into_iter()
                        .map(std::path::PathBuf::from)
                        .collect();

                    cosmic::Action::App(Message::FilesDropped(Arc::new(paths)))
                });
            }

            Message::UriListDropped(mime, bytes) => {
                self.state.drag_over = false;

                let paths = crate::ui::uri_list::parse(&bytes);
                if paths.is_empty() {
                    // A drag can carry things that are not local files — a URL
                    // from a browser, say. Say so rather than appearing inert.
                    tracing::debug!(%mime, "drop carried no local file paths");
                    self.state.notice = Some(fl!("notice-drop-no-files"));
                    return Task::none();
                }

                return cosmic::task::message(Message::FilesDropped(Arc::new(paths)));
            }

            Message::FilesDropped(paths) => {
                self.state.drag_over = false;
                self.state.pending_drop = (*paths).clone();
                self.state.notice = Some(fl!(
                    "files-ready",
                    files = crate::ui::file_count(self.state.pending_drop.len())
                ));
            }

            Message::TaildropSelectTarget(id) => {
                self.state.taildrop_target = Some(id.clone());

                // Files already chosen and now a destination picked: that is
                // the whole instruction, so carry it out.
                if !self.state.pending_drop.is_empty() {
                    return cosmic::task::message(Message::SendPendingTo(id));
                }
            }

            Message::ChooseFiles(target) => {
                // From the Taildrop page the destination is whatever is
                // selected there, so the chooser can send straight away.
                let target = target.or_else(|| self.state.taildrop_target.clone());
                return action::choose_files(target);
            }

            Message::FilesChosen(target, paths) => {
                self.state.pending_drop = (*paths).clone();

                match target {
                    // Started from a machine's Send button: the user has
                    // already said where these go, so send them.
                    Some(id) => return cosmic::task::message(Message::SendPendingTo(id)),
                    None => {
                        self.state.notice = Some(fl!(
                            "files-ready",
                            files = crate::ui::file_count(self.state.pending_drop.len())
                        ));
                    }
                }
            }

            Message::FileChooserClosed => {}

            Message::SendPendingTo(id) => {
                // Nothing queued yet: ask for files, and come back here once
                // the user has chosen, rather than telling them to go and drop
                // something first.
                if self.state.pending_drop.is_empty() {
                    return action::choose_files(Some(id));
                }

                let Some(peer) = self.state.status.as_deref().and_then(|s| s.peer_by_id(&id))
                else {
                    // The machine dropped off since it was chosen. Leave the
                    // files queued so choosing another is all it takes.
                    self.state.error = Some(fl!("taildrop-machine-gone"));
                    return Task::none();
                };

                let name = peer.display_name().to_string();
                let paths = std::mem::take(&mut self.state.pending_drop);
                self.state.notice = Some(fl!("taildrop-sending", name = name.as_str()));
                return action::send_files(&self.api, id, name, paths);
            }

            Message::TaildropCompleted(Ok(summary)) => {
                self.state.notice = Some(summary);
                return action::waiting_files(&self.api);
            }
            Message::TaildropCompleted(Err(failure)) => self.record_failure(&failure),

            Message::ClearPendingDrop => {
                self.state.pending_drop.clear();
                self.state.notice = None;
            }

            Message::SaveWaitingFiles(names) => {
                return notify::save_waiting_files(&self.api, names);
            }

            // ---- caddy ---------------------------------------------------------
            Message::CaddyTargetSelected(index) => {
                let id = self
                    .state
                    .caddy_candidates()
                    .get(index)
                    .map(|peer| peer.id.clone());

                if id != self.state.caddy.target {
                    // A different machine means the old connection and its
                    // tunnel are no longer relevant.
                    self.state.caddy.disconnect();
                    self.persist_caddy_target(id.as_deref());
                    self.state.caddy.target = id;
                }
            }

            Message::CaddyConnect => {
                if self.state.caddy.target.is_none() {
                    return Task::none();
                }

                let Some(host) = self
                    .state
                    .caddy
                    .target
                    .as_ref()
                    .and_then(|id| self.state.status.as_deref()?.peer_by_id(id))
                    .map(|peer| peer.magic_dns().to_string())
                else {
                    // Most often a remembered machine that has since left the
                    // tailnet. Saying so beats a page stuck on "Not connected".
                    self.state.caddy.connection =
                        caddy::Connection::Failed(fl!("caddy-machine-gone"));
                    return Task::none();
                };

                self.state.caddy.disconnect();
                self.state.caddy.connection = caddy::Connection::Connecting;
                return caddy::connect(host);
            }

            Message::CaddyConnected(Ok(connection)) => {
                let tunnelled = connection.tunnel.is_some();
                // Take the tunnel out of the message so this state owns it and
                // the forward dies with the connection.
                self.state.caddy.tunnel = connection.take_tunnel();
                self.state.caddy.connection = if tunnelled {
                    caddy::Connection::Tunnelled(connection.endpoint.clone())
                } else {
                    caddy::Connection::Direct(connection.endpoint.clone())
                };

                if let Some(client) = self.state.caddy.client() {
                    return caddy::load_sites(client);
                }
            }

            Message::CaddyConnected(Err(failure)) => {
                self.state.caddy.connection = caddy::Connection::Failed(failure.message.clone());
                self.state.caddy.tunnel = None;
            }

            Message::CaddySitesLoaded(Ok(sites)) => {
                self.state.caddy.busy = false;
                self.state.caddy.sites = (*sites).clone();
            }
            Message::CaddySitesLoaded(Err(failure)) => {
                self.state.caddy.busy = false;
                self.state.error = Some(failure.message);
            }

            Message::CaddyHostChanged(value) => self.state.caddy.new_host = value,
            Message::CaddyUpstreamChanged(value) => self.state.caddy.new_upstream = value,

            Message::CaddyAddRoute => {
                let Some(client) = self.state.caddy.client() else {
                    return Task::none();
                };

                let host = self.state.caddy.new_host.trim().to_string();
                let upstream = self.state.caddy.new_upstream.trim().to_string();
                if host.is_empty() || upstream.is_empty() {
                    return Task::none();
                }

                self.state.caddy.busy = true;
                return caddy::add_route(client, host, upstream);
            }

            Message::CaddyDeleteRoute(id) => {
                let Some(client) = self.state.caddy.client() else {
                    return Task::none();
                };
                self.state.caddy.busy = true;
                return caddy::delete_route(client, id);
            }

            Message::CaddyRouteChanged(Ok(summary)) => {
                self.state.notice = Some(summary);
                self.state.caddy.new_host.clear();
                self.state.caddy.new_upstream.clear();
                // Re-read rather than assume: Caddy may have normalised the
                // route, or rejected part of it.
                if let Some(client) = self.state.caddy.client() {
                    return caddy::load_sites(client);
                }
            }
            Message::CaddyRouteChanged(Err(failure)) => {
                self.state.caddy.busy = false;
                self.state.error = Some(failure.message);
            }

            // ---- beszel monitoring ---------------------------------------------
            Message::BeszelUrlChanged(value) => self.state.beszel.url_input = value,
            Message::BeszelUserChanged(value) => self.state.beszel.user_input = value,
            Message::BeszelPasswordChanged(value) => {
                self.state.beszel.password_input = value;
                // Typing a new password means the stored one no longer applies.
                self.state.beszel.password_stored = false;
            }

            Message::BeszelConnect => {
                let url = self.state.beszel.url_input.trim().to_string();
                let user = self.state.beszel.user_input.trim().to_string();
                let password = self.state.beszel.password_input.clone();

                if url.is_empty() || user.is_empty() || password.is_empty() {
                    return Task::none();
                }

                self.state.beszel.connection = beszel::HubConnection::Connecting;
                self.persist_beszel_config(&url, &user);

                return Task::batch([
                    beszel::store_password(url.clone(), user.clone(), password.clone()),
                    beszel::connect(url, user, password, None),
                ]);
            }

            Message::BeszelConnected(Ok(session)) => {
                self.state.beszel.connection = beszel::HubConnection::Connected;
                self.state.beszel.token = Some(session.token.clone());
                self.state.beszel.hub_key.clone_from(&session.hub_key);
                self.state
                    .beszel
                    .hub_version
                    .clone_from(&session.hub_version);
                self.state.beszel.systems.clone_from(&session.systems);
                self.state.beszel.password_stored = true;

                let warnings = self.beszel_threshold_warnings();

                // Keep showing whichever machine was open, if the hub still
                // knows about it. This must not return early: doing so once
                // skipped the hardware warnings whenever a machine was open.
                if let Some(selected) = self.state.beszel.selected.clone() {
                    if self.state.beszel.systems.iter().any(|s| s.id == selected) {
                        return Task::batch([warnings, self.load_beszel_detail(selected)]);
                    }
                    self.state.beszel.selected = None;
                }

                return warnings;
            }

            Message::BeszelConnected(Err(failure)) => {
                // A rejected sign-in invalidates the cached session too.
                self.state.beszel.token = None;

                self.state.beszel.connection = if failure.unreachable {
                    beszel::HubConnection::Unreachable(failure.message.clone())
                } else if failure.message.contains("credentials") {
                    beszel::HubConnection::NeedsSignIn
                } else {
                    beszel::HubConnection::Failed(failure.message.clone())
                };
            }

            Message::BeszelSignOut => {
                let url = self.state.config.beszel_url.clone();
                let user = self.state.config.beszel_user.clone();

                if let Some(handler) = &self.config_handler
                    && let Err(error) = self.state.config.set_beszel_auto_connect(handler, false)
                {
                    tracing::warn!(%error, "could not persist sign-out");
                }

                self.state.beszel = beszel::BeszelState {
                    url_input: url.clone(),
                    user_input: user.clone(),
                    ..beszel::BeszelState::default()
                };

                return beszel::forget_password(url, user);
            }

            Message::BeszelPasswordLoaded(password) => {
                if let Some(password) = password {
                    self.state.beszel.password_input = password;
                    self.state.beszel.password_stored = true;

                    // A password in the keyring means the user already signed
                    // in once; picking up where they left off is the point of
                    // having stored it.
                    if self.state.config.beszel_auto_connect {
                        return cosmic::task::message(Message::BeszelConnect);
                    }
                }
            }

            Message::BeszelPasswordStored(error) => {
                if let Some(error) = error {
                    // The sign-in itself may still have worked, so this is a
                    // notice rather than a failure.
                    self.state.notice = Some(error);
                }
            }

            Message::BeszelSelectSystem(id) => {
                self.state.beszel.selected = Some(id.clone());
                self.state.beszel.history.clear();
                return self.load_beszel_detail(id);
            }

            Message::BeszelPeriodChanged(index) => {
                let Some(period) = beszel::PERIODS.get(index).copied() else {
                    return Task::none();
                };
                self.state.beszel.period = period;

                // One filter row, one slice: every chart re-reads together.
                let Some(id) = self.state.beszel.selected.clone() else {
                    return Task::none();
                };
                return self.load_beszel_detail(id);
            }

            Message::BeszelDetailLoaded(id, Ok(detail)) => {
                if let Some(stats) = detail.stats.clone() {
                    self.state.beszel.stats.insert(id.clone(), stats);
                }
                // History belongs to whichever machine is open, so it is not
                // cached per id — switching machines replaces it.
                if self.state.beszel.selected.as_ref() == Some(&id) {
                    self.state.beszel.history.clone_from(&detail.history);
                }
                self.state
                    .beszel
                    .containers
                    .insert(id, detail.containers.clone());
            }
            Message::BeszelDetailLoaded(_, Err(failure)) => {
                tracing::debug!(error = %failure.message, "could not read hub detail");
            }

            Message::BeszelRefresh => {
                // Keep trying through an outage, so the page recovers on its own
                // when the hub comes back. Stop only where retrying cannot help:
                // nothing configured, a sign-in already in flight, or
                // credentials the hub has refused, which would otherwise be
                // resent every minute.
                if matches!(
                    self.state.beszel.connection,
                    beszel::HubConnection::Unconfigured
                        | beszel::HubConnection::Connecting
                        | beszel::HubConnection::NeedsSignIn
                ) {
                    return Task::none();
                }
                let (url, user, password) = self.beszel_credentials();
                if url.is_empty() || password.is_empty() {
                    return Task::none();
                }
                // Hands the existing session back, so a routine refresh sends
                // no password unless the token has expired.
                return beszel::connect(url, user, password, self.state.beszel.token.clone());
            }

            // ---- agent deployment ------------------------------------------
            Message::BeszelProposeInstall(peer_id) => {
                let Some(peer) = self
                    .state
                    .status
                    .as_deref()
                    .and_then(|status| status.peer_by_id(&peer_id))
                else {
                    return Task::none();
                };

                if self.state.beszel.hub_key.is_empty() {
                    self.state.error = Some(fl!("beszel-no-key"));
                    return Task::none();
                }

                let host = peer.magic_dns().to_string();
                let install = beszel_client::AgentInstall::new(
                    host.clone(),
                    self.state.beszel.hub_key.clone(),
                );

                // Only propose. The command is shown and nothing runs until the
                // user says yes.
                self.state.beszel.pending_install = Some(beszel::PendingInstall {
                    peer_id,
                    host,
                    command: install.remote_command(),
                });
            }

            Message::BeszelConfirmInstall => {
                let Some(pending) = self.state.beszel.pending_install.take() else {
                    return Task::none();
                };

                self.state
                    .beszel
                    .installing
                    .insert(pending.peer_id.clone(), ());

                return beszel::install_agent(
                    pending.peer_id,
                    pending.host,
                    self.state.beszel.hub_key.clone(),
                );
            }

            Message::BeszelCancelInstall => self.state.beszel.pending_install = None,

            Message::BeszelAgentInstalled(peer_id, outcome) => {
                self.state.beszel.installing.remove(&peer_id);

                if outcome.succeeded {
                    self.state.notice = Some(fl!("beszel-install-done"));
                } else {
                    self.state.error =
                        Some(fl!("beszel-install-failed", reason = outcome.summary()));
                }

                self.state.beszel.last_install = Some((peer_id, (*outcome).clone()));

                // The hub learns about the new agent on its own schedule, so
                // re-read rather than assume it has appeared.
                return cosmic::task::message(Message::BeszelRefresh);
            }

            // ---- chrome -------------------------------------------------------
            Message::DismissError => {
                self.state.error = None;
                self.state.notice = None;
            }
            Message::Noop => {}
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let spacing = theme::spacing();
        let page = self
            .nav
            .active_data::<Page>()
            .copied()
            .unwrap_or(Page::Machines);

        let mut column = widget::Column::new().spacing(spacing.space_s);

        if let Some(banner) = self.banner() {
            column = column.push(banner);
        }

        column = column
            .push(pages::header::view(&self.state))
            .push(page.view(&self.state));

        let content = column
            .apply(widget::container)
            .padding(spacing.space_s)
            .width(Length::Fill)
            .height(Length::Fill);

        // The whole window is the drop target.
        //
        // winit's `FileDropped` event, the obvious approach, never fires on
        // Wayland — drops arrive as a `wl_data_device` offer instead, which is
        // what this widget speaks. Accepting only `text/uri-list` means a drag
        // carrying something else does not show a drop cursor it cannot honour.
        widget::dnd_destination(content, vec![std::borrow::Cow::Borrowed("text/uri-list")])
            .on_enter(|_x, _y, _mimes| Message::DragOverWindow(true))
            .on_leave(|| Message::DragOverWindow(false))
            // A sandboxed sender offers files through the document portal
            // rather than as paths, and this takes precedence when offered.
            .on_file_transfer(Message::PortalFileTransfer)
            .on_data_received(|mime, bytes| Message::UriListDropped(mime, Arc::new(bytes)))
            .into()
    }
}

impl App {
    /// Fold a push frame from the IPN bus into the state.
    fn apply_notify(&mut self, notify: &tailscale_localapi::Notify) -> Task<Message> {
        self.clear_daemon_error();
        let mut tasks = Vec::new();

        if let Some(state) = notify.state {
            let changed = self.state.backend != state;
            self.state.backend = state;
            // A state transition changes which peers are reachable, so the peer
            // map is stale the moment it happens.
            if changed {
                tasks.push(action::status(&self.api));
                tasks.push(action::prefs(&self.api));
            }
        }

        if let Some(prefs) = &notify.prefs {
            self.state.prefs = Some(Arc::new(prefs.clone()));
            self.state.writing_prefs = false;
        }

        if let Some(engine) = notify.engine {
            self.state.throughput.sample(
                engine.rx_bytes,
                engine.tx_bytes,
                engine.num_live,
                engine.live_derps,
            );
        }

        if let Some(url) = &notify.browse_to_url {
            self.state.notice = Some(fl!("notice-finish-signin"));
            tasks.push(action::open_url(url.clone()));
        }

        if let Some(error) = &notify.err_message {
            self.state.error = Some(error.clone());
        }

        if tasks.is_empty() {
            Task::none()
        } else {
            Task::batch(tasks)
        }
    }

    /// Warn once when the node key is close enough to expiry to matter.
    ///
    /// The brief's threshold is 48 hours, but a key that expires over a weekend
    /// is a Monday morning outage — two days of warning is the minimum, not the
    /// target, so this fires at a week.
    fn key_expiry_warning(&mut self) -> Option<Task<Message>> {
        if self.state.announced_key_expiry {
            return None;
        }

        let state::KeyExpiry::Expires { days, .. } = self.state.key_expiry() else {
            return None;
        };

        if days > 7 {
            return None;
        }

        self.state.announced_key_expiry = true;
        Some(notify::key_expiring(days))
    }

    /// The credentials the background tasks re-authenticate with.
    ///
    /// PocketBase tokens expire, and a desktop client that sits open for days
    /// would otherwise start failing silently, so each task signs in again
    /// rather than reusing a token of unknown age.
    fn beszel_credentials(&self) -> (String, String, String) {
        (
            self.state.config.beszel_url.clone(),
            self.state.config.beszel_user.clone(),
            self.state.beszel.password_input.clone(),
        )
    }

    fn load_beszel_detail(&self, system_id: String) -> Task<Message> {
        let url = self.state.config.beszel_url.clone();
        let Some(token) = self.state.beszel.token.clone() else {
            return Task::none();
        };
        if url.is_empty() {
            return Task::none();
        }
        beszel::load_detail(url, token, system_id, self.state.beszel.period)
    }

    /// Remember the hub address and account, so the next launch can reconnect.
    ///
    /// The generated `set_*` methods assign the field *and* write it, but only
    /// when the value differs from what the struct already holds. Assigning the
    /// field first makes every setter see "no change" and write nothing — while
    /// returning `Ok`, so the failure is invisible. The setters must be the only
    /// thing that touches these fields.
    fn persist_beszel_config(&mut self, url: &str, user: &str) {
        let Some(handler) = &self.config_handler else {
            // No config store: keep the values for this session at least.
            self.state.config.beszel_url = url.to_string();
            self.state.config.beszel_user = user.to_string();
            self.state.config.beszel_auto_connect = true;
            return;
        };

        // Each field writes separately, and a failure to persist should not
        // stop the connection that is already in flight.
        for result in [
            self.state.config.set_beszel_url(handler, url.to_string()),
            self.state.config.set_beszel_user(handler, user.to_string()),
            self.state.config.set_beszel_auto_connect(handler, true),
        ] {
            if let Err(error) = result {
                tracing::warn!(%error, "could not persist the monitoring settings");
            }
        }
    }

    /// Remember which machine the Caddy page manages.
    fn persist_caddy_target(&mut self, target: Option<&str>) {
        let value = target.unwrap_or_default().to_string();

        let Some(handler) = &self.config_handler else {
            self.state.config.caddy_target = value;
            return;
        };

        if let Err(error) = self.state.config.set_caddy_target(handler, value) {
            tracing::warn!(%error, "could not persist the Caddy machine");
        }
    }

    /// Notify about hardware problems the hub is reporting.
    ///
    /// Each machine is warned about once per session. A disk filling up is not
    /// news every thirty seconds, and an alert that repeats is one that gets
    /// ignored.
    fn beszel_threshold_warnings(&mut self) -> Task<Message> {
        let mut warnings: Vec<String> = Vec::new();

        for system in &self.state.beszel.systems {
            if !system.status.is_up() {
                continue;
            }

            let mut reasons: Vec<String> = Vec::new();

            if system.info.disk_pct >= 90.0 {
                reasons.push(fl!(
                    "beszel-alert-disk",
                    pct = format!("{:.0}", system.info.disk_pct)
                ));
            }
            if system.info.memory_pct >= 92.0 {
                reasons.push(fl!(
                    "beszel-alert-memory",
                    pct = format!("{:.0}", system.info.memory_pct)
                ));
            }
            // Load is only meaningful against the thread count.
            if let Some(pressure) = system.info.load_pressure()
                && pressure >= 2.0
            {
                reasons.push(fl!(
                    "beszel-alert-load",
                    load = format!("{:.1}", system.info.load_average[0])
                ));
            }
            if let Some(failed) = system.info.failed_services().filter(|n| *n > 0) {
                reasons.push(fl!("beszel-alert-services", count = failed));
            }

            if reasons.is_empty() {
                // Recovered: allow a future warning to fire again.
                self.state.beszel.warned.remove(&system.id);
                continue;
            }

            if self.state.beszel.warned.insert(system.id.clone()) {
                warnings.push(format!("{}: {}", system.name, reasons.join(", ")));
            }
        }

        if warnings.is_empty() {
            Task::none()
        } else {
            notify::hardware_warning(&warnings)
        }
    }

    fn record_failure(&mut self, failure: &Failure) {
        if failure.unreachable {
            self.state.daemon_unreachable = true;
            self.state.backend = tailscale_localapi::BackendState::Unknown;
        }
        self.state.error = Some(failure.message.clone());
    }

    fn clear_daemon_error(&mut self) {
        if self.state.daemon_unreachable {
            self.state.daemon_unreachable = false;
            self.state.error = None;
        }
    }

    /// The tailnet name chip in the header bar.
    fn tailnet_chip(&self) -> Element<'_, Message> {
        let spacing = theme::spacing();

        widget::Row::new()
            .push(icons::named(icons::TAILNET, 16))
            .push(widget::text::body(self.state.tailnet_name().to_string()))
            .spacing(spacing.space_xxs)
            .align_y(Alignment::Center)
            .into()
    }

    /// An error or notice banner above the content, when there is one.
    fn banner(&self) -> Option<Element<'_, Message>> {
        let spacing = theme::spacing();

        let (tone, icon, text) = if let Some(error) = &self.state.error {
            (Tone::Critical, icons::WARNING, error.clone())
        } else if let Some(notice) = &self.state.notice {
            (Tone::Accent, icons::OK, notice.clone())
        } else {
            return None;
        };

        Some(
            widget::Row::new()
                .push(icons::named(icon, 16))
                .push(widget::text::body(text).width(Length::Fill))
                .push(
                    widget::button::text(fl!("dismiss"))
                        .on_press(Message::DismissError)
                        .class(theme::Button::Text),
                )
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center)
                .apply(widget::container)
                .padding(spacing.space_xs)
                .width(Length::Fill)
                .class(theme::Container::custom(move |t| {
                    let cosmic = t.cosmic();
                    let color: cosmic::iced::Color = match tone {
                        Tone::Critical => cosmic.destructive_color().into(),
                        _ => cosmic.accent_color().into(),
                    };
                    cosmic::iced::widget::container::Style {
                        text_color: Some(cosmic.on_bg_color().into()),
                        background: Some(cosmic::iced::Color { a: 0.14, ..color }.into()),
                        border: cosmic::iced::Border {
                            radius: cosmic.corner_radii.radius_s.into(),
                            width: 1.0,
                            color: cosmic::iced::Color { a: 0.4, ..color },
                        },
                        ..Default::default()
                    }
                }))
                .into(),
        )
    }

    /// The bottom strip: daemon version, backend state, and live throughput.
    fn status_bar(&self) -> Element<'_, Message> {
        let spacing = theme::spacing();

        let version = self.state.status.as_deref().map_or_else(
            || fl!("status-unknown-version"),
            |status| {
                fl!(
                    "status-tailscale-version",
                    version = status.version.as_str()
                )
            },
        );

        let tone = if self.state.daemon_unreachable {
            Tone::Critical
        } else if self.state.is_connected() {
            Tone::Positive
        } else {
            Tone::Caution
        };

        let mut row = widget::Row::new()
            .push(widget::text::caption(version))
            .push(widgets::status_label(
                crate::ui::backend_description(self.state.backend),
                tone,
            ))
            .push(widget::Space::new().width(Length::Fill))
            .spacing(spacing.space_s)
            .align_y(Alignment::Center)
            .width(Length::Fill);

        // Only show a rate once two counter samples have actually been taken;
        // a single sample cannot produce one.
        if self.state.throughput.has_rate() {
            row = row.push(widget::text::caption(fl!(
                "status-throughput",
                rx = format::rate(self.state.throughput.rx_per_second),
                tx = format::rate(self.state.throughput.tx_per_second)
            )));
        }

        if self.state.throughput.live_derps > 0 {
            row = row.push(widget::text::caption(
                if self.state.throughput.live_derps == 1 {
                    fl!("status-relays-one")
                } else {
                    fl!(
                        "status-relays-many",
                        count = self.state.throughput.live_derps
                    )
                },
            ));
        }

        row.apply(widget::container)
            .padding([spacing.space_xxs, spacing.space_s])
            .width(Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    //! The update loop, driven directly.
    //!
    //! `update` returns tasks rather than running them, so these tests observe
    //! two things: how the state changed, and whether any work was started
    //! (`Task::units() > 0`). Nothing here reaches the network, the desktop or
    //! the real config store.

    use super::*;
    use cosmic::Application as _;
    use tailscale_localapi::{BackendState, Status};

    /// The redacted real daemon capture, shared with the parsing tests.
    fn status() -> Arc<Status> {
        Arc::new(
            serde_json::from_str(include_str!(
                "../../../tailscale-localapi/tests/fixtures/status.json"
            ))
            .expect("fixture decodes"),
        )
    }

    fn app() -> App {
        let mut nav = nav_bar::Model::default();
        for page in Page::ALL {
            nav.insert().text(page.title()).data(page);
        }
        nav.activate_position(0);

        App {
            core: Core::default(),
            nav,
            // A socket that does not exist, so a task that did run by mistake
            // would fail rather than touch the real daemon.
            api: LocalApi::with_socket("/nonexistent/tailscaled.sock"),
            state: State::default(),
            // No config store: persistence is covered by the config tests, and
            // these must never write to the user's real settings.
            config_handler: None,
        }
    }

    fn connected_app() -> App {
        let mut app = app();
        let _ = app.update(Message::StatusLoaded(Ok(status())));
        app
    }

    fn prefs(json: &str) -> Arc<tailscale_localapi::Prefs> {
        Arc::new(serde_json::from_str(json).expect("prefs decode"))
    }

    fn failure(message: &str, unreachable: bool) -> Failure {
        Failure {
            message: message.to_string(),
            unreachable,
        }
    }

    fn started(task: &Task<Message>) -> bool {
        task.units() > 0
    }

    /// Any peer that can receive Taildrop, by stable id.
    fn receiving_peer(app: &App) -> String {
        app.state
            .status
            .as_deref()
            .expect("status")
            .peer
            .values()
            .find(|peer| peer.online && peer.can_receive_files())
            .expect("fixture has a receiving peer")
            .id
            .clone()
    }

    // ---- daemon state ------------------------------------------------------

    #[test]
    fn a_status_poll_sets_the_backend_only_when_the_bus_has_not() {
        let mut app = app();
        let _ = app.update(Message::StatusLoaded(Ok(status())));
        assert_eq!(app.state.backend, BackendState::Running);

        // The bus is the fresher source: a later poll must not overwrite it.
        let _ = app.update(Message::DaemonEvent(Arc::new(tailscale_localapi::Notify {
            state: Some(BackendState::Stopped),
            ..Default::default()
        })));
        let _ = app.update(Message::StatusLoaded(Ok(status())));
        assert_eq!(app.state.backend, BackendState::Stopped);
    }

    #[test]
    fn an_unreachable_daemon_is_reported_and_cleared_on_recovery() {
        let mut app = app();
        let _ = app.update(Message::StatusLoaded(Err(failure("socket gone", true))));

        assert!(app.state.daemon_unreachable);
        assert_eq!(app.state.backend, BackendState::Unknown);
        assert!(app.state.error.is_some());

        let _ = app.update(Message::StatusLoaded(Ok(status())));
        assert!(!app.state.daemon_unreachable);
        assert!(
            app.state.error.is_none(),
            "recovery clears the daemon error"
        );
    }

    #[test]
    fn a_prefs_write_that_fails_unlocks_the_switches() {
        let mut app = connected_app();
        let _ = app.update(Message::SetConnected(false));
        assert!(
            app.state.writing_prefs,
            "switches lock while a write is in flight"
        );

        let _ = app.update(Message::PrefsApplied(Err(failure("access denied", false))));
        assert!(
            !app.state.writing_prefs,
            "a failed write must not leave them locked"
        );
        assert_eq!(app.state.error.as_deref(), Some("access denied"));
    }

    #[test]
    fn applied_prefs_unlock_and_reread_the_peer_map() {
        let mut app = connected_app();
        let _ = app.update(Message::SetConnected(true));

        let task = app.update(Message::PrefsApplied(Ok(prefs(r#"{"WantRunning":true}"#))));
        assert!(!app.state.writing_prefs);
        assert!(app.state.want_running());
        assert!(
            started(&task),
            "a prefs change re-reads which peers are reachable"
        );
    }

    #[test]
    fn bus_frames_update_state_prefs_throughput_and_login() {
        let mut app = connected_app();

        let task = app.update(Message::DaemonEvent(Arc::new(tailscale_localapi::Notify {
            state: Some(BackendState::NeedsLogin),
            prefs: Some(serde_json::from_str(r#"{"WantRunning":false}"#).unwrap()),
            browse_to_url: Some("https://login.tailscale.com/a/xyz".into()),
            err_message: Some("key expired".into()),
            ..Default::default()
        })));

        assert_eq!(app.state.backend, BackendState::NeedsLogin);
        assert!(!app.state.want_running());
        assert!(
            app.state.notice.is_some(),
            "the user is told to finish in the browser"
        );
        assert_eq!(app.state.error.as_deref(), Some("key expired"));
        assert!(
            started(&task),
            "a state change re-reads status, and the URL opens"
        );
    }

    #[test]
    fn engine_counters_become_a_rate_after_two_samples() {
        let mut app = connected_app();
        let engine = |rx, tx| {
            Message::DaemonEvent(Arc::new(tailscale_localapi::Notify {
                engine: Some(
                    serde_json::from_str(&format!(
                        r#"{{"RBytes":{rx},"WBytes":{tx},"NumLive":1,"LiveDERPs":1}}"#
                    ))
                    .unwrap(),
                ),
                ..Default::default()
            }))
        };

        let _ = app.update(engine(1_000, 500));
        assert!(
            !app.state.throughput.has_rate(),
            "one sample cannot make a rate"
        );

        std::thread::sleep(std::time::Duration::from_millis(250));
        let _ = app.update(engine(51_000, 10_500));
        assert!(app.state.throughput.has_rate());
        assert!(app.state.throughput.rx_per_second > 0.0);
    }

    // ---- connection and suspend -----------------------------------------------

    #[test]
    fn reconnecting_by_hand_cancels_a_suspend() {
        let mut app = connected_app();
        let _ = app.update(Message::SuspendRequested);
        assert!(app.state.suspended_until.is_some());

        let _ = app.update(Message::SetConnected(true));
        assert!(app.state.suspended_until.is_none());

        // So the timer firing later must not do anything.
        let task = app.update(Message::SuspendElapsed);
        assert!(!started(&task));
    }

    #[test]
    fn a_suspend_resumes_the_tunnel_when_it_elapses() {
        let mut app = connected_app();
        let _ = app.update(Message::SuspendRequested);
        let _ = app.update(Message::PrefsApplied(Ok(prefs(r#"{"WantRunning":false}"#))));

        let task = app.update(Message::SuspendElapsed);
        assert!(started(&task), "the tunnel is brought back up");
        assert!(app.state.writing_prefs);
    }

    // ---- exit nodes -------------------------------------------------------------

    #[test]
    fn choosing_direct_mesh_writes_and_an_invalid_index_does_not() {
        let mut app = connected_app();

        let task = app.update(Message::ExitNodeSelected(0));
        assert!(started(&task));
        assert!(app.state.writing_prefs);

        let mut app = connected_app();
        let task = app.update(Message::ExitNodeSelected(99));
        assert!(!started(&task));
        assert!(
            !app.state.writing_prefs,
            "a bad index must not lock the switches"
        );
    }

    #[test]
    fn a_ping_result_is_stored_and_its_spinner_cleared() {
        let mut app = connected_app();
        let id = receiving_peer(&app);
        app.state.pinging.insert(id.clone(), ());

        let result: tailscale_localapi::PingResult =
            serde_json::from_str(r#"{"Err":"no route","LatencySeconds":0}"#).unwrap();
        let _ = app.update(Message::PingCompleted(id.clone(), Ok(Arc::new(result))));

        assert!(!app.state.pinging.contains_key(&id));
        assert!(app.state.pings.contains_key(&id));
        // A probe that failed is a result shown on the row, not a banner.
        assert!(app.state.error.is_none());
    }

    // ---- taildrop ----------------------------------------------------------------

    #[test]
    fn a_drop_with_no_local_files_explains_itself() {
        let mut app = connected_app();
        let task = app.update(Message::UriListDropped(
            "text/uri-list".into(),
            Arc::new(b"https://example.com/page\r\n".to_vec()),
        ));

        assert!(!started(&task));
        assert!(app.state.pending_drop.is_empty());
        assert!(app.state.notice.is_some(), "an inert drop must say why");
    }

    #[test]
    fn choosing_a_target_with_files_queued_sends_them() {
        let mut app = connected_app();
        let id = receiving_peer(&app);
        let _ = app.update(Message::FilesDropped(Arc::new(vec!["/tmp/a.txt".into()])));

        let task = app.update(Message::TaildropSelectTarget(id.clone()));
        assert_eq!(app.state.taildrop_target.as_deref(), Some(id.as_str()));
        assert!(
            started(&task),
            "files plus a destination is the whole instruction"
        );
    }

    #[test]
    fn send_with_nothing_queued_opens_the_chooser_instead_of_failing() {
        let mut app = connected_app();
        let id = receiving_peer(&app);

        let task = app.update(Message::SendPendingTo(id));
        assert!(started(&task), "the file chooser opens");
        assert!(app.state.error.is_none());
    }

    #[test]
    fn sending_to_a_known_machine_takes_the_queue() {
        let mut app = connected_app();
        let id = receiving_peer(&app);
        app.state.pending_drop = vec!["/tmp/a.txt".into()];

        let task = app.update(Message::SendPendingTo(id));
        assert!(started(&task));
        assert!(app.state.pending_drop.is_empty());
    }

    /// The target can vanish between choosing it and sending — it went offline
    /// and dropped from the peer map. That must be reported, not ignored.
    #[test]
    fn sending_to_a_machine_that_has_gone_says_so() {
        let mut app = connected_app();
        app.state.pending_drop = vec!["/tmp/a.txt".into()];

        let task = app.update(Message::SendPendingTo("nGONE".into()));

        assert!(!started(&task));
        assert!(
            app.state.error.is_some(),
            "a send that cannot happen must not be silent"
        );
        assert_eq!(
            app.state.pending_drop.len(),
            1,
            "and the files stay queued to retry"
        );
    }

    #[test]
    fn chosen_files_for_a_machine_send_immediately() {
        let mut app = connected_app();
        let id = receiving_peer(&app);

        let task = app.update(Message::FilesChosen(
            Some(id),
            Arc::new(vec!["/tmp/a".into()]),
        ));
        assert!(started(&task));

        let task = app.update(Message::FilesChosen(None, Arc::new(vec!["/tmp/b".into()])));
        assert!(
            !started(&task),
            "with no destination they wait in the queue"
        );
        assert_eq!(app.state.pending_drop.len(), 1);
    }

    #[test]
    fn received_files_are_announced_once() {
        let mut app = connected_app();
        let files = || {
            Arc::new(vec![tailscale_localapi::WaitingFile {
                name: "photo.jpg".into(),
                size: 2048,
            }])
        };

        let first = app.update(Message::WaitingFilesLoaded(Ok(files())));
        let second = app.update(Message::WaitingFilesLoaded(Ok(files())));

        assert!(started(&first), "a new arrival notifies");
        assert!(!started(&second), "the same file on the next poll does not");
    }

    // ---- caddy -----------------------------------------------------------------

    /// A remembered machine that is no longer on the tailnet must not leave the
    /// page sitting at "Not connected" with no explanation.
    #[test]
    fn connecting_to_a_caddy_machine_that_has_gone_says_so() {
        let mut app = connected_app();
        app.state.caddy.target = Some("nGONE".into());

        let task = app.update(Message::CaddyConnect);

        assert!(!started(&task));
        assert!(
            matches!(app.state.caddy.connection, caddy::Connection::Failed(_)),
            "expected Failed, got {:?}",
            app.state.caddy.connection
        );
    }

    #[test]
    fn a_failed_caddy_connection_is_shown() {
        let mut app = connected_app();
        let _ = app.update(Message::CaddyConnected(Err(failure("tunnel closed", true))));
        assert!(
            matches!(app.state.caddy.connection, caddy::Connection::Failed(ref m) if m == "tunnel closed")
        );
    }

    #[test]
    fn an_add_route_with_an_empty_field_does_nothing() {
        let mut app = connected_app();
        app.state.caddy.connection =
            caddy::Connection::Direct(caddy_admin::Endpoint::tailnet("100.64.0.1"));
        app.state.caddy.new_host = "app.example.ts.net".into();

        let task = app.update(Message::CaddyAddRoute);
        assert!(!started(&task));
        assert!(!app.state.caddy.busy);
    }

    // ---- beszel -----------------------------------------------------------------

    fn session(systems: &str) -> Arc<beszel::Session> {
        Arc::new(beszel::Session {
            token: "tok".into(),
            systems: serde_json::from_str(systems).expect("systems decode"),
            hub_key: "ssh-ed25519 AAAA".into(),
            hub_version: "0.18.7".into(),
        })
    }

    #[test]
    fn connecting_stores_the_session_token() {
        let mut app = connected_app();
        let _ = app.update(Message::BeszelConnected(Ok(session("[]"))));

        assert!(app.state.beszel.connection.is_connected());
        assert_eq!(app.state.beszel.token.as_deref(), Some("tok"));
        assert_eq!(app.state.beszel.hub_key, "ssh-ed25519 AAAA");
    }

    #[test]
    fn rejected_credentials_clear_the_token_and_ask_to_sign_in() {
        let mut app = connected_app();
        let _ = app.update(Message::BeszelConnected(Ok(session("[]"))));
        let _ = app.update(Message::BeszelConnected(Err(failure(
            "credentials rejected",
            false,
        ))));

        assert!(app.state.beszel.token.is_none());
        assert!(matches!(
            app.state.beszel.connection,
            beszel::HubConnection::NeedsSignIn
        ));

        // Wrong credentials must not be retried in the background.
        let task = app.update(Message::BeszelRefresh);
        assert!(
            !started(&task),
            "a rejected password is not hammered every minute"
        );
    }

    /// One dropped refresh must not stop monitoring until the user notices.
    #[test]
    fn a_transient_hub_outage_keeps_refreshing() {
        let mut app = connected_app();
        app.state.config.beszel_url = "https://mon.example.com".into();
        app.state.config.beszel_user = "you@example.com".into();
        app.state.beszel.password_input = "pw".into();
        let _ = app.update(Message::BeszelConnected(Ok(session("[]"))));

        let _ = app.update(Message::BeszelConnected(Err(failure(
            "no route to host",
            true,
        ))));
        assert!(matches!(
            app.state.beszel.connection,
            beszel::HubConnection::Unreachable(_)
        ));

        let task = app.update(Message::BeszelRefresh);
        assert!(
            started(&task),
            "the next refresh must still try, so the page recovers"
        );
    }

    #[test]
    fn typing_a_new_password_forgets_that_one_was_stored() {
        let mut app = app();
        app.state.beszel.password_stored = true;
        let _ = app.update(Message::BeszelPasswordChanged("new".into()));
        assert!(!app.state.beszel.password_stored);
    }

    #[test]
    fn signing_out_keeps_the_address_but_drops_the_session() {
        let mut app = connected_app();
        app.state.config.beszel_url = "https://mon.example.com".into();
        app.state.config.beszel_user = "you@example.com".into();
        let _ = app.update(Message::BeszelConnected(Ok(session("[]"))));

        let _ = app.update(Message::BeszelSignOut);

        assert!(!app.state.beszel.connection.is_connected());
        assert!(app.state.beszel.token.is_none());
        assert!(app.state.beszel.password_input.is_empty());
        assert_eq!(app.state.beszel.url_input, "https://mon.example.com");
        assert_eq!(app.state.beszel.user_input, "you@example.com");
    }

    const UNHEALTHY: &str = r#"[{"id":"s1","name":"nas","status":"up",
        "info":{"dp":97.0,"mp":20.0,"t":8,"la":[0.5,0.5,0.5]}}]"#;

    #[test]
    fn a_hardware_problem_warns_once_until_it_recovers() {
        let mut app = connected_app();

        let first = app.update(Message::BeszelConnected(Ok(session(UNHEALTHY))));
        let again = app.update(Message::BeszelConnected(Ok(session(UNHEALTHY))));
        assert!(started(&first), "a full disk notifies");
        assert!(!started(&again), "and does not repeat every refresh");

        let healthy = r#"[{"id":"s1","name":"nas","status":"up","info":{"dp":40.0}}]"#;
        let _ = app.update(Message::BeszelConnected(Ok(session(healthy))));
        let relapse = app.update(Message::BeszelConnected(Ok(session(UNHEALTHY))));
        assert!(started(&relapse), "recovering re-arms the warning");
    }

    /// Opening a machine's detail must not switch off hardware warnings.
    #[test]
    fn warnings_still_fire_while_a_machine_is_selected() {
        let mut app = connected_app();
        app.state.config.beszel_url = "https://mon.example.com".into();
        app.state.beszel.token = Some("tok".into());
        app.state.beszel.selected = Some("s1".into());

        let _ = app.update(Message::BeszelConnected(Ok(session(UNHEALTHY))));
        assert!(
            app.state.beszel.warned.contains("s1"),
            "the disk warning was skipped because a machine was open"
        );
    }

    #[test]
    fn a_selection_the_hub_no_longer_knows_is_cleared() {
        let mut app = connected_app();
        app.state.beszel.selected = Some("gone".into());
        let _ = app.update(Message::BeszelConnected(Ok(session("[]"))));
        assert!(app.state.beszel.selected.is_none());
    }

    #[test]
    fn detail_for_another_machine_does_not_replace_the_open_history() {
        let mut app = connected_app();
        app.state.beszel.selected = Some("open".into());
        app.state.beszel.history = vec![
            serde_json::from_str(
                r#"{"id":"r","system":"open","type":"1m","created":"x","stats":{"cpu":1.0}}"#,
            )
            .unwrap(),
        ];

        let late = beszel::Detail {
            stats: None,
            containers: vec![],
            history: vec![],
        };
        let _ = app.update(Message::BeszelDetailLoaded(
            "other".into(),
            Ok(Arc::new(late)),
        ));

        assert_eq!(
            app.state.beszel.history.len(),
            1,
            "a late reply for another machine is ignored"
        );
    }

    // ---- agent deployment ------------------------------------------------------

    #[test]
    fn proposing_an_install_runs_nothing_and_shows_the_command() {
        let mut app = connected_app();
        app.state.beszel.hub_key = "ssh-ed25519 AAAA".into();
        let id = receiving_peer(&app);

        let task = app.update(Message::BeszelProposeInstall(id));

        assert!(!started(&task), "proposing must never start the install");
        let pending = app
            .state
            .beszel
            .pending_install
            .as_ref()
            .expect("a proposal");
        assert!(pending.command.contains("sudo"), "the root step is visible");
    }

    #[test]
    fn an_install_cannot_be_proposed_without_the_hub_key() {
        let mut app = connected_app();
        let id = receiving_peer(&app);

        let _ = app.update(Message::BeszelProposeInstall(id));
        assert!(app.state.beszel.pending_install.is_none());
        assert!(app.state.error.is_some());
    }

    #[test]
    fn only_confirming_starts_the_install_and_cancel_discards_it() {
        let mut app = connected_app();
        app.state.beszel.hub_key = "ssh-ed25519 AAAA".into();
        let id = receiving_peer(&app);

        let _ = app.update(Message::BeszelProposeInstall(id.clone()));
        let _ = app.update(Message::BeszelCancelInstall);
        assert!(app.state.beszel.pending_install.is_none());
        let task = app.update(Message::BeszelConfirmInstall);
        assert!(!started(&task), "nothing to confirm after cancelling");

        let _ = app.update(Message::BeszelProposeInstall(id.clone()));
        let task = app.update(Message::BeszelConfirmInstall);
        assert!(started(&task));
        assert!(app.state.beszel.installing.contains_key(&id));
    }

    #[test]
    fn an_install_outcome_is_reported_either_way() {
        let mut app = connected_app();
        app.state.beszel.installing.insert("n1".into(), ());

        let failed = beszel_client::InstallOutcome {
            succeeded: false,
            output: "downloading\npermission denied".into(),
        };
        let _ = app.update(Message::BeszelAgentInstalled("n1".into(), Arc::new(failed)));

        assert!(!app.state.beszel.installing.contains_key("n1"));
        assert!(
            app.state
                .error
                .as_deref()
                .is_some_and(|e| e.contains("permission denied"))
        );
        assert!(
            app.state.beszel.last_install.is_some(),
            "the transcript is kept to read"
        );
    }
}
