//! Decoding tests for Beszel's record formats.
//!
//! The JSON here uses Beszel's real abbreviated keys, taken from its Go struct
//! tags. Getting one of these wrong produces a metric that silently reads zero
//! rather than an error, which is the failure mode worth testing for.

use beszel_client::model::ListResponse;
use beszel_client::{Stats, SystemRecord, SystemStatus};

const SYSTEMS_RESPONSE: &str = r#"{
  "page": 1,
  "perPage": 500,
  "totalItems": 2,
  "items": [
    {
      "id": "abc123",
      "name": "homeforge",
      "host": "100.109.86.6",
      "port": "45876",
      "status": "up",
      "updated": "2026-09-14 20:00:00.000Z",
      "info": {
        "h": "homeforge", "k": "6.11.0", "c": 8, "t": 16,
        "m": "AMD Ryzen 7", "u": 864000, "cpu": 12.5,
        "mp": 41.2, "dp": 63.8, "dt": 47.5, "v": "0.9.1",
        "bb": 1048576, "la": [0.8, 1.2, 1.1], "sv": [42, 1]
      }
    },
    {
      "id": "def456",
      "name": "nas",
      "host": "nas.example.ts.net",
      "port": "45876",
      "status": "down",
      "info": {"h": "nas", "cpu": 0, "mp": 0, "dp": 0, "v": "0.9.1"}
    }
  ]
}"#;

#[test]
fn decodes_a_systems_listing() {
    let list: ListResponse<SystemRecord> =
        serde_json::from_str(SYSTEMS_RESPONSE).expect("systems decode");

    assert_eq!(list.items.len(), 2);

    let forge = &list.items[0];
    assert_eq!(forge.name, "homeforge");
    assert_eq!(forge.status, SystemStatus::Up);
    assert!(forge.status.is_up());

    // Every one of these comes from a one- or two-letter key.
    assert_eq!(forge.info.cores, 8);
    assert_eq!(forge.info.threads, 16);
    assert!((forge.info.cpu - 12.5).abs() < f64::EPSILON);
    assert!((forge.info.memory_pct - 41.2).abs() < f64::EPSILON);
    assert!((forge.info.disk_pct - 63.8).abs() < f64::EPSILON);
    assert!((forge.info.dashboard_temp - 47.5).abs() < f64::EPSILON);
    assert_eq!(forge.info.bandwidth_bytes, 1_048_576);
    assert_eq!(forge.info.uptime_human(), "10d");
    assert_eq!(forge.info.failed_services(), Some(1));

    assert!(!list.items[1].status.is_up());
}

/// A load average means nothing without the thread count beside it.
#[test]
fn load_is_reported_relative_to_threads() {
    let list: ListResponse<SystemRecord> = serde_json::from_str(SYSTEMS_RESPONSE).unwrap();
    let forge = &list.items[0];

    // 0.8 across 16 threads is idle, not busy.
    let pressure = forge.info.load_pressure().expect("threads are known");
    assert!(pressure < 0.1, "0.8 load on 16 threads should read as idle");

    // Without a thread count there is no honest figure to give.
    assert!(list.items[1].info.load_pressure().is_none());
}

/// Beszel's `name` is user-chosen and `host` may be an IP or a DNS name, so
/// correlation has to try several fields.
#[test]
fn correlates_with_a_tailnet_peer_several_ways() {
    let list: ListResponse<SystemRecord> = serde_json::from_str(SYSTEMS_RESPONSE).unwrap();
    let by_ip = &list.items[0];
    let by_dns = &list.items[1];

    let addresses = vec!["100.109.86.6".to_string()];
    assert!(by_ip.matches_peer("homeforge", "homeforge.example.ts.net", &addresses));
    // Matching on the address alone is enough when the name differs.
    assert!(by_ip.matches_peer("something-else", "other.ts.net", &addresses));

    // A host registered by its full MagicDNS name still matches the short one.
    assert!(by_dns.matches_peer("nas", "nas.example.ts.net", &[]));

    assert!(!by_ip.matches_peer("unrelated", "unrelated.ts.net", &[]));
}

const STATS: &str = r#"{
  "cpu": 23.4,
  "m": 64.0, "mu": 48.0, "mp": 75.0, "mb": 20.0, "mz": 16.0,
  "d": 1000.0, "du": 638.0, "dp": 63.8,
  "t": {"Package id 0": 47.5, "nvme": 62.0, "Core 0": 44.0},
  "f": {"cpu_fan": 1200},
  "z": {
    "tank": {"d": 8000.0, "du": 4200.0, "h": "ONLINE", "rb": 1024, "wb": 2048},
    "backup": {"d": 4000.0, "du": 3900.0, "h": "DEGRADED"}
  },
  "b": [524288, 1048576],
  "dio": [4096, 8192],
  "la": [1.5, 1.2, 0.9]
}"#;

