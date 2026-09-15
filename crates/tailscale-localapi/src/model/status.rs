//! `GET /localapi/v0/status` — the whole tailnet as the daemon currently sees it.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;

use super::ipn::BackendState;
use super::non_zero_time;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct Status {
    pub version: String,
    #[serde(rename = "TUN")]
    pub tun: bool,
    pub backend_state: BackendState,
    pub have_node_key: bool,
    /// Non-empty while the node is waiting for a browser login.
    #[serde(rename = "AuthURL")]
    pub auth_url: String,
    #[serde(
        rename = "TailscaleIPs",
        deserialize_with = "crate::model::null_as_default"
    )]
    pub tailscale_ips: Vec<String>,
    #[serde(rename = "Self")]
    pub self_status: Option<PeerStatus>,
    /// Daemon-reported problems, already rendered as human-readable strings.
    /// Go sends `null` rather than `[]` when there is nothing wrong.
    #[serde(deserialize_with = "crate::model::null_as_default")]
    pub health: Vec<String>,
    #[serde(rename = "MagicDNSSuffix")]
    pub magic_dns_suffix: String,
    pub current_tailnet: Option<TailnetStatus>,
    pub cert_domains: Option<Vec<String>>,
    /// Keyed by node public key. A tailnet whose only machine is this one
    /// sends `null` here, not `{}`.
    #[serde(deserialize_with = "crate::model::null_as_default")]
    pub peer: BTreeMap<String, PeerStatus>,
    /// Keyed by user ID, stringified.
    #[serde(deserialize_with = "crate::model::null_as_default")]
    pub user: BTreeMap<String, UserProfile>,
}

impl Status {
    /// Peers sorted the way the UI lists them: online first, then by hostname.
    #[must_use]
    pub fn peers_sorted(&self) -> Vec<&PeerStatus> {
        let mut peers: Vec<&PeerStatus> = self.peer.values().collect();
        peers.sort_by_key(|peer| (!peer.online, peer.host_name.to_lowercase()));
        peers
    }

    /// Every peer that has advertised itself as usable for exit traffic.
    #[must_use]
    pub fn exit_node_options(&self) -> Vec<&PeerStatus> {
        let mut nodes: Vec<&PeerStatus> =
            self.peer.values().filter(|p| p.exit_node_option).collect();
        nodes.sort_by_key(|peer| peer.host_name.to_lowercase());
        nodes
    }

    /// The peer currently carrying this node's internet traffic, if any.
    #[must_use]
    pub fn active_exit_node(&self) -> Option<&PeerStatus> {
        self.peer.values().find(|p| p.exit_node)
    }

    #[must_use]
    pub fn user_for(&self, peer: &PeerStatus) -> Option<&UserProfile> {
        self.user.get(&peer.user_id.to_string())
    }

    #[must_use]
    pub fn peer_by_id(&self, id: &str) -> Option<&PeerStatus> {
        self.peer.values().find(|p| p.id == id)
    }

