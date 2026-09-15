//! Taildrop: sending files to a machine, and the files that have arrived here.
//!
//! Both directions live on one page because they are the same subject, and
//! because the inbound queue is otherwise easy to miss — a file that arrives
//! and is never claimed sits in the daemon indefinitely.

use cosmic::iced::{Alignment, Length};
use cosmic::{Apply, Element, theme, widget};
use tailscale_localapi::PeerStatus;

use crate::app::message::Message;
use crate::app::state::State;
use crate::fl;
use crate::ui::{Tone, format, icons, widgets};

pub fn view(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    widget::scrollable(
        widget::Column::new()
            .push(send_card(state))
            .push(received_card(state))
            .spacing(spacing.space_m),
    )
    .height(Length::Fill)
    .into()
}

// ---------------------------------------------------------------------------
// Outgoing
// ---------------------------------------------------------------------------

/// Pick a machine, pick files, send. In that order, because choosing the
/// destination first is what makes a single Send button unambiguous.
fn send_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let targets = taildrop_targets(state);

    let mut column = widget::Column::new()
        .push(widgets::section_header(icons::SEND, fl!("taildrop-send")))
        .spacing(spacing.space_s);

    if targets.is_empty() {
        return column
            .push(widget::text::body(fl!("taildrop-no-targets")))
            .apply(widget::container)
            .padding(spacing.space_m)
            .width(Length::Fill)
            .class(theme::Container::Card)
            .into();
    }

    column = column.push(widgets::section_label(fl!("taildrop-choose-machine")));

    for peer in &targets {
        column = column.push(target_row(state, peer));
    }

    column = column.push(drop_area(state));

    if !state.pending_drop.is_empty() {
        column = column.push(queued(state));
    }

    column
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

/// Machines that can actually receive right now.
///
/// Offering a machine that will reject the transfer is worse than not listing
/// it, so this filters rather than disabling.
fn taildrop_targets(state: &State) -> Vec<&PeerStatus> {
    let Some(status) = state.status.as_deref() else {
        return Vec::new();
    };

    let mut peers: Vec<&PeerStatus> = status
        .peer
        .values()
        .filter(|peer| peer.online && peer.can_receive_files())
        .collect();

    peers.sort_by_key(|peer| peer.host_name.to_lowercase());
    peers
}

