//! Error type shared by every LocalAPI call.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("tailscaled socket {0} is not reachable: {1}")]
    Socket(PathBuf, #[source] std::io::Error),

    #[error("tailscaled connection failed: {0}")]
    Connection(#[source] hyper::Error),

    #[error("malformed LocalAPI request: {0}")]
    Request(#[source] hyper::http::Error),

    #[error("LocalAPI returned {status}: {body}")]
    Status {
        status: hyper::StatusCode,
        body: String,
    },

    #[error("could not decode LocalAPI response: {0}")]
    Decode(#[source] serde_json::Error),

    #[error("could not encode LocalAPI request: {0}")]
    Encode(#[source] serde_json::Error),

    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    /// True when the daemon is simply not running or not reachable, as opposed
    /// to a request that the daemon actively rejected. The UI uses this to tell
    /// "tailscaled is down" apart from "that action was refused".
    #[must_use]
    pub fn is_unreachable(&self) -> bool {
        matches!(self, Self::Socket(..) | Self::Connection(_))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
