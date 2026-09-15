//! The application's model: the daemon's last known state, plus the view state
//! the daemon does not own (selection, filter text, in-flight probes).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tailscale_localapi::{
    BackendState, FileTarget, PeerStatus, PingResult, Prefs, ServeConfig, Status, WaitingFile,
};

use crate::ui::Tone;

/// A snapshot of `tailscaled` plus everything the UI derives from it.
#[derive(Default)]
pub struct State {
    // ---- what the daemon told us ---------------------------------------
    pub status: Option<Arc<Status>>,
    pub prefs: Option<Arc<Prefs>>,
    pub serve: Arc<ServeConfig>,
    pub file_targets: Arc<Vec<FileTarget>>,
    pub waiting_files: Arc<Vec<WaitingFile>>,

    /// Backend state from the IPN bus, which lands sooner than the next poll.
    pub backend: BackendState,

    // ---- derived --------------------------------------------------------
    pub throughput: Throughput,

    // ---- view state -----------------------------------------------------
    /// Stable node ID of the machine shown in the detail pane.
    pub selected: Option<String>,
    pub filter: String,
    /// Latest probe per stable node ID.
    pub pings: HashMap<String, PingResult>,
    /// Stable node IDs with a probe in flight, so the button can show progress.
    pub pinging: HashMap<String, ()>,
    /// Files waiting for the user to pick a Taildrop target.
    pub pending_drop: Vec<std::path::PathBuf>,
    /// A drag is currently over the window, so the drop zone highlights.
    pub drag_over: bool,
    /// Stable node ID the Taildrop page will send to.
    pub taildrop_target: Option<String>,

    // ---- transient ------------------------------------------------------
    pub error: Option<String>,
    pub notice: Option<String>,
    /// Set while a prefs write is in flight, so toggles do not fight the user.
    pub writing_prefs: bool,
    /// True once we have failed to reach the daemon at all.
    pub daemon_unreachable: bool,
    pub suspended_until: Option<Instant>,

    /// Everything the Caddy page needs.
    pub caddy: super::caddy::CaddyState,
    /// Everything the Monitoring page needs.
    pub beszel: super::beszel::BeszelState,
    /// Settings that outlive the session.
    pub config: super::config::Config,

    /// Names already announced, so a repeated poll does not re-notify about
    /// the same transfer.
    pub announced_files: std::collections::HashSet<String>,
    /// Set once the key-expiry warning has been shown this session.
    pub announced_key_expiry: bool,

    // ---- remote files ---------------------------------------------------
    /// The SFTP locations GVfs has mounted, refreshed while the Machines or
    /// Monitoring page is open.
    pub mounts: Vec<super::mounts::Mount>,
    /// Stable node IDs with a mount or unmount in flight.
    pub mount_busy: std::collections::HashSet<String>,
    /// SSH user names typed on the Machines page, by stable node ID, before a
    /// successful mount saves them.
    pub mount_user_inputs: HashMap<String, String>,
}

impl State {
    /// This machine's own entry, as the daemon reports it.
    #[must_use]
    pub fn self_peer(&self) -> Option<&PeerStatus> {
        self.status.as_ref()?.self_status.as_ref()
    }

    #[must_use]
    pub fn tailnet_name(&self) -> &str {
        self.status
            .as_deref()
            .map_or("not connected", Status::tailnet_name)
    }

    /// The account label under the tailnet name, e.g. `alex@orca-cat.ts.net`.
    #[must_use]
    pub fn account_name(&self) -> &str {
        self.prefs
            .as_deref()
            .and_then(Prefs::user_profile)
            .map_or("", |p| p.login_name.as_str())
    }

    #[must_use]
    pub fn initials(&self) -> String {
        self.prefs
            .as_deref()
            .and_then(Prefs::user_profile)
            .map_or_else(
                || "?".to_string(),
                tailscale_localapi::UserProfile::initials,
            )
    }

