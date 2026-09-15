//! The Machines page: a filterable list of every node on the tailnet beside a
//! detail pane for the selected one.

use cosmic::iced::{Alignment, Length};
use cosmic::{Apply, Element, theme, widget};
use tailscale_localapi::{PeerStatus, Route};

use crate::app::message::Message;
use crate::app::state::State;
use crate::fl;
use crate::ui::{Tone, format, icons, widgets};

pub fn view(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    widget::Row::new()
        .push(
            widget::container(machine_list(state))
                .width(Length::FillPortion(4))
                .height(Length::Fill),
        )
        .push(
            widget::container(detail_pane(state))
                .width(Length::FillPortion(7))
                .height(Length::Fill),
        )
        .spacing(spacing.space_s)
        .height(Length::Fill)
        .into()
}

// ---------------------------------------------------------------------------
// Left column: the machine list
// ---------------------------------------------------------------------------

fn machine_list(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let peers = state.filtered_peers();

    // The daemon does not group machines; it just reports an owner per node.
    // Splitting on "is this my account" is what produces the familiar
    // "my devices" / "shared with me" split.
    let my_user = state.self_peer().map(|p| p.user_id);
    let (mine, shared): (Vec<&PeerStatus>, Vec<&PeerStatus>) = peers
        .into_iter()
        .partition(|peer| my_user.is_some_and(|id| peer.user_id == id));

    let (mine_count, shared_count) = (mine.len(), shared.len());

    let mut column = widget::Column::new()
        .push(filter_field(state))
        .spacing(spacing.space_s);

    // This machine leads the list; it is the one the user is sitting at.
    if let Some(me) = state.self_peer() {
        column = column
            .push(widgets::section_label(fl!("this-machine")))
            .push(peer_row(state, me, true));
    }

    if mine_count > 0 {
        column = column.push(count_header(fl!("my-devices"), mine_count));
        for peer in mine {
            column = column.push(peer_row(state, peer, false));
        }
    }

    if shared_count > 0 {
        column = column.push(count_header(fl!("shared-with-me"), shared_count));
        for peer in shared {
            column = column.push(peer_row(state, peer, false));
        }
    }

    if state.status.is_none() {
        column = column.push(widget::text::body(fl!("not-connected-yet")));
    } else if mine_count == 0 && shared_count == 0 && !state.filter.is_empty() {
        column = column.push(widget::text::body(fl!(
            "no-machines-match",
            query = state.filter.as_str()
        )));
    }

    column = column.push(daemon_footer(state));

    widget::scrollable(column.padding([0, spacing.space_xxs, 0, 0]))
        .height(Length::Fill)
        .into()
}

fn filter_field(state: &State) -> Element<'_, Message> {
    let count = state
        .status
        .as_deref()
        .map_or(0, |status| status.peer.len() + 1);

    widget::text_input::search_input(fl!("filter-machines", count = count), &state.filter)
        .on_input(Message::FilterChanged)
        .on_clear(Message::FilterChanged(String::new()))
        .width(Length::Fill)
        .into()
}

fn count_header<'a>(label: impl AsRef<str>, count: usize) -> Element<'a, Message> {
    widget::Row::new()
        .push(widgets::section_label(label))
        .push(widget::Space::new().width(Length::Fill))
        .push(widget::text::caption(count.to_string()))
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}

