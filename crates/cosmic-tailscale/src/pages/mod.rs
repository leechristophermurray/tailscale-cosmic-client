//! The content pane, one module per sidebar entry.

pub mod access;
pub mod caddy;
pub mod exit_nodes;
pub mod header;
pub mod machines;
pub mod monitoring;
pub mod preferences;
pub mod services;
pub mod taildrop;

use cosmic::Element;

use crate::app::message::Message;
use crate::app::state::State;
use crate::fl;
use crate::ui::icons;

/// A sidebar destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Machines,
    ExitNodes,
    Services,
    Taildrop,
    Caddy,
    Monitoring,
    Access,
    Preferences,
}

impl Page {
    /// Every page, in sidebar order.
    pub const ALL: [Self; 8] = [
        Self::Machines,
        Self::ExitNodes,
        Self::Taildrop,
        Self::Services,
        Self::Caddy,
        Self::Monitoring,
        Self::Access,
        Self::Preferences,
    ];

    #[must_use]
    pub fn title(self) -> String {
        match self {
            Self::Machines => fl!("page-machines"),
            Self::ExitNodes => fl!("page-exit-nodes"),
            Self::Services => fl!("page-services"),
            Self::Taildrop => fl!("page-taildrop"),
            Self::Caddy => fl!("page-caddy"),
            Self::Monitoring => fl!("page-monitoring"),
            Self::Access => fl!("page-access"),
            Self::Preferences => fl!("page-preferences"),
        }
    }