fn target_row<'a>(state: &'a State, peer: &'a PeerStatus) -> Element<'a, Message> {
    let spacing = theme::spacing();
    let selected = state.taildrop_target.as_deref() == Some(peer.id.as_str());

    let content = widget::Row::new()
        .push(icons::named(icons::for_os(&peer.os), 20))
        .push(
            widget::Column::new()
                .push(widget::text::body(peer.display_name().to_string()))
                .push(widget::text::caption(format!(
                    "{} · {}",
                    peer.ipv4().unwrap_or("—"),
                    format::os_name(&peer.os)
                )))
                .spacing(spacing.space_xxxs)
                .width(Length::Fill),
        )
        .push(if selected {
            widgets::pill(fl!("taildrop-selected"), Tone::Accent)
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
        .class(theme::Button::ListItem([
            theme::active().cosmic().corner_radii.radius_s[0]; 4
        ]))
        .on_press(Message::TaildropSelectTarget(peer.id.clone()))
        .into()
}

/// The drop area, which is also the button that opens the file chooser.
fn drop_area(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let hovered = state.drag_over;
    let target = state.taildrop_target.clone();

    let (title, detail) = if hovered {
        (fl!("drop-zone-active"), String::new())
    } else {
        (
            fl!("taildrop-select-files"),
            fl!("taildrop-select-files-detail"),
        )
    };

    let mut content = widget::Column::new()
        .push(icons::named(icons::SEND, 32))
        .push(widget::text::body(title))
        .spacing(spacing.space_xs)
        .align_x(Alignment::Center);

    if !detail.is_empty() {
        content = content.push(widget::text::caption(detail));
    }

    widget::button::custom(
        content
            .apply(widget::container)
            .padding(spacing.space_l)
            .width(Length::Fill)
            .align_x(Alignment::Center),
    )
    .on_press(Message::ChooseFiles(target))
    .width(Length::Fill)
    .class(theme::Button::Custom {
        active: Box::new(move |_focused, t| area_style(t, hovered)),
        disabled: Box::new(move |t| area_style(t, hovered)),
        hovered: Box::new(move |_focused, t| area_style(t, true)),
        pressed: Box::new(move |_focused, t| area_style(t, true)),
    })
    .into()
}

fn area_style(theme: &cosmic::Theme, active: bool) -> cosmic::widget::button::Style {
    let cosmic = theme.cosmic();
    let accent: cosmic::iced::Color = cosmic.accent_color().into();
    let (fill, edge, width) = if active {
        (0.22, 0.9, 2.0)
    } else {
        (0.08, 0.35, 1.0)
    };

    cosmic::widget::button::Style {
        background: Some(cosmic::iced::Color { a: fill, ..accent }.into()),
        border_color: cosmic::iced::Color { a: edge, ..accent },
        border_width: width,
        border_radius: cosmic.corner_radii.radius_m.into(),
        text_color: Some(cosmic.on_bg_color().into()),
        icon_color: Some(accent),
        ..Default::default()
    }
}

/// Files chosen but not yet sent, with the machine they will go to.
fn queued(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let names: Vec<String> = state
        .pending_drop
        .iter()
        .map(|path| {
            path.file_name()
                .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
        })
        .collect();

    let target = state
        .taildrop_target
        .as_deref()
        .and_then(|id| state.status.as_deref()?.peer_by_id(id));

    let send = target.map(|peer| Message::SendPendingTo(peer.id.clone()));

    let mut column = widget::Column::new()
        .push(widgets::section_label(fl!("taildrop-queued")))
        .push(widget::text::body(names.join(", ")))
        .spacing(spacing.space_xxs);

    if target.is_none() {
        column = column.push(widget::text::caption(fl!("taildrop-pick-target-first")));
    }

    column
        .push(
            widget::Row::new()
                .push(
                    widget::button::suggested(match target {
                        Some(peer) => fl!("action-send-to", name = peer.display_name()),
                        None => fl!("action-send-file"),
                    })
                    .on_press_maybe(send),
                )
                .push(
                    widget::button::standard(fl!("cancel"))
                        .on_press(Message::ClearPendingDrop),
                )
                .spacing(spacing.space_xs),
        )
        .apply(widget::container)
        .padding(spacing.space_s)
        .width(Length::Fill)
        .class(theme::Container::Primary)
        .into()
}

// ---------------------------------------------------------------------------
// Incoming
// ---------------------------------------------------------------------------

/// Files that have arrived and are waiting to be saved.
fn received_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let mut column = widget::Column::new()
        .push(widgets::section_header(
            icons::DOWNLOAD,
            fl!("taildrop-received"),
        ))
        .spacing(spacing.space_s);

    if state.waiting_files.is_empty() {
        return column
            .push(widget::text::body(fl!("taildrop-none-waiting")))
            .apply(widget::container)
            .padding(spacing.space_m)
            .width(Length::Fill)
            .class(theme::Container::Card)
            .into();
    }

    for file in state.waiting_files.iter() {
        column = column.push(
            widget::Row::new()
                .push(icons::named(icons::DOWNLOAD, 16))
                .push(widget::text::body(file.name.clone()).width(Length::Fill))
                .push(widget::text::caption(file.human_size()))
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center)
                .apply(widget::container)
                .padding(spacing.space_xs)
                .width(Length::Fill)
                .class(theme::Container::Primary),
        );
    }

    // The daemon holds received files until something claims them, so this is
    // the action that actually puts them on disk.
    let names: Vec<String> = state
        .waiting_files
        .iter()
        .map(|file| file.name.clone())
        .collect();

    column
        .push(
            widget::button::suggested(fl!("taildrop-save-all"))
                .on_press(Message::SaveWaitingFiles(names)),
        )
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}
