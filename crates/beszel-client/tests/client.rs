//! The Beszel client against a stand-in PocketBase hub.
//!
//! The things worth pinning here live on the wire: that the session token is
//! actually sent, that a refused password reads as a refused password rather
//! than a generic 400, and that a system id is escaped before it is spliced
//! into a PocketBase filter expression.

use beszel_client::{BeszelHub, Error, StatsPeriod};
use http_stub::{Reply, Stub};

const SYSTEMS: &str = r#"{"page":1,"perPage":500,"totalItems":1,"items":[
  {"id":"abc","name":"homeforge","host":"100.64.0.1","port":"45876","status":"up",
   "info":{"cpu":18.2,"mp":15.2,"dp":64.8,"v":"0.18.7"}}]}"#;

const ALERT_HISTORY: &str = r#"{"page":1,"perPage":20,"totalItems":2,"items":[
  {"id":"h2","system":"abc","alert_id":"a2","name":"Status","value":0,
   "created":"2026-09-15 09:00:00.000Z","resolved":"",
   "expand":{"system":{"id":"abc","name":"homeforge"}}},
  {"id":"h1","system":"abc","alert_id":"a1","name":"Disk","value":80,
   "created":"2026-09-15 08:00:00.000Z","resolved":"2026-09-15 08:30:00.000Z",
   "expand":{"system":{"id":"abc","name":"homeforge"}}}]}"#;

const AUTH: &str = r#"{"token":"tok-123","record":{"id":"u1","email":"you@example.com"}}"#;

/// A hub that accepts one account and demands its token on everything else.
async fn hub() -> Stub {
    Stub::tcp(|request| match request.path() {
        "/api/health" => Reply::json(200, r#"{"message":"API is healthy."}"#),
        "/api/collections/users/auth-with-password" => {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap_or_default();
            if body["identity"] == "you@example.com" && body["password"] == "right" {
                Reply::json(200, AUTH)
            } else {
                // PocketBase's real answer to bad credentials is a 400.
                Reply::json(400, r#"{"message":"Failed to authenticate."}"#)
            }
        }
        _ if request.header("authorization") != Some("tok-123") => {
            Reply::json(401, r#"{"message":"The request requires valid record authorization token."}"#)
        }
        "/api/collections/systems/records" => Reply::json(200, SYSTEMS),
        "/api/collections/alerts_history/records" => Reply::json(200, ALERT_HISTORY),
        "/api/beszel/info" => Reply::json(200, r#"{"key":"ssh-ed25519 AAAA","v":"0.18.7"}"#),
        "/api/collections/system_stats/records" => Reply::json(
            200,
            r#"{"items":[{"id":"r1","system":"abc","type":"1m","created":"x","stats":{"cpu":18.2}}]}"#,
        ),
        "/api/collections/container_stats/records" => Reply::json(
            200,
            r#"{"items":[{"id":"c","system":"abc","type":"1m","created":"x","stats":[
               {"n":"idle","c":0.5,"m":10},{"n":"busy","c":40.0,"m":900},{"n":"mid","c":8.0,"m":200}]}]}"#,
        ),
        _ => Reply::json(404, r#"{"message":"not found"}"#),
    })
    .await
}

#[tokio::test]
async fn signing_in_sends_the_credentials_and_keeps_the_token() {
    let stub = hub().await;
    let mut hub = BeszelHub::new(stub.url()).unwrap();

    let auth = hub
        .authenticate("you@example.com", "right")
        .await
        .expect("signed in");
    assert_eq!(auth.token, "tok-123");
    assert!(hub.is_authenticated());

    let systems = hub.systems().await.expect("systems");
    assert_eq!(systems[0].name, "homeforge");

    let requests = stub.requests();
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].header("content-type"), Some("application/json"));
    assert_eq!(
        requests[1].header("authorization"),
        Some("tok-123"),
        "the session token must accompany every later request"
    );
}

/// PocketBase answers a wrong password with 400. Surfacing that as "bad
/// request" would send the user hunting for a bug instead of retyping.
#[tokio::test]
async fn a_wrong_password_reads_as_rejected_credentials() {
    let stub = hub().await;
    let mut hub = BeszelHub::new(stub.url()).unwrap();

    let error = hub
        .authenticate("you@example.com", "wrong")
        .await
        .expect_err("refused");

    assert!(matches!(error, Error::Unauthorized), "got {error:?}");
    assert!(error.needs_auth());
    assert!(
        !hub.is_authenticated(),
        "a failed sign-in must not leave a token behind"
    );
}

/// Reading before signing in must fail locally, without sending an
/// unauthenticated request the hub would only refuse.
#[tokio::test]
async fn reading_before_signing_in_sends_nothing() {
    let stub = hub().await;
    let hub = BeszelHub::new(stub.url()).unwrap();

    assert!(matches!(hub.systems().await, Err(Error::NotAuthenticated)));
    assert!(stub.requests().is_empty());
}

/// An expired token is the normal way a long-lived session ends; it has to be
/// recognisable so the app can sign in again rather than show an error.
#[tokio::test]
async fn an_expired_token_is_recognised_as_needing_sign_in() {
    let stub = hub().await;
    let hub = BeszelHub::new(stub.url()).unwrap().with_token("stale");

    let error = hub.systems().await.expect_err("token refused");
    assert!(error.needs_auth(), "got {error:?}");
}

