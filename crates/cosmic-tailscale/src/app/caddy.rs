//! Caddy management state and the tasks that drive it.
//!
//! Reaching a peer's Caddy admin API is a two-step negotiation: try the peer's
//! tailnet address directly, and if nothing answers there, forward the port
//! over Tailscale SSH. The tunnel is owned here so it lives exactly as long as
//! the connection does.

use std::sync::Arc;

use caddy_admin::{CaddyAdmin, Endpoint, Reachability, Route, Site, Tunnel};
use cosmic::app::Task;

use super::message::{Failure, Message};

/// Local port for the SSH forward. High and fixed: a fresh port per connection
/// would leak listeners if a tunnel ever failed to shut down cleanly.
const TUNNEL_PORT: u16 = 12019;

/// Caddy's default HTTP server name, which is what a Caddyfile compiles to.
const DEFAULT_SERVER: &str = "srv0";

/// How long to give ssh to authenticate and bind the forward.
const TUNNEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);

/// How we are talking to the remote admin API.
#[derive(Debug, Clone, Default)]
pub enum Connection {
    #[default]
    Idle,
    Connecting,
    /// The admin API answered on the peer's own tailnet address.
    Direct(Endpoint),
    /// Reached through an SSH port-forward.
    Tunnelled(Endpoint),
    Failed(String),
}

impl Connection {
    #[must_use]
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Direct(_) | Self::Tunnelled(_))
    }

    #[must_use]
    pub fn endpoint(&self) -> Option<&Endpoint> {
        match self {
            Self::Direct(endpoint) | Self::Tunnelled(endpoint) => Some(endpoint),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct CaddyState {
    /// Stable node ID of the peer being managed.
    pub target: Option<String>,
    pub connection: Connection,
    /// Flattened `host -> upstream` rows, not the raw route tree.
    pub sites: Vec<Site>,
    /// Kept alive for as long as the tunnelled connection is in use; dropping
    /// it tears the forward down.
    pub tunnel: Option<Tunnel>,

    // ---- the add-route form ---------------------------------------------
    pub new_host: String,
    pub new_upstream: String,

    pub busy: bool,
}

impl CaddyState {
    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.busy || matches!(self.connection, Connection::Connecting)
    }

    /// A client for the current connection, if there is one.
    #[must_use]
    pub fn client(&self) -> Option<CaddyAdmin> {
        self.connection.endpoint().cloned().map(CaddyAdmin::new)
    }

    /// Drop any connection and its tunnel.
    pub fn disconnect(&mut self) {
        self.connection = Connection::Idle;
        self.sites.clear();
        // Dropping the tunnel kills the ssh process.
        self.tunnel = None;
    }
}

/// Reach a peer's Caddy admin API, preferring a direct tailnet connection.
pub fn connect(host: String) -> Task<Message> {
    cosmic::task::future(async move {
        // Direct first: it needs no extra process and no SSH permission.
        if let Reachability::Direct(endpoint) = caddy_admin::probe_peer(&host).await {
            return Message::CaddyConnected(Ok(CaddyConnection {
                endpoint,
                tunnel: None,
            }));
        }

        // Nothing on the tailnet address. Caddy almost certainly has its admin
        // API on the remote loopback, so forward it over Tailscale SSH.
        let mut tunnel = match Tunnel::open(&host, TUNNEL_PORT) {
            Ok(tunnel) => tunnel,
            Err(error) => {
                return Message::CaddyConnected(Err(Failure {
                    message: format!(
                        "No admin API on {host}:2019, and the SSH tunnel could not start: {error}"
                    ),
                    unreachable: true,
                }));
            }
        };

        // Wait for the forward to actually bind, and report ssh's own error if
        // it gives up instead.
        if let Err(reason) = tunnel.wait_until_ready(TUNNEL_TIMEOUT).await {
            return Message::CaddyConnected(Err(Failure {
                message: format!("Could not tunnel to {host}: {reason}"),
                unreachable: true,
            }));
        }

        let endpoint = tunnel.endpoint().clone();
        let client = CaddyAdmin::new(endpoint.clone());

        match client.probe().await {
            Ok(()) => Message::CaddyConnected(Ok(CaddyConnection {
                endpoint,
                tunnel: Some(Arc::new(std::sync::Mutex::new(Some(tunnel)))),
            })),
            Err(error) => {
                tunnel.close().await;
                Message::CaddyConnected(Err(Failure {
                    message: format!("Tunnel reached {host}, but Caddy did not answer: {error}"),
                    unreachable: false,
                }))
            }
        }
    })
}

/// A successful connection, handed back through a message.
///
/// The tunnel travels inside an `Arc<Mutex<Option<_>>>` because messages must
/// be `Clone` while a `Tunnel` owns a child process and must not be. The
/// receiver takes it out exactly once.
#[derive(Clone)]
pub struct CaddyConnection {
    pub endpoint: Endpoint,
    pub tunnel: Option<Arc<std::sync::Mutex<Option<Tunnel>>>>,
}

impl std::fmt::Debug for CaddyConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaddyConnection")
            .field("endpoint", &self.endpoint)
            .field("tunnelled", &self.tunnel.is_some())
            .finish()
    }
}

impl CaddyConnection {
    /// Take ownership of the tunnel, if this connection carries one.
    pub fn take_tunnel(&self) -> Option<Tunnel> {
        self.tunnel
            .as_ref()
            .and_then(|slot| slot.lock().ok()?.take())
    }
}

/// Read what Caddy is actually serving.
pub fn load_sites(client: CaddyAdmin) -> Task<Message> {
    cosmic::task::future(async move {
        let result = client.sites().await.map(Arc::new).map_err(|error| Failure {
            unreachable: error.is_unreachable(),
            message: error.to_string(),
        });
        Message::CaddySitesLoaded(result)
    })
}

/// Append a reverse-proxy route, then re-read what Caddy actually has.
pub fn add_route(client: CaddyAdmin, host: String, upstream: String) -> Task<Message> {
    cosmic::task::future(async move {
        // The id is derived from the hostname so the route can be deleted by
        // id later without depending on its index in the array.
        let id = format!("cosmic-tailscale-{}", host.replace('.', "-"));
        let route = Route::reverse_proxy(&id, &host, &upstream);

        match client.add_route(DEFAULT_SERVER, &route).await {
            Ok(()) => Message::CaddyRouteChanged(Ok(format!("Added {host} → {upstream}"))),
            Err(error) => Message::CaddyRouteChanged(Err(Failure {
                unreachable: error.is_unreachable(),
                message: error.to_string(),
            })),
        }
    })
}

/// Remove a route this client created.
pub fn delete_route(client: CaddyAdmin, id: String) -> Task<Message> {
    cosmic::task::future(async move {
        match client.delete_route_by_id(&id).await {
            Ok(()) => Message::CaddyRouteChanged(Ok("Route removed".to_string())),
            Err(error) => Message::CaddyRouteChanged(Err(Failure {
                unreachable: error.is_unreachable(),
                message: error.to_string(),
            })),
        }
    })
}
