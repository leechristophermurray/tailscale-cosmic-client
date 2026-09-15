//! The Beszel password, in the system keyring.
//!
//! Storing it through the freedesktop secret service means it is encrypted at
//! rest by the user's login keyring rather than sitting in a config file, and
//! that the application never has to write it anywhere itself.
//!
//! Every operation here is blocking D-Bus work, so callers run them off the UI
//! thread.

const SERVICE: &str = "io.github.leechristophermurray.CosmicTailscale";
/// The service name used before the app ID changed. Read once and copied
/// forward; see [`load`].
const LEGACY_SERVICE: &str = super::config::LEGACY_APP_ID;

/// The keyring entry for one hub account.
///
/// Keyed by hub URL as well as account, so pointing at a different hub does not
/// silently reuse the wrong password.
fn entry(url: &str, user: &str) -> keyring::Result<keyring::Entry> {
    service_entry(SERVICE, url, user)
}

fn service_entry(service: &str, url: &str, user: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(service, &format!("{user}@{url}"))
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
///
/// A password saved under the legacy service name is copied to the current one
/// the first time it is read. The legacy entry stays, so an older build still
/// signs in; [`forget`] removes both.
pub fn load(url: &str, user: &str) -> Result<Option<String>, String> {
    match entry(url, user).and_then(|entry| entry.get_password()) {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => {
            match service_entry(LEGACY_SERVICE, url, user).and_then(|entry| entry.get_password()) {
                Ok(password) => {
                    if let Err(error) = store(url, user, &password) {
                        tracing::warn!(%error, "could not copy the password to the new keyring entry");
                    }
                    Ok(Some(password))
                }
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(error) => Err(describe(error)),
            }
        }
        Err(error) => Err(describe(error)),
    }
}

/// Forget a stored password. Succeeds when there was nothing to forget.
///
/// Also removes any copy under the legacy service name, so signing out leaves
/// no password behind.
pub fn forget(url: &str, user: &str) -> Result<(), String> {
    for service in [SERVICE, LEGACY_SERVICE] {
        match service_entry(service, url, user).and_then(|entry| entry.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(error) => return Err(describe(error)),
        }
    }
    Ok(())
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
