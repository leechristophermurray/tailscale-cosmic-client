//! Beszel monitoring state and the tasks that feed it.
//!
//! The hub is a separate service from tailscaled, reachable over the tailnet,
//! so everything here degrades independently: the client is fully usable with
//! no hub configured, an unreachable hub, or expired credentials.

use std::collections::HashMap;
use std::sync::Arc;

use beszel_client::{
    AgentInstall, BeszelHub, ContainerStats, InstallOutcome, Stats, StatsPeriod, StatsRecord,
    SystemRecord,
};
use cosmic::app::Task;
use tailscale_localapi::PeerStatus;

use super::message::{Failure, Message};

/// How the client stands with the hub.
#[derive(Debug, Clone, Default)]
pub enum HubConnection {
    /// No hub address configured.
    #[default]
    Unconfigured,
    Connecting,
    Connected,
    /// Reachable, but the credentials were refused or have expired.
    NeedsSignIn,
    /// Could not be reached at all.
    Unreachable(String),
    /// Reached, but something else went wrong.
    Failed(String),
}

impl HubConnection {
    #[must_use]
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }

    #[must_use]
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Connecting)
    }
}

/// An install the user has been asked to confirm, and not yet agreed to.
#[derive(Debug, Clone)]
pub struct PendingInstall {
    /// Stable node ID of the target peer.
    pub peer_id: String,
    pub host: String,
    /// The exact command that will run as root, shown before anything happens.
    pub command: String,
}

#[derive(Default)]
pub struct BeszelState {
    pub connection: HubConnection,
    /// Every machine the hub monitors.
    pub systems: Vec<SystemRecord>,
    /// Detailed stats, by Beszel system id. Only fetched for the selected one.
    pub stats: HashMap<String, Stats>,
    pub containers: HashMap<String, Vec<ContainerStats>>,
    /// Beszel system id currently shown in detail.
    pub selected: Option<String>,
    /// History for the selected machine, oldest first.
    pub history: Vec<StatsRecord>,
    /// Which resolution the charts are showing.
    pub period: StatsPeriod,

    /// The session token from the last successful sign-in.
    ///
    /// Reusing this is what keeps a background refresh from sending the
    /// password again every minute. PocketBase tokens expire, so a rejected
    /// token triggers one fresh sign-in rather than an error.
    pub token: Option<String>,

    /// The hub's public key, needed to enrol a new agent.
    pub hub_key: String,
    pub hub_version: String,

    // ---- the sign-in form -------------------------------------------------
    pub url_input: String,
    pub user_input: String,
    pub password_input: String,
    /// True once a password has been found in the keyring, so the form can say
    /// so rather than showing an empty box that looks unconfigured.
    pub password_stored: bool,

    // ---- agent deployment -------------------------------------------------
    /// Awaiting explicit confirmation. Never more than one at a time: this runs
    /// a root shell command on someone's machine, and a queue makes it too easy
    /// to agree to more than you read.
    pub pending_install: Option<PendingInstall>,
    /// Stable node IDs with an install running.
    pub installing: HashMap<String, ()>,
    /// The transcript of the last install, kept so a failure can be read.
    pub last_install: Option<(String, InstallOutcome)>,

    /// Names already warned about, so a threshold alert fires once rather than
    /// on every poll.
    pub warned: std::collections::HashSet<String>,
}

/// The resolutions offered in the period picker, coarsest window last.
pub const PERIODS: [StatsPeriod; 5] = [
    StatsPeriod::OneMinute,
    StatsPeriod::TenMinutes,
    StatsPeriod::TwentyMinutes,
    StatsPeriod::TwoHours,
    StatsPeriod::EightHours,
];

/// How many samples to plot. Enough to show a shape, few enough that the hub is
/// not asked for a month of rows to draw a chart 300 pixels wide.
const HISTORY_POINTS: u32 = 60;

impl BeszelState {
    /// The Beszel record matching a tailnet peer, if the hub monitors it.
    #[must_use]
    pub fn system_for(&self, peer: &PeerStatus) -> Option<&SystemRecord> {
        self.systems.iter().find(|system| {
            system.matches_peer(peer.display_name(), peer.magic_dns(), &peer.tailscale_ips)
        })
    }

    /// Tailnet peers the hub is not monitoring, and which could be.
    ///
    /// Only machines that accept Tailscale SSH are candidates, because that is
    /// how the agent gets installed. Phones and tablets are quietly excluded —
    /// they cannot run the agent, so listing them as "unmonitored" would be
    /// nagging about something impossible.
    #[must_use]
    pub fn unmonitored<'a>(&self, peers: &[&'a PeerStatus]) -> Vec<&'a PeerStatus> {
        peers
            .iter()
            .filter(|peer| peer.online && peer.supports_ssh())
            .filter(|peer| self.system_for(peer).is_none())
            .copied()
            .collect()
    }

    #[must_use]
    pub fn selected_system(&self) -> Option<&SystemRecord> {
        let id = self.selected.as_ref()?;
        self.systems.iter().find(|system| &system.id == id)
    }

    #[must_use]
    pub fn selected_stats(&self) -> Option<&Stats> {
        self.stats.get(self.selected.as_ref()?)
    }

    #[must_use]
    pub fn selected_containers(&self) -> &[ContainerStats] {
        self.selected
            .as_ref()
            .and_then(|id| self.containers.get(id))
            .map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn is_installing(&self, peer: &PeerStatus) -> bool {
        self.installing.contains_key(&peer.id)
    }
}

