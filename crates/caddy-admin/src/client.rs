//! A client for Caddy's native JSON admin API.
//!
//! Caddy is configured by PUTting JSON at config paths rather than by editing a
//! Caddyfile, so this client manipulates configuration structurally. There is
//! no text to parse and no service to restart: Caddy applies changes live.

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

use crate::error::{Error, Result};
use crate::model::{Route, Server};

/// Caddy's default admin port. It binds to localhost unless configured
/// otherwise, which is why reaching it across a tailnet usually means either
/// rebinding it to the node's tailnet address or tunnelling.
pub const DEFAULT_ADMIN_PORT: u16 = 2019;

/// Where a Caddy admin API lives, and what to call it.
///
/// Caddy checks the `Host` header of every admin request against its configured
/// `admin.origins` and answers 403 on a mismatch. Through an SSH tunnel the
/// address we dial is a local port that Caddy has never heard of, so the
/// address to connect to and the origin to claim are two different things.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    /// `Host` header to send. `None` means "use the dial authority", which is
    /// correct whenever we reach Caddy at an address it knows itself by.
    origin: Option<String>,
}

impl Endpoint {
    /// An admin API on a tailnet peer's own address.
    ///
    /// The operator must have added that address to `admin.origins`, because
    /// Caddy's default origins are loopback only.
    #[must_use]
    pub fn tailnet(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: DEFAULT_ADMIN_PORT,
            origin: None,
        }
    }

    /// An admin API reached through a local SSH port-forward.
    ///
    /// The forward's local port is arbitrary, so the request claims the
    /// loopback origin Caddy allows by default. Without this every call comes
    /// back `403 host not allowed`.
    #[must_use]
    pub fn tunnelled(local_port: u16) -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: local_port,
            origin: Some(format!("localhost:{DEFAULT_ADMIN_PORT}")),
        }
    }

    /// An admin API on this machine's own loopback, on the default port.
    #[must_use]
    pub fn local(port: u16) -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port,
            origin: None,
        }
    }

    /// `host:port`, bracketing IPv6 literals so it is a valid authority.
    #[must_use]
    pub fn authority(&self) -> String {
        if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// The value to send as `Host`, which Caddy matches against its origins.
    #[must_use]
    pub fn origin(&self) -> String {
        self.origin.clone().unwrap_or_else(|| self.authority())
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "http://{}", self.authority())
    }
}

#[derive(Debug, Clone)]
pub struct CaddyAdmin {
    endpoint: Endpoint,
}

impl CaddyAdmin {
    #[must_use]
    pub fn new(endpoint: Endpoint) -> Self {
        Self { endpoint }
    }

