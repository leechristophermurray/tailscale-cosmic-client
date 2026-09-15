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
        Self::open_with(std::ffi::OsStr::new("ssh"), host, local_port)
    }

    /// `open`, with the ssh program named explicitly, so tests can supply a
    /// stand-in rather than reaching a real machine.
    fn open_with(program: &std::ffi::OsStr, host: &str, local_port: u16) -> std::io::Result<Self> {
        let child = Command::new(program)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::Duration;

    /// Writing an executable and running it is racy under parallel tests: if
    /// another test forks while this file is still open for writing, the child
    /// inherits that descriptor and the kernel refuses to execute the file
    /// (`ETXTBSY`, "Text file busy") until the child execs. Every write of a
    /// fake and every spawn happens under this lock, so the two never overlap.
    static SPAWN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// A throwaway directory holding a fake `ssh` and a record of its argv.
    struct FakeSsh {
        dir: PathBuf,
    }

    impl FakeSsh {
        /// `body` is the shell the fake runs after recording its arguments.
        fn new(name: &str, body: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "fake-ssh-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos())
            ));
            std::fs::create_dir_all(&dir).unwrap();

            let script = format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{args}'\necho $$ > '{pid}'\n{body}\n",
                args = dir.join("args").display(),
                pid = dir.join("pid").display(),
            );
            let program = dir.join("ssh");
            std::fs::write(&program, script).unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();

            Self { dir }
        }

        fn program(&self) -> PathBuf {
            self.dir.join("ssh")
        }

        fn args(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.join("args"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }

        fn pid(&self) -> Option<u32> {
            std::fs::read_to_string(self.dir.join("pid"))
                .ok()?
                .trim()
                .parse()
                .ok()
        }
    }

    impl Drop for FakeSsh {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    async fn wait_for_args(fake: &FakeSsh) -> Vec<String> {
        for _ in 0..50 {
            let args = fake.args();
            if !args.is_empty() {
                return args;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        fake.args()
    }

    /// The forward must bind loopback only. Binding every interface would
    /// republish the remote admin API to this machine's whole network.
    #[tokio::test]
    async fn the_forward_binds_loopback_and_never_prompts() {
        let guard = SPAWN_LOCK.lock().await;
        let fake = FakeSsh::new("args", "exec sleep 30");
        let _tunnel = Tunnel::open_with(fake.program().as_os_str(), "server.example.ts.net", 12345)
            .expect("spawns");
        drop(guard);

        let args = wait_for_args(&fake).await;

        assert!(
            args.windows(2)
                .any(|w| w == ["-L", "127.0.0.1:12345:127.0.0.1:2019"]),
            "{args:?}"
        );
        assert!(
            args.contains(&"-N".to_string()),
            "no remote command, forward only"
        );
        for option in [
            "BatchMode=yes",
            "StrictHostKeyChecking=accept-new",
            "ExitOnForwardFailure=yes",
        ] {
            assert!(
                args.windows(2).any(|w| w[0] == "-o" && w[1] == option),
                "missing {option}: {args:?}"
            );
        }
        assert_eq!(
            args.last().map(String::as_str),
            Some("server.example.ts.net")
        );
    }

    #[tokio::test]
    async fn a_tunnel_is_ready_once_its_port_accepts_connections() {
        // The test plays the forwarded port; the fake ssh just stays alive.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let guard = SPAWN_LOCK.lock().await;
        let fake = FakeSsh::new("ready", "exec sleep 30");
        let mut tunnel = Tunnel::open_with(fake.program().as_os_str(), "host", port).unwrap();
        drop(guard);

        tunnel
            .wait_until_ready(Duration::from_secs(5))
            .await
            .expect("ready when the port answers");

        // And it claims the origin Caddy allows, not the forwarded port.
        assert_eq!(tunnel.endpoint().origin(), "localhost:2019");
        assert_eq!(tunnel.endpoint().authority(), format!("127.0.0.1:{port}"));
    }

    /// "Permission denied" is the actionable part; a generic failure is not.
    #[tokio::test]
    async fn a_refused_tunnel_reports_what_ssh_said() {
        let guard = SPAWN_LOCK.lock().await;
        let fake = FakeSsh::new(
            "denied",
            "echo 'debug1: connecting' >&2\necho 'user@host: Permission denied (tailscale).' >&2\nexit 255",
        );
        let mut tunnel = Tunnel::open_with(fake.program().as_os_str(), "host", 1).unwrap();
        drop(guard);

        let reason = tunnel
            .wait_until_ready(Duration::from_secs(5))
            .await
            .expect_err("ssh refused");

        assert_eq!(reason, "user@host: Permission denied (tailscale).");
    }

    #[tokio::test]
    async fn a_tunnel_that_never_opens_times_out_rather_than_hanging() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let guard = SPAWN_LOCK.lock().await;
        let fake = FakeSsh::new("stall", "exec sleep 30");
        let mut tunnel = Tunnel::open_with(fake.program().as_os_str(), "host", port).unwrap();
        drop(guard);

        let reason = tunnel
            .wait_until_ready(Duration::from_millis(400))
            .await
            .expect_err("never ready");

        assert!(reason.contains("did not start forwarding"), "{reason}");
    }

    /// The ssh process must not outlive the tunnel that owns it, or every
    /// reconnect leaks a forward.
    #[tokio::test]
    async fn dropping_the_tunnel_kills_ssh() {
        let guard = SPAWN_LOCK.lock().await;
        let fake = FakeSsh::new("drop", "exec sleep 30");
        let tunnel = Tunnel::open_with(fake.program().as_os_str(), "host", 1).unwrap();
        drop(guard);

        let _ = wait_for_args(&fake).await;
        let pid = fake.pid().expect("fake recorded its pid");
        assert!(
            std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "running while held"
        );

        drop(tunnel);

        let mut gone = false;
        for _ in 0..50 {
            let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
            // Killed and reaped, or at worst a zombie awaiting the reaper.
            if status.is_empty() || status.contains("State:\tZ") {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(gone, "ssh {pid} survived its tunnel being dropped");
    }
}
