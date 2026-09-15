//! `GET`/`PATCH /localapi/v0/prefs`.
//!
//! Writes use tailscaled's "masked prefs" convention: a prefs object where each
//! field you intend to change is accompanied by a `<Field>Set: true` sibling.
//! Anything without its mask set is left alone, so two settings pages cannot
//! clobber each other.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct Prefs {
    #[serde(rename = "ControlURL")]
    pub control_url: String,
    /// Accept subnet routes advertised by peers.
    pub route_all: bool,
    /// Stable ID of the exit node in use; empty for direct mesh.
    #[serde(rename = "ExitNodeID")]
    pub exit_node_id: String,
    #[serde(rename = "ExitNodeIP")]
    pub exit_node_ip: String,
    /// Keep LAN reachable while an exit node carries internet traffic.
    #[serde(rename = "ExitNodeAllowLANAccess")]
    pub exit_node_allow_lan_access: bool,
    /// Use the tailnet's DNS settings.
    #[serde(rename = "CorpDNS")]
    pub corp_dns: bool,
    /// Accept inbound Tailscale SSH.
    #[serde(rename = "RunSSH")]
    pub run_ssh: bool,
    pub run_web_client: bool,
    /// The master switch: whether the user wants the tunnel up.
    pub want_running: bool,
    pub logged_out: bool,
    /// Block all inbound connections.
    pub shields_up: bool,
    pub advertise_tags: Option<Vec<String>>,
    pub hostname: String,
    pub advertise_routes: Option<Vec<String>>,
    pub operator_user: String,
    pub config: Option<PrefsConfig>,
}

impl Prefs {
    #[must_use]
    pub fn is_exit_node_active(&self) -> bool {
        !self.exit_node_id.is_empty()
    }

    /// True when this node advertises a default route, i.e. offers itself as an
    /// exit node for the rest of the tailnet.
    #[must_use]
    pub fn advertises_exit_node(&self) -> bool {
        self.advertise_routes
            .as_ref()
            .is_some_and(|routes| routes.iter().any(|r| r == "0.0.0.0/0" || r == "::/0"))
    }

    /// Advertised routes with the two exit-node default routes filtered out, so
    /// the subnet-router list shows only real subnets.
    #[must_use]
    pub fn subnet_routes(&self) -> Vec<&str> {
        self.advertise_routes
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(String::as_str)
            .filter(|r| *r != "0.0.0.0/0" && *r != "::/0")
            .collect()
    }

    #[must_use]
    pub fn user_profile(&self) -> Option<&super::status::UserProfile> {
        self.config.as_ref().map(|c| &c.user_profile)
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct PrefsConfig {
    pub user_profile: super::status::UserProfile,
    #[serde(rename = "NodeID")]
    pub node_id: String,
}

/// A prefs write. Build one with the `set_*` helpers so the mask flag and the
/// value can never drift apart.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct MaskedPrefs {
    #[serde(skip_serializing_if = "Option::is_none")]
    want_running: Option<bool>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    want_running_set: bool,

    #[serde(rename = "ExitNodeID", skip_serializing_if = "Option::is_none")]
    exit_node_id: Option<String>,
    #[serde(rename = "ExitNodeIDSet", skip_serializing_if = "std::ops::Not::not")]
    exit_node_id_set: bool,

    #[serde(
        rename = "ExitNodeAllowLANAccess",
        skip_serializing_if = "Option::is_none"
    )]
    exit_node_allow_lan_access: Option<bool>,
    #[serde(
        rename = "ExitNodeAllowLANAccessSet",
        skip_serializing_if = "std::ops::Not::not"
    )]
    exit_node_allow_lan_access_set: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    route_all: Option<bool>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    route_all_set: bool,

    #[serde(rename = "CorpDNS", skip_serializing_if = "Option::is_none")]
    corp_dns: Option<bool>,
    #[serde(rename = "CorpDNSSet", skip_serializing_if = "std::ops::Not::not")]
    corp_dns_set: bool,

    #[serde(rename = "RunSSH", skip_serializing_if = "Option::is_none")]
    run_ssh: Option<bool>,
    #[serde(rename = "RunSSHSet", skip_serializing_if = "std::ops::Not::not")]
    run_ssh_set: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    shields_up: Option<bool>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    shields_up_set: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    advertise_routes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    advertise_routes_set: bool,
}

impl MaskedPrefs {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bring the tunnel up or take it down.
    #[must_use]
    pub fn want_running(mut self, value: bool) -> Self {
        self.want_running = Some(value);
        self.want_running_set = true;
        self
    }

    /// Route internet traffic through the peer with this stable ID. An empty
    /// string clears the exit node and returns to direct mesh.
    #[must_use]
    pub fn exit_node_id(mut self, value: impl Into<String>) -> Self {
        self.exit_node_id = Some(value.into());
        self.exit_node_id_set = true;
        self
    }

    #[must_use]
    pub fn exit_node_allow_lan_access(mut self, value: bool) -> Self {
        self.exit_node_allow_lan_access = Some(value);
        self.exit_node_allow_lan_access_set = true;
        self
    }

    #[must_use]
    pub fn route_all(mut self, value: bool) -> Self {
        self.route_all = Some(value);
        self.route_all_set = true;
        self
    }

    #[must_use]
    pub fn corp_dns(mut self, value: bool) -> Self {
        self.corp_dns = Some(value);
        self.corp_dns_set = true;
        self
    }

    #[must_use]
    pub fn run_ssh(mut self, value: bool) -> Self {
        self.run_ssh = Some(value);
        self.run_ssh_set = true;
        self
    }

    #[must_use]
    pub fn shields_up(mut self, value: bool) -> Self {
        self.shields_up = Some(value);
        self.shields_up_set = true;
        self
    }

    #[must_use]
    pub fn advertise_routes(mut self, routes: Vec<String>) -> Self {
        self.advertise_routes = Some(routes);
        self.advertise_routes_set = true;
        self
    }

    /// True when nothing was actually set, so callers can skip a no-op write.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !(self.want_running_set
            || self.exit_node_id_set
            || self.exit_node_allow_lan_access_set
            || self.route_all_set
            || self.corp_dns_set
            || self.run_ssh_set
            || self.shields_up_set
            || self.advertise_routes_set)
    }
}
