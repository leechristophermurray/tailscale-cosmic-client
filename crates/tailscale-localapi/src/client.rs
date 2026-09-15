//! The typed LocalAPI surface the COSMIC client talks to.

use futures_util::Stream;
use http_body_util::BodyExt;
use hyper::Method;

use crate::error::{Error, Result};
use crate::model::{
    FileTarget, MaskedPrefs, Notify, PingResult, Prefs, ServeConfig, Status, WaitingFile,
    ipn::notify_mask, ping::PingType,
};
use crate::transport::Transport;

/// An async handle to the local `tailscaled`.
///
/// Cloning is cheap; every method opens its own short-lived connection, so a
/// clone per background task is the intended usage.
#[derive(Debug, Clone, Default)]
pub struct LocalApi {
    transport: Transport,
}

impl LocalApi {
    #[must_use]
    pub fn new(transport: Transport) -> Self {
        Self { transport }
    }

    /// Point at a non-default socket path, e.g. for a containerised daemon.
    #[must_use]
    pub fn with_socket(socket: impl AsRef<std::path::Path>) -> Self {
        Self::new(Transport::new(socket))
    }

    #[must_use]
    pub fn socket_path(&self) -> &std::path::Path {
        self.transport.socket()
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let bytes = self
            .transport
            .request_bytes(Method::GET, path, None)
            .await?;
        serde_json::from_slice(&bytes).map_err(Error::Decode)
    }

    /// Like [`Self::get_json`], but for endpoints that return a JSON list.
    ///
    /// Go marshals a nil slice as `null` rather than `[]`, so every list
    /// endpoint here can legitimately answer `null` when it has nothing to
    /// report. That is an empty list, not a decode failure.
    async fn get_list<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<Vec<T>> {
        let bytes = self
            .transport
            .request_bytes(Method::GET, path, None)
            .await?;

        if bytes.as_ref().trim_ascii().is_empty() {
            return Ok(Vec::new());
        }

        let value: Option<Vec<T>> = serde_json::from_slice(&bytes).map_err(Error::Decode)?;
        Ok(value.unwrap_or_default())
    }

    // ---- state -----------------------------------------------------------

    /// Full tailnet status: this node, every peer, and daemon health.
    pub async fn status(&self) -> Result<Status> {
        self.get_json("/localapi/v0/status").await
    }

    /// Status without the peer map. Cheap enough to poll frequently when all
    /// the caller needs is the connection state.
    pub async fn status_without_peers(&self) -> Result<Status> {
        self.get_json("/localapi/v0/status?peers=false").await
    }

    pub async fn prefs(&self) -> Result<Prefs> {
        self.get_json("/localapi/v0/prefs").await
    }

    /// Apply a partial prefs change. Returns the daemon's resulting prefs so
    /// the caller can reconcile rather than assume the write took effect.
    pub async fn set_prefs(&self, prefs: MaskedPrefs) -> Result<Prefs> {
        if prefs.is_empty() {
            return self.prefs().await;
        }

        let body = serde_json::to_vec(&prefs).map_err(Error::Encode)?;
        let bytes = self
            .transport
            .request_bytes(Method::PATCH, "/localapi/v0/prefs", Some(body))
            .await?;
        serde_json::from_slice(&bytes).map_err(Error::Decode)
    }

    // ---- connection control ----------------------------------------------

    /// Bring the tunnel up.
    pub async fn connect(&self) -> Result<Prefs> {
        self.set_prefs(MaskedPrefs::new().want_running(true)).await
    }

    /// Take the tunnel down without logging out.
    pub async fn disconnect(&self) -> Result<Prefs> {
        self.set_prefs(MaskedPrefs::new().want_running(false)).await
    }

    /// Start an interactive login, returning the URL the user must visit.
    pub async fn login_interactive(&self) -> Result<()> {
        self.transport
            .request_bytes(Method::POST, "/localapi/v0/login-interactive", None)
            .await?;
        Ok(())
    }

    pub async fn logout(&self) -> Result<()> {
        self.transport
            .request_bytes(Method::POST, "/localapi/v0/logout", None)
            .await?;
        Ok(())
    }

    // ---- exit nodes -------------------------------------------------------

    /// Route internet traffic through the peer with this stable node ID.
    pub async fn set_exit_node(&self, stable_id: &str) -> Result<Prefs> {
        self.set_prefs(MaskedPrefs::new().exit_node_id(stable_id))
            .await
    }

    /// Return to direct mesh routing.
    pub async fn clear_exit_node(&self) -> Result<Prefs> {
        self.set_prefs(MaskedPrefs::new().exit_node_id("")).await
    }

    /// Offer this machine as an exit node for the tailnet. The tailnet admin
    /// still has to approve the advertised route.
    pub async fn advertise_exit_node(&self, enabled: bool, keep: &[String]) -> Result<Prefs> {
        let mut routes: Vec<String> = keep.to_vec();
        routes.retain(|r| r != "0.0.0.0/0" && r != "::/0");
        if enabled {
            routes.push("0.0.0.0/0".to_string());
            routes.push("::/0".to_string());
        }
        self.set_prefs(MaskedPrefs::new().advertise_routes(routes))
            .await
    }

    // ---- diagnostics ------------------------------------------------------

    /// Probe a peer. `disco` reports the real WireGuard path and is what the
    /// machine detail pane shows.
    pub async fn ping(&self, ip: &str, kind: PingType) -> Result<PingResult> {
        let path = format!("/localapi/v0/ping?ip={ip}&type={}", kind.as_str());
        let bytes = self
            .transport
            .request_bytes(Method::POST, &path, None)
            .await?;
        serde_json::from_slice(&bytes).map_err(Error::Decode)
    }

