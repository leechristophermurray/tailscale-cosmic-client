//! A client for Caddy's native JSON admin API, aimed at Caddy instances running
//! on tailnet peers.
//!
//! Caddy is configured structurally: you PUT JSON at config paths and it
//! applies the change live. This crate leans on that rather than generating
//! Caddyfile text, so adding a reverse proxy is a typed value, not a string
//! template, and it takes effect without restarting the server.

#![warn(clippy::pedantic)]
// Prose about Caddy and ssh reads better without backticks on every mention.
#![allow(
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions
)]

pub mod client;
pub mod discovery;
pub mod error;
pub mod model;
pub mod tunnel;

pub use client::{CaddyAdmin, DEFAULT_ADMIN_PORT, Endpoint};
pub use discovery::{Reachability, probe_peer};
pub use error::{Error, Result};
pub use model::{Handler, Matcher, Route, Server, Site, Upstream};
pub use tunnel::Tunnel;