/// One row in the machine list: device icon, name, IP, and how it is reached.
fn peer_row<'a>(state: &'a State, peer: &'a PeerStatus, is_self: bool) -> Element<'a, Message> {
    let spacing = theme::spacing();
    let selected = state.is_selected(peer);

    let mut name_row = widget::Row::new()
        .push(widget::text::body(peer.display_name().to_string()))
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center);

    if is_self {
        name_row = name_row.push(widgets::pill(fl!("this-machine"), Tone::Accent));
    } else if peer.exit_node {
        name_row = name_row.push(widgets::pill(fl!("exit-node-label"), Tone::Accent));
    }

    let identity = widget::Column::new()
        .push(name_row)
        .push(widget::text::monotext(peer.ipv4().unwrap_or("—").to_string()).size(11.0))
        .spacing(spacing.space_xxxs)
        .width(Length::Fill);

    let status = widget::Column::new()
        .push(connectivity_label(state, peer))
        .push(widget::text::caption(format::os_name(&peer.os)))
        .spacing(spacing.space_xxxs)
        .align_x(Alignment::End);

    let content = widget::Row::new()
        .push(icons::named(icons::for_os(&peer.os), 20))
        .push(identity)
        .push(status)
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center)
        .width(Length::Fill);

    widget::button::custom(content)
        .padding(spacing.space_xs)
        .width(Length::Fill)
        .selected(selected)
        .class(theme::Button::ListItem(
            [theme::active().cosmic().corner_radii.radius_s[0]; 4],
        ))
        .on_press(Message::SelectPeer(peer.id.clone()))
        .into()
}

/// The right-hand status of a list row: online state, plus measured latency
/// when a probe has been run.
fn connectivity_label<'a>(state: &'a State, peer: &'a PeerStatus) -> Element<'a, Message> {
    if !peer.online {
        let label = peer
            .last_seen_at()
            .map_or_else(|| fl!("route-offline"), format::relative_past);
        return widgets::status_label(label, Tone::Neutral);
    }

    if let Some(ping) = state.ping_for(peer).filter(|p| p.is_ok()) {
        let tone = if ping.is_direct() {
            Tone::Positive
        } else {
            Tone::Caution
        };
        return widgets::status_label(format::latency_ms(ping.latency_ms()), tone);
    }

    match peer.route() {
        Route::Direct(_) => widgets::status_label(fl!("route-direct"), Tone::Positive),
        Route::Derp(region) => {
            widgets::status_label(fl!("route-relay", region = region), Tone::Caution)
        }
        Route::Idle => widgets::status_label(fl!("route-online"), Tone::Positive),
    }
}

/// The socket the client is talking to, matching the mockup's LocalAPI card.
/// It is the first thing worth seeing when something is wrong.
fn daemon_footer(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let (tone, detail) = if state.daemon_unreachable {
        (Tone::Critical, fl!("daemon-unreachable-short"))
    } else if let Some(status) = state.status.as_deref() {
        (
            Tone::Positive,
            fl!("daemon-version", version = status.version.as_str()),
        )
    } else {
        (Tone::Neutral, fl!("daemon-connecting"))
    };

    widget::Column::new()
        .push(
            widget::Row::new()
                .push(widgets::section_header(
                    icons::TAILNET,
                    fl!("localapi-daemon"),
                ))
                .push(widget::Space::new().width(Length::Fill))
                .push(widgets::status_label(detail, tone))
                .align_y(Alignment::Center)
                .width(Length::Fill),
        )
        .push(
            widget::text::monotext(format!("unix://{}", tailscale_localapi::DEFAULT_SOCKET))
                .size(11.0),
        )
        .spacing(spacing.space_xxs)
        .apply(widget::container)
        .padding(spacing.space_xs)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

// ---------------------------------------------------------------------------
// Right column: the detail pane
// ---------------------------------------------------------------------------

fn detail_pane(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let Some(peer) = state.selected_peer() else {
        return widgets::empty_state(
            icons::MACHINES,
            fl!("machine-none-selected"),
            fl!("machine-none-selected-detail"),
        );
    };

    let content = widget::Column::new()
        .push(detail_heading(state, peer))
        .push(address_row(peer))
        .push(action_row(state, peer))
        .push(files_row(state, peer))
        .push(stat_row(state, peer))
        .push(capability_row(state, peer))
        .push(taildrop_prompt(state, peer))
        .spacing(spacing.space_m)
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card);

    widget::scrollable(content).height(Length::Fill).into()
}

fn detail_heading<'a>(state: &'a State, peer: &'a PeerStatus) -> Element<'a, Message> {
    let spacing = theme::spacing();

    let title = widget::Row::new()
        .push(widget::text::title2(peer.display_name().to_string()))
        .push(widgets::pill(format::os_name(&peer.os), Tone::Neutral))
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center);

    widget::Row::new()
        .push(title)
        .push(widget::Space::new().width(Length::Fill))
        .push(mesh_quality(state, peer))
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}

