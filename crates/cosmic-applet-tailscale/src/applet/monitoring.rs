//! Hardware health in the panel.
//!
//! The applet reads the same Beszel hub as the main window, using the hub
//! address and account the window stored in cosmic-config and the password it
//! stored in the keyring. The applet never prompts for credentials of its own:
//! if monitoring has not been set up in the window, the panel simply does not
//! show it.

use std::sync::Arc;

use beszel_client::{BeszelHub, SystemRecord};
use cosmic::cosmic_config::ConfigGet as _;

use super::message::Message;

/// The same service name the main window writes under.
const KEYRING_SERVICE: &str = "com.system76.CosmicTailscale";
/// The main window's config, which the applet only reads.
const CONFIG_ID: &str = "com.system76.CosmicTailscale";
const CONFIG_VERSION: u64 = 1;

/// What the applet knows about the hub.
#[derive(Default)]
pub struct MonitoringState {
    /// Machines the hub is monitoring.
    pub systems: Vec<SystemRecord>,
    /// The machine whose metric is pinned to the panel, by Beszel record id.
    pub pinned: Option<String>,
    /// True once a hub has been configured in the main window.
    pub configured: bool,
}

impl MonitoringState {
    /// The system pinned to the panel, if it is still being monitored.
    #[must_use]
    pub fn pinned_system(&self) -> Option<&SystemRecord> {
        let id = self.pinned.as_ref()?;
        self.systems.iter().find(|system| &system.id == id)
    }

    /// A machine's metrics, matched to a tailnet peer.
    #[must_use]
    pub fn system_for(&self, peer: &tailscale_localapi::PeerStatus) -> Option<&SystemRecord> {
        self.systems.iter().find(|system| {
            system.matches_peer(peer.display_name(), peer.magic_dns(), &peer.tailscale_ips)
        })
    }

    /// Machines the hub says are in trouble, for the flyout's summary line.
    #[must_use]
    pub fn unhealthy(&self) -> Vec<&SystemRecord> {
        self.systems
            .iter()
            .filter(|system| {
                !system.status.is_up()
                    || system.info.disk_pct >= 90.0
                    || system.info.memory_pct >= 92.0
                    || system.info.failed_services().is_some_and(|n| n > 0)
            })
            .collect()
    }
}

/// The hub address and account the main window saved.
fn stored_config() -> Option<(String, String)> {
    let config = cosmic::cosmic_config::Config::new(CONFIG_ID, CONFIG_VERSION).ok()?;

    let url: String = config.get("beszel_url").ok()?;
    let user: String = config.get("beszel_user").ok()?;

    if url.trim().is_empty() || user.trim().is_empty() {
        return None;
    }

    Some((url, user))
}

/// Read the hub, using whatever the main window configured.
///
/// Every failure here is silent by design: a panel applet that pops errors
/// about a monitoring hub the user may not have set up is worse than one that
/// simply shows nothing.
pub fn poll() -> cosmic::app::Task<Message> {
    cosmic::task::future(async move {
        let Some((url, user)) = stored_config() else {
            return Message::MonitoringLoaded(None);
        };

        let lookup_url = url.clone();
        let lookup_user = user.clone();
        let password = tokio::task::spawn_blocking(move || {
            keyring::Entry::new(KEYRING_SERVICE, &format!("{lookup_user}@{lookup_url}"))
                .and_then(|entry| entry.get_password())
                .ok()
        })
        .await
        .ok()
        .flatten();

        let Some(password) = password else {
            return Message::MonitoringLoaded(None);
        };

        let Ok(mut hub) = BeszelHub::new(&url) else {
            return Message::MonitoringLoaded(None);
        };

        if hub.authenticate(&user, &password).await.is_err() {
            return Message::MonitoringLoaded(None);
        }

        match hub.systems().await {
            Ok(systems) => Message::MonitoringLoaded(Some(Arc::new(systems))),
            Err(error) => {
                tracing::debug!(%error, "could not read the monitoring hub");
                Message::MonitoringLoaded(None)
            }
        }
    })
}
