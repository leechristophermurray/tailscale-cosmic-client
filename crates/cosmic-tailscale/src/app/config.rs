//! Settings that outlive a session.
//!
//! This holds only what is not secret. The Beszel password goes to the system
//! keyring (see [`super::secrets`]); what lives here is the address to reach
//! and the account name to reach it as, both of which the user typed and
//! neither of which is worth protecting.

use std::path::Path;

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};

pub const CONFIG_VERSION: u64 = 1;

/// The ID this application used before it moved out of System76's namespace.
/// Settings saved under it are carried over once; see [`migrate_legacy_settings`].
pub const LEGACY_APP_ID: &str = "com.system76.CosmicTailscale";

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
    /// Silence desktop notifications for the hub's alerts. Stored as "muted" so
    /// that a setting never written reads as notifications on. The panel
    /// applet, which sends them, reads this.
    pub beszel_alerts_muted: bool,
    /// The SSH user each machine was last mounted as, by stable node ID.
    pub mount_users: std::collections::BTreeMap<String, String>,
}

impl Config {
    /// Load the stored configuration, falling back to defaults.
    ///
    /// A missing or unreadable config is normal on first run and must not stop
    /// the application from starting.
    #[must_use]
    pub fn load() -> (Option<cosmic_config::Config>, Self) {
        let app_id = <super::App as cosmic::Application>::APP_ID;
        if let Some(config_home) = dirs::config_dir() {
            match migrate_legacy_settings(&config_home, LEGACY_APP_ID, app_id) {
                Ok(true) => tracing::info!("carried settings over from {LEGACY_APP_ID}"),
                Ok(false) => {}
                Err(error) => tracing::warn!(%error, "could not carry over earlier settings"),
            }
        }

        match cosmic_config::Config::new(
            <super::App as cosmic::Application>::APP_ID,
            CONFIG_VERSION,
        ) {
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

/// Copy settings saved under `legacy_id` to `app_id`, if there are any and
/// nothing has been saved under `app_id` yet. Returns whether it copied.
///
/// The legacy settings are left where they are, so going back to an older
/// build still finds them. The copy is staged beside the destination and moved
/// into place in one step: a copy interrupted halfway must not look like
/// settings the user chose, or the next launch would skip the migration.
///
/// This checks for saved files rather than for the directory, because opening a
/// store (which the panel applet does) creates an empty directory.
pub fn migrate_legacy_settings(
    config_home: &Path,
    legacy_id: &str,
    app_id: &str,
) -> std::io::Result<bool> {
    let version = format!("v{CONFIG_VERSION}");
    let legacy = config_home.join("cosmic").join(legacy_id).join(&version);
    let target = config_home.join("cosmic").join(app_id).join(&version);

    let has_files = |dir: &Path| -> std::io::Result<bool> {
        match std::fs::read_dir(dir) {
            Ok(mut entries) => Ok(entries.next().is_some()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    };
    if !has_files(&legacy)? || has_files(&target)? {
        return Ok(false);
    }

    let staging = config_home
        .join("cosmic")
        .join(format!(".{app_id}.migrating"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    for entry in std::fs::read_dir(&legacy)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), staging.join(entry.file_name()))?;
        }
    }

    // An empty directory left by the applet would block the rename.
    if target.is_dir() {
        std::fs::remove_dir(&target)?;
    }
    std::fs::create_dir_all(target.parent().expect("version directory has a parent"))?;
    std::fs::rename(&staging, &target)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config store in a throwaway directory, so the tests never touch the
    /// real `~/.config`.
    fn scratch_store() -> (tempdir::Scratch, cosmic_config::Config) {
        let dir = tempdir::Scratch::new();
        let store = cosmic_config::Config::with_custom_path(
            "io.github.leechristophermurray.CosmicTailscale.Test",
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
            .set_caddy_target(&store, "nExample0005CNTRL".to_string())
            .unwrap();

        assert_eq!(reload(&store).caddy_target, "nExample0005CNTRL");
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

    /// The per-machine SSH users are a map, which has to survive the store's
    /// serialisation, and the alert switch has to read as "on" when unset.
    #[test]
    fn mount_users_and_the_alert_switch_round_trip() {
        let (_dir, store) = scratch_store();
        assert!(
            !reload(&store).beszel_alerts_muted,
            "notifications default to on"
        );

        let mut config = Config::default();
        let users: std::collections::BTreeMap<String, String> = [
            ("nExample0002CNTRL".to_string(), "alex".to_string()),
            ("nExample0005CNTRL".to_string(), "root".to_string()),
        ]
        .into();
        config.set_mount_users(&store, users.clone()).unwrap();
        config.set_beszel_alerts_muted(&store, true).unwrap();

        let restored = reload(&store);
        assert_eq!(restored.mount_users, users);
        assert!(restored.beszel_alerts_muted);
    }

    // ---- migration from the System76 app ID ------------------------------------

    const OLD: &str = "com.example.Old";
    const NEW: &str = "io.example.New";

    fn write_setting(home: &Path, id: &str, key: &str, value: &str) {
        let dir = home.join("cosmic").join(id).join("v1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(key), value).unwrap();
    }

    fn read_setting(home: &Path, id: &str, key: &str) -> Option<String> {
        std::fs::read_to_string(home.join("cosmic").join(id).join("v1").join(key)).ok()
    }

    #[test]
    fn settings_saved_under_the_old_id_are_carried_over_and_kept() {
        let home = tempdir::Scratch::new();
        write_setting(
            home.path(),
            OLD,
            "beszel_url",
            "\"https://mon.example.com\"",
        );
        write_setting(home.path(), OLD, "caddy_target", "\"nExample0005CNTRL\"");

        assert!(migrate_legacy_settings(home.path(), OLD, NEW).unwrap());

        assert_eq!(
            read_setting(home.path(), NEW, "beszel_url").as_deref(),
            Some("\"https://mon.example.com\"")
        );
        assert_eq!(
            read_setting(home.path(), NEW, "caddy_target").as_deref(),
            Some("\"nExample0005CNTRL\"")
        );
        // An older build can still find its settings.
        assert!(read_setting(home.path(), OLD, "beszel_url").is_some());
        // Nothing is left staged.
        assert!(
            !home
                .path()
                .join("cosmic")
                .join(format!(".{NEW}.migrating"))
                .exists()
        );
    }

    #[test]
    fn settings_already_saved_under_the_new_id_are_never_overwritten() {
        let home = tempdir::Scratch::new();
        write_setting(
            home.path(),
            OLD,
            "beszel_url",
            "\"https://old.example.com\"",
        );
        write_setting(
            home.path(),
            NEW,
            "beszel_url",
            "\"https://new.example.com\"",
        );

        assert!(!migrate_legacy_settings(home.path(), OLD, NEW).unwrap());
        assert_eq!(
            read_setting(home.path(), NEW, "beszel_url").as_deref(),
            Some("\"https://new.example.com\"")
        );
    }

    /// The panel applet opens the store before the window ever runs, which
    /// creates the new directory empty. That must not count as saved settings.
    #[test]
    fn an_empty_directory_left_by_the_applet_does_not_block_migration() {
        let home = tempdir::Scratch::new();
        write_setting(home.path(), OLD, "beszel_user", "\"alex\"");
        std::fs::create_dir_all(home.path().join("cosmic").join(NEW).join("v1")).unwrap();

        assert!(migrate_legacy_settings(home.path(), OLD, NEW).unwrap());
        assert_eq!(
            read_setting(home.path(), NEW, "beszel_user").as_deref(),
            Some("\"alex\"")
        );
    }

    #[test]
    fn a_fresh_install_has_nothing_to_migrate() {
        let home = tempdir::Scratch::new();
        assert!(!migrate_legacy_settings(home.path(), OLD, NEW).unwrap());
        assert!(!home.path().join("cosmic").join(NEW).exists());
    }

    #[test]
    fn migrated_settings_load_through_the_config_store() {
        let home = tempdir::Scratch::new();
        write_setting(
            home.path(),
            OLD,
            "beszel_url",
            "\"https://mon.example.com\"",
        );
        write_setting(home.path(), OLD, "beszel_auto_connect", "true");
        migrate_legacy_settings(home.path(), OLD, NEW).unwrap();

        let store =
            cosmic_config::Config::with_custom_path(NEW, CONFIG_VERSION, home.path().to_path_buf())
                .expect("store opens");
        let loaded = reload(&store);
        assert_eq!(loaded.beszel_url, "https://mon.example.com");
        assert!(loaded.beszel_auto_connect);
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
