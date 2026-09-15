//! Hardware health in the panel.
//!
//! The applet reads the same Beszel hub as the main window, using the hub
//! address and account the window stored in cosmic-config and the password it
//! stored in the keyring. The applet never prompts for credentials of its own:
//! if monitoring has not been set up in the window, the panel simply does not
//! show it.

use std::sync::Arc;

use beszel_client::{
    AlertEvent, AlertHistoryRecord, AlertKind, AlertTracker, BeszelHub, SystemRecord,
};
use cosmic::cosmic_config::ConfigGet as _;

use super::message::Message;

/// The same service name the main window writes under.
const KEYRING_SERVICE: &str = "io.github.leechristophermurray.CosmicTailscale";
/// The main window's config, which the applet only reads.
const CONFIG_ID: &str = "io.github.leechristophermurray.CosmicTailscale";
const CONFIG_VERSION: u64 = 1;
/// Where the window saved both before its app ID changed. Read as a fallback
/// until the window has run once and carried them over.
const LEGACY_ID: &str = "com.system76.CosmicTailscale";

/// How many recent alert firings to read each poll. Far more than change in a
/// minute, so nothing is missed between polls.
const ALERT_HISTORY_LIMIT: u32 = 50;

/// One read of the hub.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub systems: Vec<SystemRecord>,
    /// The user's recent alert firings, or `None` when they could not be read —
    /// a hub older than 0.12 has none. Machines are still shown either way.
    pub alerts: Option<Vec<AlertHistoryRecord>>,
    /// Which hub and account this came from.
    pub hub: String,
    /// Whether the user wants desktop notifications for alerts.
    pub notify: bool,
}

/// What the applet knows about the hub.
#[derive(Default)]
pub struct MonitoringState {
    /// Machines the hub is monitoring.
    pub systems: Vec<SystemRecord>,
    /// The machine whose metric is pinned to the panel, by Beszel record id.
    pub pinned: Option<String>,
    /// True once a hub has been configured in the main window.
    pub configured: bool,
    /// Which alert firings have already been seen.
    tracker: AlertTracker,
    /// The hub and account the tracker describes.
    tracker_hub: String,
}