    #[must_use]
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    async fn request(&self, method: Method, path: &str, body: Option<Vec<u8>>) -> Result<Bytes> {
        let authority = self.endpoint.authority();

        let stream = TcpStream::connect(&authority)
            .await
            .map_err(|source| Error::Unreachable {
                endpoint: self.endpoint.to_string(),
                source,
            })?;

        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(Error::Connection)?;

        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::debug!(%error, "Caddy admin connection closed");
            }
        });

        // Origin-form (just the path), not an absolute URL. Given an
        // absolute-form request line, Go's HTTP server takes `r.Host` from the
        // URL's authority and ignores the Host header entirely — so Caddy would
        // match its origins against the tunnel's local port and refuse with
        // `403 host not allowed`, whatever we put in the header.
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(hyper::header::HOST, self.endpoint.origin());

        if body.is_some() {
            builder = builder.header(hyper::header::CONTENT_TYPE, "application/json");
        }

        let request = builder
            .body(Full::new(Bytes::from(body.unwrap_or_default())))
            .map_err(Error::Request)?;

        let response = sender
            .send_request(request)
            .await
            .map_err(Error::Connection)?;

        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(Error::Connection)?
            .to_bytes();

        if !status.is_success() {
            return Err(Error::Rejected {
                status,
                message: caddy_error_message(&bytes),
            });
        }

        Ok(bytes)
    }

    async fn get_json<T: serde::de::DeserializeOwned + Default>(&self, path: &str) -> Result<T> {
        let bytes = self.request(Method::GET, path, None).await?;

        // Caddy answers `null` for a config path that exists but is unset.
        if bytes.as_ref().trim_ascii().is_empty() || bytes.as_ref().trim_ascii() == b"null" {
            return Ok(T::default());
        }

        serde_json::from_slice(&bytes).map_err(Error::Decode)
    }

    /// Confirm something at this endpoint is actually Caddy.
    ///
    /// A bare TCP connect proves nothing — plenty of things listen on a port.
    /// The admin API has no version endpoint, so the test is that `/config/`
    /// answers with JSON; anything else on that port will not.
    pub async fn probe(&self) -> Result<()> {
        let bytes = self.request(Method::GET, "/config/", None).await?;
        let trimmed = bytes.as_ref().trim_ascii();

        // An unconfigured Caddy answers `null`, which is still Caddy.
        if trimmed == b"null" {
            return Ok(());
        }

        serde_json::from_slice::<serde_json::Value>(trimmed)
            .map(|_| ())
            .map_err(|_| Error::BadEndpoint(self.endpoint.to_string()))
    }

    /// The whole configuration document, as raw JSON.
    pub async fn config(&self) -> Result<serde_json::Value> {
        let bytes = self.request(Method::GET, "/config/", None).await?;
        serde_json::from_slice(&bytes).map_err(Error::Decode)
    }

    /// Every HTTP server block, keyed by Caddy's server name (`srv0`, …).
    pub async fn servers(&self) -> Result<std::collections::BTreeMap<String, Server>> {
        self.get_json("/config/apps/http/servers").await
    }

    /// Every site published by every HTTP server, flattened for display.
    ///
    /// This is what the UI wants: a Caddyfile compiles to nested subroutes, so
    /// the raw route list is a poor description of what is actually served.
    pub async fn sites(&self) -> Result<Vec<crate::model::Site>> {
        let servers = self.servers().await?;

        let mut sites: Vec<crate::model::Site> = servers.values().flat_map(Server::sites).collect();
        sites.sort_by(|a, b| a.host.cmp(&b.host).then_with(|| a.path.cmp(&b.path)));
        Ok(sites)
    }

    /// Routes on one server.
    pub async fn routes(&self, server: &str) -> Result<Vec<Route>> {
        self.get_json(&format!("/config/apps/http/servers/{server}/routes"))
            .await
    }

    /// Append a route to a server. Caddy applies it immediately, with no
    /// restart and no dropped connections.
    pub async fn add_route(&self, server: &str, route: &Route) -> Result<()> {
        let body = serde_json::to_vec(route).map_err(Error::Encode)?;
        // POST to a path ending in `/...` appends to the array rather than
        // replacing it, which is what keeps existing sites intact.
        self.request(
            Method::POST,
            &format!("/config/apps/http/servers/{server}/routes/..."),
            Some(body),
        )
        .await?;
        Ok(())
    }

    /// Remove a route previously created with an `@id`.
    pub async fn delete_route_by_id(&self, id: &str) -> Result<()> {
        self.request(Method::DELETE, &format!("/id/{id}"), None)
            .await?;
        Ok(())
    }

    /// Replace the entire configuration. Used only when a structural edit
    /// cannot be expressed as a single path write.
    pub async fn load(&self, config: &serde_json::Value) -> Result<()> {
        let body = serde_json::to_vec(config).map_err(Error::Encode)?;
        self.request(Method::POST, "/load", Some(body)).await?;
        Ok(())
    }
}

/// Caddy reports configuration errors as `{"error": "..."}`. Surfacing that
/// message is the difference between a usable error and "HTTP 400".
fn caddy_error_message(bytes: &[u8]) -> String {
    #[derive(serde::Deserialize)]
    struct CaddyError {
        error: String,
    }

    serde_json::from_slice::<CaddyError>(bytes).map_or_else(
        |_| String::from_utf8_lossy(bytes).trim().to_string(),
        |parsed| parsed.error,
    )
}
