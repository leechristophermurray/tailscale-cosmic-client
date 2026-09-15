//! A client for a [Beszel](https://beszel.dev) monitoring hub, reached over a
//! tailnet.
//!
//! Beszel stores everything in PocketBase and exposes it over REST, so a
//! desktop client can read metrics directly without loading the web interface
//! and without the hub being on the public internet.
//!
//! The record types mirror Beszel's wire format, which uses very short keys
//! because these rows are written once per interval per machine and kept for
//! months. The abbreviations are confined to the `serde` attributes; nothing
//! above this crate needs to know that memory percent is `mp`.

#![warn(clippy::pedantic)]
#![allow(
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions
)]

pub mod client;
pub mod deploy;
pub mod error;
pub mod model;

pub use client::{AuthResponse, BeszelHub, HubInfo};
pub use deploy::{AgentInstall, DEFAULT_AGENT_PORT, InstallOutcome};
pub use error::{Error, Result};
pub use model::{
    AlertEvent, AlertHistoryRecord, AlertKind, AlertTracker, ContainerStats, Info, Stats,
    StatsPeriod, SystemRecord, SystemStatus, ZfsPool, stats::StatsRecord,
};