/// The "mesh quality" readout. It reports a measured round trip when one has
/// been taken, and otherwise says how the path is built rather than inventing
/// a number.
fn mesh_quality<'a>(state: &'a State, peer: &'a PeerStatus) -> Element<'a, Message> {
    let spacing = theme::spacing();

    // The machine the client runs on is not reached across the tailnet, so the
    // route fields describe its home relay rather than a path to it.
    if state.is_self(peer) {
        let home = if peer.relay.is_empty() {
            fl!("route-no-home-relay")
        } else {
            fl!("route-home-relay", region = peer.relay.as_str())
        };

        return widget::Column::new()
            .push(widgets::section_label(fl!("this-machine")))
            .push(widgets::status_label(
                if peer.online {
                    fl!("route-online")
                } else {
                    fl!("route-offline")
                },
                if peer.online {
                    Tone::Positive
                } else {
                    Tone::Neutral
                },
            ))
            .push(widget::text::caption(home))
            .spacing(spacing.space_xxxs)
            .align_x(Alignment::End)
            .apply(widget::container)
            .padding(spacing.space_xs)
            .class(theme::Container::Primary)
            .into();
    }

    let (headline, detail, tone) = match state.ping_for(peer) {
        Some(ping) if ping.is_ok() => (
            format::latency_ms(ping.latency_ms()),
            if ping.is_direct() {
                fl!("route-direct").to_lowercase()
            } else {
                fl!("route-derp", region = ping.derp_region_code.as_str())
            },
            if ping.is_direct() {
                Tone::Positive
            } else {
                Tone::Caution
            },
        ),
        Some(ping) => (fl!("route-unreachable"), ping.err.clone(), Tone::Critical),
        None => match peer.route() {
            Route::Direct(endpoint) => (fl!("route-direct"), endpoint.to_string(), Tone::Positive),
            Route::Derp(region) => (
                fl!("route-relayed"),
                fl!("route-derp", region = region),
                Tone::Caution,
            ),
            Route::Idle => (
                if peer.online {
                    fl!("route-idle")
                } else {
                    fl!("route-offline")
                },
                fl!("route-no-path"),
                Tone::Neutral,
            ),
        },
    };

    widget::Column::new()
        .push(widgets::section_label(fl!("mesh-quality")))
        .push(widgets::status_label(headline, tone))
        .push(widget::text::caption(detail))
        .spacing(spacing.space_xxxs)
        .align_x(Alignment::End)
        .apply(widget::container)
        .padding(spacing.space_xs)
        .class(theme::Container::Primary)
        .into()
}

/// Copyable IPv4, IPv6, and MagicDNS name.
fn address_row(peer: &PeerStatus) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let mut row = widget::Row::new().spacing(spacing.space_xs);

    if let Some(ip) = peer.ipv4() {
        row = row.push(widgets::copy_chip(
            ip.to_string(),
            Message::CopyToClipboard(ip.to_string()),
        ));
    }

    if let Some(ip) = peer.ipv6() {
        row = row.push(widgets::copy_chip(
            ip.to_string(),
            Message::CopyToClipboard(ip.to_string()),
        ));
    }

    let dns = peer.magic_dns().to_string();
    if !dns.is_empty() {
        row = row.push(widgets::copy_chip(
            dns.clone(),
            Message::CopyToClipboard(dns),
        ));
    }

    widget::scrollable(row)
        .horizontal()
        .width(Length::Fill)
        .into()
}

