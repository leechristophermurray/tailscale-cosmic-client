//! The LocalAPI client against a stand-in tailscaled on a real UNIX socket.
//!
//! These check what the parsing tests cannot: the request actually sent — the
//! method, the path, the Host and CSRF headers tailscaled insists on, the query
//! string — and how the client behaves when the socket answers badly or not at
//! all.

use std::time::Duration;

use futures_util::StreamExt;
use http_stub::{Reply, Stub};
use tailscale_localapi::{Error, LocalApi, MaskedPrefs, PingType};

const STATUS: &str = include_str!("fixtures/status.json");

/// A unique socket path per test, so parallel tests never share one.
fn socket(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "tsapi-{name}-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ))
}

async fn daemon(
    name: &str,
    handler: impl Fn(&http_stub::Request) -> Reply + Send + Sync + 'static,
) -> (Stub, LocalApi) {
    let path = socket(name);
    let stub = Stub::unix(&path, handler).await;
    (stub, LocalApi::with_socket(&path))
}

#[tokio::test]
async fn status_sends_the_headers_tailscaled_requires() {
    let (stub, api) = daemon("status", |_| Reply::json(200, STATUS)).await;

    let status = api.status().await.expect("status decodes");
    assert_eq!(status.peer.len(), 6);

    let request = stub.only_request();
    assert_eq!(request.method, "GET");
    assert_eq!(request.target, "/localapi/v0/status");
    // tailscaled rejects requests whose Host it does not recognise.
    assert_eq!(request.header("host"), Some("local-tailscaled.sock"));
    // And marks non-browser clients with this, as its CSRF defence.
    assert_eq!(request.header("sec-tailscale"), Some("localapi"));
}

