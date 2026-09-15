//! The IPN bus: `tailscaled`'s push channel for state changes.

use serde::{Deserialize, Serialize};

use super::prefs::Prefs;

/// Where the backend is in its connect/authenticate lifecycle.
///
/// This arrives in two different encodings depending on the endpoint: `status`
/// sends the variant name as a string, while the IPN bus sends the daemon's
/// integer. The custom `Deserialize` below accepts either, and the numbering
/// matches Go's `ipn.State` constants exactly — note that `InUseOtherUser`
/// occupies slot 1, which is easy to miss and shifts everything after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[repr(i32)]
pub enum BackendState {
    NoState = 0,
    /// Another user on this machine is already using the daemon.
    InUseOtherUser = 1,
    NeedsLogin = 2,
    NeedsMachineAuth = 3,
    Stopped = 4,
    Starting = 5,
    Running = 6,
    /// Not a daemon state: what we report before the first poll lands, and what
    /// an unrecognised value from a newer daemon degrades to.
    #[default]
    Unknown = -1,
}

impl BackendState {
    #[must_use]
    pub fn from_i64(value: i64) -> Self {
        match value {
            0 => Self::NoState,
            1 => Self::InUseOtherUser,
            2 => Self::NeedsLogin,
            3 => Self::NeedsMachineAuth,
            4 => Self::Stopped,
            5 => Self::Starting,
            6 => Self::Running,
            _ => Self::Unknown,
        }
    }

    #[must_use]
    pub fn from_name(value: &str) -> Self {
        match value {
            "NoState" => Self::NoState,
            "InUseOtherUser" => Self::InUseOtherUser,
            "NeedsLogin" => Self::NeedsLogin,
            "NeedsMachineAuth" => Self::NeedsMachineAuth,
            "Stopped" => Self::Stopped,
            "Starting" => Self::Starting,
            "Running" => Self::Running,
            _ => Self::Unknown,
        }
    }

    #[must_use]
    pub fn is_running(self) -> bool {
        matches!(self, Self::Running)
    }

    /// True when the user has to do something before traffic can flow.
    #[must_use]
    pub fn needs_attention(self) -> bool {
        matches!(
            self,
            Self::NeedsLogin | Self::NeedsMachineAuth | Self::InUseOtherUser
        )
    }

    /// Short label for the header pill.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Connected",
            Self::Starting => "Connecting",
            Self::Stopped => "Disconnected",
            Self::NeedsLogin => "Sign in required",
            Self::NeedsMachineAuth => "Awaiting approval",
            Self::InUseOtherUser => "In use by another user",
            Self::NoState | Self::Unknown => "Unknown",
        }
    }

    /// Longer sentence for the status bar, explaining what the user should do.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::Running => "Connected to your tailnet",
            Self::Starting => "Bringing the tunnel up",
            Self::Stopped => "Tailscale is off",
            Self::NeedsLogin => "Sign in to join your tailnet",
            Self::NeedsMachineAuth => "Waiting for an admin to approve this machine",
            Self::InUseOtherUser => "Another user on this machine holds the daemon",
            Self::NoState => "The daemon has not reported a state yet",
            Self::Unknown => "Cannot reach tailscaled",
        }
    }
}

impl<'de> Deserialize<'de> for BackendState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl serde::de::Visitor<'_> for Visitor {
            type Value = BackendState;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an ipn.State name or integer")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(BackendState::from_name(value))
            }

            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(BackendState::from_i64(value))
            }

            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                // Any value that does not fit is not a state we know anyway.
                Ok(i64::try_from(value).map_or(BackendState::Unknown, BackendState::from_i64))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// One frame from `watch-ipn-bus`. Every field is optional: the daemon sends
/// only what changed.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct Notify {
    pub version: String,
    #[serde(rename = "SessionID")]
    pub session_id: Option<String>,
    pub err_message: Option<String>,
    pub login_finished: Option<serde_json::Value>,
    pub state: Option<BackendState>,
    pub prefs: Option<Prefs>,
    /// Set when the daemon wants the user to complete login in a browser.
    #[serde(rename = "BrowseToURL")]
    pub browse_to_url: Option<String>,
    pub engine: Option<EngineStatus>,
}

impl Notify {
    /// True when this frame carries something the UI cares about. The bus is
    /// chatty with engine-only heartbeats; those still matter for throughput,
    /// but a frame with nothing at all is worth dropping.
    #[must_use]
    pub fn is_meaningful(&self) -> bool {
        self.state.is_some()
            || self.prefs.is_some()
            || self.engine.is_some()
            || self.browse_to_url.is_some()
            || self.err_message.is_some()
            || self.login_finished.is_some()
    }
}

/// Cumulative WireGuard counters, used for the throughput readout.
#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct EngineStatus {
    #[serde(rename = "RBytes")]
    pub rx_bytes: u64,
    #[serde(rename = "WBytes")]
    pub tx_bytes: u64,
    pub num_live: u32,
    #[serde(rename = "LiveDERPs")]
    pub live_derps: u32,
}

/// Which notifications to subscribe to. These are bit flags on the
/// `watch-ipn-bus` query string, matching Go's `ipn.NotifyWatchOpt`.
///
/// Bit 0 is engine updates, not initial state — the initial-* flags all sit one
/// position higher than you would guess from the order they are documented in.
pub mod notify_mask {
    /// Periodic WireGuard throughput and live-peer counters.
    pub const WATCH_ENGINE_UPDATES: u32 = 1 << 0;
    /// Send the current backend state immediately on connect.
    pub const INITIAL_STATE: u32 = 1 << 1;
    /// Send the current prefs immediately on connect.
    pub const INITIAL_PREFS: u32 = 1 << 2;
    /// Send the full network map. Large; the COSMIC client polls `status`
    /// instead and leaves this off.
    pub const INITIAL_NETMAP: u32 = 1 << 3;
    /// Never include private key material in any frame.
    pub const NO_PRIVATE_KEYS: u32 = 1 << 4;
    /// Send daemon health warnings on connect.
    pub const INITIAL_HEALTH_STATE: u32 = 1 << 7;

    /// What the COSMIC front-ends ask for: state and prefs up front, live
    /// throughput, and never any private keys.
    pub const CLIENT: u32 =
        WATCH_ENGINE_UPDATES | INITIAL_STATE | INITIAL_PREFS | NO_PRIVATE_KEYS;
}
