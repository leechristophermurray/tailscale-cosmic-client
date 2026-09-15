//! Tests for the Caddy config subset.
//!
//! The important property is that a read-modify-write does not silently drop
//! configuration this client does not model, because a dropped field on a live
//! reverse proxy takes a site down.

use caddy_admin::{Endpoint, Route, Server};

/// A Caddyfile compiles to nested subroutes, so upstreams are not at the top
/// level of a route's handlers.
const COMPILED_SITE: &str = r#"{
  "@id": "site-1",
  "match": [{"host": ["app.example.ts.net"]}],
  "handle": [{
    "handler": "subroute",
    "routes": [{
      "handle": [{
        "handler": "reverse_proxy",
        "upstreams": [{"dial": "127.0.0.1:8080"}]
      }]
    }]
  }],
  "terminal": true
}"#;

#[test]
fn finds_upstreams_nested_inside_subroutes() {
    let route: Route = serde_json::from_str(COMPILED_SITE).expect("route decodes");

    assert_eq!(route.hosts(), vec!["app.example.ts.net"]);
    assert_eq!(route.upstreams(), vec!["127.0.0.1:8080"]);
    assert_eq!(route.summary(), "127.0.0.1:8080");
    assert_eq!(route.id.as_deref(), Some("site-1"));
}

/// Fields the client does not model must survive a round trip untouched.
#[test]
fn unmodelled_fields_round_trip() {
    let original = r#"{
        "listen": [":443"],
        "routes": [],
        "automatic_https": {"disable_redirects": true},
        "read_timeout": "10s"
    }"#;

    let server: Server = serde_json::from_str(original).expect("server decodes");
    let round_tripped = serde_json::to_value(&server).expect("server encodes");

    assert_eq!(
        round_tripped["automatic_https"]["disable_redirects"],
        serde_json::json!(true),
        "an unmodelled field was dropped, which would change the live config"
    );
    assert_eq!(round_tripped["read_timeout"], serde_json::json!("10s"));
    assert_eq!(round_tripped["listen"][0], serde_json::json!(":443"));
}

#[test]
fn builds_a_reverse_proxy_route() {
    let route = Route::reverse_proxy("my-id", "grafana.example.ts.net", "127.0.0.1:3000");
    let json = serde_json::to_value(&route).expect("route encodes");

    assert_eq!(json["@id"], serde_json::json!("my-id"));
    assert_eq!(
        json["match"][0]["host"][0],
        serde_json::json!("grafana.example.ts.net")
    );
    assert_eq!(
        json["handle"][0]["handler"],
        serde_json::json!("reverse_proxy")
    );
    assert_eq!(
        json["handle"][0]["upstreams"][0]["dial"],
        serde_json::json!("127.0.0.1:3000")
    );
    // Without `terminal`, Caddy would keep evaluating later routes.
    assert_eq!(json["terminal"], serde_json::json!(true));
}

