//! Errors from the Beszel hub.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not reach the Beszel hub at {url}: {source}")]
    Unreachable {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    /// The hub answered, but rejected the credentials.
    #[error("the Beszel hub rejected those credentials")]
    Unauthorized,

    /// A request was made without authenticating first.
    #[error("not signed in to the Beszel hub")]
    NotAuthenticated,

    #[error("the Beszel hub returned {status}: {message}")]
    Status {
        status: reqwest::StatusCode,
        message: String,
    },

    #[error("could not decode the hub's response: {0}")]
    Decode(#[source] reqwest::Error),

    #[error("{url} is not a valid hub address: {reason}")]
    BadUrl { url: String, reason: String },
}

impl Error {
    /// True when the hub could not be contacted at all, as opposed to
    /// answering with a refusal. The UI distinguishes "hub is down" from
    /// "your password is wrong".
    #[must_use]
    pub fn is_unreachable(&self) -> bool {
        matches!(self, Self::Unreachable { .. })
    }

    /// True when the fix is to sign in again.
    #[must_use]
    pub fn needs_auth(&self) -> bool {
        matches!(self, Self::Unauthorized | Self::NotAuthenticated)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
