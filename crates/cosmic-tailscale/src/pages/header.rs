//! The connection header that sits above every page: who you are, whether the
//! tunnel is up, which exit node is carrying your traffic, and the quick
//! actions that belong next to a master switch.

use cosmic::iced::{Alignment, Length};
use cosmic::{Apply, Element, theme, widget};

use crate::app::message::Message;
use crate::fl;
use crate::app::state::State;
use crate::ui::{Tone, icons, widgets};

pub fn view(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    widget::Column::new()
        .push(identity_row(state))
        .push(widget::divider::horizontal::default())
        .push(quick_actions(state))
        .spacing(spacing.space_s)
        .apply(widget::container)
        .padding(spacing.space_s)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

/// Avatar, tailnet name, connection pill, this machine's IP, exit-node
/// selector, and the master switch.
fn identity_row(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let (tone, label) = if state.daemon_unreachable {
        (Tone::Critical, fl!("connection-daemon-unreachable"))
    } else {
        let tone = if state.backend.is_running() {
            Tone::Positive
        } else if state.backend.needs_attention() {
            Tone::Caution
        } else {
            Tone::Neutral
        };
        (tone, crate::ui::backend_label(state.backend))
    };

    let mut title = widget::Row::new()
        .push(widget::text::title4(state.tailnet_name().to_string()))
        .push(widgets::pill(label, tone))
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center);

    if let Some(ip) = state.self_peer().and_then(|p| p.ipv4()) {
        title = title.push(widgets::copy_chip(
            ip.to_string(),
            Message::CopyToClipboard(ip.to_string()),
        ));
    }

    let identity = widget::Column::new()
        .push(title)
        .push(widget::text::caption(account_line(state)))
        .spacing(spacing.space_xxxs);

    widget::Row::new()
        .push(avatar(state))
        .push(identity)
        .push(widget::Space::new().width(Length::Fill))
        .push(exit_node_selector(state))
        .push(master_switch(state))
        .spacing(spacing.space_s)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}

/// `alex@orca-cat.ts.net · 6 peers · 1 direct`
fn account_line(state: &State) -> String {
    let mut parts: Vec<String> = Vec::new();

    let account = state.account_name();
    if !account.is_empty() {
        parts.push(account.to_string());
    }

    if let Some(status) = state.status.as_deref() {
        let online = status.peer.values().filter(|p| p.online).count();
        parts.push(fl!(
            "peers-online",
            online = online,
            total = status.peer.len()
        ));
    }

    if state.throughput.live_peers > 0 {
        parts.push(fl!("peers-active", count = state.throughput.live_peers));
    }

    parts.join(" • ")
}

/// The initials bubble beside the tailnet name.
fn avatar(state: &State) -> Element<'_, Message> {
    widget::text::title4(state.initials())
        .apply(widget::container)
        .width(Length::Fixed(40.0))
        .height(Length::Fixed(40.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .class(theme::Container::custom(|t| {
            let cosmic = t.cosmic();
            cosmic::iced::widget::container::Style {
                text_color: Some(cosmic.on_bg_color().into()),
                background: Some(cosmic.bg_component_color().into()),
                border: cosmic::iced::Border {
                    radius: 20.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }))
        .into()
}

/// Dropdown of every advertised exit node, with "None (direct mesh)" first.
fn exit_node_selector(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let options = state.exit_node_options();

    let mut labels: Vec<String> = Vec::with_capacity(options.len() + 1);
    labels.push(fl!("exit-node-none"));
    for peer in &options {
        labels.push(if peer.online {
            peer.display_name().to_string()
        } else {
            fl!("peer-offline-suffix", name = peer.display_name())
        });
    }

    // With nothing to choose from, a dropdown whose only entry is "None" is
    // just noise — state the situation once instead.
    let selector: Element<'_, Message> = if options.is_empty() {
        widget::text::caption(fl!("exit-node-unavailable")).into()
    } else {
        widget::Row::new()
            .push(widget::text::caption(fl!("exit-node-label")))
            .push(widget::dropdown(
                labels,
                Some(state.exit_node_index()),
                Message::ExitNodeSelected,
            ))
            .spacing(spacing.space_xxs)
            .align_y(Alignment::Center)
            .into()
    };

    widget::Row::new()
        .push(icons::named(icons::EXIT_NODE, 16))
        .push(selector)
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center)
        .apply(widget::container)
        .padding([spacing.space_xxxs, spacing.space_xs])
        .class(theme::Container::Primary)
        .into()
}

/// The master switch. Disabled while a prefs write is in flight so a
/// double-click cannot queue two contradictory writes.
fn master_switch(state: &State) -> Element<'_, Message> {
    let toggler = widget::toggler(state.want_running());

    let toggler = if state.writing_prefs || state.daemon_unreachable {
        toggler
    } else {
        toggler.on_toggle(Message::SetConnected)
    };

    toggler.into()
}

