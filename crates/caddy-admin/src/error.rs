//! Errors from the Caddy admin API.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not reach the Caddy admin API at {endpoint}: {source}")]
    Unreachable {
        endpoint: String,
        #[source]
        source: std::io::Error,
    },

    #[error("connection to the Caddy admin API failed: {0}")]
    Connection(#[source] hyper::Error),

    #[error("malformed admin API request: {0}")]
    Request(#[source] hyper::http::Error),

    /// Caddy rejects bad configuration with a JSON body explaining why, which
    /// is far more useful than the status code alone.
    #[error("Caddy rejected the request ({status}): {message}")]
    Rejected {
        status: hyper::StatusCode,
        message: String,
    },

    #[error("could not decode the admin API response: {0}")]
    Decode(#[source] serde_json::Error),

    #[error("could not encode the admin API request: {0}")]
    Encode(#[source] serde_json::Error),

    #[error("{0} is not a usable admin endpoint")]
    BadEndpoint(String),
}

impl Error {
    /// True when nothing answered, as opposed to Caddy answering with a
    /// refusal. Discovery uses this to tell "no Caddy here" from "Caddy here,
    /// but it said no".
    #[must_use]
    pub fn is_unreachable(&self) -> bool {
        matches!(self, Self::Unreachable { .. } | Self::Connection(_))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
