//! Decoding tests against real `tailscaled` output.
//!
//! The fixture is a redacted capture from a live daemon rather than a
//! hand-written sample, because the bugs worth catching here are exactly the
//! ones hand-written JSON does not reproduce: Go's `null` for empty slices, the
//! acronym casing in `TailscaleIPs`, and `BackendState` arriving as a string
//! from `status` but an integer from the IPN bus.

use tailscale_localapi::{BackendState, Notify, Prefs, ServeConfig, Status};

fn status() -> Status {
    serde_json::from_str(include_str!("fixtures/status.json")).expect("fixture should decode")
}

#[test]
fn decodes_a_real_status_document() {
    let status = status();

    assert!(status.backend_state.is_running());
    assert_eq!(status.peer.len(), 6);
    assert_eq!(status.tailnet_name(), "tailcaa06c.ts.net");

    let me = status.self_status.as_ref().expect("self is always present");
    assert_eq!(me.display_name(), "fedora");
}

/// `TailscaleIPs` does not round-trip through serde's PascalCase rename, which
/// silently produced machines with no addresses at all.
#[test]
fn peer_addresses_survive_the_acronym_casing() {
    let status = status();

    for peer in status.peers_sorted() {
        assert!(
            !peer.tailscale_ips.is_empty(),
            "{} decoded with no addresses",
            peer.display_name()
        );
        assert!(
            peer.ipv4().is_some_and(|ip| ip.starts_with("100.")),
            "{} has no CGNAT address",
            peer.display_name()
        );
        assert!(peer.ipv6().is_some(), "{} has no IPv6", peer.display_name());
    }
}

/// Go marshals a nil slice as `null`. `Health` is empty on a healthy daemon, so
/// this is the common case, not the edge case.
#[test]
fn null_slices_decode_as_empty() {
    let status: Status = serde_json::from_str(
        r#"{"BackendState":"Running","Health":null,"Self":{"TailscaleIPs":null},"Peer":null}"#,
    )
    .expect("null slices are valid");

    assert!(status.health.is_empty());
    assert!(status.peer.is_empty());
    assert!(
        status
            .self_status
            .expect("self decoded")
            .tailscale_ips
            .is_empty()
    );
}

/// `status` sends the state's name; the IPN bus sends Go's integer. Both have
/// to land on the same variant.
#[test]
fn backend_state_accepts_both_encodings() {
    let from_name: BackendState = serde_json::from_str(r#""Running""#).unwrap();
    let from_integer: BackendState = serde_json::from_str("6").unwrap();

    assert_eq!(from_name, BackendState::Running);
    assert_eq!(from_integer, BackendState::Running);
    assert_eq!(from_name, from_integer);

    // `InUseOtherUser` occupies slot 1, which shifts every later state.
    assert_eq!(
        serde_json::from_str::<BackendState>("4").unwrap(),
        BackendState::Stopped
    );

    // A state from a newer daemon degrades rather than failing to decode.
    assert_eq!(
        serde_json::from_str::<BackendState>("99").unwrap(),
        BackendState::Unknown
    );
}

#[test]
fn ipn_bus_frames_decode() {
    let frame = r#"{"Version":"1.102.3","SessionID":"abc","State":6,"Prefs":null,
        "Engine":{"RBytes":100,"WBytes":50,"NumLive":1,"LiveDERPs":1}}"#;

    let notify: Notify = serde_json::from_str(frame).expect("bus frame decodes");

    assert_eq!(notify.state, Some(BackendState::Running));
    assert!(notify.is_meaningful());

    let engine = notify.engine.expect("engine present");
    assert_eq!(engine.rx_bytes, 100);
    assert_eq!(engine.tx_bytes, 50);
}

/// A frame carrying nothing but a version is noise the UI should drop.
#[test]
fn empty_bus_frames_are_not_meaningful() {
    let notify: Notify = serde_json::from_str(r#"{"Version":"1.102.3"}"#).unwrap();
    assert!(!notify.is_meaningful());
}

/// `ExitNodeAllowLANAccess` is another acronym serde would otherwise mangle.
#[test]
fn prefs_decode_including_acronym_fields() {
    let prefs: Prefs = serde_json::from_str(
        r#"{"WantRunning":true,"ExitNodeID":"nABC","ExitNodeAllowLANAccess":true,
            "CorpDNS":true,"RunSSH":false,"AdvertiseRoutes":["10.0.0.0/24","0.0.0.0/0","::/0"]}"#,
    )
    .expect("prefs decode");

    assert!(prefs.want_running);
    assert!(prefs.exit_node_allow_lan_access);
    assert!(prefs.is_exit_node_active());

    // The two default routes mean "exit node" and are not subnet routes.
    assert!(prefs.advertises_exit_node());
    assert_eq!(prefs.subnet_routes(), vec!["10.0.0.0/24"]);
}

