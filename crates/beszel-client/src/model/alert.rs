//! Alerts the hub has fired.
//!
//! A user sets alert rules in the hub (`alerts`: this machine, this metric,
//! this threshold). Each time a rule starts firing the hub writes a row to
//! `alerts_history`, and when the condition clears it stamps that row's
//! `resolved` time. The history is therefore the one place a client can see
//! both "fired" and "cleared", and it is what this reads.
//!
//! Two things the history does not hold, per Beszel's source: the value that
//! was measured (`value` is the rule's threshold) and which filesystem or sensor
//! crossed it. Notifications are worded accordingly.

use serde::{Deserialize, Deserializer};

use super::null_as_default;

/// One firing of an alert rule. Newest first when listed.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct AlertHistoryRecord {
    pub id: String,
    /// The monitored system's record id.
    pub system: String,
    /// The `alerts` rule that fired.
    pub alert_id: String,
    /// The rule's metric name, e.g. `Disk` or `LoadAvg5`. Free text in this
    /// collection, and the set has grown between releases.
    pub name: String,
    /// The rule's threshold, not the measured value.
    #[serde(deserialize_with = "null_as_default")]
    pub value: f64,
    pub created: String,
    /// When the condition cleared; `None` while the alert is still firing.
    #[serde(deserialize_with = "empty_as_none")]
    pub resolved: Option<String>,
    expand: Expand,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
struct Expand {
    system: Option<ExpandedSystem>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(default)]
struct ExpandedSystem {
    name: String,
}

/// PocketBase writes an unset date as `""`, and a missing field is the same.
fn empty_as_none<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|v| !v.trim().is_empty()))
}

impl AlertHistoryRecord {
    /// Still firing.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.resolved.is_none()
    }

    /// The system's name, when the hub expanded it into the record. The hub
    /// leaves it out when the user can no longer see that system.
    #[must_use]
    pub fn system_name(&self) -> Option<&str> {
        self.expand
            .system
            .as_ref()
            .map(|system| system.name.as_str())
            .filter(|name| !name.is_empty())
    }

    #[must_use]
    pub fn kind(&self) -> AlertKind {
        AlertKind::from_name(&self.name)
    }
}

/// What an alert measures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertKind {
    /// The machine stopped reporting.
    Status,
    Cpu,
    Memory,
    Disk,
    Temperature,
    Bandwidth,
    Gpu,
    /// Load average over 1, 5 or 15 minutes.
    LoadAverage(u8),
    /// The only alert that fires below its threshold.
    Battery,
    /// A metric added after this client was written.
    Other(String),
}

impl AlertKind {
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "Status" => Self::Status,
            "CPU" => Self::Cpu,
            "Memory" => Self::Memory,
            "Disk" => Self::Disk,
            "Temperature" => Self::Temperature,
            "Bandwidth" => Self::Bandwidth,
            "GPU" => Self::Gpu,
            "LoadAvg1" => Self::LoadAverage(1),
            "LoadAvg5" => Self::LoadAverage(5),
            "LoadAvg15" => Self::LoadAverage(15),
            "Battery" => Self::Battery,
            other => Self::Other(other.to_string()),
        }
    }

    /// The unit the hub's threshold is in, per Beszel's alert code.
    #[must_use]
    pub fn unit(&self) -> &'static str {
        match self {
            Self::Cpu | Self::Memory | Self::Disk | Self::Gpu | Self::Battery => "%",
            Self::Temperature => "°C",
            Self::Bandwidth => " MB/s",
            Self::Status | Self::LoadAverage(_) | Self::Other(_) => "",
        }
    }
}

/// A change worth telling the user about.
#[derive(Debug, Clone, PartialEq)]
pub enum AlertEvent {
    Fired(AlertHistoryRecord),
    Resolved(AlertHistoryRecord),
}

/// Turns successive reads of the alert history into fired and resolved events.
///
/// The first read only establishes what is already known: alerts that were
/// firing before the client started are not news, and announcing them at every
/// login would train the user to ignore the notifications.
#[derive(Debug, Default)]
pub struct AlertTracker {
    /// History id → whether it was active when last seen.
    seen: std::collections::HashMap<String, bool>,
    primed: bool,
}