    /// Whether the user wants the tunnel up. Reflects prefs rather than the
    /// backend state, so the master switch does not flicker while connecting.
    #[must_use]
    pub fn want_running(&self) -> bool {
        self.prefs.as_deref().is_some_and(|p| p.want_running)
    }

    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.backend.is_running()
    }

    /// Peers matching the current filter, in list order.
    #[must_use]
    pub fn filtered_peers(&self) -> Vec<&PeerStatus> {
        let Some(status) = self.status.as_deref() else {
            return Vec::new();
        };

        status
            .peers_sorted()
            .into_iter()
            .filter(|peer| peer.matches(&self.filter))
            .collect()
    }

    /// The machine the detail pane is showing. Falls back to this machine so
    /// the pane is never empty on a fresh launch.
    #[must_use]
    pub fn selected_peer(&self) -> Option<&PeerStatus> {
        let status = self.status.as_deref()?;

        if let Some(id) = &self.selected {
            if let Some(peer) = status.peer_by_id(id) {
                return Some(peer);
            }
            if status.self_status.as_ref().is_some_and(|s| &s.id == id) {
                return status.self_status.as_ref();
            }
        }

        status.self_status.as_ref()
    }

    /// The machine a Beszel system reports on, if it is on this tailnet.
    #[must_use]
    pub fn peer_for_system(&self, system: &beszel_client::SystemRecord) -> Option<&PeerStatus> {
        let status = self.status.as_deref()?;
        status
            .self_status
            .iter()
            .chain(status.peer.values())
            .find(|peer| {
                system.matches_peer(peer.display_name(), peer.magic_dns(), &peer.tailscale_ips)
            })
    }

    /// Whether offering to mount this machine makes sense: it is another
    /// machine, it is online, and it is not a phone or tablet, which do not run
    /// an SSH server. Whether SSH actually answers is found out by trying.
    #[must_use]
    pub fn can_mount(&self, peer: &PeerStatus) -> bool {
        peer.online
            && !self.is_self(peer)
            && !matches!(peer.os.to_ascii_lowercase().as_str(), "android" | "ios")
    }

    /// The host to mount a machine by: its MagicDNS name when this machine
    /// resolves MagicDNS, otherwise its Tailscale IPv4 address.
    #[must_use]
    pub fn mount_host(&self, peer: &PeerStatus) -> Option<String> {
        let magic_dns = self.prefs.as_deref().is_some_and(|prefs| prefs.corp_dns);
        if magic_dns && !peer.magic_dns().is_empty() {
            return Some(peer.magic_dns().to_ascii_lowercase());
        }
        peer.ipv4().map(str::to_string)
    }

    /// The SSH user to mount a machine as: what was typed, else what was last
    /// used for it, else the local user name.
    #[must_use]
    pub fn mount_user(&self, peer: &PeerStatus) -> String {
        self.mount_user_inputs
            .get(&peer.id)
            .or_else(|| self.config.mount_users.get(&peer.id))
            .cloned()
            .unwrap_or_else(local_user_name)
    }

    /// The mount for this machine, whichever of its names or users it was
    /// mounted under — by this app or from the file manager. One for the user
    /// named on the page is preferred.
    #[must_use]
    pub fn mount_for(&self, peer: &PeerStatus) -> Option<&super::mounts::Mount> {
        let dns = peer.magic_dns().to_ascii_lowercase();
        let user = self.mount_user(peer);
        let mut matches = self.mounts.iter().filter(|mount| {
            mount.remote.host == dns || peer.tailscale_ips.contains(&mount.remote.host)
        });
        let first = matches.next()?;
        if first.remote.user == user {
            return Some(first);
        }
        matches
            .find(|mount| mount.remote.user == user)
            .or(Some(first))
    }

    #[must_use]
    pub fn is_selected(&self, peer: &PeerStatus) -> bool {
        self.selected_peer().is_some_and(|s| s.id == peer.id)
    }

    /// True when this peer is the machine the client is running on.
    #[must_use]
    pub fn is_self(&self, peer: &PeerStatus) -> bool {
        self.self_peer().is_some_and(|s| s.id == peer.id)
    }

    /// Peers that advertise themselves as exit nodes, plus whichever one is
    /// currently active. Index 0 of the rendered dropdown is always "None".
    #[must_use]
    pub fn exit_node_options(&self) -> Vec<&PeerStatus> {
        self.status
            .as_deref()
            .map(Status::exit_node_options)
            .unwrap_or_default()
    }

    /// Dropdown index of the active exit node, offset by the "None" row.
    #[must_use]
    pub fn exit_node_index(&self) -> usize {
        let Some(prefs) = self.prefs.as_deref() else {
            return 0;
        };
        if prefs.exit_node_id.is_empty() {
            return 0;
        }

        self.exit_node_options()
            .iter()
            .position(|p| p.id == prefs.exit_node_id)
            .map_or(0, |index| index + 1)
    }

    /// Machines to surface as quick Taildrop/copy targets: online peers that
    /// can actually receive a transfer, most recently active first.
    #[must_use]
    pub fn quick_peers(&self, limit: usize) -> Vec<&PeerStatus> {
        let Some(status) = self.status.as_deref() else {
            return Vec::new();
        };

        let mut peers: Vec<&PeerStatus> = status
            .peer
            .values()
            .filter(|p| p.online && p.can_receive_files())
            .collect();

        peers.sort_by(|a, b| {
            b.last_handshake_at()
                .cmp(&a.last_handshake_at())
                .then_with(|| a.host_name.to_lowercase().cmp(&b.host_name.to_lowercase()))
        });
        peers.truncate(limit);
        peers
    }

    /// Daemon-reported health problems, which the UI shows as a warning banner.
    #[must_use]
    pub fn health_warnings(&self) -> &[String] {
        self.status.as_deref().map_or(&[], |s| s.health.as_slice())
    }

    /// Key expiry for this machine, as a countdown plus a tone for the meter.
    ///
    /// Tailnets can disable key expiry entirely, in which case the daemon sends
    /// no expiry at all — that is reported honestly rather than as a fake
    /// countdown.
    #[must_use]
    pub fn key_expiry(&self) -> KeyExpiry {
        let Some(peer) = self.self_peer() else {
            return KeyExpiry::Unknown;
        };

        let Some(expiry) = peer.key_expiry_at() else {
            return KeyExpiry::Disabled;
        };

        let days = (expiry - chrono::Utc::now()).num_days();
        KeyExpiry::Expires { expiry, days }
    }

    /// Peers that could plausibly host a Caddy instance we can manage: online,
    /// and accepting Tailscale SSH so a tunnel is possible if the admin API is
    /// not directly reachable.
    #[must_use]
    pub fn caddy_candidates(&self) -> Vec<&PeerStatus> {
        let Some(status) = self.status.as_deref() else {
            return Vec::new();
        };

        let mut peers: Vec<&PeerStatus> = status
            .peer
            .values()
            .filter(|peer| peer.online && peer.supports_ssh())
            .collect();
        peers.sort_by_key(|peer| peer.host_name.to_lowercase());
        peers
    }

    #[must_use]
    pub fn ping_for(&self, peer: &PeerStatus) -> Option<&PingResult> {
        self.pings.get(&peer.id)
    }

    #[must_use]
    pub fn is_pinging(&self, peer: &PeerStatus) -> bool {
        self.pinging.contains_key(&peer.id)
    }
}

