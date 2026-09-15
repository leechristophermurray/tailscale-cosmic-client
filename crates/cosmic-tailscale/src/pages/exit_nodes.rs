//! Exit nodes: choosing one to carry this machine's internet traffic, and
//! offering this machine as one for the rest of the tailnet.

use cosmic::iced::{Alignment, Length};
use cosmic::{Apply, Element, theme, widget};
use tailscale_localapi::PeerStatus;

use crate::app::message::Message;
use crate::app::state::State;
use crate::fl;
use crate::ui::{Tone, format, icons, widgets};

pub fn view(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let options = state.exit_node_options();

    let mut column = widget::Column::new()
        .push(active_card(state))
        .spacing(spacing.space_m);

    if options.is_empty() {
        column = column.push(
            widget::Column::new()
                .push(widgets::section_label(fl!("exit-nodes-available")))
                .push(widget::text::body(fl!("exit-nodes-none")))
                .spacing(spacing.space_xs)
                .apply(widget::container)
                .padding(spacing.space_m)
                .width(Length::Fill)
                .class(theme::Container::Card),
        );
    } else {
        let mut list = widget::Column::new()
            .push(widgets::section_label(fl!("exit-nodes-available")))
            .spacing(spacing.space_xs);

        // "None" is a real choice, so it gets a real row rather than living
        // only inside the header's dropdown.
        list = list.push(direct_mesh_row(state));

        for peer in options {
            list = list.push(exit_node_row(state, peer));
        }

        column = column.push(
            list.apply(widget::container)
                .padding(spacing.space_m)
                .width(Length::Fill)
                .class(theme::Container::Card),
        );
    }

    column = column.push(advertise_card(state));

    widget::scrollable(column).height(Length::Fill).into()
}

/// What is carrying internet traffic right now, plus the LAN-access switch that
/// only matters while an exit node is in use.
fn active_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let active = state
        .status
        .as_deref()
        .and_then(tailscale_localapi::Status::active_exit_node);

    let (tone, headline, detail) = match active {
        Some(peer) => (
            Tone::Accent,
            peer.display_name().to_string(),
            fl!("exit-node-carrying", os = format::os_name(&peer.os)),
        ),
        None => (
            Tone::Neutral,
            fl!("exit-node-direct-mesh"),
            fl!("exit-node-direct-mesh-detail"),
        ),
    };

    let allow_lan = state
        .prefs
        .as_deref()
        .is_some_and(|p| p.exit_node_allow_lan_access);

    let lan_toggle = widget::toggler(allow_lan);
    let lan_toggle = if state.writing_prefs {
        lan_toggle
    } else {
        lan_toggle.on_toggle(Message::SetAllowLanAccess)
    };

    let mut column = widget::Column::new()
        .push(widgets::section_header(icons::EXIT_NODE, fl!("routing")))
        .push(widgets::status_label(headline, tone))
        .push(widget::text::caption(detail))
        .spacing(spacing.space_xxs);

    if active.is_some() {
        column = column.push(widget::divider::horizontal::default()).push(
            widget::Row::new()
                .push(
                    widget::Column::new()
                        .push(widget::text::body(fl!("allow-lan-access")))
                        .push(widget::text::caption(fl!("allow-lan-access-detail")))
                        .spacing(spacing.space_xxxs)
                        .width(Length::Fill),
                )
                .push(lan_toggle)
                .spacing(spacing.space_s)
                .align_y(Alignment::Center)
                .width(Length::Fill),
        );
    }

    column
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

fn direct_mesh_row(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let selected = state.exit_node_index() == 0;

    let content = widget::Row::new()
        .push(icons::named(icons::TAILNET, 20))
        .push(
            widget::Column::new()
                .push(widget::text::body(fl!("exit-node-none")))
                .push(widget::text::caption(fl!("exit-node-direct-mesh-row")))
                .spacing(spacing.space_xxxs)
                .width(Length::Fill),
        )
        .push(if selected {
            widgets::pill(fl!("exit-node-active"), Tone::Accent)
        } else {
            widget::Space::new().width(0).height(0).into()
        })
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
        .on_press(Message::ExitNodeSelected(0))
        .into()
}

fn exit_node_row<'a>(state: &'a State, peer: &'a PeerStatus) -> Element<'a, Message> {
    let spacing = theme::spacing();

    let index = state
        .exit_node_options()
        .iter()
        .position(|p| p.id == peer.id)
        .map_or(0, |i| i + 1);
    let selected = state.exit_node_index() == index;

    let status = if !peer.online {
        widgets::status_label(fl!("route-offline"), Tone::Neutral)
    } else if peer.exit_node {
        widgets::pill(fl!("exit-node-active"), Tone::Accent)
    } else {
        widgets::status_label(fl!("exit-node-available"), Tone::Positive)
    };

    let latency = if state.is_pinging(peer) {
        fl!("action-pinging")
    } else {
        match state.ping_for(peer) {
            Some(ping) if ping.is_ok() => format::latency_ms(ping.latency_ms()),
            Some(_) => fl!("route-unreachable"),
            None => String::new(),
        }
    };

    let content = widget::Row::new()
        .push(icons::named(icons::for_os(&peer.os), 20))
        .push(
            widget::Column::new()
                .push(widget::text::body(peer.display_name().to_string()))
                .push(widget::text::monotext(peer.ipv4().unwrap_or("—").to_string()).size(11.0))
                .spacing(spacing.space_xxxs)
                .width(Length::Fill),
        )
        .push(widget::text::caption(latency))
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
        // Selecting an offline node would just fail, so the row stays inert.
        .on_press_maybe(peer.online.then_some(Message::ExitNodeSelected(index)))
        .into()
}

/// Offer this machine as an exit node for the rest of the tailnet.
fn advertise_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let advertising = state
        .prefs
        .as_deref()
        .is_some_and(tailscale_localapi::Prefs::advertises_exit_node);

    let toggle = widget::toggler(advertising);
    let toggle = if state.writing_prefs {
        toggle
    } else {
        toggle.on_toggle(Message::SetAdvertiseExitNode)
    };

    let mut column = widget::Column::new()
        .push(widgets::section_header(
            icons::EXIT_NODE,
            fl!("run-as-exit-node"),
        ))
        .push(
            widget::Row::new()
                .push(
                    widget::Column::new()
                        .push(widget::text::body(fl!("advertise-exit-node")))
                        .push(widget::text::caption(fl!("advertise-exit-node-detail")))
                        .spacing(spacing.space_xxxs)
                        .width(Length::Fill),
                )
                .push(toggle)
                .spacing(spacing.space_s)
                .align_y(Alignment::Center)
                .width(Length::Fill),
        )
        .spacing(spacing.space_s);

    // Advertising is only half the story: a tailnet admin has to approve the
    // route before any peer can actually use it.
    if advertising {
        let approved = state.self_peer().is_some_and(|peer| peer.exit_node_option);

        column = column.push(if approved {
            widgets::status_label(fl!("advertise-approved"), Tone::Positive)
        } else {
            widgets::status_label(fl!("advertise-pending"), Tone::Caution)
        });
    }

    column
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}
