//! `serve-config` — what this node publishes over the tailnet (`serve`) or to
//! the public internet (`funnel`).
//!
//! The daemon's own schema is a deeply nested map keyed by "host:port" strings.
//! We keep the raw document for round-tripping and flatten it into a list of
//! [`ServeEntry`] rows for display, because the UI only ever shows one row per
//! published handler.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ServeConfig {
    /// Keyed by port. `true` means raw TCP forwarding is enabled there.
    #[serde(rename = "TCP", skip_serializing_if = "BTreeMap::is_empty")]
    pub tcp: BTreeMap<String, TcpHandler>,
    /// Keyed by `host:port`.
    #[serde(rename = "Web", skip_serializing_if = "BTreeMap::is_empty")]
    pub web: BTreeMap<String, WebHandlerSet>,
    /// Keyed by `host:port`; presence with `true` means publicly funnelled.
    #[serde(rename = "AllowFunnel", skip_serializing_if = "BTreeMap::is_empty")]
    pub allow_funnel: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct TcpHandler {
    #[serde(rename = "HTTPS", skip_serializing_if = "std::ops::Not::not")]
    pub https: bool,
    #[serde(rename = "HTTP", skip_serializing_if = "std::ops::Not::not")]
    pub http: bool,
    #[serde(rename = "TCPForward", skip_serializing_if = "Option::is_none")]
    pub tcp_forward: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct WebHandlerSet {
    /// Keyed by URL path prefix, e.g. `/`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub handlers: BTreeMap<String, WebHandler>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct WebHandler {
    /// Upstream to reverse-proxy to, e.g. `http://127.0.0.1:8080`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
    /// Static directory served from disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Literal response body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl WebHandler {
    /// What this handler points at, however it was configured.
    #[must_use]
    pub fn target(&self) -> String {
        if let Some(proxy) = &self.proxy {
            proxy.clone()
        } else if let Some(path) = &self.path {
            path.clone()
        } else if self.text.is_some() {
            "inline text".to_string()
        } else {
            "unconfigured".to_string()
        }
    }
}

/// Who can reach a published service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeScope {
    /// Reachable only from inside the tailnet.
    Tailnet,
    /// Published to the public internet via Funnel.
    Funnel,
}

impl ServeScope {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Tailnet => "Tailnet only",
            Self::Funnel => "Public funnel",
        }
    }
}

/// One published service, flattened for display.
#[derive(Debug, Clone)]
pub struct ServeEntry {
    /// The address users visit, e.g. `grafana.orca-cat.ts.net:443`.
    pub host_port: String,
    /// URL path prefix this handler is mounted at.
    pub path: String,
    /// Where the request is sent.
    pub target: String,
    pub scope: ServeScope,
}

impl ServeEntry {
    /// The full URL to put on a copyable chip.
    #[must_use]
    pub fn url(&self) -> String {
        let (host, port) = self
            .host_port
            .rsplit_once(':')
            .unwrap_or((self.host_port.as_str(), "443"));

        let path = self.path.trim_end_matches('/');

        if port == "443" {
            format!("https://{host}{path}")
        } else {
            format!("https://{host}:{port}{path}")
        }
    }
}

impl ServeConfig {
    /// Flatten the nested config into the rows the Services page renders.
    #[must_use]
    pub fn entries(&self) -> Vec<ServeEntry> {
        let mut entries = Vec::new();

        for (host_port, handler_set) in &self.web {
            let scope = if self.allow_funnel.get(host_port).copied().unwrap_or(false) {
                ServeScope::Funnel
            } else {
                ServeScope::Tailnet
            };

            for (path, handler) in &handler_set.handlers {
                entries.push(ServeEntry {
                    host_port: host_port.clone(),
                    path: path.clone(),
                    target: handler.target(),
                    scope,
                });
            }
        }

        // Raw TCP forwards have no web handler, so they need their own pass.
        for (port, handler) in &self.tcp {
            if let Some(forward) = &handler.tcp_forward {
                entries.push(ServeEntry {
                    host_port: port.clone(),
                    path: String::new(),
                    target: forward.clone(),
                    scope: if self.allow_funnel.get(port).copied().unwrap_or(false) {
                        ServeScope::Funnel
                    } else {
                        ServeScope::Tailnet
                    },
                });
            }
        }

        entries.sort_by(|a, b| a.host_port.cmp(&b.host_port).then(a.path.cmp(&b.path)));
        entries
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.web.is_empty() && self.tcp.is_empty()
    }
}
