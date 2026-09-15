//! Typed mirrors of the JSON that `tailscaled` speaks.
//!
//! These deliberately cover only the fields the COSMIC client renders or
//! writes. Everything tailscaled sends that we do not model is ignored rather
//! than rejected, so a daemon upgrade cannot break the UI.

pub mod ipn;
pub mod ping;
pub mod prefs;
pub mod serve;
pub mod status;
pub mod taildrop;

pub use ipn::{BackendState, Notify};
pub use ping::PingResult;
pub use prefs::{MaskedPrefs, Prefs};
pub use serve::{ServeConfig, ServeEntry, ServeScope};
pub use status::{PeerStatus, Status, TailnetStatus, UserProfile};
pub use taildrop::{FileTarget, WaitingFile};

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Go marshals a zero `time.Time` as year 1, which means "never" rather than
/// "the year 1". Everything in this crate funnels timestamps through here so
/// that distinction never leaks into the UI.
pub(crate) fn non_zero_time(value: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    value.filter(|t| t.timestamp() > 0)
}

/// Deserialize a field that Go may marshal as `null` instead of its empty
/// value. Used for the slice fields that tailscaled leaves nil when empty.
pub(crate) fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}
