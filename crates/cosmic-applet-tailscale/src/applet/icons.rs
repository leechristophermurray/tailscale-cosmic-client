//! Panel icon lookup.
//!
//! The applet ships its own Tailscale mark in three states, but an icon that
//! fails to load leaves a blank panel slot — so every lookup falls back to a
//! stock freedesktop name that any icon theme provides.

use std::path::PathBuf;
use std::sync::LazyLock;

use cosmic::widget::icon::{IconFallback, Named, from_name};

/// Directories searched before the icon theme chain.
///
/// The installed location comes first; the second entry makes an uninstalled
/// `cargo run` from the source tree show the right icons too.
static EXTRA_PATHS: LazyLock<Vec<PathBuf>> = LazyLock::new(|| {
    let mut paths = vec![PathBuf::from("/usr/share/icons/hicolor/scalable/status")];

    if let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") {
        paths.push(
            PathBuf::from(dir)
                .join("../../data/icons/scalable/status")
                .clean(),
        );
    }

    paths
});

/// Build a handle for one of the applet's own icons, falling back to `stock`.
pub fn branded(name: &'static str, stock: &'static str) -> Named {
    let mut icon = from_name(name).with_extra_paths(EXTRA_PATHS.clone());
    icon.fallback = Some(IconFallback::Names(vec![stock.into()]));
    icon
}

/// Minimal `..` resolution so the dev-time path is a real directory.
trait Clean {
    fn clean(self) -> PathBuf;
}

impl Clean for PathBuf {
    fn clean(self) -> PathBuf {
        let mut out = PathBuf::new();
        for component in self.components() {
            match component {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                other => out.push(other),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every panel state must resolve to a real file. A missing icon shows up
    /// as an empty panel slot rather than an error, so it is worth asserting.
    #[test]
    fn every_icon_state_resolves() {
        for (name, stock) in [
            (
                "com.system76.CosmicAppletTailscale-connected-symbolic",
                "network-transmit-receive-symbolic",
            ),
            (
                "com.system76.CosmicAppletTailscale-disconnected-symbolic",
                "network-wired-disconnected-symbolic",
            ),
            (
                "com.system76.CosmicAppletTailscale-exitnode-symbolic",
                "security-high-symbolic",
            ),
        ] {
            assert!(
                branded(name, stock).path().is_some(),
                "neither {name} nor its fallback {stock} could be located",
            );
        }
    }
}
