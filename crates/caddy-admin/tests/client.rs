//! The Caddy admin client against a stand-in admin API.
//!
//! The bug that made Caddy unreachable was invisible above the wire: requests
//! went out in absolute-form, and Go's HTTP server took the Host from that URL
//! instead of the header, so Caddy refused them. These tests read the raw
//! request line.

use caddy_admin::{CaddyAdmin, Endpoint, Error, Route};
use http_stub::{Reply, Stub};

/// An endpoint that dials the stub but claims Caddy's loopback origin, exactly
/// as a tunnelled connection does.
fn tunnelled(stub: &Stub) -> CaddyAdmin {
    CaddyAdmin::new(Endpoint::tunnelled(stub.port()))
}

#[tokio::test]
async fn requests_are_sent_in_origin_form_with_the_claimed_host() {
    let stub = Stub::tcp(|_| Reply::json(200, "null")).await;

    tunnelled(&stub).probe().await.expect("probe");

    let request = stub.only_request();
    assert_eq!(
        request.target, "/config/",
        "absolute-form makes Caddy read the Host from the URL and refuse"
    );
    assert!(!request.target.starts_with("http"));
    assert_eq!(
        request.header("host"),
        Some("localhost:2019"),
        "a tunnel must claim the origin Caddy allows, not the forwarded port"
    );
}

/// Reproduces the real failure: a Caddy that checks origins, like the one that
/// answered `403 host not allowed`.
#[tokio::test]
async fn a_caddy_checking_origins_accepts_the_tunnelled_request() {
    let stub = Stub::tcp(|request| {
        if request.target.starts_with("http") {
            return Reply::json(403, r#"{"error":"host not allowed: from URL"}"#);
        }
        match request.header("host") {
            Some("localhost:2019" | "127.0.0.1:2019") => Reply::json(200, "{}"),
            other => Reply::json(
                403,
                format!(r#"{{"error":"host not allowed: {}"}}"#, other.unwrap_or("")),
            ),
        }
    })
    .await;

    tunnelled(&stub)
        .probe()
        .await
        .expect("an origin-checking Caddy accepts it");
}

#[tokio::test]
async fn an_unconfigured_caddy_still_probes_as_caddy() {
    let stub = Stub::tcp(|_| Reply::json(200, "null")).await;
    tunnelled(&stub)
        .probe()
        .await
        .expect("null config is still Caddy");
}

/// Something else listening on the port is not Caddy, however it answers.
#[tokio::test]
async fn a_non_caddy_server_on_the_port_is_rejected() {
    let stub = Stub::tcp(|_| Reply::text(200, "<html>welcome</html>")).await;

    match tunnelled(&stub).probe().await {
        Err(Error::BadEndpoint(_)) => {}
        other => panic!("expected BadEndpoint, got {other:?}"),
    }
}

/// Caddy explains refusals in a JSON body; that message is the useful part.
#[tokio::test]
async fn a_rejection_surfaces_caddys_own_message() {
    let stub =
        Stub::tcp(|_| Reply::json(400, r#"{"error":"loading config: invalid upstream"}"#)).await;

    let route = Route::reverse_proxy("id", "app.example.ts.net", "not a dial");
    match tunnelled(&stub).add_route("srv0", &route).await {
        Err(Error::Rejected { status, message }) => {
            assert_eq!(status.as_u16(), 400);
            assert_eq!(message, "loading config: invalid upstream");
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
}

/// Appending must use the `/...` suffix. Without it the POST would replace the
/// whole route list and take every existing site down.
#[tokio::test]
async fn adding_a_route_appends_rather_than_replacing() {
    let stub = Stub::tcp(|_| Reply::json(200, "")).await;

    let route = Route::reverse_proxy(
        "cosmic-tailscale-app",
        "app.example.ts.net",
        "127.0.0.1:8080",
    );
    tunnelled(&stub)
        .add_route("srv0", &route)
        .await
        .expect("added");

    let request = stub.only_request();
    assert_eq!(request.method, "POST");
    assert_eq!(request.target, "/config/apps/http/servers/srv0/routes/...");

    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("JSON");
    assert_eq!(body["@id"], "cosmic-tailscale-app");
    assert_eq!(body["handle"][0]["upstreams"][0]["dial"], "127.0.0.1:8080");
}

#[tokio::test]
async fn removing_a_route_addresses_it_by_id() {
    let stub = Stub::tcp(|_| Reply::json(200, "")).await;

    tunnelled(&stub)
        .delete_route_by_id("cosmic-tailscale-app")
        .await
        .expect("removed");

    let request = stub.only_request();
    assert_eq!(request.method, "DELETE");
    assert_eq!(request.target, "/id/cosmic-tailscale-app");
}

/// The shape a Caddyfile compiles to, read end to end: one wrapper route whose
/// children are the real sites.
#[tokio::test]
async fn sites_are_read_from_every_server_and_flattened() {
    let servers = r#"{
      "srv0": {"listen": [":443"], "routes": [{
        "match": [{"host": ["*.example.com"]}],
        "handle": [{"handler": "subroute", "routes": [
          {"match": [{"host": ["git.example.com"]}],
           "handle": [{"handler": "reverse_proxy", "upstreams": [{"dial": "127.0.0.1:3000"}]}]},
          {"match": [{"host": ["docs.example.com"]}],
           "handle": [{"handler": "reverse_proxy", "upstreams": [{"dial": "100.64.0.9:8080"}]}]}
        ]}]
      }]},
      "srv1": {"listen": [":80"], "routes": [
        {"match": [{"host": ["app.example.ts.net"]}],
         "handle": [{"handler": "reverse_proxy", "upstreams": [{"dial": "127.0.0.1:9000"}]}]}
      ]}
    }"#;
    let stub = Stub::tcp(move |_| Reply::json(200, servers)).await;

    let sites = tunnelled(&stub).sites().await.expect("sites");
    let hosts: Vec<&str> = sites.iter().map(|s| s.host.as_str()).collect();

    assert_eq!(
        hosts,
        ["app.example.ts.net", "docs.example.com", "git.example.com"]
    );
    assert_eq!(stub.only_request().target, "/config/apps/http/servers");
}

/// An unset config path answers `null`; that is an empty list, not an error.
#[tokio::test]
async fn a_server_with_no_routes_is_empty() {
    let stub = Stub::tcp(|_| Reply::json(200, "null")).await;
    assert!(
        tunnelled(&stub)
            .routes("srv0")
            .await
            .expect("routes")
            .is_empty()
    );
}

#[tokio::test]
async fn nothing_listening_reads_as_unreachable() {
    // Bind and release a port so nothing is listening on it.
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };

    let error = CaddyAdmin::new(Endpoint::local(port))
        .probe()
        .await
        .expect_err("nothing there");
    assert!(error.is_unreachable(), "got {error:?}");
}