impl MonitoringState {
    /// Alerts that fired or cleared since the last read.
    ///
    /// Pointing the main window at another hub or account starts afresh, so
    /// its existing alerts are not announced as new.
    pub fn alert_events(&mut self, snapshot: &Snapshot) -> Vec<AlertEvent> {
        if self.tracker_hub != snapshot.hub {
            self.tracker = AlertTracker::default();
            self.tracker_hub.clone_from(&snapshot.hub);
        }
        snapshot
            .alerts
            .as_deref()
            .map(|alerts| self.tracker.observe(alerts))
            .unwrap_or_default()
    }

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

/// The hub address and account the main window saved, and whether alert
/// notifications are muted.
fn stored_config() -> Option<(String, String, bool)> {
    [CONFIG_ID, LEGACY_ID].into_iter().find_map(|id| {
        let config = cosmic::cosmic_config::Config::new(id, CONFIG_VERSION).ok()?;

        let url: String = config.get("beszel_url").ok()?;
        let user: String = config.get("beszel_user").ok()?;
        // Absent until the switch is first touched: notifications are on.
        let muted: bool = config.get("beszel_alerts_muted").unwrap_or(false);

        if url.trim().is_empty() || user.trim().is_empty() {
            return None;
        }

        Some((url, user, muted))
    })
}

/// Read the hub, using whatever the main window configured.
///
/// Every failure here is silent by design: a panel applet that pops errors
/// about a monitoring hub the user may not have set up is worse than one that
/// simply shows nothing.
pub fn poll() -> cosmic::app::Task<Message> {
    cosmic::task::future(async move {
        let Some((url, user, muted)) = stored_config() else {
            return Message::MonitoringLoaded(None);
        };

        let lookup_url = url.clone();
        let lookup_user = user.clone();
        let password = tokio::task::spawn_blocking(move || {
            let account = format!("{lookup_user}@{lookup_url}");
            [KEYRING_SERVICE, LEGACY_ID]
                .into_iter()
                .find_map(|service| {
                    keyring::Entry::new(service, &account)
                        .and_then(|entry| entry.get_password())
                        .ok()
                })
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
            Ok(systems) => {
                let alerts = match hub.alert_history(ALERT_HISTORY_LIMIT).await {
                    Ok(alerts) => Some(alerts),
                    Err(error) => {
                        tracing::debug!(%error, "could not read the alert history");
                        None
                    }
                };
                Message::MonitoringLoaded(Some(Arc::new(Snapshot {
                    systems,
                    alerts,
                    hub: format!("{user}@{url}"),
                    notify: !muted,
                })))
            }
            Err(error) => {
                tracing::debug!(%error, "could not read the monitoring hub");
                Message::MonitoringLoaded(None)
            }
        }
    })
}

/// A desktop notification for an alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertNotification {
    pub summary: String,
    pub body: String,
    /// A machine that stopped reporting is worth interrupting for.
    pub urgent: bool,
}

/// Word an alert event for a notification.
///
/// The hub records the rule's threshold, not the reading, and not which disk or
/// sensor crossed it, so the wording claims no more than that.
#[must_use]
pub fn alert_notification(event: &AlertEvent, systems: &[SystemRecord]) -> AlertNotification {
    let (record, fired) = match event {
        AlertEvent::Fired(record) => (record, true),
        AlertEvent::Resolved(record) => (record, false),
    };

    let machine = record
        .system_name()
        .map(str::to_string)
        .or_else(|| {
            systems
                .iter()
                .find(|system| system.id == record.system)
                .map(|system| system.name.clone())
        })
        .unwrap_or_else(|| crate::fl!("alert-unknown-machine"));

    let kind = record.kind();
    let threshold = format_threshold(record.value, &kind);
    let label = match &kind {
        AlertKind::Status => String::new(),
        AlertKind::Cpu => crate::fl!("alert-label-cpu"),
        AlertKind::Memory => crate::fl!("alert-label-memory"),
        AlertKind::Disk => crate::fl!("alert-label-disk"),
        AlertKind::Temperature => crate::fl!("alert-label-temperature"),
        AlertKind::Bandwidth => crate::fl!("alert-label-bandwidth"),
        AlertKind::Gpu => crate::fl!("alert-label-gpu"),
        AlertKind::LoadAverage(minutes) => crate::fl!("alert-label-load", minutes = minutes),
        AlertKind::Battery => crate::fl!("alert-label-battery"),
        AlertKind::Other(name) => name.clone(),
    };

    let body = match (&kind, fired) {
        (AlertKind::Status, true) => crate::fl!("alert-status-down", machine = machine.as_str()),
        (AlertKind::Status, false) => crate::fl!("alert-status-up", machine = machine.as_str()),
        (AlertKind::Battery, true) => {
            crate::fl!("alert-below", label = label, threshold = threshold)
        }
        (AlertKind::Battery, false) => {
            crate::fl!("alert-back-above", label = label, threshold = threshold)
        }
        (_, true) => crate::fl!("alert-above", label = label, threshold = threshold),
        (_, false) => crate::fl!("alert-back-below", label = label, threshold = threshold),
    };

    AlertNotification {
        summary: if fired {
            crate::fl!("alert-fired-summary", machine = machine.as_str())
        } else {
            crate::fl!("alert-resolved-summary", machine = machine.as_str())
        },
        body,
        urgent: fired && kind == AlertKind::Status,
    }
}

fn format_threshold(value: f64, kind: &AlertKind) -> String {
    let number = if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    format!("{number}{}", kind.unit())
}

/// Show alert notifications.
///
/// More than a few at once — a hub coming back after an outage, say — become
/// one notification listing them, rather than a stack the user has to dismiss.
pub fn notify(notifications: Vec<AlertNotification>) -> cosmic::app::Task<Message> {
    const MAX_SEPARATE: usize = 3;

    if notifications.is_empty() {
        return cosmic::app::Task::none();
    }

    let batch: Vec<AlertNotification> = if notifications.len() > MAX_SEPARATE {
        vec![AlertNotification {
            summary: crate::fl!("alert-many-summary", count = notifications.len()),
            body: notifications
                .iter()
                .map(|n| format!("{} — {}", n.summary, n.body))
                .collect::<Vec<_>>()
                .join("\n"),
            urgent: notifications.iter().any(|n| n.urgent),
        }]
    } else {
        notifications
    };

    cosmic::task::future(async move {
        let _ = tokio::task::spawn_blocking(move || {
            for notification in batch {
                let result = notify_rust::Notification::new()
                    .appname("Tailscale")
                    .summary(&notification.summary)
                    .body(&notification.body)
                    .icon("io.github.leechristophermurray.CosmicTailscale-symbolic")
                    .hint(notify_rust::Hint::DesktopEntry(
                        "io.github.leechristophermurray.CosmicTailscale".to_string(),
                    ))
                    .urgency(if notification.urgent {
                        notify_rust::Urgency::Critical
                    } else {
                        notify_rust::Urgency::Normal
                    })
                    .show();
                if let Err(error) = result {
                    tracing::debug!(%error, "could not show an alert notification");
                }
            }
        })
        .await;
        Message::Noop
    })
}
