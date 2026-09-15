//! Finding a Caddy admin API on a tailnet peer.
//!
//! Caddy binds its admin API to `localhost:2019` by default, so a peer running
//! Caddy is usually *not* reachable across the tailnet without either
//! rebinding the admin listener or tunnelling to it. Both are supported; the
//! direct probe is tried first because it needs no extra process.

use std::time::Duration;

use crate::client::{CaddyAdmin, Endpoint};

/// How long to wait for a peer to answer before deciding it has no admin API.
/// Peers on this list are often asleep, so this stays short.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// How a peer's Caddy admin API was reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reachability {
    /// The admin API answers on the peer's own tailnet address.
    Direct(Endpoint),
    /// Nothing answered directly; a tunnel is required.
    NeedsTunnel,
}

/// Probe a peer's tailnet address for a Caddy admin API.
pub async fn probe_peer(host: &str) -> Reachability {
    let endpoint = Endpoint::tailnet(host);

    let client = CaddyAdmin::new(endpoint.clone());

    match tokio::time::timeout(PROBE_TIMEOUT, client.probe()).await {
        Ok(Ok(())) => Reachability::Direct(endpoint),
        Ok(Err(error)) => {
            tracing::debug!(%host, %error, "no Caddy admin API on the tailnet address");
            Reachability::NeedsTunnel
        }
        Err(_) => {
            tracing::debug!(%host, "Caddy admin probe timed out");
            Reachability::NeedsTunnel
        }
    }
}
