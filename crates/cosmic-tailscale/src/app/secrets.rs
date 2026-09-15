//! The Beszel password, in the system keyring.
//!
//! Storing it through the freedesktop secret service means it is encrypted at
//! rest by the user's login keyring rather than sitting in a config file, and
//! that the application never has to write it anywhere itself.
//!
//! Every operation here is blocking D-Bus work, so callers run them off the UI
//! thread.

const SERVICE: &str = "com.system76.CosmicTailscale";

/// The keyring entry for one hub account.
///
/// Keyed by hub URL as well as account, so pointing at a different hub does not
/// silently reuse the wrong password.
fn entry(url: &str, user: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, &format!("{user}@{url}"))
}

/// Store a password.
pub fn store(url: &str, user: &str, password: &str) -> Result<(), String> {
    entry(url, user)
        .and_then(|entry| entry.set_password(password))
        .map_err(describe)
}

/// Read a stored password, if there is one.
///
/// A missing entry is an ordinary outcome — the user has not signed in yet —
/// and is reported as `None` rather than as an error.
pub fn load(url: &str, user: &str) -> Result<Option<String>, String> {
    match entry(url, user).and_then(|entry| entry.get_password()) {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(describe(error)),
    }
}

/// Forget a stored password. Succeeds when there was nothing to forget.
pub fn forget(url: &str, user: &str) -> Result<(), String> {
    match entry(url, user).and_then(|entry| entry.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(describe(error)),
    }
}

/// Turn a keyring failure into something worth showing a person.
///
/// The common one is a locked or absent keyring daemon, which is fixable by the
/// user but unintelligible in its raw form.
fn describe(error: keyring::Error) -> String {
    match error {
        keyring::Error::NoStorageAccess(inner) => {
            format!("The system keyring could not be reached ({inner}). It may be locked.")
        }
        keyring::Error::PlatformFailure(inner) => {
            format!("The system keyring refused the request ({inner}).")
        }
        other => other.to_string(),
    }
}
