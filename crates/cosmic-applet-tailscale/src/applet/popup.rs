//! The flyout: the panel-sized version of the client.

use cosmic::iced::{Alignment, Length};
use cosmic::widget::dropdown::popup_dropdown;
use cosmic::{Apply, Element, theme, widget};
use tailscale_localapi::{PeerStatus, Status};

use crate::fl;

use super::Applet;
use super::message::Message;

/// How many peers fit in the flyout before it stops being a quick-access list.
const QUICK_PEER_LIMIT: usize = 4;

pub fn view(applet: &Applet) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let mut column = widget::Column::new()
        .push(header(applet))
        .push(widget::divider::horizontal::default())
        .spacing(spacing.space_xs)
        .padding(spacing.space_xs);

    if !applet.reachable {
        return column
            .push(widget::text::body(fl!("daemon-unreachable")))
            .into();
    }

    column = column
        .push(exit_node_section(applet))
        .push(widget::divider::horizontal::default())
        .push(peer_section(applet));

    if applet.monitoring.configured && !applet.monitoring.systems.is_empty() {
        column = column
            .push(widget::divider::horizontal::default())
            .push(monitoring_section(applet));
    }

    column = column
        .push(widget::divider::horizontal::default())
        .push(links(applet));

    column.into()
}

/// Tailnet name, this machine's address, and the master switch.
fn header(applet: &Applet) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let subtitle = if applet.suspended {
        fl!("suspended")
    } else if !applet.reachable {
        fl!("daemon-offline")
    } else {
        applet
            .status
            .as_deref()
            .and_then(|s| s.self_status.as_ref())
            .and_then(PeerStatus::ipv4)
            .map_or_else(
                || applet.backend.description().to_string(),
                |ip| format!("{} · {ip}", applet.backend.label()),
            )
    };

    let toggler = widget::toggler(applet.want_running());
    let toggler = if applet.reachable {
        toggler.on_toggle(Message::SetConnected)
    } else {
        toggler
    };

    widget::Row::new()
        .push(
            widget::Column::new()
                .push(widget::text::body(applet.tailnet_name().to_string()))
                .push(widget::text::caption(subtitle))
                .spacing(spacing.space_xxxs)
                .width(Length::Fill),
        )
        .push(toggler)
        .spacing(spacing.space_s)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}

/// Exit node picker. Rendered as a popup dropdown so the list is not clipped by
/// the flyout's own surface.
fn exit_node_section(applet: &Applet) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let options = applet
        .status
        .as_deref()
        .map(Status::exit_node_options)
        .unwrap_or_default();

    if options.is_empty() {
        return widget::Column::new()
            .push(widget::text::caption_heading(fl!("exit-node")))
            .push(widget::text::caption(fl!("exit-node-unavailable")))
            .spacing(spacing.space_xxxs)
            .into();
    }

    let mut labels: Vec<String> = Vec::with_capacity(options.len() + 1);
    labels.push(fl!("exit-node-none"));
    for peer in &options {
        labels.push(if peer.online {
            peer.display_name().to_string()
        } else {
            fl!("peer-offline-suffix", name = peer.display_name())
        });
    }

    let selected = applet
        .prefs
        .as_deref()
        .filter(|p| p.is_exit_node_active())
        .and_then(|prefs| {
            options
                .iter()
                .position(|p| p.id == prefs.exit_node_id)
                .map(|index| index + 1)
        })
        .unwrap_or(0);

    widget::Column::new()
        .push(widget::text::caption_heading(fl!("exit-node")))
        .push(popup_dropdown(
            labels,
            Some(selected),
            Message::ExitNodeSelected,
            applet.popup_id(),
            Message::Surface,
            |message| message,
        ))
        .spacing(spacing.space_xxs)
        .into()
}

