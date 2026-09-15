//! Icon names, resolved through the user's active COSMIC icon theme.
//!
//! Everything here is a freedesktop symbolic name rather than a bundled asset,
//! so the client picks up whatever icon theme the desktop is set to.

use cosmic::widget::icon;

/// Load a named symbolic icon at the given pixel size.
pub fn named(name: &'static str, size: u16) -> icon::Icon {
    icon::icon(icon::from_name(name).size(size).handle()).size(size)
}

/// The device glyph for a machine, chosen from the OS the daemon reports.
#[must_use]
pub fn for_os(os: &str) -> &'static str {
    match os {
        "android" | "iOS" => "phone-symbolic",
        "macOS" => "laptop-symbolic",
        "windows" => "computer-symbolic",
        "tvOS" => "video-display-symbolic",
        "linux" | "freebsd" | "openbsd" => "computer-symbolic",
        _ => "network-workgroup-symbolic",
    }
}

pub const TAILNET: &str = "network-workgroup-symbolic";
pub const MACHINES: &str = "computer-symbolic";
pub const EXIT_NODE: &str = "network-wired-symbolic";
pub const SERVICES: &str = "network-server-symbolic";
pub const MONITORING: &str = "utilities-system-monitor-symbolic";
pub const ACCESS: &str = "security-high-symbolic";
pub const PREFERENCES: &str = "preferences-system-symbolic";

pub const COPY: &str = "edit-copy-symbolic";
pub const REFRESH: &str = "view-refresh-symbolic";
pub const TERMINAL: &str = "utilities-terminal-symbolic";
pub const SEND: &str = "document-send-symbolic";
pub const DOWNLOAD: &str = "folder-download-symbolic";
pub const OK: &str = "emblem-ok-symbolic";
pub const WARNING: &str = "dialog-warning-symbolic";
pub const ADD: &str = "list-add-symbolic";
pub const FORWARD: &str = "go-next-symbolic";
pub const ACCOUNT: &str = "user-info-symbolic";