/// Build a hub client from the current configuration.
fn hub(url: &str, token: Option<&str>) -> Result<BeszelHub, Failure> {
    let hub = BeszelHub::new(url).map_err(|error| Failure {
        message: error.to_string(),
        unreachable: error.is_unreachable(),
    })?;

    Ok(match token {
        Some(token) => hub.with_token(token),
        None => hub,
    })
}

/// Sign in, then read the hub's systems.
///
/// `token` is the session from a previous sign-in. When it is still valid this
/// sends no password at all; when the hub rejects it, the password is used once
/// to get a new one.
pub fn connect(
    url: String,
    user: String,
    password: String,
    token: Option<String>,
) -> Task<Message> {
    cosmic::task::future(async move {
        match connect_inner(&url, &user, &password, token.as_deref()).await {
            Ok(session) => Message::BeszelConnected(Ok(Arc::new(session))),
            Err(failure) => Message::BeszelConnected(Err(failure)),
        }
    })
}

async fn connect_inner(
    url: &str,
    user: &str,
    password: &str,
    token: Option<&str>,
) -> Result<Session, Failure> {
    // Try the existing session first.
    if let Some(token) = token {
        let hub = hub(url, Some(token))?;
        if let Ok(systems) = hub.systems().await {
            let info = hub.info().await.ok();
            return Ok(Session {
                token: token.to_string(),
                systems,
                hub_key: info.as_ref().map(|i| i.key.clone()).unwrap_or_default(),
                hub_version: info.map(|i| i.version).unwrap_or_default(),
            });
        }
    }

    let mut hub = hub(url, None)?;

    // Check the address before the credentials, so a typo in the URL does not
    // present as a rejected password.
    hub.health().await.map_err(as_failure)?;

    let auth = hub.authenticate(user, password).await.map_err(as_failure)?;
    let info = hub.info().await.ok();
    let systems = hub.systems().await.map_err(as_failure)?;

    Ok(Session {
        token: auth.token,
        systems,
        hub_key: info.as_ref().map(|i| i.key.clone()).unwrap_or_default(),
        hub_version: info.map(|i| i.version).unwrap_or_default(),
    })
}

fn as_failure(error: beszel_client::Error) -> Failure {
    Failure {
        unreachable: error.is_unreachable(),
        message: if error.needs_auth() {
            "credentials rejected".to_string()
        } else {
            error.to_string()
        },
    }
}

/// What a successful connection yields.
#[derive(Debug, Clone)]
pub struct Session {
    pub token: String,
    pub systems: Vec<SystemRecord>,
    pub hub_key: String,
    pub hub_version: String,
}

/// Read one machine's stats, history and containers, using the live session.
pub fn load_detail(
    url: String,
    token: String,
    system_id: String,
    period: StatsPeriod,
) -> Task<Message> {
    cosmic::task::future(async move {
        let hub = match hub(&url, Some(&token)) {
            Ok(hub) => hub,
            Err(failure) => return Message::BeszelDetailLoaded(system_id, Err(failure)),
        };

        // One request covers both the charts and the current figures: the
        // newest row of the series is the latest sample, so asking for it
        // separately would be a second round trip for data already in hand.
        let mut history = match hub.stats(&system_id, period, HISTORY_POINTS).await {
            Ok(history) => history,
            // An expired token here is not worth surfacing: the next hub
            // refresh renews it and this detail is re-read.
            Err(error) => return Message::BeszelDetailLoaded(system_id, Err(as_failure(error))),
        };

        let stats = history.first().map(|record| record.stats.clone());

        // The hub returns newest first; charts read left to right.
        history.reverse();

        let containers = hub.containers(&system_id).await.unwrap_or_default();

        Message::BeszelDetailLoaded(
            system_id,
            Ok(Arc::new(Detail {
                stats,
                containers,
                history,
            })),
        )
    })
}

#[derive(Debug, Clone)]
pub struct Detail {
    pub stats: Option<Stats>,
    pub containers: Vec<ContainerStats>,
    /// Oldest first, ready to plot.
    pub history: Vec<StatsRecord>,
}

/// Run an agent installation that the user has confirmed.
pub fn install_agent(peer_id: String, host: String, hub_key: String) -> Task<Message> {
    cosmic::task::future(async move {
        let outcome = AgentInstall::new(host, hub_key).run().await;
        Message::BeszelAgentInstalled(peer_id, Arc::new(outcome))
    })
}

/// Read the stored password for a hub account, off the UI thread.
pub fn load_password(url: String, user: String) -> Task<Message> {
    cosmic::task::future(async move {
        let result = tokio::task::spawn_blocking(move || super::secrets::load(&url, &user))
            .await
            .unwrap_or_else(|error| Err(format!("keyring lookup panicked: {error}")));

        Message::BeszelPasswordLoaded(result.unwrap_or_else(|error| {
            tracing::warn!(%error, "could not read the keyring");
            None
        }))
    })
}

/// Save a password to the keyring, off the UI thread.
pub fn store_password(url: String, user: String, password: String) -> Task<Message> {
    cosmic::task::future(async move {
        let result =
            tokio::task::spawn_blocking(move || super::secrets::store(&url, &user, &password))
                .await
                .unwrap_or_else(|error| Err(format!("keyring write panicked: {error}")));

        Message::BeszelPasswordStored(result.err())
    })
}

/// Remove a stored password, off the UI thread.
pub fn forget_password(url: String, user: String) -> Task<Message> {
    cosmic::task::future(async move {
        let _ = tokio::task::spawn_blocking(move || super::secrets::forget(&url, &user)).await;
        Message::BeszelPasswordStored(None)
    })
}