/// The most recently active peers, each with a one-click address copy.
fn peer_section(applet: &Applet) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let Some(status) = applet.status.as_deref() else {
        return widget::text::caption(fl!("peers-loading")).into();
    };

    let mut peers: Vec<&PeerStatus> = status.peer.values().filter(|p| p.online).collect();
    peers.sort_by(|a, b| {
        b.last_handshake_at()
            .cmp(&a.last_handshake_at())
            .then_with(|| a.host_name.to_lowercase().cmp(&b.host_name.to_lowercase()))
    });
    peers.truncate(QUICK_PEER_LIMIT);

    let mut column = widget::Column::new()
        .push(widget::text::caption_heading(fl!("recent-peers")))
        .spacing(spacing.space_xxs);

    if peers.is_empty() {
        return column
            .push(widget::text::caption(fl!("peers-none")))
            .into();
    }

    for peer in peers {
        let ip = peer.ipv4().unwrap_or_default().to_string();

        // When the hub monitors this machine, its health belongs right here —
        // that is the whole point of correlating the two.
        let health: Element<'_, Message> = match applet.monitoring.system_for(peer) {
            Some(system) => widget::text::caption(format!(
                "{:.0}% / {:.0}%",
                system.info.cpu, system.info.memory_pct
            ))
            .into(),
            None => widget::text::monotext(ip.clone()).size(11.0).into(),
        };

        column = column.push(
            widget::button::custom(
                widget::Row::new()
                    .push(widget::text::body(peer.display_name().to_string()).width(Length::Fill))
                    .push(health)
                    .spacing(spacing.space_xs)
                    .align_y(Alignment::Center),
            )
            .padding([spacing.space_xxs, spacing.space_xs])
            .width(Length::Fill)
            .class(theme::Button::MenuItem)
            .on_press(Message::CopyToClipboard(ip)),
        );
    }

    column
        .push(widget::text::caption(fl!("peers-copy-hint")))
        .into()
}

/// Hardware health, when a monitoring hub is configured in the main window.
fn monitoring_section(applet: &Applet) -> Element<'_, Message> {
    let spacing = theme::spacing();

    if !applet.monitoring.configured || applet.monitoring.systems.is_empty() {
        return widget::Space::new().width(0).height(0).into();
    }

    let mut column = widget::Column::new()
        .push(widget::text::caption_heading(fl!("monitoring")))
        .spacing(spacing.space_xxs);

    // Lead with anything wrong; a health panel that buries the one problem
    // among nine healthy machines is not doing its job.
    let unhealthy = applet.monitoring.unhealthy();
    if unhealthy.is_empty() {
        column = column.push(widget::text::caption(fl!(
            "monitoring-all-well",
            count = applet.monitoring.systems.len()
        )));
    } else {
        for system in &unhealthy {
            column = column.push(widget::text::caption(format!(
                "{}: {}",
                system.name,
                system.status.label()
            )));
        }
    }

    // Pinning puts a machine's figure in the panel itself.
    for system in &applet.monitoring.systems {
        let pinned = applet.monitoring.pinned.as_deref() == Some(system.id.as_str());

        column = column.push(
            widget::button::custom(
                widget::Row::new()
                    .push(widget::text::body(system.name.clone()).width(Length::Fill))
                    .push(widget::text::caption(format!(
                        "{:.0}% / {:.0}%",
                        system.info.cpu, system.info.memory_pct
                    )))
                    .push(widget::text::caption(if pinned {
                        fl!("monitoring-unpin")
                    } else {
                        fl!("monitoring-pin")
                    }))
                    .spacing(spacing.space_xs)
                    .align_y(Alignment::Center),
            )
            .padding([spacing.space_xxs, spacing.space_xs])
            .width(Length::Fill)
            .class(theme::Button::MenuItem)
            .on_press(if pinned {
                Message::UnpinSystem
            } else {
                Message::PinSystem(system.id.clone())
            }),
        );
    }

    column.into()
}

/// The quick links at the bottom of the flyout.
fn links(applet: &Applet) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let suspend = if applet.suspended || !applet.want_running() {
        None
    } else {
        Some(Message::SuspendForAnHour)
    };

    widget::Column::new()
        .push(
            widget::button::text(fl!("open-main-window"))
                .width(Length::Fill)
                .class(theme::Button::MenuItem)
                .on_press(Message::OpenMainWindow),
        )
        .push(
            widget::button::text(fl!("suspend-for-an-hour"))
                .width(Length::Fill)
                .class(theme::Button::MenuItem)
                .on_press_maybe(suspend),
        )
        .push(
            widget::button::text(fl!("admin-console"))
                .width(Length::Fill)
                .class(theme::Button::MenuItem)
                .on_press(Message::OpenAdminConsole),
        )
        .spacing(spacing.space_xxxs)
        .apply(Element::from)
}
