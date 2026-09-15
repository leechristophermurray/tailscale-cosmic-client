//! An async client for the `tailscaled` LocalAPI.
//!
//! `tailscaled` exposes a REST-ish API on a UNIX domain socket rather than a
//! TCP port. This crate wraps that socket in typed calls and a push stream, so
//! the COSMIC front-ends never shell out to the `tailscale` binary or parse its
//! human-readable output.
//!
//! ```no_run
//! # async fn example() -> Result<(), tailscale_localapi::Error> {
//! let api = tailscale_localapi::LocalApi::default();
//! let status = api.status().await?;
//! println!("{} peers on {}", status.peer.len(), status.tailnet_name());
//! # Ok(())
//! # }
//! ```

#![warn(clippy::pedantic)]
// The prefs and status types mirror Go structs field for field, so their shape
// is the daemon's to decide, not ours.
#![allow(
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::struct_excessive_bools
)]

pub mod client;
pub mod error;
pub mod model;
pub mod transport;

pub use client::LocalApi;
pub use error::{Error, Result};
pub use model::ping::PingType;
pub use model::status::Route;
pub use model::{
    BackendState, FileTarget, MaskedPrefs, Notify, PeerStatus, PingResult, Prefs, ServeConfig,
    ServeEntry, ServeScope, Status, TailnetStatus, UserProfile, WaitingFile,
};
pub use transport::{DEFAULT_SOCKET, Transport};
