//! Taildrop: `file-targets`, `file-put`, and the inbound `files/` queue.

use serde::Deserialize;

/// One node that can currently receive a Taildrop transfer.
#[derive(Debug, Clone, Deserialize)]
pub struct FileTarget {
    #[serde(rename = "Node")]
    pub node: FileTargetNode,
    /// Base URL of the peer's file-receiving endpoint.
    #[serde(rename = "PeerAPIURL", default)]
    pub peer_api_url: String,
}

impl FileTarget {
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.node
            .name
            .trim_end_matches('.')
            .split('.')
            .next()
            .unwrap_or(&self.node.name)
    }

    #[must_use]
    pub fn stable_id(&self) -> &str {
        &self.node.stable_id
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct FileTargetNode {
    #[serde(rename = "StableID")]
    pub stable_id: String,
    /// Fully-qualified MagicDNS name with Go's trailing dot.
    pub name: String,
    pub addresses: Vec<String>,
}

/// A file that has arrived and is waiting to be saved.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct WaitingFile {
    pub name: String,
    pub size: u64,
}

impl WaitingFile {
    /// Human-readable size for the notification body.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // display only
    pub fn human_size(&self) -> String {
        const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
        let mut size = self.size as f64;
        let mut unit = 0;
        while size >= 1024.0 && unit < UNITS.len() - 1 {
            size /= 1024.0;
            unit += 1;
        }
        if unit == 0 {
            format!("{} {}", self.size, UNITS[0])
        } else {
            format!("{size:.1} {}", UNITS[unit])
        }
    }
}