/// The key-expiry state of this machine.
#[derive(Debug, Clone, Copy)]
pub enum KeyExpiry {
    Expires {
        expiry: chrono::DateTime<chrono::Utc>,
        days: i64,
    },
    /// The tailnet has key expiry turned off for this node.
    Disabled,
    /// We have not heard from the daemon yet.
    Unknown,
}

impl KeyExpiry {
    /// Fraction of the assumed 180-day key lifetime still remaining, for the
    /// sidebar meter.
    #[must_use]
    pub fn fraction_remaining(self) -> f32 {
        match self {
            Self::Expires { days, .. } => (days as f32 / 180.0).clamp(0.0, 1.0),
            Self::Disabled => 1.0,
            Self::Unknown => 0.0,
        }
    }

    #[must_use]
    pub fn tone(self) -> Tone {
        match self {
            Self::Expires { days, .. } if days < 0 => Tone::Critical,
            Self::Expires { days, .. } if days < 7 => Tone::Critical,
            Self::Expires { days, .. } if days < 30 => Tone::Caution,
            Self::Expires { .. } | Self::Disabled => Tone::Positive,
            Self::Unknown => Tone::Neutral,
        }
    }

    #[must_use]
    pub fn summary(self) -> String {
        match self {
            Self::Expires { days, .. } if days < 0 => "Expired".to_string(),
            Self::Expires { days, .. } => format!("{days} days"),
            Self::Disabled => "Does not expire".to_string(),
            Self::Unknown => "Unknown".to_string(),
        }
    }
}

/// Rolling throughput, derived from the cumulative counters on the IPN bus.
///
/// The daemon only ever sends totals, so a rate needs two samples. Until the
/// second one lands there is nothing honest to show.
#[derive(Debug, Default)]
pub struct Throughput {
    last: Option<(u64, u64, Instant)>,
    pub rx_per_second: f64,
    pub tx_per_second: f64,
    pub total_rx: u64,
    pub total_tx: u64,
    pub live_peers: u32,
    pub live_derps: u32,
}

impl Throughput {
    /// Fold in a fresh counter sample.
    pub fn sample(&mut self, rx: u64, tx: u64, live_peers: u32, live_derps: u32) {
        let now = Instant::now();
        self.live_peers = live_peers;
        self.live_derps = live_derps;

        if let Some((last_rx, last_tx, at)) = self.last {
            let elapsed = now.duration_since(at).as_secs_f64();
            // Ignore samples too close together to divide by, and counter
            // resets (a daemon restart) rather than reporting a negative rate.
            if elapsed > 0.2 && rx >= last_rx && tx >= last_tx {
                self.rx_per_second = (rx - last_rx) as f64 / elapsed;
                self.tx_per_second = (tx - last_tx) as f64 / elapsed;
            } else if rx < last_rx || tx < last_tx {
                self.rx_per_second = 0.0;
                self.tx_per_second = 0.0;
            }
        }

        self.total_rx = rx;
        self.total_tx = tx;
        self.last = Some((rx, tx, now));
    }

    /// True once a rate has actually been measured.
    #[must_use]
    pub fn has_rate(&self) -> bool {
        self.last.is_some() && (self.rx_per_second > 0.0 || self.tx_per_second > 0.0)
    }
}

/// The name of the user running this application, the usual SSH login.
#[must_use]
pub fn local_user_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_default()
}
