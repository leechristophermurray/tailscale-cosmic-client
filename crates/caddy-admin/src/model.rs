//! The slice of Caddy's config the UI reads and writes.
//!
//! Caddy's configuration is a single open-ended JSON document, and most of it
//! is none of this client's business. Rather than model the whole schema and
//! risk dropping fields on a round trip, the types here cover the HTTP routes
//! the UI renders and keep everything else as raw JSON.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// One HTTP server block, e.g. Caddy's default `srv0`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Server {
    /// Addresses the server listens on, e.g. `[":443"]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listen: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<Route>,

    /// Everything else Caddy had here, preserved verbatim so a write does not
    /// silently discard settings this client does not understand.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A route: match some requests, then handle them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Route {
    /// Caddy's optional identifier, usable with the `/id/<id>` endpoint.
    #[serde(rename = "@id", default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(rename = "match", default, skip_serializing_if = "Vec::is_empty")]
    pub matchers: Vec<Matcher>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handle: Vec<Handler>,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub terminal: bool,

    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Route {
    /// Hostnames this route answers for, gathered across all its matchers.
    #[must_use]
    pub fn hosts(&self) -> Vec<&str> {
        self.matchers
            .iter()
            .flat_map(|m| m.host.iter())
            .map(String::as_str)
            .collect()
    }

    /// Path prefixes this route matches, if any.
    #[must_use]
    pub fn paths(&self) -> Vec<&str> {
        self.matchers
            .iter()
            .flat_map(|m| m.path.iter())
            .map(String::as_str)
            .collect()
    }

    /// Where this route sends traffic, flattened for display.
    #[must_use]
    pub fn upstreams(&self) -> Vec<String> {
        self.handle.iter().flat_map(Handler::upstreams).collect()
    }

    /// A one-line description of what this route does.
    #[must_use]
    pub fn summary(&self) -> String {
        let upstreams = self.upstreams();
        if upstreams.is_empty() {
            self.handle
                .first()
                .map_or_else(|| "no handler".to_string(), |h| h.handler.clone())
        } else {
            upstreams.join(", ")
        }
    }
}

/// A request matcher. Only the fields the UI shows are named.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Matcher {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,

    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A handler. `handler` is Caddy's discriminator field.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Handler {
    pub handler: String,

    /// Present on `reverse_proxy` handlers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upstreams: Vec<Upstream>,

    /// `subroute` handlers nest further routes inside themselves, which is how
    /// a Caddyfile's site blocks are compiled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<Route>,

    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Handler {
    /// Upstream dials this handler forwards to, descending into subroutes.
    #[must_use]
    pub fn upstreams(&self) -> Vec<String> {
        let mut out: Vec<String> = self.upstreams.iter().map(|u| u.dial.clone()).collect();
        for route in &self.routes {
            out.extend(route.upstreams());
        }
        out
    }
}

/// One addressable site, flattened out of the route tree for display.
///
/// A Caddyfile compiles to nested `subroute` handlers: an outer route matching
/// the shared suffix, wrapping one child route per site. Rendering the raw
/// route list therefore shows a single row with every upstream in the tailnet
/// jammed together — the tree has to be walked to the leaves to recover the
/// `host -> upstream` pairs a person actually configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// The hostname this site answers on.
    pub host: String,
    /// Path prefix, when the site is mounted under one.
    pub path: Option<String>,
    /// Where requests go: an upstream dial, a directory, or a handler name.
    pub target: String,
    /// The `@id` of the nearest enclosing route that has one. Only routes this
    /// client created carry an id, so this is `None` for anything compiled from
    /// a Caddyfile — which is why removal is offered only where it exists.
    pub id: Option<String>,
}

/// A reverse-proxy backend.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Upstream {
    /// Host and port, e.g. `127.0.0.1:8080`, or a unix socket path.
    pub dial: String,

    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Route {
    /// Build a reverse-proxy route: match `host`, forward to `upstream`.
    ///
    /// The `@id` is set so the route can later be read or deleted through
    /// Caddy's `/id/` endpoint without depending on its position in the array.
    #[must_use]
    pub fn reverse_proxy(id: &str, host: &str, upstream: &str) -> Self {
        Self {
            id: Some(id.to_string()),
            matchers: vec![Matcher {
                host: vec![host.to_string()],
                ..Default::default()
            }],
            handle: vec![Handler {
                handler: "reverse_proxy".to_string(),
                upstreams: vec![Upstream {
                    dial: upstream.to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            terminal: true,
            extra: BTreeMap::new(),
        }
    }
}

impl Route {
    /// Every addressable site under this route.
    #[must_use]
    pub fn sites(&self) -> Vec<Site> {
        let mut out = Vec::new();
        self.collect_sites(None, None, &mut out);
        out
    }

    /// Walk the route tree, carrying the nearest enclosing host, path, and id
    /// down into nested subroutes.
    ///
    /// An inner route's own matcher wins over the one it inherits: that is what
    /// turns an outer `*.example.com` wrapper into the concrete `git.example.com`
    /// of the child it contains.
    fn collect_sites(
        &self,
        inherited_host: Option<&str>,
        inherited_path: Option<&str>,
        out: &mut Vec<Site>,
    ) {
        let own_hosts = self.hosts();
        let own_paths = self.paths();

        let host = own_hosts.first().copied().or(inherited_host);
        let path = own_paths.first().copied().or(inherited_path);
        let id = self.id.as_deref();

        for handler in &self.handle {
            handler.collect_sites(host, path, id, out);
        }
    }
}

impl Handler {
    fn collect_sites(
        &self,
        host: Option<&str>,
        path: Option<&str>,
        id: Option<&str>,
        out: &mut Vec<Site>,
    ) {
        // A subroute is scaffolding, not a destination; keep descending.
        if !self.routes.is_empty() {
            for route in &self.routes {
                let mut nested = Vec::new();
                route.collect_sites(host, path, &mut nested);

                // A nested route without its own id still belongs to the
                // outermost one that has one.
                for mut site in nested {
                    if site.id.is_none() {
                        site.id = id.map(ToString::to_string);
                    }
                    out.push(site);
                }
            }
            return;
        }

        // Caddy ends every compiled server with a hostless fallback. It is not
        // a site anyone configured, so it is not one anyone wants listed.
        let Some(host) = host else {
            return;
        };

        let target = if self.upstreams.is_empty() {
            match self.handler.as_str() {
                // A bare handler name is more honest than pretending to know
                // where it sends traffic.
                "file_server" => "static files".to_string(),
                "static_response" => "static response".to_string(),
                other => other.to_string(),
            }
        } else {
            self.upstreams
                .iter()
                .map(|upstream| upstream.dial.clone())
                .collect::<Vec<_>>()
                .join(", ")
        };

        out.push(Site {
            host: host.to_string(),
            path: path.map(ToString::to_string),
            target,
            id: id.map(ToString::to_string),
        });
    }
}

impl Server {
    /// Every site this server publishes, in hostname order.
    #[must_use]
    pub fn sites(&self) -> Vec<Site> {
        let mut sites: Vec<Site> = self.routes.iter().flat_map(Route::sites).collect();
        sites.sort_by(|a, b| a.host.cmp(&b.host).then_with(|| a.path.cmp(&b.path)));
        sites
    }
}
