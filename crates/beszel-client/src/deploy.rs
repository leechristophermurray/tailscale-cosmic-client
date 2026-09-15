//! Installing the Beszel agent on a tailnet machine, over Tailscale SSH.
//!
//! This runs a vendor install script as root on a remote machine. That is a
//! consequential thing to do, so the command is built here as a value the UI
//! can display verbatim before anything runs, and nothing is executed until the
//! caller explicitly asks for one specific host.

use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Where the vendor install script lives.
const INSTALL_SCRIPT_URL: &str = "https://get.beszel.dev";

/// The agent's default listening port, which the hub dials.
pub const DEFAULT_AGENT_PORT: u16 = 45876;

/// Installing over SSH includes a download and a package step, so this is
/// generous compared to an ordinary request.
const INSTALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// A planned installation, not yet run.
///
/// Holding the plan separately from running it is what lets the UI show the
/// exact command, on the exact host, and get a yes before anything happens.
#[derive(Debug, Clone)]
pub struct AgentInstall {
    /// MagicDNS name of the target machine.
    pub host: String,
    /// The hub's public key, from `/api/beszel/info`.
    pub hub_key: String,
    pub port: u16,
}

impl AgentInstall {
    #[must_use]
    pub fn new(host: impl Into<String>, hub_key: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            hub_key: hub_key.into(),
            port: DEFAULT_AGENT_PORT,
        }
    }

    /// The exact shell command that will run on the remote machine.
    ///
    /// Show this to the user before running it. It downloads a script and
    /// executes it as root, which nobody should agree to sight unseen.
    #[must_use]
    pub fn remote_command(&self) -> String {
        format!(
            "curl -sL {INSTALL_SCRIPT_URL} -o /tmp/install-agent.sh \
             && chmod +x /tmp/install-agent.sh \
             && sudo /tmp/install-agent.sh -k {}",
            shell_quote(&self.hub_key)
        )
    }

    /// A one-line summary of what this will do, for a confirmation prompt.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "Install the Beszel agent on {} as root, listening on port {}",
            self.host, self.port
        )
    }

    /// Run the installation.
    ///
    /// Returns the script's combined output either way: on success it says what
    /// was installed, and on failure it is the only explanation of why.
    pub async fn run(&self) -> InstallOutcome {
        self.run_with(std::ffi::OsStr::new("ssh"), INSTALL_TIMEOUT)
            .await
    }

    /// `run`, with the ssh program and time limit supplied, so tests can use a
    /// stand-in instead of a real machine and a short limit instead of minutes.
    async fn run_with(
        &self,
        program: &std::ffi::OsStr,
        timeout: std::time::Duration,
    ) -> InstallOutcome {
        let mut command = Command::new(program);
        command
            .args(["-o", "BatchMode=yes"])
            // The tailnet already authenticated the node; refusing on a
            // first-seen host key would block the normal case.
            .args(["-o", "StrictHostKeyChecking=accept-new"])
            .args(["-o", "ConnectTimeout=10"])
            .arg(&self.host)
            .arg(self.remote_command())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // When the install is abandoned — timed out, or the task dropped —
            // stop it. Otherwise the UI reports failure while a root installer
            // carries on remotely, and a retry starts a second one alongside.
            .kill_on_drop(true);

        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return InstallOutcome::failed(format!("could not start ssh: {error}"));
            }
        };

        match tokio::time::timeout(timeout, wait_with_output(child)).await {
            Ok(outcome) => outcome,
            Err(_) => InstallOutcome::failed(format!(
                "the installation on {} did not finish within {} seconds",
                self.host,
                timeout.as_secs()
            )),
        }
    }
}

/// What happened when an install ran.
#[derive(Debug, Clone)]
pub struct InstallOutcome {
    pub succeeded: bool,
    /// Combined stdout and stderr from the remote script.
    pub output: String,
}

impl InstallOutcome {
    fn failed(message: String) -> Self {
        Self {
            succeeded: false,
            output: message,
        }
    }

    /// The last non-empty line, which is usually the part worth showing in a
    /// notification rather than the whole transcript.
    #[must_use]
    pub fn summary(&self) -> &str {
        self.output
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("no output")
    }
}

async fn wait_with_output(mut child: tokio::process::Child) -> InstallOutcome {
    let mut stdout = String::new();
    let mut stderr = String::new();

    if let Some(mut handle) = child.stdout.take() {
        let _ = handle.read_to_string(&mut stdout).await;
    }
    if let Some(mut handle) = child.stderr.take() {
        let _ = handle.read_to_string(&mut stderr).await;
    }

    let status = child.wait().await;

    let mut output = stdout;
    if !stderr.trim().is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&stderr);
    }

    match status {
        // The doc's own verification step: a zero exit code means installed.
        Ok(status) if status.success() => InstallOutcome {
            succeeded: true,
            output,
        },
        Ok(status) => InstallOutcome {
            succeeded: false,
            output: format!("{output}\nexited with {status}"),
        },
        Err(error) => InstallOutcome::failed(format!("could not wait for ssh: {error}")),
    }
}