    /// The tailnet's MagicDNS domain, e.g. `orca-cat.ts.net`.
    #[must_use]
    pub fn tailnet_name(&self) -> &str {
        self.current_tailnet
            .as_ref()
            .map_or(self.magic_dns_suffix.as_str(), |t| {
                t.magic_dns_suffix.as_str()
            })
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct TailnetStatus {
    pub name: String,
    #[serde(rename = "MagicDNSSuffix")]
    pub magic_dns_suffix: String,
    #[serde(rename = "MagicDNSEnabled")]
    pub magic_dns_enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct UserProfile {
    #[serde(rename = "ID")]
    pub id: u64,
    pub login_name: String,
    pub display_name: String,
    #[serde(rename = "ProfilePicURL")]
    pub profile_pic_url: String,
}

impl UserProfile {
    /// Two-letter monogram for the avatar bubble, as in the mockup's "AC".
    #[must_use]
    pub fn initials(&self) -> String {
        let source = if self.display_name.is_empty() {
            &self.login_name
        } else {
            &self.display_name
        };

        let letters: String = source
            .split(|c: char| c.is_whitespace() || c == '.' || c == '@')
            .filter(|part| !part.is_empty())
            .filter_map(|part| part.chars().next())
            .take(2)
            .collect();

        if letters.is_empty() {
            "?".to_string()
        } else {
            letters.to_uppercase()
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct PeerStatus {
    /// Stable node ID, e.g. `nenpcEYJVH11CNTRL`. This is what the exit-node and
    /// Taildrop endpoints expect, not the public key.
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "NodeID")]
    pub node_id: u64,
    pub public_key: String,
    pub host_name: String,
    /// Fully-qualified MagicDNS name, with the trailing dot Go includes.
    #[serde(rename = "DNSName")]
    pub dns_name: String,
    #[serde(rename = "OS")]
    pub os: String,
    #[serde(rename = "UserID")]
    pub user_id: u64,
    #[serde(
        rename = "TailscaleIPs",
        deserialize_with = "crate::model::null_as_default"
    )]
    pub tailscale_ips: Vec<String>,
    /// Present when traffic is flowing peer-to-peer rather than via DERP.
    pub cur_addr: String,
    /// DERP region code, e.g. `fra`. Empty when there is no relay in play.
    pub relay: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub created: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub last_handshake: Option<DateTime<Utc>>,
    pub key_expiry: Option<DateTime<Utc>>,
    pub online: bool,
    /// This peer is currently acting as our exit node.
    pub exit_node: bool,
    /// This peer offers itself as an exit node.
    pub exit_node_option: bool,
    pub active: bool,
    /// 1 means this peer can receive Taildrop transfers.
    pub taildrop_target: i32,
    pub no_file_sharing_reason: String,
    #[serde(rename = "SSH_HostKeys", alias = "sshHostKeys")]
    pub ssh_host_keys: Option<Vec<String>>,
    pub capabilities: Option<Vec<String>>,
}

impl PeerStatus {
    /// MagicDNS name without Go's trailing dot.
    #[must_use]
    pub fn magic_dns(&self) -> &str {
        self.dns_name.trim_end_matches('.')
    }

    /// The short label the sidebar shows; falls back to the DNS label.
    #[must_use]
    pub fn display_name(&self) -> &str {
        if self.host_name.is_empty() {
            self.magic_dns().split('.').next().unwrap_or("unknown")
        } else {
            &self.host_name
        }
    }

    #[must_use]
    pub fn ipv4(&self) -> Option<&str> {
        self.tailscale_ips
            .iter()
            .find(|ip| ip.contains('.'))
            .map(String::as_str)
    }

    #[must_use]
    pub fn ipv6(&self) -> Option<&str> {
        self.tailscale_ips
            .iter()
            .find(|ip| ip.contains(':'))
            .map(String::as_str)
    }

    /// How this peer is reached right now. Drives the "Direct" vs "DERP" pill.
    #[must_use]
    pub fn route(&self) -> Route<'_> {
        if !self.cur_addr.is_empty() {
            Route::Direct(&self.cur_addr)
        } else if !self.relay.is_empty() {
            Route::Derp(&self.relay)
        } else {
            Route::Idle
        }
    }

    #[must_use]
    pub fn can_receive_files(&self) -> bool {
        self.taildrop_target == 1 && self.no_file_sharing_reason.is_empty()
    }

    #[must_use]
    pub fn supports_ssh(&self) -> bool {
        self.ssh_host_keys.as_ref().is_some_and(|k| !k.is_empty())
    }

    #[must_use]
    pub fn last_seen_at(&self) -> Option<DateTime<Utc>> {
        non_zero_time(self.last_seen)
    }

    #[must_use]
    pub fn last_handshake_at(&self) -> Option<DateTime<Utc>> {
        non_zero_time(self.last_handshake)
    }

    #[must_use]
    pub fn key_expiry_at(&self) -> Option<DateTime<Utc>> {
        non_zero_time(self.key_expiry)
    }

    /// Whole days until the node key expires. Negative once it already has.
    #[must_use]
    pub fn days_until_key_expiry(&self) -> Option<i64> {
        self.key_expiry_at()
            .map(|expiry| (expiry - Utc::now()).num_days())
    }

    /// Case-insensitive match across the fields a user would type to find a
    /// machine: hostname, MagicDNS name, any IP, and the OS.
    #[must_use]
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        self.host_name.to_lowercase().contains(&query)
            || self.dns_name.to_lowercase().contains(&query)
            || self.os.to_lowercase().contains(&query)
            || self.tailscale_ips.iter().any(|ip| ip.contains(&query))
    }
}

/// How traffic currently reaches a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route<'a> {
    /// Peer-to-peer WireGuard, with the endpoint in use.
    Direct(&'a str),
    /// Relayed through a DERP region, identified by its code.
    Derp(&'a str),
    /// No path established; the peer is idle or offline.
    Idle,
}