/// Recent peers, the Taildrop drop zone, and the links that belong beside a
/// connection switch.
fn quick_actions(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let header = widget::Row::new()
        .push(widgets::section_header(
            icons::TAILNET,
            &fl!("recent-peers-heading"),
        ))
        .push(widget::Space::new().width(Length::Fill))
        .push(suspend_button(state))
        .push(
            widgets::icon_button(
                icons::ACCESS,
                fl!("admin-console"),
                Some(Message::OpenUrl(admin_console_url(state))),
            )
            .class(theme::Button::Text),
        )
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center)
        .width(Length::Fill);

    let mut peers = widget::Row::new().spacing(spacing.space_xs).width(Length::Fill);

    for peer in state.quick_peers(2) {
        peers = peers.push(quick_peer(peer));
    }

    peers = peers.push(drop_zone(state));

    widget::Column::new()
        .push(header)
        .push(peers)
        .spacing(spacing.space_s)
        .into()
}

/// One compact peer card with a one-click IP copy.
fn quick_peer<'a>(peer: &'a tailscale_localapi::PeerStatus) -> Element<'a, Message> {
    let spacing = theme::spacing();
    let ip = peer.ipv4().unwrap_or("").to_string();

    widget::Row::new()
        .push(widgets::dot(Tone::Positive))
        .push(
            widget::Column::new()
                .push(widget::text::body(peer.display_name().to_string()))
                .push(widget::text::monotext(ip.clone()).size(11.0))
                .spacing(spacing.space_xxxs)
                .width(Length::Fill),
        )
        .push(
            widget::button::icon(widget::icon::from_name(icons::COPY).size(14))
                .on_press(Message::CopyToClipboard(ip))
                .class(theme::Button::Icon),
        )
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center)
        .apply(widget::container)
        .padding(spacing.space_xs)
        .width(Length::FillPortion(1))
        .class(theme::Container::Primary)
        .into()
}

/// The Taildrop target. Shows what is queued when files have been dropped.
fn drop_zone(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let hovered = state.drag_over;

    let (title, detail) = if state.drag_over {
        (fl!("drop-zone-title"), fl!("drop-zone-active"))
    } else if state.pending_drop.is_empty() {
        // The card is a button first and a drop target second: dragging from
        // the file manager does not reliably reach a Wayland client, so the
        // label promises only the thing that always works.
        (fl!("drop-zone-choose-files"), fl!("drop-zone-or-drop"))
    } else {
        (
            fl!("drop-zone-ready", count = crate::ui::file_count(state.pending_drop.len())),
            fl!("drop-zone-choose"),
        )
    };

    let content = widget::Row::new()
        .push(icons::named(icons::SEND, 20))
        .push(
            widget::Column::new()
                .push(widget::text::body(title))
                .push(widget::text::caption(detail))
                .spacing(spacing.space_xxxs)
                .width(Length::Fill),
        )
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center);

    widget::button::custom(content)
        .on_press(Message::ChooseFiles(None))
        .padding(spacing.space_xs)
        .width(Length::FillPortion(1))
        .class(theme::Button::Custom {
            active: Box::new(move |_focused, t| drop_zone_style(t, hovered)),
            disabled: Box::new(move |t| drop_zone_style(t, hovered)),
            hovered: Box::new(move |_focused, t| drop_zone_style(t, true)),
            pressed: Box::new(move |_focused, t| drop_zone_style(t, true)),
        })
        .into()
}

/// The drop zone's fill and edge, which brighten both on pointer hover and
/// while a drag is over the window.
fn drop_zone_style(theme: &cosmic::Theme, active: bool) -> cosmic::widget::button::Style {
    let cosmic = theme.cosmic();
    let accent: cosmic::iced::Color = cosmic.accent_color().into();
    let (fill, edge, width) = if active {
        (0.26, 0.9, 2.0)
    } else {
        (0.12, 0.4, 1.0)
    };

    cosmic::widget::button::Style {
        background: Some(cosmic::iced::Color { a: fill, ..accent }.into()),
        border_color: cosmic::iced::Color { a: edge, ..accent },
        border_width: width,
        border_radius: cosmic.corner_radii.radius_s.into(),
        text_color: Some(cosmic.on_bg_color().into()),
        icon_color: Some(accent),
        ..Default::default()
    }
}


fn suspend_button(state: &State) -> Element<'_, Message> {
    let label = if state.suspended_until.is_some() {
        fl!("suspended")
    } else {
        fl!("suspend-for-an-hour")
    };

    let action = if state.suspended_until.is_some() || !state.want_running() {
        None
    } else {
        Some(Message::SuspendRequested)
    };

    widgets::icon_button(icons::WARNING, label, action)
        .class(theme::Button::Text)
        .into()
}

/// The admin console URL for this tailnet.
fn admin_console_url(state: &State) -> String {
    let _ = state;
    "https://login.tailscale.com/admin/machines".to_string()
}