impl AlertTracker {
    /// Compare a fresh read with the last one. `records` is the hub's most
    /// recent history, newest first; events come back oldest first.
    pub fn observe(&mut self, records: &[AlertHistoryRecord]) -> Vec<AlertEvent> {
        let mut events = Vec::new();

        if self.primed {
            for record in records.iter().rev() {
                match (self.seen.get(&record.id), record.is_active()) {
                    // New and firing.
                    (None, true) => events.push(AlertEvent::Fired(record.clone())),
                    // Fired and cleared between two reads, or was firing and
                    // has now cleared: either way, what is new is that it is over.
                    (None | Some(true), false) => {
                        events.push(AlertEvent::Resolved(record.clone()));
                    }
                    (Some(_), _) => {}
                }
            }
        }

        // Only what the hub still returns is remembered, so this stays the size
        // of one read.
        self.seen = records
            .iter()
            .map(|record| (record.id.clone(), record.is_active()))
            .collect();
        self.primed = true;
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, name: &str, resolved: &str) -> AlertHistoryRecord {
        serde_json::from_value(serde_json::json!({
            "id": id, "system": "s1", "alert_id": "a1", "name": name, "value": 80,
            "created": "2026-09-15 08:12:03.200Z", "resolved": resolved,
            "expand": {"system": {"id": "s1", "name": "homelab"}}
        }))
        .unwrap()
    }

    #[test]
    fn a_history_row_decodes_with_the_system_name_and_an_empty_resolved_date() {
        let active = record("h1", "Disk", "");
        assert!(active.is_active());
        assert_eq!(active.system_name(), Some("homelab"));
        assert_eq!(active.kind(), AlertKind::Disk);
        assert!((active.value - 80.0).abs() < f64::EPSILON);

        let cleared = record("h2", "Disk", "2026-09-15 08:20:00.000Z");
        assert!(!cleared.is_active());
    }

    #[test]
    fn a_row_without_expand_or_with_nulls_still_decodes() {
        let row: AlertHistoryRecord = serde_json::from_str(
            r#"{"id":"h1","system":"s1","name":"Status","value":null,"resolved":null}"#,
        )
        .unwrap();
        assert!(row.is_active());
        assert_eq!(row.system_name(), None);
    }

    #[test]
    fn alert_names_beszel_adds_later_are_kept_rather_than_rejected() {
        assert_eq!(
            AlertKind::from_name("LoadAvg15"),
            AlertKind::LoadAverage(15)
        );
        assert_eq!(
            AlertKind::from_name("NetworkErrors"),
            AlertKind::Other("NetworkErrors".into())
        );
    }

    #[test]
    fn alerts_already_firing_at_startup_are_not_announced() {
        let mut tracker = AlertTracker::default();
        assert!(tracker.observe(&[record("h1", "Disk", "")]).is_empty());
        assert!(tracker.observe(&[record("h1", "Disk", "")]).is_empty());
    }

    #[test]
    fn a_new_alert_fires_once_and_resolves_once() {
        let mut tracker = AlertTracker::default();
        tracker.observe(&[]);

        let fired = tracker.observe(&[record("h1", "CPU", "")]);
        assert_eq!(fired, vec![AlertEvent::Fired(record("h1", "CPU", ""))]);
        assert!(tracker.observe(&[record("h1", "CPU", "")]).is_empty());

        let cleared = record("h1", "CPU", "2026-09-15 09:00:00.000Z");
        assert_eq!(
            tracker.observe(std::slice::from_ref(&cleared)),
            vec![AlertEvent::Resolved(cleared.clone())]
        );
        assert!(tracker.observe(&[cleared]).is_empty());
    }

    #[test]
    fn an_alert_already_firing_at_startup_is_still_announced_when_it_clears() {
        let mut tracker = AlertTracker::default();
        tracker.observe(&[record("h1", "Status", "")]);
        let events = tracker.observe(&[record("h1", "Status", "2026-09-15 09:00:00.000Z")]);
        assert!(matches!(events.as_slice(), [AlertEvent::Resolved(_)]));
    }

    #[test]
    fn an_alert_that_fired_and_cleared_between_reads_is_reported_as_resolved() {
        let mut tracker = AlertTracker::default();
        tracker.observe(&[]);
        let events = tracker.observe(&[record("h1", "Memory", "2026-09-15 09:00:00.000Z")]);
        assert!(matches!(events.as_slice(), [AlertEvent::Resolved(_)]));
    }

    #[test]
    fn events_come_oldest_first() {
        let mut tracker = AlertTracker::default();
        tracker.observe(&[]);
        // The hub lists newest first.
        let events = tracker.observe(&[record("h2", "Disk", ""), record("h1", "CPU", "")]);
        let names: Vec<&str> = events
            .iter()
            .map(|event| match event {
                AlertEvent::Fired(r) | AlertEvent::Resolved(r) => r.name.as_str(),
            })
            .collect();
        assert_eq!(names, vec!["CPU", "Disk"]);
    }
}
