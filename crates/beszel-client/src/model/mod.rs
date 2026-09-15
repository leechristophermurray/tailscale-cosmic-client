//! Typed mirrors of the records a Beszel hub stores in PocketBase.
//!
//! Beszel optimises these records for size: almost every field is a one- or
//! two-letter key, because they are written once per interval per machine and
//! kept for months. `cpu` is readable, but memory percent is `mp`, disk percent
//! is `dp` and temperatures are `t`. Every field here carries the wire name it
//! maps to so the abbreviations stay in one place instead of leaking into the
//! UI.

pub mod container;
pub mod stats;
pub mod system;

pub use container::ContainerStats;
pub use stats::{Stats, StatsPeriod, ZfsPool};
pub use system::{Info, SystemRecord, SystemStatus};

use serde::Deserialize;

/// PocketBase wraps every list response in this envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct ListResponse<T> {
    pub items: Vec<T>,
    #[serde(default)]
    pub page: u32,
    #[serde(default)]
    pub total_items: u32,
}

/// Deserialize a field the hub may send as `null` rather than omitting it.
pub(crate) fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}