#[tokio::test]
async fn a_prefs_write_is_a_patch_carrying_only_the_masked_fields() {
    let (stub, api) = daemon("prefs", |_| Reply::json(200, r#"{"WantRunning":false}"#)).await;

    let prefs = api
        .set_prefs(MaskedPrefs::new().want_running(false))
        .await
        .expect("write accepted");
    assert!(
        !prefs.want_running,
        "the daemon's resulting prefs are returned"
    );

    let request = stub.only_request();
    assert_eq!(request.method, "PATCH");
    assert_eq!(request.target, "/localapi/v0/prefs");
    assert_eq!(request.header("content-type"), Some("application/json"));

    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("JSON body");
    assert_eq!(
        body,
        serde_json::json!({"WantRunning": false, "WantRunningSet": true})
    );
}

/// An empty masked write would change nothing, so it must not be sent.
#[tokio::test]
async fn an_empty_prefs_write_reads_instead_of_writing() {
    let (stub, api) = daemon("noop", |_| Reply::json(200, "{}")).await;

    api.set_prefs(MaskedPrefs::new())
        .await
        .expect("reads prefs");

    let request = stub.only_request();
    assert_eq!(
        request.method, "GET",
        "nothing to change means nothing is written"
    );
}

/// Go sends `null` for an empty slice or an absent config. That is empty, not
/// an error — getting this wrong broke the Taildrop view on a quiet machine.
#[tokio::test]
async fn null_list_and_config_responses_are_empty_not_errors() {
    let (_stub, api) = daemon("null", |request| match request.path() {
        "/localapi/v0/serve-config" => Reply::json(200, "null"),
        "/localapi/v0/files/" => Reply::json(200, "null"),
        "/localapi/v0/file-targets" => Reply::json(200, ""),
        _ => Reply::empty(404),
    })
    .await;

    assert!(api.serve_config().await.expect("serve config").is_empty());
    assert!(api.waiting_files().await.expect("waiting files").is_empty());
    assert!(api.file_targets().await.expect("file targets").is_empty());
}

#[tokio::test]
async fn a_refusal_carries_the_daemon_message_and_is_not_unreachable() {
    let (_stub, api) = daemon("refused", |_| {
        Reply::text(403, "access denied: not the operator\n")
    })
    .await;

    let error = api.prefs().await.expect_err("refused");

    match &error {
        Error::Status { status, body } => {
            assert_eq!(status.as_u16(), 403);
            assert_eq!(body, "access denied: not the operator");
        }
        other => panic!("expected a status error, got {other:?}"),
    }
    // A refusal is not "tailscaled is down" — the UI shows these differently.
    assert!(!error.is_unreachable());
}

#[tokio::test]
async fn a_missing_socket_reads_as_the_daemon_being_unreachable() {
    let api = LocalApi::with_socket(socket("absent"));
    let error = api.status().await.expect_err("no daemon");
    assert!(error.is_unreachable(), "got {error:?}");
}

#[tokio::test]
async fn ping_and_exit_node_calls_address_the_right_endpoints() {
    let (stub, api) = daemon("calls", |request| match request.path() {
        "/localapi/v0/ping" => Reply::json(
            200,
            r#"{"LatencySeconds":0.012,"Endpoint":"1.2.3.4:41641"}"#,
        ),
        _ => Reply::json(200, "{}"),
    })
    .await;

    let ping = api.ping("100.64.0.9", PingType::Disco).await.expect("ping");
    assert!(ping.is_direct());
    api.set_exit_node("nEXIT").await.expect("exit node");

    let requests = stub.requests();
    assert_eq!(requests[0].method, "POST");
    assert_eq!(
        requests[0].target,
        "/localapi/v0/ping?ip=100.64.0.9&type=disco"
    );

    let exit: serde_json::Value = serde_json::from_slice(&requests[1].body).unwrap();
    assert_eq!(exit["ExitNodeID"], "nEXIT");
    assert_eq!(exit["ExitNodeIDSet"], true);
}

/// Filenames arrive with spaces and non-ASCII characters; unencoded, they break
/// the request line.
#[tokio::test]
async fn file_transfers_percent_encode_the_filename() {
    let (stub, api) = daemon("files", |_| Reply::empty(200)).await;

    api.send_file("nPEER", "Q3 report (final).pdf", b"%PDF".to_vec())
        .await
        .expect("send");
    api.fetch_file("résumé.pdf").await.expect("fetch");
    api.acknowledge_file("résumé.pdf")
        .await
        .expect("acknowledge");

    let requests = stub.requests();
    assert_eq!(requests[0].method, "PUT");
    assert_eq!(
        requests[0].target,
        "/localapi/v0/file-put/nPEER/Q3%20report%20%28final%29.pdf"
    );
    assert_eq!(requests[0].body, b"%PDF");

    assert_eq!(requests[1].method, "GET");
    assert_eq!(
        requests[1].target,
        "/localapi/v0/files/r%C3%A9sum%C3%A9.pdf"
    );
    assert_eq!(requests[2].method, "DELETE");
    assert_eq!(
        requests[2].target,
        "/localapi/v0/files/r%C3%A9sum%C3%A9.pdf"
    );
}

/// The notify mask was once one bit off, so initial prefs never arrived. Pin
/// the exact value: engine updates, initial state, initial prefs, no keys.
#[tokio::test]
async fn watching_the_bus_asks_for_the_right_notifications() {
    let (stub, api) = daemon("mask", |_| Reply::json(200, "")).await;

    let stream = api.watch().await.expect("watch starts");
    drop(stream);

    let request = stub.only_request();
    assert_eq!(request.path(), "/localapi/v0/watch-ipn-bus");
    assert_eq!(request.query(), Some("mask=23"));
}

/// The bus writes one JSON object per line, but the transport delivers bytes in
/// whatever pieces it likes. Lines split across pieces, several lines in one
/// piece, blank keep-alive lines and a final line with no newline must all come
/// out as whole frames.
#[tokio::test]
async fn bus_frames_are_reassembled_across_chunk_boundaries() {
    let pieces: Vec<Vec<u8>> = [
        r#"{"State":6,"Sessi"#, // a line split mid-token
        "onID\":\"abc\"}\n\n",  // its end, then a blank line
        "{\"Engine\":{\"RBytes\":100,\"WBytes\":50}}\n{\"State\":4}\n", // two lines at once
        r#"{"Engine":{"RBytes":200,"WBytes":90}}"#, // last line, no newline
    ]
    .iter()
    .map(|piece| piece.as_bytes().to_vec())
    .collect();

    let (_stub, api) = daemon("chunks", move |_| Reply::Stream {
        status: 200,
        pieces: pieces.clone(),
        pause: Duration::from_millis(30),
    })
    .await;

    let frames: Vec<_> = api
        .watch()
        .await
        .expect("watch starts")
        .collect::<Vec<_>>()
        .await;

    let frames: Vec<_> = frames
        .into_iter()
        .map(|frame| frame.expect("every frame decodes"))
        .collect();

    assert_eq!(
        frames.len(),
        4,
        "blank lines are skipped, split lines rejoined"
    );
    assert_eq!(frames[0].session_id.as_deref(), Some("abc"));
    assert_eq!(frames[1].engine.map(|e| e.rx_bytes), Some(100));
    assert_eq!(
        frames[2].state,
        Some(tailscale_localapi::BackendState::Stopped)
    );
    assert_eq!(
        frames[3].engine.map(|e| e.rx_bytes),
        Some(200),
        "the final line without a newline is not lost"
    );
}

#[tokio::test]
async fn a_bus_that_refuses_to_start_is_an_error_not_an_empty_stream() {
    let (_stub, api) = daemon("busrefused", |_| Reply::text(403, "denied")).await;

    match api.watch().await {
        Err(Error::Status { status, .. }) => assert_eq!(status.as_u16(), 403),
        Err(other) => panic!("expected a status error, got {other:?}"),
        Ok(_) => panic!("a refused bus must not look like a quiet one"),
    }
}