/// SSH, ping, and Taildrop. Each is offered only when the peer actually
/// supports it, so nothing here fails after the click.
fn action_row<'a>(state: &'a State, peer: &'a PeerStatus) -> Element<'a, Message> {
    let spacing = theme::spacing();

    let ssh = if peer.supports_ssh() && peer.online && !state.is_self(peer) {
        Some(Message::SshToPeer(peer.magic_dns().to_string()))
    } else {
        None
    };

    let ping = peer
        .ipv4()
        .filter(|_| peer.online && !state.is_pinging(peer))
        .map(|_| Message::PingPeer(peer.id.clone()));

    let send = if peer.can_receive_files() && peer.online && !state.is_self(peer) {
        Some(Message::SendPendingTo(peer.id.clone()))
    } else {
        None
    };

    let ping_label = if state.is_pinging(peer) {
        fl!("action-pinging")
    } else {
        fl!("action-ping")
    };

    let send_label = if state.pending_drop.is_empty() {
        // With nothing queued this opens the file chooser and sends straight
        // to this machine, so it is one action, not a prerequisite.
        fl!("action-send-file")
    } else {
        fl!("action-send-queued")
    };

    widget::Row::new()
        .push(
            widgets::icon_button(icons::TERMINAL, fl!("action-ssh"), ssh)
                .class(theme::Button::Suggested),
        )
        .push(widgets::icon_button(icons::REFRESH, ping_label, ping))
        .push(widgets::icon_button(icons::SEND, send_label, send))
        .spacing(spacing.space_xs)
        .into()
}

/// Mount this machine's home directory, or open and unmount it once mounted.
///
/// Hidden for this machine and for phones and tablets; for an offline machine
/// only an existing mount can still be unmounted.
fn files_row<'a>(state: &'a State, peer: &'a PeerStatus) -> Element<'a, Message> {
    let spacing = theme::spacing();
    let busy = state.mount_busy.contains(&peer.id);
    let mount = state.mount_for(peer);

    if mount.is_none() && !state.can_mount(peer) {
        return widget::Space::new().width(0).height(0).into();
    }

    let mut row = widget::Row::new()
        .spacing(spacing.space_xs)
        .align_y(Alignment::End);

    let caption = if let Some(mount) = mount {
        row = row
            .push(
                widgets::icon_button(
                    icons::FOLDER_REMOTE,
                    fl!("files-open"),
                    Some(Message::OpenPeerFiles(peer.id.clone())),
                )
                .class(theme::Button::Suggested),
            )
            .push(widgets::icon_button(
                icons::EJECT,
                if busy {
                    fl!("files-unmounting")
                } else {
                    fl!("files-unmount")
                },
                (!busy).then(|| Message::UnmountPeer(peer.id.clone())),
            ));
        fl!("files-mounted-at", location = mount.location())
    } else {
        let peer_id = peer.id.clone();
        row = row
            .push(
                widget::text_input::text_input(
                    crate::app::state::local_user_name(),
                    state.mount_user(peer),
                )
                .label(fl!("files-user"))
                .on_input(move |user| Message::MountUserChanged(peer_id.clone(), user))
                .width(Length::Fixed(180.0)),
            )
            .push(widgets::icon_button(
                icons::FOLDER_REMOTE,
                if busy {
                    fl!("files-mounting")
                } else {
                    fl!("files-mount")
                },
                (!busy).then(|| Message::MountPeer(peer.id.clone())),
            ));
        fl!("files-mount-detail")
    };

    widget::Column::new()
        .push(widgets::section_label(fl!("files-heading")))
        .push(row)
        .push(widget::text::caption(caption))
        .spacing(spacing.space_xxs)
        .into()
}

