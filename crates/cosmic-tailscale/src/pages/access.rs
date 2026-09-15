//! Access controls: who can reach this machine, and the key material that keeps
//! it on the tailnet.

use cosmic::iced::{Alignment, Length};
use cosmic::{Apply, Element, theme, widget};

use crate::app::message::Message;
use crate::app::state::{KeyExpiry, State};
use crate::fl;
use crate::ui::{Tone, format, icons, widgets};

pub fn view(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    widget::scrollable(
        widget::Column::new()
            .push(key_card(state))
            .push(inbound_card(state))
            .push(routes_card(state))
            .spacing(spacing.space_m),
    )
    .height(Length::Fill)
    .into()
}

/// Node key state, with a countdown when the tailnet enforces expiry.
fn key_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let expiry = state.key_expiry();

    let mut column = widget::Column::new()
        .push(widgets::section_header(icons::ACCESS, fl!("machine-key")))
        .push(
            widget::Row::new()
                .push(widget::text::title3(expiry.summary()))
                .push(widget::Space::new().width(Length::Fill))
                .push(match expiry {
                    KeyExpiry::Expires { days, .. } if days < 0 => {
                        widgets::pill(fl!("key-expired"), Tone::Critical)
                    }
                    KeyExpiry::Expires { days, .. } if days < 30 => {
                        widgets::pill(fl!("key-renew-soon"), Tone::Caution)
                    }
                    KeyExpiry::Expires { .. } => widgets::pill(fl!("key-valid"), Tone::Positive),
                    KeyExpiry::Disabled => widgets::pill(fl!("key-no-expiry"), Tone::Positive),
                    KeyExpiry::Unknown => widgets::pill(fl!("key-unknown"), Tone::Neutral),
                })
                .align_y(Alignment::Center)
                .width(Length::Fill),
        )
        .push(widgets::meter(expiry.fraction_remaining(), expiry.tone()))
        .spacing(spacing.space_xs);

    column = column.push(widget::text::caption(match expiry {
        KeyExpiry::Expires { expiry, .. } => {
            fl!("key-expiry-explain", date = format::calendar_date(expiry))
        }
        KeyExpiry::Disabled => fl!("key-expiry-off-explain"),
        KeyExpiry::Unknown => fl!("key-expiry-waiting"),
    }));

    column = column.push(
        widget::Row::new()
            .push(
                widgets::icon_button(
                    icons::REFRESH,
                    fl!("reauthenticate"),
                    Some(Message::LoginRequested),
                )
                .class(theme::Button::Suggested),
            )
            .push(widgets::icon_button(
                icons::ACCESS,
                fl!("admin-console"),
                Some(Message::OpenUrl(
                    "https://login.tailscale.com/admin/machines".to_string(),
                )),
            ))
            .spacing(spacing.space_xs),
    );

    column
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

/// Inbound connection policy: Tailscale SSH and the shields-up switch.
fn inbound_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let prefs = state.prefs.as_deref();

    let ssh_on = prefs.is_some_and(|p| p.run_ssh);
    let shields_on = prefs.is_some_and(|p| p.shields_up);

    widget::Column::new()
        .push(widgets::section_header(
            icons::ACCESS,
            fl!("inbound-access"),
        ))
        .push(switch_row(
            state,
            fl!("accept-ssh"),
            fl!("accept-ssh-detail"),
            ssh_on,
            Message::SetRunSsh as fn(bool) -> Message,
        ))
        .push(widget::divider::horizontal::default())
        .push(switch_row(
            state,
            fl!("shields-up"),
            fl!("shields-up-detail"),
            shields_on,
            Message::SetShieldsUp as fn(bool) -> Message,
        ))
        .spacing(spacing.space_s)
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

/// Subnet routes this machine advertises to the tailnet.
fn routes_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let routes = state
        .prefs
        .as_deref()
        .map(tailscale_localapi::Prefs::subnet_routes)
        .unwrap_or_default();

    let mut column = widget::Column::new()
        .push(widgets::section_header(
            icons::EXIT_NODE,
            fl!("advertised-routes"),
        ))
        .spacing(spacing.space_s);

    if routes.is_empty() {
        column = column.push(widget::text::body(fl!("advertised-routes-none")));
    } else {
        for route in routes {
            column = column.push(
                widget::Row::new()
                    .push(widgets::dot(Tone::Positive))
                    .push(widget::text::monotext(route.to_string()).size(13.0))
                    .spacing(spacing.space_xs)
                    .align_y(Alignment::Center),
            );
        }
        column = column.push(widget::text::caption(fl!("advertised-routes-note")));
    }

    column
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

/// A labelled switch row. Shared by this page and Preferences.
pub fn switch_row<'a>(
    state: &'a State,
    title: impl Into<String>,
    detail: impl Into<String>,
    value: bool,
    message: fn(bool) -> Message,
) -> Element<'a, Message> {
    let spacing = theme::spacing();

    let toggle = widget::toggler(value);
    // While a write is in flight the daemon has not confirmed the last change
    // yet; letting the user queue another would race it.
    let toggle = if state.writing_prefs || state.prefs.is_none() {
        toggle
    } else {
        toggle.on_toggle(message)
    };

    widget::Row::new()
        .push(
            widget::Column::new()
                .push(widget::text::body(title.into()))
                .push(widget::text::caption(detail.into()))
                .spacing(spacing.space_xxxs)
                .width(Length::Fill),
        )
        .push(toggle)
        .spacing(spacing.space_s)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}