/// A masked prefs write must send only the fields it set, or it would reset
/// everything else to its zero value.
#[test]
fn masked_prefs_only_serialise_what_was_set() {
    let prefs = tailscale_localapi::MaskedPrefs::new().want_running(false);
    let json = serde_json::to_value(&prefs).unwrap();

    assert_eq!(json["WantRunning"], serde_json::json!(false));
    assert_eq!(json["WantRunningSet"], serde_json::json!(true));
    assert!(json.get("CorpDNS").is_none());
    assert!(json.get("ExitNodeID").is_none());

    assert!(tailscale_localapi::MaskedPrefs::new().is_empty());
    assert!(!prefs.is_empty());
}

/// Clearing the exit node means sending an empty string, which must still be
/// serialised rather than skipped as "unset".
#[test]
fn clearing_the_exit_node_is_an_explicit_write() {
    let json =
        serde_json::to_value(tailscale_localapi::MaskedPrefs::new().exit_node_id("")).unwrap();

    assert_eq!(json["ExitNodeID"], serde_json::json!(""));
    assert_eq!(json["ExitNodeIDSet"], serde_json::json!(true));
}

#[test]
fn serve_config_flattens_into_display_rows() {
    let config: ServeConfig = serde_json::from_str(
        r#"{
            "TCP": {"443": {"HTTPS": true}},
            "Web": {
                "app.example.ts.net:443": {
                    "Handlers": {"/": {"Proxy": "http://127.0.0.1:8080"}}
                },
                "public.example.ts.net:443": {
                    "Handlers": {"/": {"Proxy": "http://127.0.0.1:3000"}}
                }
            },
            "AllowFunnel": {"public.example.ts.net:443": true}
        }"#,
    )
    .expect("serve config decodes");

    let entries = config.entries();
    assert_eq!(entries.len(), 2);

    let private = &entries[0];
    assert_eq!(private.url(), "https://app.example.ts.net");
    assert_eq!(private.target, "http://127.0.0.1:8080");
    assert_eq!(private.scope, tailscale_localapi::ServeScope::Tailnet);

    // Funnel means the whole internet can reach it; the UI leans on this.
    let public = &entries[1];
    assert_eq!(public.scope, tailscale_localapi::ServeScope::Funnel);
}

/// `null` is what the daemon sends when nothing is published.
#[test]
fn an_unconfigured_serve_config_is_empty() {
    let config: Option<ServeConfig> = serde_json::from_str("null").unwrap();
    assert!(config.unwrap_or_default().is_empty());
}

#[test]
fn peer_filtering_matches_what_a_user_would_type() {
    let status = status();
    let peer = status.peers_sorted()[0];

    assert!(peer.matches(""));
    assert!(peer.matches(&peer.display_name().to_uppercase()));
    assert!(peer.matches(peer.ipv4().unwrap()));
    assert!(!peer.matches("definitely-not-a-machine"));
}

/// Go's zero time is year 1, which must read as "never", not as a date.
#[test]
fn go_zero_timestamps_read_as_never() {
    let peer: tailscale_localapi::PeerStatus = serde_json::from_str(
        r#"{"HostName":"test","LastSeen":"0001-01-01T00:00:00Z","TailscaleIPs":["100.1.1.1"]}"#,
    )
    .unwrap();

    assert!(peer.last_seen_at().is_none());
    assert!(peer.key_expiry_at().is_none());
    assert!(peer.days_until_key_expiry().is_none());
}

#[test]
fn user_initials_come_from_the_display_name() {
    let profile = tailscale_localapi::UserProfile {
        display_name: "Alex Chen".to_string(),
        login_name: "alex@example.com".to_string(),
        ..Default::default()
    };
    assert_eq!(profile.initials(), "AC");

    // With no display name, the login is the next best source.
    let profile = tailscale_localapi::UserProfile {
        login_name: "alex.chen@example.com".to_string(),
        ..Default::default()
    };
    assert_eq!(profile.initials(), "AC");

    assert_eq!(tailscale_localapi::UserProfile::default().initials(), "?");
}
