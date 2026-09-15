//! The `container_stats` collection: per-container metrics for a machine.

use serde::Deserialize;

/// One container's metrics for an interval.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ContainerStats {
    #[serde(rename = "n")]
    pub name: String,
    /// CPU busy percentage.
    #[serde(rename = "c")]
    pub cpu: f64,
    /// Memory in MiB.
    #[serde(rename = "m")]
    pub memory: f64,
    /// `[sent, received]` bytes per second.
    #[serde(rename = "b", deserialize_with = "crate::model::null_as_default")]
    pub bandwidth: [u64; 2],
    /// The agent has seen a newer image than the one running.
    #[serde(rename = "u")]
    pub update_available: bool,
}

/// A `container_stats` record, which holds every container on one machine.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ContainerStatsRecord {
    pub id: String,
    pub system: String,
    #[serde(rename = "type")]
    pub period: String,
    pub created: String,
    #[serde(deserialize_with = "crate::model::null_as_default")]
    pub stats: Vec<ContainerStats>,
}
