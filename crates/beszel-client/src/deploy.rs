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
        let mut command = Command::new("ssh");
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
            .stderr(Stdio::piped());

        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return InstallOutcome::failed(format!("could not start ssh: {error}"));
            }
        };

        match tokio::time::timeout(INSTALL_TIMEOUT, wait_with_output(child)).await {
            Ok(outcome) => outcome,
            Err(_) => InstallOutcome::failed(format!(
                "the installation on {} did not finish within {} seconds",
                self.host,
                INSTALL_TIMEOUT.as_secs()
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

    #[test]
    fn the_outcome_summary_is_the_last_meaningful_line() {
        let outcome = InstallOutcome {
            succeeded: true,
            output: "downloading...\ninstalling...\nBeszel agent installed\n\n".to_string(),
        };
        assert_eq!(outcome.summary(), "Beszel agent installed");
    }
}
