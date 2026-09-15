//! Presentation helpers shared by every page: formatting, icon names, and the
//! small reusable widgets that make the pages look like one application.

pub mod chart;
pub mod format;
pub mod icons;
pub mod uri_list;
pub mod widgets;

pub use widgets::Tone;

use crate::fl;

/// "1 file" or "3 files", as a phrase the surrounding message can embed.
///
/// Fluent handles plurals with select expressions, but the count appears inside
/// several different sentences here, so it is simpler to build the noun phrase
/// once and interpolate it.
#[must_use]
pub fn file_count(count: usize) -> String {
    if count == 1 {
        fl!("files-one")
    } else {
        fl!("files-many", count = count)
    }
}

/// Short label for a backend state.
#[must_use]
pub fn backend_label(state: tailscale_localapi::BackendState) -> String {
    use tailscale_localapi::BackendState as S;
    match state {
        S::Running => fl!("state-connected"),
        S::Starting => fl!("state-connecting"),
        S::Stopped => fl!("state-disconnected"),
        S::NeedsLogin => fl!("state-needs-login"),
        S::NeedsMachineAuth => fl!("state-needs-approval"),
        S::InUseOtherUser => fl!("state-in-use"),
        S::NoState | S::Unknown => fl!("state-unknown"),
    }
}

/// A sentence explaining a backend state, for the status bar.
#[must_use]
pub fn backend_description(state: tailscale_localapi::BackendState) -> String {
    use tailscale_localapi::BackendState as S;
    match state {
        S::Running => fl!("state-connected-detail"),
        S::Starting => fl!("state-connecting-detail"),
        S::Stopped => fl!("state-disconnected-detail"),
        S::NeedsLogin => fl!("state-needs-login-detail"),
        S::NeedsMachineAuth => fl!("state-needs-approval-detail"),
        S::InUseOtherUser => fl!("state-in-use-detail"),
        S::NoState => fl!("state-no-state-detail"),
        S::Unknown => fl!("state-unknown-detail"),
    }
}