    // ---- taildrop ---------------------------------------------------------

    /// Peers currently able to receive a Taildrop transfer.
    pub async fn file_targets(&self) -> Result<Vec<FileTarget>> {
        self.get_list("/localapi/v0/file-targets").await
    }

    /// Files that have arrived and are waiting to be saved.
    pub async fn waiting_files(&self) -> Result<Vec<WaitingFile>> {
        self.get_list("/localapi/v0/files/").await
    }

    /// Download a file that has arrived, by the name `waiting_files` reported.
    ///
    /// The file stays in the queue until [`Self::acknowledge_file`] removes it,
    /// so a failed write does not lose the transfer.
    pub async fn fetch_file(&self, name: &str) -> Result<Vec<u8>> {
        let path = format!("/localapi/v0/files/{}", urlencode(name));
        let bytes = self
            .transport
            .request_bytes(Method::GET, &path, None)
            .await?;
        Ok(bytes.to_vec())
    }

    /// Remove a received file from the daemon's queue, once it is safely saved.
    pub async fn acknowledge_file(&self, name: &str) -> Result<()> {
        let path = format!("/localapi/v0/files/{}", urlencode(name));
        self.transport
            .request_bytes(Method::DELETE, &path, None)
            .await?;
        Ok(())
    }

    /// Push one file to a peer, addressed by its stable node ID.
    pub async fn send_file(
        &self,
        stable_id: &str,
        filename: &str,
        contents: Vec<u8>,
    ) -> Result<()> {
        let encoded = urlencode(filename);
        let path = format!("/localapi/v0/file-put/{stable_id}/{encoded}");
        self.transport
            .request_bytes(Method::PUT, &path, Some(contents))
            .await?;
        Ok(())
    }

    // ---- serve & funnel ---------------------------------------------------

    /// Current serve/funnel configuration. The daemon sends `null` when nothing
    /// is published, which we normalise to an empty config.
    pub async fn serve_config(&self) -> Result<ServeConfig> {
        let bytes = self
            .transport
            .request_bytes(Method::GET, "/localapi/v0/serve-config", None)
            .await?;

        if bytes.as_ref().trim_ascii().is_empty() {
            return Ok(ServeConfig::default());
        }

        let value: Option<ServeConfig> = serde_json::from_slice(&bytes).map_err(Error::Decode)?;
        Ok(value.unwrap_or_default())
    }

    pub async fn set_serve_config(&self, config: &ServeConfig) -> Result<()> {
        let body = serde_json::to_vec(config).map_err(Error::Encode)?;
        self.transport
            .request_bytes(Method::POST, "/localapi/v0/serve-config", Some(body))
            .await?;
        Ok(())
    }

    // ---- push notifications ----------------------------------------------

    /// Subscribe to the IPN bus.
    ///
    /// The daemon holds this connection open and writes one JSON object per
    /// line as things change. The returned stream ends when the daemon closes
    /// the connection; callers are expected to reconnect.
    pub async fn watch(&self) -> Result<impl Stream<Item = Result<Notify>> + Send> {
        self.watch_with_mask(notify_mask::CLIENT).await
    }

    pub async fn watch_with_mask(
        &self,
        mask: u32,
    ) -> Result<impl Stream<Item = Result<Notify>> + Send> {
        let path = format!("/localapi/v0/watch-ipn-bus?mask={mask}");
        let response = self.transport.send(Method::GET, &path, None).await?;

        let status = response.status();
        if !status.is_success() {
            let bytes = response
                .into_body()
                .collect()
                .await
                .map_err(Error::Connection)?
                .to_bytes();
            return Err(Error::Status {
                status,
                body: String::from_utf8_lossy(&bytes).trim().to_string(),
            });
        }

        Ok(notify_stream(response.into_body()))
    }
}

/// Turn the chunked body into a stream of `Notify` values.
///
/// Chunk boundaries have nothing to do with line boundaries, so this buffers
/// until it sees a newline rather than parsing each chunk on its own.
fn notify_stream(body: hyper::body::Incoming) -> impl Stream<Item = Result<Notify>> + Send {
    futures_util::stream::unfold(
        (body, Vec::<u8>::new(), false),
        |(mut body, mut buffer, mut done)| async move {
            loop {
                // Drain whole lines already buffered before reading more.
                if let Some(index) = buffer.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = buffer.drain(..=index).collect();
                    let line = &line[..line.len() - 1];

                    if line.trim_ascii().is_empty() {
                        continue;
                    }

                    let parsed = serde_json::from_slice::<Notify>(line).map_err(Error::Decode);
                    return Some((parsed, (body, buffer, done)));
                }

                if done {
                    return None;
                }

                match body.frame().await {
                    Some(Ok(frame)) => {
                        if let Some(data) = frame.data_ref() {
                            buffer.extend_from_slice(data);
                        }
                    }
                    Some(Err(error)) => {
                        return Some((Err(Error::Connection(error)), (body, buffer, true)));
                    }
                    // Body finished: flush whatever trailing line is left.
                    None => {
                        done = true;
                        if buffer.trim_ascii().is_empty() {
                            return None;
                        }
                        buffer.push(b'\n');
                    }
                }
            }
        },
    )
}

/// Percent-encode a path segment. Taildrop filenames routinely contain spaces
/// and other characters that would otherwise break the request line.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}
