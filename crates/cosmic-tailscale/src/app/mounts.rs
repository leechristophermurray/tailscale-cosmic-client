//! A tailnet machine's files, mounted through GVfs.
//!
//! COSMIC Files browses remote locations through GIO, so a machine mounted with
//! `gio mount sftp://user@host/` appears there like any other location, and
//! stays mounted for every GIO application until it is unmounted. This module
//! drives the `gio` and `ssh` command-line tools; it has no GUI dependencies.
//!
//! GVfs runs `ssh` with `BatchMode yes`, so it cannot answer a first-contact
//! host-key question or a password prompt, and `gio mount` with no terminal
//! aborts either one. Mounting therefore starts with a short `ssh` probe that
//! records the host key and turns an authentication failure into a readable
//! error before GVfs is involved.

use std::ffi::OsString;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

/// Long enough for a relayed first connection; short enough that a machine that
/// will never answer does not leave a spinner running.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const MOUNT_TIMEOUT: Duration = Duration::from_secs(60);
const LIST_TIMEOUT: Duration = Duration::from_secs(15);

/// A user on a machine: what one SFTP mount is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub user: String,
    pub host: String,
}

impl Remote {
    /// A remote with a user name and host that are safe to put in a URI and on
    /// a command line.
    ///
    /// Both come from outside the program — the user types one, the daemon
    /// reports the other — and a value starting with `-` would be read by `ssh`
    /// as an option.
    pub fn new(user: &str, host: &str) -> Result<Self, String> {
        let user = user.trim();
        let host = host.trim().trim_end_matches('.');

        let valid_user = !user.is_empty()
            && !user.starts_with('-')
            && user
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if !valid_user {
            return Err(format!(
                "“{user}” is not a user name that can be used over SSH"
            ));
        }

        let valid_host = !host.is_empty()
            && !host.starts_with('-')
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
        if !valid_host {
            return Err(format!("“{host}” is not a host name or IPv4 address"));
        }

        Ok(Self {
            user: user.to_string(),
            host: host.to_ascii_lowercase(),
        })
    }

    /// The URI GVfs mounts, and reports as the mount's root.
    #[must_use]
    pub fn uri(&self) -> String {
        format!("sftp://{}@{}/", self.user, self.host)
    }

    fn destination(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }
}

/// An SFTP location GVfs currently has mounted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub remote: Remote,
    /// Where the user's home directory is, as a URI. GVfs asks the server when
    /// it mounts; `None` when it could not find out.
    pub home: Option<String>,
}

impl Mount {
    /// Where to open a file manager: the home directory when known, otherwise
    /// the root of the mount.
    #[must_use]
    pub fn location(&self) -> String {
        self.home.clone().unwrap_or_else(|| self.remote.uri())
    }
}

/// Read the SFTP mounts out of `gio mount --list --detail`.
///
/// Each mount is a `Mount(n): <name> -> <root uri>` line, followed by indented
/// detail lines, one of which is `default_location=<uri>`. The name is
/// translated and ignored; everything is read from the URIs. Mounts of other
/// kinds, drives and volumes are skipped.
#[must_use]
pub fn parse_mount_list(output: &str) -> Vec<Mount> {
    let mut mounts = Vec::new();
    let mut current: Option<Mount> = None;

    for line in output.lines() {
        let line = line.trim();

        if line.starts_with("Mount(") {
            mounts.extend(current.take());
            current = line
                .rsplit_once(" -> ")
                .and_then(|(_, uri)| remote_from_uri(uri))
                .map(|remote| Mount { remote, home: None });
        } else if line.starts_with("Drive(") || line.starts_with("Volume(") {
            mounts.extend(current.take());
        } else if let (Some(mount), Some(location)) =
            (current.as_mut(), line.strip_prefix("default_location="))
        {
            // Only a location on the same server counts.
            if remote_from_uri(location).as_ref() == Some(&mount.remote) {
                mount.home = Some(location.to_string());
            }
        }
    }

    mounts.extend(current);
    mounts
}

