//! The `system_stats` collection: one record per machine per interval.

use serde::Deserialize;
use std::collections::BTreeMap;

/// How coarse a stats series is. Beszel keeps several resolutions and expires
/// the fine ones first, so a long window has to read a coarse series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatsPeriod {
    /// The default: the finest resolution, covering roughly the last hour.
    #[default]
    OneMinute,
    TenMinutes,
    TwentyMinutes,
    TwoHours,
    EightHours,
}

impl StatsPeriod {
    /// The value Beszel stores in the record's `type` field.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OneMinute => "1m",
            Self::TenMinutes => "10m",
            Self::TwentyMinutes => "20m",
            Self::TwoHours => "120m",
            Self::EightHours => "480m",
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::OneMinute => "Last hour",
            Self::TenMinutes => "Last 12 hours",
            Self::TwentyMinutes => "Last day",
            Self::TwoHours => "Last week",
            Self::EightHours => "Last month",
        }
    }
}

/// One stats record.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct StatsRecord {
    pub id: String,
    /// The `systems` record this belongs to.
    pub system: String,
    #[serde(rename = "type")]
    pub period: String,
    pub created: String,
    pub stats: Stats,
}

/// A machine's metrics for one interval.
///
/// Sizes are GiB and rates are bytes per second, both as Beszel stores them.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Stats {
    /// CPU busy percentage.
    pub cpu: f64,
    /// Total memory, GiB.
    #[serde(rename = "m")]
    pub memory_total: f64,
    /// Memory actually committed to processes, GiB.
    ///
    /// The agent has *already* excluded buffers, cache and the ZFS ARC from
    /// this figure and reports each separately, so this is the number to show
    /// as "used" — subtracting the others again double-counts them.
    #[serde(rename = "mu")]
    pub memory_used: f64,
    /// Percentage, which the agent computes as `memory_used / memory_total`.
    #[serde(rename = "mp")]
    pub memory_pct: f64,
    /// Buffers and cache, GiB. Reclaimable, and a sibling of `memory_used`
    /// rather than part of it.
    #[serde(rename = "mb")]
    pub memory_buff_cache: f64,
    /// ZFS ARC, GiB. Also reclaimable and also already excluded from
    /// `memory_used`.
    #[serde(rename = "mz")]
    pub memory_zfs_arc: f64,
    #[serde(rename = "s")]
    pub swap_total: f64,
    #[serde(rename = "su")]
    pub swap_used: f64,
    /// Root filesystem total, GiB.
    #[serde(rename = "d")]
    pub disk_total: f64,
    #[serde(rename = "du")]
    pub disk_used: f64,
    #[serde(rename = "dp")]
    pub disk_pct: f64,
    /// Sensor readings in Celsius, keyed by sensor name.
    #[serde(rename = "t")]
    pub temperatures: BTreeMap<String, f64>,
    /// Fan speeds in RPM, keyed by fan name.
    #[serde(rename = "f")]
    pub fans: BTreeMap<String, u16>,
    /// ZFS pools, keyed by pool name.
    #[serde(rename = "z")]
    pub zfs_pools: BTreeMap<String, ZfsPool>,
    /// `[sent, received]` bytes per second.
    #[serde(rename = "b", deserialize_with = "crate::model::null_as_default")]
    pub bandwidth: [u64; 2],
    /// `[read, write]` bytes per second.
    #[serde(rename = "dio", deserialize_with = "crate::model::null_as_default")]
    pub disk_io: [u64; 2],
    /// 1, 5 and 15 minute load averages.
    #[serde(rename = "la", deserialize_with = "crate::model::null_as_default")]
    pub load_average: [f64; 3],
}

impl Stats {
    /// Memory the kernel would hand back under pressure: buffers, cache and
    /// the ZFS ARC.
    ///
    /// Worth showing beside `memory_used` so a machine with a large ARC does
    /// not look idle, but it must never be added to it — the agent already
    /// counts these separately.
    #[must_use]
    pub fn memory_reclaimable(&self) -> f64 {
        self.memory_buff_cache + self.memory_zfs_arc
    }

    /// Everything not free, committed and reclaimable together.
    ///
    /// This is what `free -h` calls "used + buff/cache", and it is the figure
    /// to use when explaining where the RAM went — not when answering whether
    /// the machine is short of memory.
    #[must_use]
    pub fn memory_in_use(&self) -> f64 {
        self.memory_used + self.memory_reclaimable()
    }

    /// The hottest sensor, which is the one worth showing when there is room
    /// for only one number.
    #[must_use]
    pub fn peak_temperature(&self) -> Option<(&str, f64)> {
        self.temperatures
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(name, value)| (name.as_str(), *value))
    }

    /// Any ZFS pool that is not `ONLINE`.
    #[must_use]
    pub fn degraded_pools(&self) -> Vec<(&str, &str)> {
        self.zfs_pools
            .iter()
            .filter(|(_, pool)| !pool.is_healthy())
            .map(|(name, pool)| (name.as_str(), pool.health.as_str()))
            .collect()
    }

    #[must_use]
    pub fn bandwidth_sent(&self) -> u64 {
        self.bandwidth[0]
    }

    #[must_use]
    pub fn bandwidth_received(&self) -> u64 {
        self.bandwidth[1]
    }
}

/// One ZFS pool's capacity and health.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ZfsPool {
    /// Total capacity, GiB.
    #[serde(rename = "d")]
    pub total: f64,
    /// Allocated, GiB.
    #[serde(rename = "du")]
    pub used: f64,
    #[serde(rename = "rb")]
    pub read_bytes: u64,
    #[serde(rename = "wb")]
    pub write_bytes: u64,
    /// `ONLINE`, `DEGRADED`, `FAULTED`, and so on.
    #[serde(rename = "h")]
    pub health: String,
}

impl ZfsPool {
    /// An empty health string means an older agent that did not report it —
    /// unknown, not unhealthy.
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.health.is_empty() || self.health.eq_ignore_ascii_case("ONLINE")
    }

    #[must_use]
    pub fn used_pct(&self) -> f64 {
        if self.total <= 0.0 {
            return 0.0;
        }
        (self.used / self.total) * 100.0
    }
}