/// Single-quote a value for a POSIX shell.
///
/// The hub key is base64-ish and would almost certainly survive unquoted, but
/// this string is interpolated into a command that runs as root — "almost
/// certainly" is not the standard to hold that to.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_shown_exactly_as_it_will_run() {
        let install = AgentInstall::new("server.tailnet.ts.net", "ssh-ed25519 AAAAC3Nz");
        let command = install.remote_command();

        assert!(command.contains("get.beszel.dev"));
        assert!(command.contains("sudo /tmp/install-agent.sh -k"));
        assert!(command.contains("'ssh-ed25519 AAAAC3Nz'"));
    }

    /// The key is interpolated into a command run as root.
    #[test]
    fn a_key_containing_a_quote_cannot_break_out() {
        let install = AgentInstall::new("host", "key'; rm -rf /tmp/x; echo '");
        let command = install.remote_command();

        // The injected quote is escaped, so the whole value stays one argument.
        assert!(command.contains(r"'key'\''; rm -rf /tmp/x; echo '\'''"));
        assert!(!command.contains("-k 'key'; rm"));
    }

    #[test]
    fn the_summary_names_the_host_and_port() {
        let summary = AgentInstall::new("nas.ts.net", "k").summary();
        assert!(summary.contains("nas.ts.net"));
        assert!(summary.contains("45876"));
    }

    /// Writing an executable and running it is racy under parallel tests: if
    /// another test forks while this file is still open for writing, the child
    /// inherits that descriptor and the kernel refuses to execute the file
    /// (`ETXTBSY`, "Text file busy") until the child execs. Every write of a
    /// fake and every spawn happens under this lock, so the two never overlap.
    static SPAWN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// A stand-in `ssh` that records its arguments and pid, then runs `body`.
    /// Its directory is removed when this is dropped.
    struct FakeSsh {
        dir: std::path::PathBuf,
    }

    impl FakeSsh {
        fn new(name: &str, body: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;

            let dir = std::env::temp_dir().join(format!(
                "fake-deploy-ssh-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos())
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let program = dir.join("ssh");
            std::fs::write(
                &program,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\necho $$ > '{}'\n{body}\n",
                    dir.join("args").display(),
                    dir.join("pid").display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
            Self { dir }
        }

        fn program(&self) -> std::path::PathBuf {
            self.dir.join("ssh")
        }

        fn args(&self) -> String {
            std::fs::read_to_string(self.dir.join("args")).unwrap_or_default()
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

    fn alive(pid: u32) -> bool {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        !status.is_empty() && !status.contains("State:\tZ")
    }

    /// The remote command travels as ssh's last argument, after the host, so
    /// the machine that runs it is the one the user approved.
    #[tokio::test]
    async fn the_approved_command_runs_on_the_approved_host() {
        let install = AgentInstall::new("nas.example.ts.net", "ssh-ed25519 AAAA");
        let _guard = SPAWN_LOCK.lock().await;
        let fake = FakeSsh::new("host", "echo installed; exit 0");
        let program = fake.program();

        let outcome = install
            .run_with(program.as_os_str(), std::time::Duration::from_secs(5))
            .await;
        assert!(outcome.succeeded, "{}", outcome.output);

        let args = fake.args();
        let args: Vec<&str> = args.lines().collect();
        let host = args
            .iter()
            .position(|a| *a == "nas.example.ts.net")
            .expect("host passed");
        assert_eq!(
            args[host + 1],
            install.remote_command(),
            "the exact command shown to the user"
        );
        assert!(
            args.windows(2).any(|w| w == ["-o", "BatchMode=yes"]),
            "never prompts"
        );
    }

    #[tokio::test]
    async fn a_failing_install_keeps_stderr_and_the_exit_status() {
        let install = AgentInstall::new("host", "k");
        let _guard = SPAWN_LOCK.lock().await;
        let fake = FakeSsh::new(
            "fail",
            "echo downloading; echo 'sudo: a password is required' >&2; exit 1",
        );
        let program = fake.program();

        let outcome = install
            .run_with(program.as_os_str(), std::time::Duration::from_secs(5))
            .await;

        assert!(!outcome.succeeded);
        assert!(outcome.output.contains("downloading"), "stdout kept");
        assert!(
            outcome.output.contains("sudo: a password is required"),
            "stderr kept"
        );
        assert!(
            outcome.output.contains("exited with"),
            "the status is reported"
        );
    }

    #[tokio::test]
    async fn a_hung_install_is_abandoned_with_a_reason() {
        let install = AgentInstall::new("slow.ts.net", "k");
        let _guard = SPAWN_LOCK.lock().await;
        let fake = FakeSsh::new("hang", "exec sleep 30");

        let outcome = install
            .run_with(
                fake.program().as_os_str(),
                std::time::Duration::from_millis(300),
            )
            .await;

        assert!(!outcome.succeeded);
        assert!(
            outcome.output.contains("did not finish"),
            "{}",
            outcome.output
        );
        assert!(outcome.output.contains("slow.ts.net"));

        // Reporting a timeout while the install carries on would leave a root
        // installer running behind the user's back — and a retry would start a
        // second one on the same machine.
        let pid = fake.pid().expect("the fake ssh started");
        let mut stopped = false;
        for _ in 0..50 {
            if !alive(pid) {
                stopped = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(
            stopped,
            "ssh {pid} kept running after the install was abandoned"
        );
    }

    #[tokio::test]
    async fn a_missing_ssh_is_reported_not_panicked() {
        let _guard = SPAWN_LOCK.lock().await;
        let outcome = AgentInstall::new("host", "k")
            .run_with(
                std::ffi::OsStr::new("/nonexistent/ssh"),
                std::time::Duration::from_secs(1),
            )
            .await;

        assert!(!outcome.succeeded);
        assert!(
            outcome.output.contains("could not start ssh"),
            "{}",
            outcome.output
        );
    }

    #[test]
    fn the_outcome_summary_is_the_last_meaningful_line() {
        let outcome = InstallOutcome {
            succeeded: true,
            output: "downloading...\ninstalling...\nBeszel agent installed\n\n".to_string(),
        };
        assert_eq!(outcome.summary(), "Beszel agent installed");
    }
}