    #[must_use]
    pub fn icon(self) -> &'static str {
        match self {
            Self::Machines => icons::MACHINES,
            Self::ExitNodes => icons::EXIT_NODE,
            Self::Services => icons::SERVICES,
            Self::Taildrop => icons::SEND,
            Self::Caddy => icons::FORWARD,
            Self::Monitoring => icons::MONITORING,
            Self::Access => icons::ACCESS,
            Self::Preferences => icons::PREFERENCES,
        }
    }

    /// Render this page's content.
    pub fn view(self, state: &State) -> Element<'_, Message> {
        match self {
            Self::Machines => machines::view(state),
            Self::ExitNodes => exit_nodes::view(state),
            Self::Services => services::view(state),
            Self::Taildrop => taildrop::view(state),
            Self::Caddy => caddy::view(state),
            Self::Monitoring => monitoring::view(state),
            Self::Access => access::view(state),
            Self::Preferences => preferences::view(state),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::State;
    use std::sync::Arc;

    /// A state built from a real daemon capture, so the pages render against
    /// the shapes tailscaled actually produces.
    fn populated_state() -> State {
        let status: tailscale_localapi::Status = serde_json::from_str(include_str!(
            "../../../tailscale-localapi/tests/fixtures/status.json"
        ))
        .expect("fixture decodes");

        State {
            backend: status.backend_state,
            status: Some(Arc::new(status)),
            prefs: Some(Arc::new(
                serde_json::from_str(
                    r#"{"WantRunning":true,"CorpDNS":true,"RunSSH":true,
                        "AdvertiseRoutes":["10.0.0.0/24"]}"#,
                )
                .expect("prefs decode"),
            )),
            ..State::default()
        }
    }

    /// Every page must build its view without panicking.
    ///
    /// Screenshots only ever cover the page that happens to be open; this
    /// exercises all of them, including the branches that only appear with data
    /// present.
    #[test]
    fn every_page_renders_with_data() {
        let state = populated_state();

        for page in Page::ALL {
            let _element = page.view(&state);
            assert!(!page.title().is_empty(), "{page:?} has no title");
        }
    }

    /// The pages also have to survive having heard nothing from the daemon yet,
    /// which is what they render for the first moments after launch.
    #[test]
    fn every_page_renders_while_empty() {
        let state = State::default();

        for page in Page::ALL {
            let _element = page.view(&state);
        }
    }

    /// The header is rendered above every page, in both states.
    #[test]
    fn the_header_renders() {
        let empty = State::default();
        let populated = populated_state();

        let _empty_view = header::view(&empty);
        let _populated_view = header::view(&populated);
    }

    /// With files queued, the machine detail pane grows a Taildrop prompt and
    /// the drop zone changes what it says.
    #[test]
    fn the_taildrop_prompt_renders() {
        let mut state = populated_state();
        state.pending_drop = vec![std::path::PathBuf::from("/tmp/example.txt")];

        let _element = Page::Machines.view(&state);
        let _header = header::view(&state);
    }

    /// The Files row has three shapes: offer to mount, busy, and mounted.
    #[test]
    fn the_files_row_renders_unmounted_busy_and_mounted() {
        let mut state = populated_state();
        state.selected = Some("nExample0005CNTRL".to_string());
        drop(Page::Machines.view(&state));

        state.mount_busy.insert("nExample0005CNTRL".to_string());
        drop(Page::Machines.view(&state));

        state.mount_busy.clear();
        state.mounts = vec![crate::app::mounts::Mount {
            remote: crate::app::mounts::Remote::new("alex", "homeforge.tail000000.ts.net").unwrap(),
            home: Some("sftp://alex@homeforge.tail000000.ts.net/home/alex".to_string()),
        }];
        let _mounted = Page::Machines.view(&state);
    }

    /// The Caddy page only grows its route list and add-route form once a
    /// connection exists, so the default state never reaches that code.
    #[test]
    fn the_caddy_page_renders_when_connected() {
        let mut state = populated_state();

        state.caddy.connection =
            crate::app::caddy::Connection::Tunnelled(caddy_admin::Endpoint::tunnelled(12019));
        state.caddy.sites = vec![
            caddy_admin::Site {
                host: "git.example.com".to_string(),
                path: None,
                target: "127.0.0.1:3000".to_string(),
                // Compiled from a Caddyfile: not removable.
                id: None,
            },
            caddy_admin::Site {
                host: "app.example.ts.net".to_string(),
                path: Some("/api".to_string()),
                target: "127.0.0.1:8080".to_string(),
                id: Some("cosmic-tailscale-app-example-ts-net".to_string()),
            },
        ];
        state.caddy.new_host = "new.example.ts.net".to_string();
        state.caddy.new_upstream = "127.0.0.1:9000".to_string();

        // Scoped so the borrow ends before the connection is reassigned.
        {
            let _element = Page::Caddy.view(&state);
        }

        // ...and in each of the states the connection can be in.
        for connection in [
            crate::app::caddy::Connection::Idle,
            crate::app::caddy::Connection::Connecting,
            crate::app::caddy::Connection::Direct(caddy_admin::Endpoint::tailnet("100.64.0.1")),
            crate::app::caddy::Connection::Failed("no route to host".to_string()),
        ] {
            state.caddy.connection = connection;
            let _element = Page::Caddy.view(&state);
        }
    }

    /// The Monitoring page renders differently in each hub state, and the
    /// connected branches only exist once systems are present.
    #[test]
    fn the_monitoring_page_renders_in_every_hub_state() {
        use crate::app::beszel::HubConnection;

        let mut state = populated_state();

        for connection in [
            HubConnection::Unconfigured,
            HubConnection::Connecting,
            HubConnection::NeedsSignIn,
            HubConnection::Unreachable("no route to host".to_string()),
            HubConnection::Failed("something else".to_string()),
        ] {
            state.beszel.connection = connection;
            let _element = Page::Monitoring.view(&state);
        }

        // Connected, with a machine selected and its detail loaded.
        state.beszel.connection = HubConnection::Connected;
        state.beszel.hub_version = "0.18.7".to_string();
        state.beszel.hub_key = "ssh-ed25519 AAAA".to_string();
        state.beszel.systems = vec![
            serde_json::from_str(
                r#"{"id":"s1","name":"homeforge","status":"up",
                    "info":{"h":"homeforge","c":8,"t":16,"cpu":18.2,"mp":15.2,
                            "dp":64.8,"u":432000,"v":"0.18.7","la":[1.68,1.71,1.69]}}"#,
            )
            .expect("system decodes"),
            serde_json::from_str(
                r#"{"id":"s2","name":"nas","status":"down",
                    "info":{"h":"nas","dp":95.0,"mp":94.0,"sv":[40,2]}}"#,
            )
            .expect("system decodes"),
        ];
        state.beszel.selected = Some("s1".to_string());
        state.beszel.stats.insert(
            "s1".to_string(),
            serde_json::from_str(
                r#"{"cpu":18.2,"m":31.2,"mu":4.74,"mp":15.2,"mb":12.0,
                    "d":235.8,"du":152.9,"dp":64.8,
                    "t":{"coretemp_package_id_0":64.0,"nvme_composite":32.85},
                    "z":{"tank":{"d":8000.0,"du":4200.0,"h":"DEGRADED"}},
                    "la":[1.68,1.71,1.69]}"#,
            )
            .expect("stats decode"),
        );
        state.beszel.containers.insert(
            "s1".to_string(),
            vec![serde_json::from_str(r#"{"n":"grafana","c":2.1,"m":128.0,"u":true}"#).unwrap()],
        );

        let _connected = Page::Monitoring.view(&state);
    }

    /// The charts only render once there is history, and each period is a
    /// different slice, so both need exercising.
    #[test]
    fn the_monitoring_charts_render_with_history() {
        use crate::app::beszel::{HubConnection, PERIODS};

        let mut state = populated_state();
        state.beszel.connection = HubConnection::Connected;
        state.beszel.systems = vec![
            serde_json::from_str(
                r#"{"id":"s1","name":"homeforge","status":"up",
                    "info":{"h":"homeforge","c":8,"t":8,"cpu":18.1,"mp":16.0,"dp":65.0}}"#,
            )
            .unwrap(),
        ];
        state.beszel.selected = Some("s1".to_string());

        // The detail card draws nothing below its header without the machine's
        // current stats. This test once omitted them, stopped at "No metrics
        // recorded yet", and passed without ever building a chart.
        state.beszel.stats.insert(
            "s1".to_string(),
            serde_json::from_str(r#"{"cpu":18.1,"mp":16.0,"dp":65.0}"#).unwrap(),
        );

        // A series with movement in every metric the charts plot, including the
        // eight sensors that drive the emphasis chart.
        state.beszel.history = (0..40)
            .map(|i| {
                let f = f64::from(i);
                serde_json::from_str::<beszel_client::StatsRecord>(&format!(
                    r#"{{"id":"r{i}","system":"s1","type":"1m","created":"2026-09-14 16:00:00Z",
                        "stats":{{"cpu":{},"m":31.2,"mu":4.7,"mp":{},"d":236.0,"du":153.0,"dp":65.0,
                        "dio":[{},{}],"b":[{},{}],"la":[{},{},{}],
                        "t":{{"coretemp_core_0":{},"coretemp_core_1":52.0,"coretemp_core_2":64.0,
                              "coretemp_core_3":59.0,"coretemp_package_id_0":{},
                              "nvme_composite":33.0,"nvme_sensor_1":33.0,"nvme_sensor_2":37.0}}}}}}"#,
                    10.0 + f % 20.0,
                    15.0 + f % 5.0,
                    (1024.0 * f) as u64,
                    (2048.0 * f) as u64,
                    (512.0 * f) as u64,
                    (4096.0 * f) as u64,
                    1.4 + f % 3.0,
                    1.5,
                    1.6,
                    50.0 + f % 10.0,
                    60.0 + f % 8.0,
                ))
                .expect("stats record decodes")
            })
            .collect();

        for period in PERIODS {
            state.beszel.period = period;
            let _element = Page::Monitoring.view(&state);
        }
    }

    /// A machine with a single sample has nothing to plot; the page must say so
    /// rather than draw an empty axis.
    #[test]
    fn the_monitoring_charts_handle_thin_history() {
        let mut state = populated_state();
        state.beszel.connection = crate::app::beszel::HubConnection::Connected;
        state.beszel.systems = vec![
            serde_json::from_str(r#"{"id":"s1","name":"x","status":"up","info":{}}"#).unwrap(),
        ];
        state.beszel.selected = Some("s1".to_string());
        state.beszel.history = vec![
            serde_json::from_str(
                r#"{"id":"r0","system":"s1","type":"1m","created":"x","stats":{"cpu":5.0}}"#,
            )
            .unwrap(),
        ];

        let _element = Page::Monitoring.view(&state);
    }

    /// The install confirmation is the one screen that must render correctly:
    /// it is what stands between a click and a root shell on another machine.
    #[test]
    fn the_install_confirmation_shows_the_command() {
        let mut state = populated_state();
        state.beszel.connection = crate::app::beszel::HubConnection::Connected;

        let install = beszel_client::AgentInstall::new("nas.ts.net", "ssh-ed25519 AAAA");
        let command = install.remote_command();

        assert!(
            command.contains("sudo"),
            "the command must not hide that it uses root"
        );

        state.beszel.pending_install = Some(crate::app::beszel::PendingInstall {
            peer_id: "nABC".to_string(),
            host: "nas.ts.net".to_string(),
            command,
        });

        let _element = Page::Monitoring.view(&state);
    }

    /// The Taildrop page has a target-selected branch and an empty one.
    #[test]
    fn the_taildrop_page_renders_both_ways() {
        let mut state = populated_state();

        {
            let _no_target = Page::Taildrop.view(&state);
        }

        let target = state
            .status
            .as_deref()
            .expect("status")
            .peer
            .values()
            .find(|peer| peer.can_receive_files())
            .map(|peer| peer.id.clone());

        state.taildrop_target = target;
        state.pending_drop = vec![std::path::PathBuf::from("/tmp/report.pdf")];

        let _with_target = Page::Taildrop.view(&state);
    }

    /// A drag hovering over the window changes how the drop zone draws.
    #[test]
    fn the_drop_zone_reacts_to_a_drag() {
        let mut state = populated_state();
        state.drag_over = true;

        let _hovered = header::view(&state);
    }

    /// Selecting a peer switches the detail pane away from this machine.
    #[test]
    fn selecting_a_peer_renders_its_detail_pane() {
        let mut state = populated_state();

        let peer_ids: Vec<String> = state
            .status
            .as_deref()
            .expect("status present")
            .peer
            .values()
            .map(|peer| peer.id.clone())
            .collect();

        assert!(!peer_ids.is_empty(), "the fixture should contain peers");

        for id in peer_ids {
            state.selected = Some(id.clone());
            assert_eq!(
                state.selected_peer().map(|peer| peer.id.clone()),
                Some(id),
                "selection did not resolve to the peer"
            );
            let _element = Page::Machines.view(&state);
        }
    }
}
