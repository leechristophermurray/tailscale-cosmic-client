//! Tunnelling to a Caddy admin API that is bound to the remote localhost.
//!
//! The tunnel rides Tailscale SSH: because Tailscale intercepts port 22 within
//! the tailnet, a plain `ssh` to a MagicDNS name is authenticated by the user's
//! tailnet identity and the node's advertised host key. No key distribution, no
//! admin port exposed to the network, and nothing to clean up on the server.

use std::process::Stdio;

use tokio::process::{Child, Command};

use crate::client::{DEFAULT_ADMIN_PORT, Endpoint};

/// A live SSH port-forward. Dropping this kills the forward.
#[derive(Debug)]
pub struct Tunnel {
    child: Child,
    endpoint: Endpoint,
}

impl Tunnel {
    /// Forward `127.0.0.1:<local_port>` here to `127.0.0.1:2019` on `host`.
    ///
    /// The local end binds to loopback only, so the tunnel does not
    /// accidentally republish the remote admin API to this machine's network.
    pub fn open(host: &str, local_port: u16) -> std::io::Result<Self> {
        let child = Command::new("ssh")
            .arg("-N")
            // No prompts: this runs behind a GUI with no terminal to answer on.
            .args(["-o", "BatchMode=yes"])
            // Fail loudly if the forward cannot bind, rather than sitting there
            // with a connection that forwards nothing.
            .args(["-o", "ExitOnForwardFailure=yes"])
            // Trust a host key we have not seen before. The transport is
            // already WireGuard to a specific node key that Tailscale
            // authenticated, so refusing on first contact would block the
            // common case without adding protection.
            .args(["-o", "StrictHostKeyChecking=accept-new"])
            // A silently dead forward looks identical to a slow one; make ssh
            // notice and exit so `is_running` reports the truth.
            .args(["-o", "ServerAliveInterval=15"])
            .args(["-o", "ServerAliveCountMax=3"])
            .args([
                "-L",
                &format!("127.0.0.1:{local_port}:127.0.0.1:{DEFAULT_ADMIN_PORT}"),
            ])
            .arg(host)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        Ok(Self {
            child,
            endpoint: Endpoint::tunnelled(local_port),
        })
    }

    /// Wait until the forwarded port accepts a connection.
    ///
    /// ssh needs to authenticate and bind before anything can be sent through,
    /// and how long that takes depends on the network. Polling for the port
    /// beats a fixed sleep, which is either too short on a slow link or wasted
    /// time on a fast one.
    pub async fn wait_until_ready(&mut self, timeout: std::time::Duration) -> Result<(), String> {
        let deadline = std::time::Instant::now() + timeout;
        let address = self.endpoint.authority();

        loop {
            if !self.is_running() {
                return Err(self.failure_reason().await);
            }

            if tokio::net::TcpStream::connect(&address).await.is_ok() {
                return Ok(());
            }

            if std::time::Instant::now() >= deadline {
                return Err(format!(
                    "the SSH tunnel did not start forwarding {address} within {}s",
                    timeout.as_secs()
                ));
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// Whatever ssh printed before giving up.
    ///
    /// "Permission denied" or "Connection refused" is far more actionable than
    /// a generic connection failure, and it is the only place that detail
    /// exists.
    async fn failure_reason(&mut self) -> String {
        use tokio::io::AsyncReadExt;

        let mut message = String::new();
        if let Some(mut stderr) = self.child.stderr.take() {
            let _ = stderr.read_to_string(&mut message).await;
        }

        let message = message.trim();
        if message.is_empty() {
            "the SSH tunnel closed immediately".to_string()
        } else {
            message.lines().last().unwrap_or(message).to_string()
        }
    }

    /// The local endpoint the tunnel exposes.
    #[must_use]
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Whether the forward is still up.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Tear the tunnel down.
    pub async fn close(mut self) {
        let _ = self.child.kill().await;
    }
}
