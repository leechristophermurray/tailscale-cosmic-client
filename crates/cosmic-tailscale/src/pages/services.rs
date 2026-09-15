//! Services & DNS: what this machine publishes over the tailnet with
//! `tailscale serve`, what it exposes publicly with `tailscale funnel`, and the
//! MagicDNS state that makes those names resolve.

use cosmic::iced::{Alignment, Length};
use cosmic::{Apply, Element, theme, widget};
use tailscale_localapi::{ServeEntry, ServeScope};

use crate::app::message::Message;
use crate::fl;
use crate::app::state::State;
use crate::ui::{Tone, icons, widgets};

pub fn view(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    widget::scrollable(
        widget::Column::new()
            .push(dns_card(state))
            .push(serve_card(state))
            .spacing(spacing.space_m),
    )
    .height(Length::Fill)
    .into()
}

/// MagicDNS status and this machine's name on the tailnet.
fn dns_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let magic_dns_on = state
        .status
        .as_deref()
        .and_then(|s| s.current_tailnet.as_ref())
        .is_some_and(|t| t.magic_dns_enabled);

    let accept_dns = state.prefs.as_deref().is_some_and(|p| p.corp_dns);

    let toggle = widget::toggler(accept_dns);
    let toggle = if state.writing_prefs {
        toggle
    } else {
        toggle.on_toggle(Message::SetAcceptDns)
    };

    let mut column = widget::Column::new()
        .push(widgets::section_header(icons::SERVICES, fl!("magic-dns")))
        .push(if magic_dns_on {
            widgets::status_label(fl!("magic-dns-on"), Tone::Positive)
        } else {
            widgets::status_label(fl!("magic-dns-off"), Tone::Caution)
        })
        .spacing(spacing.space_xs);

    if let Some(peer) = state.self_peer() {
        let name = peer.magic_dns().to_string();
        column = column.push(
            widget::Row::new()
                .push(widget::text::caption(fl!("resolves-as")))
                .push(widgets::copy_chip(
                    name.clone(),
                    Message::CopyToClipboard(name),
                ))
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center),
        );
    }

    column = column.push(widget::divider::horizontal::default()).push(
        widget::Row::new()
            .push(
                widget::Column::new()
                    .push(widget::text::body(fl!("use-tailnet-dns")))
                    .push(widget::text::caption(fl!("use-tailnet-dns-detail")))
                    .spacing(spacing.space_xxxs)
                    .width(Length::Fill),
            )
            .push(toggle)
            .spacing(spacing.space_s)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    );

    column
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

/// Published services, one row per handler.
fn serve_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let entries = state.serve.entries();

    let header = widget::Row::new()
        .push(widgets::section_header(
            icons::SERVICES,
            fl!("serve-and-funnel"),
        ))
        .push(widget::Space::new().width(Length::Fill))
        .push(widget::text::caption(fl!(
            "serve-published",
            count = entries.len()
        )))
        .align_y(Alignment::Center)
        .width(Length::Fill);

    let mut column = widget::Column::new().push(header).spacing(spacing.space_s);

    if entries.is_empty() {
        column = column.push(widget::text::body(fl!("serve-none")));
    } else {
        for entry in &entries {
            column = column.push(serve_row(entry));
        }
    }

    column
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

fn serve_row(entry: &ServeEntry) -> Element<'static, Message> {
    let spacing = theme::spacing();
    let url = entry.url();

    // A funnelled service is reachable by anyone on the internet. That
    // difference is the single most important thing on this row.
    let (tone, label) = match entry.scope {
        ServeScope::Tailnet => (Tone::Positive, fl!("scope-tailnet")),
        ServeScope::Funnel => (Tone::Caution, fl!("scope-funnel")),
    };

    widget::Column::new()
        .push(
            widget::Row::new()
                .push(widget::text::monotext(url.clone()).size(13.0))
                .push(widgets::pill(label, tone))
                .push(widget::Space::new().width(Length::Fill))
                .push(
                    widget::button::icon(widget::icon::from_name(icons::COPY).size(14))
                        .on_press(Message::CopyToClipboard(url.clone()))
                        .class(theme::Button::Icon),
                )
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center)
                .width(Length::Fill),
        )
        .push(
            widget::Row::new()
                .push(icons::named(icons::FORWARD, 14))
                .push(widget::text::monotext(entry.target.clone()).size(12.0))
                .spacing(spacing.space_xxs)
                .align_y(Alignment::Center),
        )
        .spacing(spacing.space_xxs)
        .apply(widget::container)
        .padding(spacing.space_s)
        .width(Length::Fill)
        .class(theme::Container::Primary)
        .into()
}
