//! The `systems` collection: one record per monitored machine.

use serde::Deserialize;

/// Whether the hub is currently hearing from a machine's agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SystemStatus {
    /// The agent is reporting.
    Up,
    /// The hub has lost contact.
    Down,
    /// Monitoring deliberately suspended.
    Paused,
    /// Registered but not yet heard from.
    Pending,
    #[default]
    #[serde(other)]
    Unknown,
}

impl SystemStatus {
    #[must_use]
    pub fn is_up(self) -> bool {
        matches!(self, Self::Up)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Up => "Reporting",
            Self::Down => "Not reporting",
            Self::Paused => "Paused",
            Self::Pending => "Waiting for first report",
            Self::Unknown => "Unknown",
        }
    }
}

/// One machine as the hub knows it.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct SystemRecord {
    pub id: String,
    /// The display name given in Beszel, which need not match the hostname.
    pub name: String,
    /// Address the hub reaches the agent on.
    pub host: String,
    pub port: String,
    pub status: SystemStatus,
    /// The summary metrics Beszel keeps on the record itself, so the systems
    /// list needs no per-machine stats query.
    pub info: Info,
    pub updated: String,
}

impl SystemRecord {
    /// Whether this record plausibly describes the same machine as a tailnet
    /// peer, matching on hostname or address.
    ///
    /// Beszel's `name` is user-chosen and its `host` is whatever address the
    /// hub dials, so neither alone is reliable — a machine may be registered by
    /// IP with a friendly name, or by MagicDNS name.
    #[must_use]
    pub fn matches_peer(&self, hostname: &str, dns_name: &str, addresses: &[String]) -> bool {
        let candidates = [
            self.name.as_str(),
            self.host.as_str(),
            self.info.hostname.as_str(),
        ];

        candidates.iter().any(|candidate| {
            if candidate.is_empty() {
                return false;
            }

            let candidate_lower = candidate.to_lowercase();

            candidate_lower == hostname.to_lowercase()
                || candidate_lower == dns_name.to_lowercase()
                // A MagicDNS name registered in full still refers to the short
                // hostname the tailnet knows.
                || candidate_lower.split('.').next() == Some(&hostname.to_lowercase())
                || addresses.iter().any(|address| address == candidate)
        })
    }
}

/// The at-a-glance metrics stored on the system record itself.
///
/// The one- and two-letter keys are Beszel's wire format, not a choice here.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Info {
    #[serde(rename = "h")]
    pub hostname: String,
    #[serde(rename = "k")]
    pub kernel_version: String,
    #[serde(rename = "c")]
    pub cores: u32,
    #[serde(rename = "t")]
    pub threads: u32,
    #[serde(rename = "m")]
    pub cpu_model: String,
    /// Seconds since boot.
    #[serde(rename = "u")]
    pub uptime: u64,
    /// CPU busy percentage.
    pub cpu: f64,
    #[serde(rename = "mp")]
    pub memory_pct: f64,
    #[serde(rename = "dp")]
    pub disk_pct: f64,
    #[serde(rename = "g")]
    pub gpu_pct: f64,
    /// The temperature Beszel surfaces on its dashboard, in Celsius.
    #[serde(rename = "dt")]
    pub dashboard_temp: f64,
    #[serde(rename = "v")]
    pub agent_version: String,
    /// Throughput in bytes per second.
    #[serde(rename = "bb")]
    pub bandwidth_bytes: u64,
    /// 1, 5 and 15 minute load averages.
    #[serde(rename = "la", deserialize_with = "crate::model::null_as_default")]
    pub load_average: [f64; 3],
    /// `[total services, failed services]` when systemd monitoring is on.
    #[serde(rename = "sv")]
    pub services: Option<[u16; 2]>,
}

impl Info {
    /// Uptime as a coarse human phrase.
    #[must_use]
    pub fn uptime_human(&self) -> String {
        let seconds = self.uptime;
        match seconds {
            s if s < 60 => format!("{s}s"),
            s if s < 3600 => format!("{}m", s / 60),
            s if s < 86_400 => format!("{}h", s / 3600),
            s => format!("{}d", s / 86_400),
        }
    }

    /// Failed systemd services, when the agent reports them.
    #[must_use]
    pub fn failed_services(&self) -> Option<u16> {
        self.services.map(|[_, failed]| failed)
    }

    /// Load average relative to thread count, which is what makes a figure like
    /// "4.0" mean anything: saturating on 4 threads, idling on 64.
    #[must_use]
    pub fn load_pressure(&self) -> Option<f64> {
        if self.threads == 0 {
            return None;
        }
        Some(self.load_average[0] / f64::from(self.threads))
    }
}
