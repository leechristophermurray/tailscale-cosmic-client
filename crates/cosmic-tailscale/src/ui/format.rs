//! Turning daemon numbers into strings a person can read at a glance.

use chrono::{DateTime, Utc};

use crate::fl;

/// Byte counts for the throughput readout in the status bar.
#[must_use]
pub fn bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// Per-second rate, e.g. `1.2 MB/s`.
#[must_use]
pub fn rate(bytes_per_second: f64) -> String {
    const UNITS: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    let mut rate = bytes_per_second.max(0.0);
    let mut unit = 0;
    while rate >= 1024.0 && unit < UNITS.len() - 1 {
        rate /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{rate:.0} {}", UNITS[0])
    } else {
        format!("{rate:.1} {}", UNITS[unit])
    }
}

/// Coarse "how long ago", matching the mockup's `5m ago`.
#[must_use]
pub fn relative_past(time: DateTime<Utc>) -> String {
    let delta = Utc::now() - time;
    let seconds = delta.num_seconds();

    if seconds < 0 {
        return "just now".to_string();
    }

    match seconds {
        s if s < 45 => "just now".to_string(),
        s if s < 90 => "1m ago".to_string(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 7200 => "1h ago".to_string(),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s if s < 172_800 => "yesterday".to_string(),
        s if s < 2_592_000 => format!("{}d ago", s / 86_400),
        s => format!("{}mo ago", s / 2_592_000),
    }
}

/// Forward-looking duration for key expiry, e.g. `in 42 days`.
#[must_use]
pub fn relative_future(time: DateTime<Utc>) -> String {
    let days = (time - Utc::now()).num_days();
    match days {
        d if d < 0 => "expired".to_string(),
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        d if d < 60 => format!("in {d} days"),
        d => format!("in {} months", d / 30),
    }
}

/// Absolute date for the "valid until" line under a countdown.
#[must_use]
pub fn calendar_date(time: DateTime<Utc>) -> String {
    time.format("%b %-d, %Y").to_string()
}

/// Latency with a sensible number of digits: sub-millisecond paths deserve a
/// decimal, anything slower does not.
#[must_use]
pub fn latency_ms(value: f64) -> String {
    if value < 1.0 {
        format!("{value:.1} ms")
    } else {
        format!("{value:.0} ms")
    }
}

/// Human name for a Tailscale OS string. The daemon uses short lowercase
/// tokens; these are what the UI shows next to a machine.
///
/// An OS the daemon knows but this list does not is passed through verbatim
/// rather than shown as "Unknown".
#[must_use]
pub fn os_name(os: &str) -> String {
    match os {
        "linux" => fl!("os-linux"),
        "macOS" => fl!("os-macos"),
        "windows" => fl!("os-windows"),
        "iOS" => fl!("os-ios"),
        "android" => fl!("os-android"),
        "freebsd" => fl!("os-freebsd"),
        "openbsd" => fl!("os-openbsd"),
        "tvOS" => fl!("os-tvos"),
        "" => fl!("os-unknown"),
        other => other.to_string(),
    }
}
