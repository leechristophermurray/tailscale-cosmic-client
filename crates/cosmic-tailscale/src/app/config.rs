//! Settings that outlive a session.
//!
//! This holds only what is not secret. The Beszel password goes to the system
//! keyring (see [`super::secrets`]); what lives here is the address to reach
//! and the account name to reach it as, both of which the user typed and
//! neither of which is worth protecting.

use cosmic::cosmic_config::{
    self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry,
};

pub const CONFIG_VERSION: u64 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq, CosmicConfigEntry)]
#[version = 1]
pub struct Config {
    /// Base URL of the Beszel hub, e.g. `https://mon.example.com`. Empty until
    /// the user configures one.
    pub beszel_url: String,
    /// The account to sign in as. The password is not stored here.
    pub beszel_user: String,
    /// Sign in to the hub automatically at startup, using the stored password.
    pub beszel_auto_connect: bool,
    /// Stable node ID of the machine last managed on the Caddy page. Node IDs
    /// survive restarts and IP changes, which hostnames and addresses do not.
    pub caddy_target: String,
}

impl Config {
    /// Load the stored configuration, falling back to defaults.
    ///
    /// A missing or unreadable config is normal on first run and must not stop
    /// the application from starting.
    #[must_use]
    pub fn load() -> (Option<cosmic_config::Config>, Self) {
        match cosmic_config::Config::new(<super::App as cosmic::Application>::APP_ID, CONFIG_VERSION) {
            Ok(handler) => {
                let config = Self::get_entry(&handler).unwrap_or_else(|(errors, config)| {
                    for error in errors {
                        tracing::debug!(%error, "ignoring unreadable config field");
                    }
                    config
                });
                (Some(handler), config)
            }
            Err(error) => {
                tracing::warn!(%error, "could not open the config store; using defaults");
                (None, Self::default())
            }
        }
    }

    #[must_use]
    pub fn is_beszel_configured(&self) -> bool {
        !self.beszel_url.trim().is_empty() && !self.beszel_user.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config store in a throwaway directory, so the tests never touch the
    /// real `~/.config`.
    fn scratch_store() -> (tempdir::Scratch, cosmic_config::Config) {
        let dir = tempdir::Scratch::new();
        let store = cosmic_config::Config::with_custom_path(
            "com.system76.CosmicTailscale.Test",
            CONFIG_VERSION,
            dir.path().to_path_buf(),
        )
        .expect("scratch config store opens");
        (dir, store)
    }

    fn reload(store: &cosmic_config::Config) -> Config {
        Config::get_entry(store).unwrap_or_else(|(_, config)| config)
    }

    /// The bug that lost the Beszel sign-in between sessions, pinned down.
    ///
    /// The generated setters write only when the value differs from what the
    /// struct holds. Assign first and every setter sees "no change", writes
    /// nothing, and still returns `Ok` — so this asserts the trap is real, not
    /// just that the fix works.
    #[test]
    fn assigning_before_setting_writes_nothing() {
        let (_dir, store) = scratch_store();

        // The struct already holds the value when the setter runs — the same
        // position the old code put it in.
        let mut config = Config {
            beszel_url: "https://mon.example.com".to_string(),
            ..Config::default()
        };

        let wrote = config
            .set_beszel_url(&store, "https://mon.example.com".to_string())
            .expect("setter does not error");

        assert!(!wrote, "the setter reports no change");
        assert!(
            reload(&store).beszel_url.is_empty(),
            "and nothing reached disk — this is exactly what lost the sign-in"
        );
    }

    #[test]
    fn hub_settings_survive_a_restart() {
        let (_dir, store) = scratch_store();
        let mut config = Config::default();

        config
            .set_beszel_url(&store, "https://mon.example.com".to_string())
            .unwrap();
        config
            .set_beszel_user(&store, "you@example.com".to_string())
            .unwrap();
        config.set_beszel_auto_connect(&store, true).unwrap();

        // A fresh read, as the next launch does.
        let restored = reload(&store);
        assert_eq!(restored.beszel_url, "https://mon.example.com");
        assert_eq!(restored.beszel_user, "you@example.com");
        assert!(restored.beszel_auto_connect);
        assert!(restored.is_beszel_configured());
    }

    #[test]
    fn the_caddy_machine_survives_a_restart() {
        let (_dir, store) = scratch_store();
        let mut config = Config::default();

        config
            .set_caddy_target(&store, "n5uN76kDwB21CNTRL".to_string())
            .unwrap();

        assert_eq!(reload(&store).caddy_target, "n5uN76kDwB21CNTRL");
    }

    /// Signing out must stop the next launch reconnecting, while keeping the
    /// address and account so signing back in needs only the password.
    #[test]
    fn signing_out_disables_auto_connect_but_keeps_the_address() {
        let (_dir, store) = scratch_store();
        let mut config = Config::default();

        config
            .set_beszel_url(&store, "https://mon.example.com".to_string())
            .unwrap();
        config.set_beszel_auto_connect(&store, true).unwrap();
        config.set_beszel_auto_connect(&store, false).unwrap();

        let restored = reload(&store);
        assert!(!restored.beszel_auto_connect);
        assert_eq!(restored.beszel_url, "https://mon.example.com");
    }

    /// Minimal self-cleaning temp directory, to avoid a dependency for tests.
    mod tempdir {
        use std::path::{Path, PathBuf};

        pub struct Scratch(PathBuf);

        impl Scratch {
            pub fn new() -> Self {
                let unique = format!(
                    "cosmic-tailscale-config-test-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_nanos())
                );
                let path = std::env::temp_dir().join(unique);
                std::fs::create_dir_all(&path).expect("scratch dir");
                Self(path)
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