#[test]
fn decodes_a_stats_sample() {
    let stats: Stats = serde_json::from_str(STATS).expect("stats decode");

    assert!((stats.cpu - 23.4).abs() < f64::EPSILON);
    assert!((stats.memory_total - 64.0).abs() < f64::EPSILON);
    assert!((stats.disk_pct - 63.8).abs() < f64::EPSILON);
    assert_eq!(stats.bandwidth_sent(), 524_288);
    assert_eq!(stats.bandwidth_received(), 1_048_576);
    assert_eq!(stats.disk_io, [4096, 8192]);
    assert_eq!(stats.fans.get("cpu_fan"), Some(&1200));
}

/// Cache and the ZFS ARC are siblings of used memory, not components of it —
/// the agent has already excluded them. Subtracting them again produced a
/// machine using 4.7 GiB reporting 0.0, which is how this was caught.
#[test]
fn reclaimable_memory_is_reported_beside_used_not_inside_it() {
    let stats: Stats = serde_json::from_str(STATS).unwrap();

    assert!((stats.memory_used - 48.0).abs() < f64::EPSILON);
    assert!((stats.memory_reclaimable() - 36.0).abs() < f64::EPSILON);
    assert!((stats.memory_in_use() - 84.0).abs() < f64::EPSILON);
}

/// The invariant that pins the semantics: the agent derives `mp` from
/// `mu / m`, so any transformation of `memory_used` breaks this. It is a real
/// sample from a machine running 4.7 of 31.2 GiB at a reported 15.2%.
#[test]
fn used_memory_is_the_figure_the_percentage_is_derived_from() {
    let stats: Stats =
        serde_json::from_str(r#"{"m": 31.2, "mu": 4.74, "mp": 15.2, "mb": 12.0}"#).unwrap();

    let derived = stats.memory_used / stats.memory_total * 100.0;
    assert!(
        (derived - stats.memory_pct).abs() < 0.2,
        "memory_used ({}) must be what memory_pct ({}) is computed from, but it derives {derived}",
        stats.memory_used,
        stats.memory_pct
    );
}

#[test]
fn surfaces_the_hottest_sensor() {
    let stats: Stats = serde_json::from_str(STATS).unwrap();
    let (name, value) = stats.peak_temperature().expect("sensors present");

    assert_eq!(name, "nvme");
    assert!((value - 62.0).abs() < f64::EPSILON);
}

#[test]
fn flags_zfs_pools_that_are_not_online() {
    let stats: Stats = serde_json::from_str(STATS).unwrap();
    let degraded = stats.degraded_pools();

    assert_eq!(degraded, vec![("backup", "DEGRADED")]);

    let tank = &stats.zfs_pools["tank"];
    assert!(tank.is_healthy());
    assert!((tank.used_pct() - 52.5).abs() < 0.01);
}

/// An older agent reports no health string. That is unknown, not unhealthy —
/// treating it as a fault would cry wolf on every pool.
#[test]
fn a_pool_with_no_reported_health_is_not_called_degraded() {
    let stats: Stats = serde_json::from_str(r#"{"z": {"old": {"d": 100.0, "du": 50.0}}}"#).unwrap();
    assert!(stats.degraded_pools().is_empty());
}

/// A pool with no capacity must not divide by zero.
#[test]
fn an_empty_pool_reports_zero_usage() {
    let stats: Stats = serde_json::from_str(r#"{"z": {"p": {"d": 0.0, "du": 0.0}}}"#).unwrap();
    assert!((stats.zfs_pools["p"].used_pct() - 0.0).abs() < f64::EPSILON);
}

/// A minimal sample from an agent reporting only the basics must still decode.
#[test]
fn a_sparse_sample_decodes_with_defaults() {
    let stats: Stats = serde_json::from_str(r#"{"cpu": 5.0}"#).expect("sparse decode");

    assert!((stats.cpu - 5.0).abs() < f64::EPSILON);
    assert!(stats.temperatures.is_empty());
    assert!(stats.zfs_pools.is_empty());
    assert!(stats.peak_temperature().is_none());
    assert_eq!(stats.bandwidth, [0, 0]);
}

/// A status Beszel adds in a later version must not break the listing.
#[test]
fn an_unknown_status_degrades_rather_than_failing() {
    let record: SystemRecord =
        serde_json::from_str(r#"{"id":"x","name":"y","status":"hibernating"}"#).unwrap();
    assert_eq!(record.status, SystemStatus::Unknown);
    assert!(!record.status.is_up());
}