/// The user and host of an `sftp://user@host/...` URI.
///
/// A URI without a user, or with a port, is not one this application created,
/// so it is not matched to a machine.
fn remote_from_uri(uri: &str) -> Option<Remote> {
    let url = url::Url::parse(uri.trim()).ok()?;
    if url.scheme() != "sftp" || url.port().is_some() || url.username().is_empty() {
        return None;
    }
    let user = percent_decode(url.username())?;
    Remote::new(&user, url.host_str()?).ok()
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The commands this module runs. Tests replace them with scripts.
#[derive(Debug, Clone)]
pub struct Tools {
    pub gio: Vec<OsString>,
    pub ssh: Vec<OsString>,
    pub files: Vec<OsString>,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            gio: vec!["gio".into()],
            ssh: vec!["ssh".into()],
            files: vec!["cosmic-files".into()],
        }
    }
}

fn command(program: &[OsString]) -> Command {
    let mut command = Command::new(&program[0]);
    command
        .args(&program[1..])
        // Nothing here may prompt: there is no terminal to answer on, and gio
        // aborts a question it cannot read an answer to.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

/// Run a command to completion, returning its stdout, or its last line of
/// stderr as the error.
async fn run(mut command: Command, timeout: Duration, what: &str) -> Result<String, String> {
    let output = match tokio::time::timeout(timeout, command.output()).await {
        Err(_) => return Err(format!("{what} timed out after {}s", timeout.as_secs())),
        Ok(Err(error)) => return Err(format!("{what} could not start: {error}")),
        Ok(Ok(output)) => output,
    };

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let reason = stderr
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("no error message");
    Err(format!("{what} failed: {reason}"))
}

/// Every SFTP mount GVfs has right now.
pub async fn list(tools: &Tools) -> Result<Vec<Mount>, String> {
    let mut gio = command(&tools.gio);
    gio.args(["mount", "--list", "--detail"]);
    run(gio, LIST_TIMEOUT, "Listing mounts")
        .await
        .map(|output| parse_mount_list(&output))
}

/// Mount a machine and report where its home directory is.
pub async fn mount(tools: &Tools, remote: &Remote) -> Result<Mount, String> {
    // Record the host key and prove the login works, where the failure can
    // still be explained. The WireGuard link already authenticated this exact
    // node, so accepting a first-seen key adds no exposure.
    let mut probe = command(&tools.ssh);
    probe
        .args(["-o", "BatchMode=yes"])
        .args(["-o", "StrictHostKeyChecking=accept-new"])
        .args(["-o", "ConnectTimeout=15"])
        .arg("--")
        .arg(remote.destination())
        .arg("true");
    run(
        probe,
        PROBE_TIMEOUT,
        &format!("Signing in to {}", remote.host),
    )
    .await?;

    let mut gio = command(&tools.gio);
    gio.arg("mount").arg(remote.uri());
    run(gio, MOUNT_TIMEOUT, &format!("Mounting {}", remote.host)).await?;

    let mounts = list(tools).await?;
    Ok(mounts
        .into_iter()
        .find(|mount| &mount.remote == remote)
        .unwrap_or_else(|| Mount {
            remote: remote.clone(),
            home: None,
        }))
}

/// Unmount a machine. Other applications with files open there lose them, so
/// this is only ever done at the user's request.
pub async fn unmount(tools: &Tools, remote: &Remote) -> Result<(), String> {
    let mut gio = command(&tools.gio);
    gio.args(["mount", "--unmount"]).arg(remote.uri());
    run(gio, MOUNT_TIMEOUT, &format!("Unmounting {}", remote.host))
        .await
        .map(|_| ())
}

/// Open a location in COSMIC Files, falling back to the desktop's default file
/// manager when COSMIC Files is not installed.
pub async fn open_in_files(tools: &Tools, location: &str) -> Result<(), String> {
    let mut files = command(&tools.files);
    files
        .arg(location)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // The file manager outlives this call. Dropping the handle without
        // killing it leaves tokio to reap it when it exits.
        .kill_on_drop(false);
    match files.spawn() {
        Ok(_child) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            open::that_detached(location).map_err(|e| format!("Could not open {location}: {e}"))
        }
        Err(error) => Err(format!("Could not open {location}: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "\
Drive(0): Samsung SSD 990 PRO 1TB
  Type: GProxyDrive (GProxyVolumeMonitorUDisks2)
  Volume(0): Data
    Type: GProxyVolume (GProxyVolumeMonitorUDisks2)
    Mount(0): Data -> file:///run/media/alex/Data
      Type: GProxyShadowMount (GProxyVolumeMonitorUDisks2)
Mount(0): alex on homelab.tail000000.ts.net -> sftp://alex@homelab.tail000000.ts.net/
  Type: GDaemonMount
  default_location=sftp://alex@homelab.tail000000.ts.net/home/alex
  can_unmount=1
Mount(1): root on 100.101.0.5 -> sftp://root@100.101.0.5/
  Type: GDaemonMount
  default_location=sftp://root@100.101.0.5/root
Mount(2): share on nas -> smb://nas/share/
  Type: GDaemonMount
Mount(3): anonymous -> sftp://files.example.com/
  Type: GDaemonMount
";

    #[test]
    fn sftp_mounts_are_read_with_their_home_directories() {
        let mounts = parse_mount_list(LISTING);
        assert_eq!(
            mounts,
            vec![
                Mount {
                    remote: Remote::new("alex", "homelab.tail000000.ts.net").unwrap(),
                    home: Some("sftp://alex@homelab.tail000000.ts.net/home/alex".into()),
                },
                Mount {
                    remote: Remote::new("root", "100.101.0.5").unwrap(),
                    home: Some("sftp://root@100.101.0.5/root".into()),
                },
            ]
        );
    }

    #[test]
    fn a_default_location_on_another_server_is_not_taken_as_home() {
        let listing = "Mount(0): a on h -> sftp://alex@h/\n  default_location=sftp://bob@elsewhere/home/bob\n";
        assert_eq!(parse_mount_list(listing)[0].home, None);
        assert_eq!(parse_mount_list(listing)[0].location(), "sftp://alex@h/");
    }

    #[test]
    fn detail_lines_after_a_skipped_mount_are_not_attributed_to_the_previous_one() {
        let listing = "\
Mount(0): a on h -> sftp://alex@h/
Mount(1): share -> smb://nas/share/
  default_location=sftp://alex@h/srv
";
        assert_eq!(parse_mount_list(listing)[0].home, None);
    }

    #[test]
    fn user_names_and_hosts_that_ssh_would_read_as_options_are_refused() {
        assert!(Remote::new("-oProxyCommand=evil", "homelab").is_err());
        assert!(Remote::new("alex", "-oProxyCommand=evil").is_err());
        assert!(Remote::new("alex smith", "homelab").is_err());
        assert!(Remote::new("alex", "home lab").is_err());
        assert!(Remote::new("", "homelab").is_err());
        assert!(Remote::new("alex", "fd7a:115c:a1e0::a:1").is_err());
    }

    #[test]
    fn hosts_are_normalised_so_mounts_match_the_machine() {
        let remote = Remote::new(" alex ", "HomeLab.tail000000.ts.net.").unwrap();
        assert_eq!(remote.uri(), "sftp://alex@homelab.tail000000.ts.net/");
    }

    // ---- driving the tools --------------------------------------------------

    /// Fake `gio`, `ssh` and file manager, as shell scripts run through `sh`
    /// (so no freshly written file is ever executed) that log their arguments.
    struct Fakes {
        dir: std::path::PathBuf,
    }

    impl Fakes {
        fn new(ssh_body: &str, gio_body: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "cosmic-tailscale-mounts-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let log = dir.join("log");
            for (name, body) in [("ssh", ssh_body), ("gio", gio_body), ("files", "exit 0")] {
                std::fs::write(
                    dir.join(name),
                    format!("echo \"{name} $*\" >> '{}'\n{body}\n", log.display()),
                )
                .unwrap();
            }
            Self { dir }
        }

        fn tools(&self) -> Tools {
            let script = |name: &str| vec!["sh".into(), self.dir.join(name).into_os_string()];
            Tools {
                gio: script("gio"),
                ssh: script("ssh"),
                files: script("files"),
            }
        }

        fn log(&self) -> String {
            std::fs::read_to_string(self.dir.join("log")).unwrap_or_default()
        }
    }

    impl Drop for Fakes {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn gio_that_lists(listing: &str) -> String {
        format!(
            "case \"$1 $2\" in\n  'mount --list') cat <<'EOF'\n{listing}EOF\n;;\n  *) exit 0 ;;\nesac"
        )
    }

    #[tokio::test]
    async fn mounting_probes_then_mounts_then_reports_the_home_directory() {
        let fakes = Fakes::new("exit 0", &gio_that_lists(LISTING));
        let remote = Remote::new("alex", "homelab.tail000000.ts.net").unwrap();

        let mount = mount(&fakes.tools(), &remote).await.unwrap();

        assert_eq!(
            mount.location(),
            "sftp://alex@homelab.tail000000.ts.net/home/alex"
        );
        let log = fakes.log();
        let calls: Vec<&str> = log.lines().collect();
        assert_eq!(
            calls,
            vec![
                "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=15 -- alex@homelab.tail000000.ts.net true",
                "gio mount sftp://alex@homelab.tail000000.ts.net/",
                "gio mount --list --detail",
            ]
        );
    }

    #[tokio::test]
    async fn a_refused_login_stops_before_gvfs_and_says_why() {
        let fakes = Fakes::new(
            "echo 'Warning: Permanently added host' >&2; echo 'alex@homelab: Permission denied (publickey).' >&2; exit 255",
            "exit 0",
        );
        let remote = Remote::new("alex", "homelab").unwrap();

        let error = mount(&fakes.tools(), &remote).await.unwrap_err();

        assert!(error.contains("Permission denied (publickey)"), "{error}");
        assert!(!fakes.log().contains("gio"), "gio ran after a failed login");
    }

    #[tokio::test]
    async fn a_failed_mount_reports_gio_s_reason() {
        let fakes = Fakes::new(
            "exit 0",
            "echo 'gio: sftp://alex@homelab/: Connection refused' >&2; exit 2",
        );
        let remote = Remote::new("alex", "homelab").unwrap();

        let error = mount(&fakes.tools(), &remote).await.unwrap_err();
        assert!(error.starts_with("Mounting homelab failed"), "{error}");
        assert!(error.contains("Connection refused"), "{error}");
    }

    #[tokio::test]
    async fn unmounting_names_the_mount_root() {
        let fakes = Fakes::new("exit 0", "exit 0");
        let remote = Remote::new("alex", "homelab").unwrap();

        unmount(&fakes.tools(), &remote).await.unwrap();
        assert_eq!(
            fakes.log().trim(),
            "gio mount --unmount sftp://alex@homelab/"
        );
    }

    #[tokio::test]
    async fn the_file_manager_is_given_the_location() {
        let fakes = Fakes::new("exit 0", "exit 0");
        open_in_files(&fakes.tools(), "sftp://alex@homelab/home/alex")
            .await
            .unwrap();

        // The file manager is detached, so wait briefly for its log line.
        for _ in 0..50 {
            if !fakes.log().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(fakes.log().trim(), "files sftp://alex@homelab/home/alex");
    }
}
