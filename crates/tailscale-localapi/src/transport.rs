//! HTTP/1.1 transport over the tailscaled UNIX domain socket.
//!
//! `tailscaled` serves a plain HTTP API on a UNIX socket rather than a TCP
//! port, so there is no off-the-shelf client to point at it. Each call opens a
//! fresh connection: the socket is local, handshakes are free, and a
//! per-request connection keeps the long-lived IPN bus stream from blocking
//! ordinary polls behind it.

use std::path::{Path, PathBuf};

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;

use crate::error::{Error, Result};

/// Where `tailscaled` places its socket on Linux.
pub const DEFAULT_SOCKET: &str = "/var/run/tailscale/tailscaled.sock";

/// The daemon rejects requests whose Host header it does not recognise.
const HOST: &str = "local-tailscaled.sock";

/// Marks the request as coming from a local API client rather than a browser,
/// which is how tailscaled distinguishes us from a cross-site request.
const CSRF_HEADER: &str = "Sec-Tailscale";
const CSRF_VALUE: &str = "localapi";

#[derive(Debug, Clone)]
pub struct Transport {
    socket: PathBuf,
}

impl Default for Transport {
    fn default() -> Self {
        Self::new(DEFAULT_SOCKET)
    }
}

impl Transport {
    pub fn new(socket: impl AsRef<Path>) -> Self {
        Self {
            socket: socket.as_ref().to_path_buf(),
        }
    }

    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Issue a request and hand back the streaming response.
    ///
    /// The connection task is detached: it stays alive as long as the caller
    /// holds the body, which is what lets `watch-ipn-bus` stream for hours.
    pub async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<Response<Incoming>> {
        let stream = UnixStream::connect(&self.socket)
            .await
            .map_err(|e| Error::Socket(self.socket.clone(), e))?;

        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(Error::Connection)?;

        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::debug!(%error, "tailscaled connection closed");
            }
        });

        let has_body = body.is_some();
        // Origin-form (just the path) with an explicit Host. An absolute URL
        // here produces an absolute-form request line, which HTTP/1.1 reserves
        // for proxies; it only worked because tailscaled happens to derive
        // Host from the URL. The same shape broke the Caddy client outright.
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(hyper::header::HOST, HOST)
            .header(CSRF_HEADER, CSRF_VALUE);

        if has_body {
            request = request.header(hyper::header::CONTENT_TYPE, "application/json");
        }

        let request = request
            .body(Full::new(Bytes::from(body.unwrap_or_default())))
            .map_err(Error::Request)?;

        sender
            .send_request(request)
            .await
            .map_err(Error::Connection)
    }

    /// Issue a request and buffer the whole response body.
    pub async fn request_bytes(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<Bytes> {
        let response = self.send(method, path, body).await?;
        let status = response.status();

        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(Error::Connection)?
            .to_bytes();

        if !status.is_success() {
            return Err(Error::Status {
                status,
                body: String::from_utf8_lossy(&bytes).trim().to_string(),
            });
        }

        Ok(bytes)
    }
}