/// A route with no handler should not claim to be empty-but-fine.
#[test]
fn a_route_with_no_handler_says_so() {
    let route: Route = serde_json::from_str(r#"{"match":[{"host":["x.ts.net"]}]}"#).unwrap();
    assert_eq!(route.summary(), "no handler");
}

#[test]
fn endpoints_bracket_ipv6_literals() {
    assert_eq!(
        Endpoint::tailnet("100.64.0.1").authority(),
        "100.64.0.1:2019"
    );
    assert_eq!(Endpoint::local(12019).authority(), "127.0.0.1:12019");

    // An unbracketed IPv6 authority would make the request line unparseable.
    assert_eq!(
        Endpoint::tailnet("fd7a:115c:a1e0::1").authority(),
        "[fd7a:115c:a1e0::1]:2019"
    );
    assert_eq!(
        Endpoint::tailnet("100.64.0.1").to_string(),
        "http://100.64.0.1:2019"
    );
}

/// Caddy answers `403 host not allowed` when the `Host` header is not one of
/// its configured origins. A tunnel's local port never is, so a tunnelled
/// endpoint has to claim the loopback origin Caddy allows by default.
#[test]
fn a_tunnelled_endpoint_claims_the_origin_caddy_allows() {
    let tunnelled = Endpoint::tunnelled(12019);

    // Dial the forwarded port...
    assert_eq!(tunnelled.authority(), "127.0.0.1:12019");
    // ...but claim the origin the remote Caddy recognises.
    assert_eq!(tunnelled.origin(), "localhost:2019");
    assert_ne!(
        tunnelled.origin(),
        tunnelled.authority(),
        "claiming the dial authority is exactly what produces a 403"
    );
}

/// Reaching Caddy at an address it knows itself by needs no such fiction.
#[test]
fn a_direct_endpoint_claims_its_own_authority() {
    let direct = Endpoint::tailnet("100.109.86.6");

    assert_eq!(direct.authority(), "100.109.86.6:2019");
    assert_eq!(direct.origin(), direct.authority());
}

/// The shape a Caddyfile actually compiles to: an outer route matching the
/// shared suffix, wrapping one child route per site.
///
/// Reading the top level alone reports a single route with every upstream in
/// it, which is useless for display. This is the regression test for walking
/// down to the leaves.
const COMPILED_CADDYFILE: &str = r#"[{
  "match": [{"host": ["*.example.com"]}],
  "handle": [{
    "handler": "subroute",
    "routes": [
      {
        "match": [{"host": ["git.example.com"]}],
        "handle": [{"handler": "subroute", "routes": [
          {"handle": [{"handler": "reverse_proxy",
                       "upstreams": [{"dial": "127.0.0.1:3000"}]}]}
        ]}]
      },
      {
        "match": [{"host": ["docs.example.com"]}],
        "handle": [{"handler": "subroute", "routes": [
          {"handle": [{"handler": "reverse_proxy",
                       "upstreams": [{"dial": "100.64.0.9:8080"}]}]}
        ]}]
      },
      {
        "handle": [{"handler": "subroute", "routes": [
          {"handle": [{"handler": "static_response"}]}
        ]}]
      }
    ]
  }],
  "terminal": true
}]"#;

#[test]
fn nested_subroutes_flatten_to_one_site_per_host() {
    let routes: Vec<Route> = serde_json::from_str(COMPILED_CADDYFILE).expect("routes decode");
    let server = Server {
        routes,
        ..Default::default()
    };

    let sites = server.sites();
    let by_host: Vec<(&str, &str)> = sites
        .iter()
        .map(|site| (site.host.as_str(), site.target.as_str()))
        .collect();

    assert_eq!(
        by_host,
        vec![
            // The wildcard fallback the Caddyfile ends with, inheriting its
            // host from the wrapper.
            ("*.example.com", "static response"),
            ("docs.example.com", "100.64.0.9:8080"),
            ("git.example.com", "127.0.0.1:3000"),
        ],
        "an inner host matcher must win over the wrapper it is nested in"
    );
}

/// A route this client created carries an `@id`, and its nested sites inherit
/// it — otherwise the row could never be removed.
#[test]
fn nested_sites_inherit_the_owning_route_id() {
    let route: Route = serde_json::from_str(
        r#"{
            "@id": "cosmic-tailscale-app-example-ts-net",
            "match": [{"host": ["app.example.ts.net"]}],
            "handle": [{"handler": "subroute", "routes": [
              {"handle": [{"handler": "reverse_proxy",
                           "upstreams": [{"dial": "127.0.0.1:8080"}]}]}
            ]}]
        }"#,
    )
    .expect("route decodes");

    let sites = route.sites();
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].host, "app.example.ts.net");
    assert_eq!(sites[0].target, "127.0.0.1:8080");
    assert_eq!(
        sites[0].id.as_deref(),
        Some("cosmic-tailscale-app-example-ts-net")
    );
}

/// Caddy ends a compiled server with a hostless fallback. It is not a site
/// anyone configured, so it should not be listed as one.
#[test]
fn a_hostless_handler_is_not_a_site() {
    let route: Route =
        serde_json::from_str(r#"{"handle":[{"handler":"static_response"}]}"#).unwrap();
    assert!(route.sites().is_empty());
}
