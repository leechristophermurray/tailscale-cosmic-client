//! Applet messages.

use std::sync::Arc;

use cosmic::applet::token::subscription::TokenUpdate;
use cosmic::iced::window::Id;
use tailscale_localapi::{Notify, Prefs, Status};

#[derive(Debug, Clone)]
pub enum Message {
    // ---- popup lifecycle -------------------------------------------------
    PopupClosed(Id),
    Surface(cosmic::surface::Action<Message>),

    // ---- daemon ----------------------------------------------------------
    Refresh,
    StatusLoaded(Option<Arc<Status>>),
    PrefsLoaded(Option<Arc<Prefs>>),
    DaemonEvent(Arc<Notify>),

    // ---- actions ---------------------------------------------------------
    SetConnected(bool),
    ExitNodeSelected(usize),
    CopyToClipboard(String),
    OpenAdminConsole,
    OpenMainWindow,
    /// The compositor answered a request for an XDG activation token.
    TokenUpdate(TokenUpdate),
    SuspendForAnHour,
    SuspendElapsed,

    // ---- monitoring --------------------------------------------------------
    /// `None` means no hub is configured or it could not be read; the panel
    /// simply shows nothing in that case.
    MonitoringLoaded(Option<Arc<Vec<beszel_client::SystemRecord>>>),
    MonitoringRefresh,
    /// Pin this machine's metrics to the panel, by Beszel record id.
    PinSystem(String),
    UnpinSystem,
}
