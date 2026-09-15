//! The small visual vocabulary the pages share.
//!
//! Every colour here comes from the active COSMIC theme rather than a literal,
//! so light mode, dark mode, and a user's custom accent tint all work without
//! the pages knowing anything about it.

use cosmic::iced::{Alignment, Border, Length};
use cosmic::iced::widget::container::Style as ContainerStyle;
use cosmic::widget;
use cosmic::{Apply, Element, theme};

/// Semantic meaning of a status indicator, mapped onto theme colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Online, connected, healthy.
    Positive,
    /// Active but worth noticing — an exit node carrying your traffic.
    Accent,
    /// Degraded but working, e.g. relayed via DERP or a key expiring soon.
    Caution,
    /// Offline or failed.
    Critical,
    /// Informational, no judgement.
    Neutral,
}

impl Tone {
    fn color(self, cosmic: &cosmic::cosmic_theme::Theme) -> cosmic::iced::Color {
        match self {
            Self::Positive => cosmic.success_color().into(),
            Self::Accent => cosmic.accent_color().into(),
            Self::Caution => cosmic.warning_color().into(),
            Self::Critical => cosmic.destructive_color().into(),
            Self::Neutral => {
                let mut color: cosmic::iced::Color = cosmic.on_bg_color().into();
                color.a *= 0.55;
                color
            }
        }
    }
}

/// The small filled circle that precedes a machine name.
pub fn dot<'a, Message: 'a>(tone: Tone) -> Element<'a, Message> {
    widget::container(widget::Space::new().width(0).height(0))
        .width(Length::Fixed(8.0))
        .height(Length::Fixed(8.0))
        .class(theme::Container::custom(move |t| {
            let color = tone.color(t.cosmic());
            ContainerStyle {
                background: Some(color.into()),
                border: Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }))
        .into()
}

/// A rounded status badge: tinted fill, matching border, small bold label.
pub fn pill<'a, Message: 'a>(label: impl Into<String>, tone: Tone) -> Element<'a, Message> {
    let spacing = theme::spacing();

    widget::text::caption(label.into())
        .apply(widget::container)
        .padding([spacing.space_xxxs, spacing.space_xs])
        .class(theme::Container::custom(move |t| {
            let cosmic = t.cosmic();
            let color = tone.color(cosmic);
            ContainerStyle {
                text_color: Some(color),
                background: Some(
                    cosmic::iced::Color {
                        a: 0.16,
                        ..color
                    }
                    .into(),
                ),
                border: Border {
                    radius: cosmic.corner_radii.radius_m.into(),
                    width: 1.0,
                    color: cosmic::iced::Color { a: 0.4, ..color },
                },
                ..Default::default()
            }
        }))
        .into()
}

/// A dot followed by a label — the "● Online" pattern used throughout the list.
pub fn status_label<'a, Message: 'a>(
    label: impl Into<String>,
    tone: Tone,
) -> Element<'a, Message> {
    widget::Row::new()
        .push(dot(tone))
        .push(widget::text::caption(label.into()))
        .spacing(theme::spacing().space_xxs)
        .align_y(Alignment::Center)
        .into()
}

/// Monospace value with a copy button beside it, for IPs and DNS names.
pub fn copy_chip<'a, Message: Clone + 'static>(
    value: impl Into<String>,
    on_copy: Message,
) -> Element<'a, Message> {
    let spacing = theme::spacing();
    let value = value.into();

    widget::Row::new()
        .push(widget::text::monotext(value).size(13.0))
        .push(
            widget::button::icon(widget::icon::from_name(super::icons::COPY).size(14))
                .on_press(on_copy)
                .class(theme::Button::Icon),
        )
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center)
        .apply(widget::container)
        .padding([spacing.space_xxxs, spacing.space_xs])
        .class(theme::Container::Primary)
        .into()
}

/// An uppercase section label, as above "RECENT PEERS & TRANSFER TARGETS".
pub fn section_label<'a, Message: 'a>(text: impl AsRef<str>) -> Element<'a, Message> {
    widget::text::caption_heading(text.as_ref().to_uppercase()).into()
}

/// A section label preceded by an icon.
pub fn section_header<'a, Message: 'a>(
    icon: &'static str,
    text: impl AsRef<str>,
) -> Element<'a, Message> {
    widget::Row::new()
        .push(super::icons::named(icon, 14))
        .push(widget::text::caption_heading(text.as_ref().to_uppercase()))
        .spacing(theme::spacing().space_xxs)
        .align_y(Alignment::Center)
        .into()
}

/// One of the small labelled fact boxes in the detail pane — "OS & PLATFORM",
/// "PEER ENDPOINT", "MACHINE KEY".
pub fn stat_card<'a, Message: 'a>(
    icon: &'static str,
    title: impl AsRef<str>,
    value: impl Into<String>,
    detail: impl Into<String>,
) -> Element<'a, Message> {
    let spacing = theme::spacing();
    let detail = detail.into();

    let mut column = widget::Column::new()
        .push(section_header(icon, title))
        .push(widget::text::body(value.into()))
        .spacing(spacing.space_xxs);

    if !detail.is_empty() {
        column = column.push(widget::text::caption(detail));
    }

    column
        .apply(widget::container)
        .padding(spacing.space_s)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

/// A button that pairs a symbolic icon with a label, which plain
/// `button::standard` cannot do on its own.
pub fn icon_button<'a, Message: Clone + 'static>(
    icon: &'static str,
    label: impl Into<String>,
    on_press: Option<Message>,
) -> widget::Button<'a, Message> {
    let spacing = theme::spacing();

    widget::button::custom(
        widget::Row::new()
            .push(super::icons::named(icon, 16))
            .push(widget::text::body(label.into()))
            .spacing(spacing.space_xxs)
            .align_y(Alignment::Center),
    )
    .padding([spacing.space_xxs, spacing.space_s])
    .on_press_maybe(on_press)
}

/// An empty-state panel: a muted icon, a headline, and a sentence explaining
/// what would appear here.
pub fn empty_state<'a, Message: 'a>(
    icon: &'static str,
    title: impl Into<String>,
    detail: impl Into<String>,
) -> Element<'a, Message> {
    let spacing = theme::spacing();

    widget::Column::new()
        .push(super::icons::named(icon, 48))
        .push(widget::text::title4(title.into()))
        .push(widget::text::body(detail.into()))
        .spacing(spacing.space_xs)
        .align_x(Alignment::Center)
        .apply(widget::container)
        .padding(spacing.space_xl)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

/// A thin progress meter, used for the key-expiry countdown.
pub fn meter<'a, Message: 'a>(progress: f32, tone: Tone) -> Element<'a, Message> {
    let filled = progress.clamp(0.0, 1.0);

    widget::container(
        widget::container(widget::Space::new().width(Length::Fill).height(0))
            .height(Length::Fixed(6.0))
            .width(Length::FillPortion((filled * 1000.0) as u16))
            .class(theme::Container::custom(move |t| {
                let color = tone.color(t.cosmic());
                ContainerStyle {
                    background: Some(color.into()),
                    border: Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })),
    )
    .width(Length::Fill)
    .height(Length::Fixed(6.0))
    .class(theme::Container::custom(|t| {
        let cosmic = t.cosmic();
        ContainerStyle {
            background: Some(cosmic.bg_component_color().into()),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }))
    .into()
}
