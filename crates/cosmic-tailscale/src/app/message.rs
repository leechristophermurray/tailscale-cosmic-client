//! Everything that can happen to the application, in one enum.

use std::sync::Arc;

use tailscale_localapi::{FileTarget, Notify, PingResult, Prefs, ServeConfig, Status, WaitingFile};

/// A daemon call that failed. The error is pre-rendered because
/// `tailscale_localapi::Error` is not `Clone`, and messages must be.
#[derive(Debug, Clone)]
pub struct Failure {
    pub message: String,
    /// True when tailscaled itself is unreachable, rather than the call being
    /// rejected. The UI shows a different, more actionable banner for that.
    pub unreachable: bool,
}

impl From<tailscale_localapi::Error> for Failure {
    fn from(error: tailscale_localapi::Error) -> Self {
        Self {
            unreachable: error.is_unreachable(),
            message: error.to_string(),
        }
    }
}

pub type Loaded<T> = Result<T, Failure>;

#[derive(Debug, Clone)]
pub enum Message {
    // ---- data arriving from the daemon --------------------------------
    /// Re-read everything. Triggered by the toolbar button and after writes.
    Refresh,
    StatusLoaded(Loaded<Arc<Status>>),
    PrefsLoaded(Loaded<Arc<Prefs>>),
    ServeLoaded(Loaded<Arc<ServeConfig>>),
    FileTargetsLoaded(Loaded<Arc<Vec<FileTarget>>>),
    WaitingFilesLoaded(Loaded<Arc<Vec<WaitingFile>>>),
    /// A push frame from the IPN bus.
    DaemonEvent(Arc<Notify>),
    /// The IPN bus dropped; the subscription will reconnect.
    DaemonStreamEnded,

    // ---- navigation and selection --------------------------------------
    FilterChanged(String),
    /// Select a machine by its stable node ID.
    SelectPeer(String),

    // ---- connection control --------------------------------------------
    SetConnected(bool),
    LoginRequested,
    /// Take the tunnel down for a while, then bring it back automatically.
    SuspendRequested,
    SuspendElapsed,

    // ---- exit nodes ------------------------------------------------------
    /// Index into the exit-node dropdown; 0 is "None (direct mesh)".
    ExitNodeSelected(usize),
    SetAllowLanAccess(bool),
    SetAdvertiseExitNode(bool),

    // ---- preferences -----------------------------------------------------
    SetAcceptRoutes(bool),
    SetAcceptDns(bool),
    SetRunSsh(bool),
    SetShieldsUp(bool),
    /// A prefs write came back; carries the daemon's resulting prefs.
    PrefsApplied(Loaded<Arc<Prefs>>),

    // ---- per-machine actions ---------------------------------------------
    CopyToClipboard(String),
    OpenUrl(String),
    /// Open a Tailscale SSH session to this peer in cosmic-term.
    SshToPeer(String),
    PingPeer(String),
    /// Probe every advertised exit node, so the list can show real latency
    /// rather than an empty column.
    PingExitNodes,
    PingCompleted(String, Loaded<Arc<PingResult>>),

    // ---- taildrop ---------------------------------------------------------
    /// Files were dropped onto the window; remember them and ask for a target.
    FilesDropped(Arc<Vec<std::path::PathBuf>>),
    /// A Wayland drop, which arrives as a `text/uri-list` payload rather than
    /// as ready-made paths.
    UriListDropped(String, Arc<Vec<u8>>),
    /// A drag entered or left the window, for drop-target feedback.
    DragOverWindow(bool),
    /// A drop delivered through the XDG document portal, which hands over a key
    /// to exchange for the actual paths rather than the paths themselves.
    PortalFileTransfer(String),
    /// Open the system file chooser.
    ///
    /// Carries the machine the files are destined for, when the user started
    /// from a machine's Send button — then the transfer starts as soon as they
    /// pick, with no second step.
    ChooseFiles(Option<String>),
    /// The chooser returned a selection.
    FilesChosen(Option<String>, Arc<Vec<std::path::PathBuf>>),
    /// The chooser was dismissed without a selection.
    FileChooserClosed,

    /// Choose which machine the Taildrop page sends to.
    TaildropSelectTarget(String),
    /// Send the pending drop to this peer's stable node ID.
    SendPendingTo(String),
    TaildropCompleted(Loaded<String>),
    ClearPendingDrop,
    /// Write these received files into the downloads folder.
    SaveWaitingFiles(Vec<String>),

    // ---- caddy -------------------------------------------------------------
    /// Index into the list of SSH-reachable peers.
    CaddyTargetSelected(usize),
    CaddyConnect,
    CaddyConnected(Loaded<crate::app::caddy::CaddyConnection>),
    CaddySitesLoaded(Loaded<Arc<Vec<caddy_admin::Site>>>),
    CaddyHostChanged(String),
    CaddyUpstreamChanged(String),
    CaddyAddRoute,
    CaddyDeleteRoute(String),
    CaddyRouteChanged(Loaded<String>),

    // ---- beszel monitoring -------------------------------------------------
    BeszelUrlChanged(String),
    BeszelUserChanged(String),
    BeszelPasswordChanged(String),
    /// Sign in with what is in the form, and remember the password.
    BeszelConnect,
    BeszelConnected(Loaded<Arc<crate::app::beszel::Session>>),
    /// Forget the stored password and sign out.
    BeszelSignOut,
    BeszelPasswordLoaded(Option<String>),
    /// Carries the keyring error, when there was one.
    BeszelPasswordStored(Option<String>),
    /// Show this Beszel system in detail, by its hub record id.
    BeszelSelectSystem(String),
    BeszelDetailLoaded(String, Loaded<Arc<crate::app::beszel::Detail>>),
    /// Index into the chart period picker.
    BeszelPeriodChanged(usize),
    /// Periodic hub poll.
    BeszelRefresh,

    // ---- agent deployment ----------------------------------------------------
    /// Ask to install the agent on this peer. Shows the command for approval;
    /// nothing runs yet.
    BeszelProposeInstall(String),
    /// The user approved the pending install.
    BeszelConfirmInstall,
    BeszelCancelInstall,
    BeszelAgentInstalled(String, Arc<beszel_client::InstallOutcome>),

    // ---- chrome -----------------------------------------------------------
    /// Dismiss the error banner.
    DismissError,
    /// Periodic tick that drives polling.
    Tick,
    Noop,
}