/// The three fact cards under the actions.
fn stat_row<'a>(state: &'a State, peer: &'a PeerStatus) -> Element<'a, Message> {
    let spacing = theme::spacing();

    let endpoint = match peer.route() {
        Route::Direct(addr) => (addr.to_string(), fl!("stat-direct-wireguard")),
        Route::Derp(region) => (
            fl!("route-derp", region = region),
            fl!("stat-relayed-no-direct"),
        ),
        Route::Idle => ("—".to_string(), fl!("stat-no-path")),
    };

    let transfer = fl!(
        "stat-transfer",
        rx = format::bytes(peer.rx_bytes),
        tx = format::bytes(peer.tx_bytes)
    );

    let key = match peer.key_expiry_at() {
        Some(expiry) => (
            format::relative_future(expiry),
            fl!("key-valid-until", date = format::calendar_date(expiry)),
        ),
        // A tailnet can turn key expiry off entirely. Saying so is more useful
        // than an empty card or a made-up countdown.
        None => (fl!("key-does-not-expire"), fl!("key-expiry-disabled")),
    };

    let _ = state;

    widget::Row::new()
        .push(widgets::stat_card(
            icons::MACHINES,
            fl!("stat-os"),
            format::os_name(&peer.os),
            peer.created.map_or_else(String::new, |created| {
                fl!("stat-joined", date = format::calendar_date(created))
            }),
        ))
        .push(widgets::stat_card(
            icons::TAILNET,
            fl!("stat-endpoint"),
            endpoint.0,
            format!("{}\n{transfer}", endpoint.1),
        ))
        .push(widgets::stat_card(
            icons::ACCESS,
            fl!("stat-machine-key"),
            key.0,
            key.1,
        ))
        .spacing(spacing.space_xs)
        .width(Length::Fill)
        .into()
}

/// Feature availability for this peer: SSH and exit-node capability.
fn capability_row<'a>(state: &'a State, peer: &'a PeerStatus) -> Element<'a, Message> {
    let spacing = theme::spacing();

    let ssh = if peer.supports_ssh() {
        (Tone::Positive, fl!("cap-ssh-on"), fl!("cap-ssh-on-detail"))
    } else {
        (Tone::Neutral, fl!("cap-ssh-off"), fl!("cap-ssh-off-detail"))
    };

    let exit = if peer.exit_node {
        (
            Tone::Accent,
            fl!("cap-exit-active"),
            fl!("cap-exit-active-detail"),
        )
    } else if peer.exit_node_option {
        (
            Tone::Positive,
            fl!("cap-exit-available"),
            fl!("cap-exit-available-detail"),
        )
    } else {
        (
            Tone::Neutral,
            fl!("cap-exit-none"),
            fl!("cap-exit-none-detail"),
        )
    };

    let _ = state;

    widget::Row::new()
        .push(capability_card(ssh.0, ssh.1, ssh.2))
        .push(capability_card(exit.0, exit.1, exit.2))
        .spacing(spacing.space_xs)
        .width(Length::Fill)
        .into()
}

fn capability_card<'a>(tone: Tone, title: String, detail: String) -> Element<'a, Message> {
    let spacing = theme::spacing();

    widget::Column::new()
        .push(widgets::status_label(title, tone))
        .push(widget::text::caption(detail))
        .spacing(spacing.space_xxxs)
        .apply(widget::container)
        .padding(spacing.space_s)
        .width(Length::Fill)
        .class(theme::Container::Primary)
        .into()
}

/// Shown only when files are queued: a clear confirm/cancel for the transfer.
fn taildrop_prompt<'a>(state: &'a State, peer: &'a PeerStatus) -> Element<'a, Message> {
    let spacing = theme::spacing();

    if state.pending_drop.is_empty() {
        return widget::Space::new().width(0).height(0).into();
    }

    let names: Vec<String> = state
        .pending_drop
        .iter()
        .map(|p| {
            p.file_name().map_or_else(
                || p.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        })
        .collect();

    let can_send = peer.can_receive_files() && peer.online;

    let action = widget::Row::new()
        .push(
            widget::button::suggested(fl!("action-send-to", name = peer.display_name()))
                .on_press_maybe(can_send.then(|| Message::SendPendingTo(peer.id.clone()))),
        )
        .push(widget::button::standard(fl!("cancel")).on_press(Message::ClearPendingDrop))
        .spacing(spacing.space_xs);

    let mut column = widget::Column::new()
        .push(widgets::section_header(icons::SEND, fl!("taildrop-queued")))
        .push(widget::text::body(names.join(", ")))
        .spacing(spacing.space_xxs);

    if !can_send {
        column = column.push(widget::text::caption(fl!("taildrop-cannot-receive")));
    }

    column
        .push(action)
        .apply(widget::container)
        .padding(spacing.space_s)
        .width(Length::Fill)
        .class(theme::Container::Primary)
        .into()
}