#[tokio::test]
async fn hub_info_carries_the_key_agents_are_installed_with() {
    let stub = hub().await;
    let mut hub = BeszelHub::new(stub.url()).unwrap();
    hub.authenticate("you@example.com", "right").await.unwrap();

    let info = hub.info().await.expect("info");
    assert_eq!(info.key, "ssh-ed25519 AAAA");
    assert_eq!(info.version, "0.18.7");
}

#[tokio::test]
async fn stats_queries_filter_by_system_and_resolution() {
    let stub = hub().await;
    let mut hub = BeszelHub::new(stub.url()).unwrap();
    hub.authenticate("you@example.com", "right").await.unwrap();

    let latest = hub
        .latest_stats("abc")
        .await
        .expect("stats")
        .expect("a sample");
    assert!((latest.cpu - 18.2).abs() < f64::EPSILON);

    let request = stub
        .requests()
        .into_iter()
        .find(|r| r.path() == "/api/collections/system_stats/records")
        .expect("stats requested");
    let query = request.query().expect("query");

    // system='abc' && type='1m', percent-encoded.
    assert!(
        query.contains("filter=system%3D%27abc%27%20%26%26%20type%3D%271m%27"),
        "{query}"
    );
    assert!(query.contains("sort=-created"), "newest first");
    assert!(
        query.contains("perPage=1"),
        "latest asks for one row, not a month"
    );
}

/// A requested history can never ask for more than PocketBase serves in a page.
#[tokio::test]
async fn history_requests_are_capped_at_a_page() {
    let stub = hub().await;
    let mut hub = BeszelHub::new(stub.url()).unwrap();
    hub.authenticate("you@example.com", "right").await.unwrap();

    hub.stats("abc", StatsPeriod::EightHours, 10_000)
        .await
        .expect("stats");

    let request = stub.requests().pop().expect("stats requested");
    let query = request.query().unwrap();
    assert!(query.contains("type%3D%27480m%27"), "{query}");
    assert!(query.contains("perPage=500"), "{query}");
}

/// A system id is spliced into a filter expression. Unescaped, a quote in it
/// would change what the query selects.
#[tokio::test]
async fn a_quote_in_a_system_id_cannot_alter_the_filter() {
    let stub = hub().await;
    let mut hub = BeszelHub::new(stub.url()).unwrap();
    hub.authenticate("you@example.com", "right").await.unwrap();

    let _ = hub.latest_stats("x' || system!='").await;

    let request = stub.requests().pop().expect("stats requested");
    let query = request.query().unwrap();
    // The injected quotes arrive escaped (\\'), so the value stays one literal.
    assert!(query.contains("%5C%27"), "quotes must be escaped: {query}");
    assert!(
        !query.contains("system%3D%27x%27%20%7C%7C"),
        "the filter was broken out of: {query}"
    );
}

#[tokio::test]
async fn containers_come_back_busiest_first() {
    let stub = hub().await;
    let mut hub = BeszelHub::new(stub.url()).unwrap();
    hub.authenticate("you@example.com", "right").await.unwrap();

    let names: Vec<String> = hub
        .containers("abc")
        .await
        .expect("containers")
        .into_iter()
        .map(|c| c.name)
        .collect();

    assert_eq!(names, ["busy", "mid", "idle"]);
}

#[tokio::test]
async fn health_needs_no_credentials() {
    let stub = hub().await;
    BeszelHub::new(stub.url())
        .unwrap()
        .health()
        .await
        .expect("healthy");
    assert!(stub.only_request().header("authorization").is_none());
}

#[tokio::test]
async fn a_non_http_address_is_rejected_before_any_request() {
    assert!(matches!(
        BeszelHub::new("mon.example.com"),
        Err(Error::BadUrl { .. })
    ));
}

#[tokio::test]
async fn an_address_with_nothing_listening_is_unreachable() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let hub = BeszelHub::new(format!("http://127.0.0.1:{port}")).unwrap();

    let error = hub.health().await.expect_err("nothing there");
    assert!(error.is_unreachable(), "got {error:?}");
}

/// A trailing slash in a pasted URL must not produce `//api/...` paths.
#[tokio::test]
async fn a_trailing_slash_in_the_address_is_tolerated() {
    let stub = hub().await;
    BeszelHub::new(format!("{}/", stub.url()))
        .unwrap()
        .health()
        .await
        .expect("healthy");
    assert_eq!(stub.only_request().path(), "/api/health");
}

/// Alert history is only readable as a list, newest first, and the system name
/// has to be asked for with `expand`; without it a notification could only name
/// a record id.
#[tokio::test]
async fn alert_history_is_listed_newest_first_with_system_names() {
    let stub = hub().await;
    let hub = BeszelHub::new(stub.url()).unwrap().with_token("tok-123");

    let history = hub.alert_history(20).await.unwrap();
    assert_eq!(history.len(), 2);
    assert!(history[0].is_active());
    assert_eq!(history[0].system_name(), Some("homeforge"));
    assert!(!history[1].is_active());

    let request = &stub.requests()[0];
    assert_eq!(request.path(), "/api/collections/alerts_history/records");
    for part in ["sort=-created", "perPage=20", "expand=system"] {
        assert!(
            request.target.contains(part),
            "{} lacks {part}",
            request.target
        );
    }
    assert_eq!(request.header("authorization"), Some("tok-123"));
}
