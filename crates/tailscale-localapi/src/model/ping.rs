//! `POST /localapi/v0/ping` — latency and path probe for one peer.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct PingResult {
    #[serde(rename = "IP")]
    pub ip: String,
    pub node_name: String,
    /// Non-empty when the probe itself failed.
    pub err: String,
    pub latency_seconds: f64,
    /// The peer-to-peer endpoint that answered, when the path is direct.
    pub endpoint: String,
    #[serde(rename = "DERPRegionCode")]
    pub derp_region_code: String,
}

impl PingResult {
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.err.is_empty()
    }

    #[must_use]
    pub fn latency_ms(&self) -> f64 {
        self.latency_seconds * 1000.0
    }

    /// True when the probe travelled peer-to-peer rather than via a relay.
    #[must_use]
    pub fn is_direct(&self) -> bool {
        !self.endpoint.is_empty() && self.derp_region_code.is_empty()
    }

    /// One-line summary for the detail pane, e.g. `14 ms · direct`.
    #[must_use]
    pub fn summary(&self) -> String {
        if !self.is_ok() {
            return self.err.clone();
        }
        let path = if self.is_direct() {
            "direct".to_string()
        } else if !self.derp_region_code.is_empty() {
            format!("relayed via {}", self.derp_region_code)
        } else {
            "unknown path".to_string()
        };
        format!("{:.0} ms · {path}", self.latency_ms())
    }
}

/// Which probe the daemon should run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PingType {
    /// Measures the real WireGuard path, including DERP fallback.
    Disco,
    /// ICMP inside the tunnel.
    Icmp,
    /// Probes the peer's PeerAPI over TCP.
    Tsmp,
}

impl PingType {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disco => "disco",
            Self::Icmp => "icmp",
            Self::Tsmp => "tsmp",
        }
    }
}
