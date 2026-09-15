//! Preferences: the routing and DNS switches, plus what the client is talking
//! to and as whom.

use cosmic::iced::Length;
use cosmic::{Apply, Element, theme, widget};

use crate::app::message::Message;
use crate::app::state::State;
use crate::fl;
use crate::ui::{Tone, icons, widgets};

use super::access::switch_row;

pub fn view(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    widget::scrollable(
        widget::Column::new()
            .push(routing_card(state))
            .push(account_card(state))
            .push(connection_card(state))
            .spacing(spacing.space_m),
    )
    .height(Length::Fill)
    .into()
}

fn routing_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let prefs = state.prefs.as_deref();

    widget::Column::new()
        .push(widgets::section_header(icons::EXIT_NODE, fl!("routing")))
        .push(switch_row(
            state,
            fl!("accept-routes"),
            fl!("accept-routes-detail"),
            prefs.is_some_and(|p| p.route_all),
            Message::SetAcceptRoutes as fn(bool) -> Message,
        ))
        .push(widget::divider::horizontal::default())
        .push(switch_row(
            state,
            fl!("use-tailnet-dns"),
            fl!("use-tailnet-dns-detail"),
            prefs.is_some_and(|p| p.corp_dns),
            Message::SetAcceptDns as fn(bool) -> Message,
        ))
        .spacing(spacing.space_s)
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

fn account_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let profile = state
        .prefs
        .as_deref()
        .and_then(tailscale_localapi::Prefs::user_profile);

    let mut column = widget::Column::new()
        .push(widgets::section_header(icons::ACCOUNT, fl!("account")))
        .spacing(spacing.space_xs);

    if let Some(profile) = profile {
        if !profile.display_name.is_empty() {
            column = column.push(widget::text::body(profile.display_name.clone()));
        }
        column = column.push(widget::text::caption(profile.login_name.clone()));
    } else {
        column = column.push(widget::text::body(fl!("not-signed-in")));
    }

    column = column.push(widget::text::caption(fl!(
        "tailnet-named",
        name = state.tailnet_name()
    )));

    let actions = if state.want_running() {
        widget::Row::new()
            .push(widgets::icon_button(
                icons::REFRESH,
                fl!("reauthenticate"),
                Some(Message::LoginRequested),
            ))
            .spacing(spacing.space_xs)
    } else {
        widget::Row::new()
            .push(
                widgets::icon_button(icons::OK, fl!("sign-in"), Some(Message::LoginRequested))
                    .class(theme::Button::Suggested),
            )
            .spacing(spacing.space_xs)
    };

    column
        .push(actions)
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

/// What the client is connected to. This is the page a user lands on when
/// something is wrong, so it names the socket and the daemon version plainly.
fn connection_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let (tone, summary) = if state.daemon_unreachable {
        (Tone::Critical, fl!("daemon-cannot-reach"))
    } else if let Some(status) = state.status.as_deref() {
        (
            Tone::Positive,
            fl!(
                "daemon-summary",
                version = status.version.as_str(),
                state = crate::ui::backend_description(state.backend)
            ),
        )
    } else {
        (Tone::Neutral, fl!("daemon-connecting-long"))
    };

    let mut column = widget::Column::new()
        .push(widgets::section_header(icons::TAILNET, fl!("daemon")))
        .push(widgets::status_label(summary, tone))
        .push(
            widget::text::monotext(format!("unix://{}", tailscale_localapi::DEFAULT_SOCKET))
                .size(12.0),
        )
        .spacing(spacing.space_xs);

    if state.throughput.has_rate() {
        column = column.push(widget::text::caption(fl!(
            "daemon-transfer",
            rx = crate::ui::format::bytes(state.throughput.total_rx),
            tx = crate::ui::format::bytes(state.throughput.total_tx)
        )));
    }

    let warnings = state.health_warnings();
    if !warnings.is_empty() {
        column = column.push(widget::divider::horizontal::default());
        column = column.push(widgets::section_label(fl!("daemon-warnings")));
        for warning in warnings {
            column = column.push(widgets::status_label(warning.clone(), Tone::Caution));
        }
    }

    column
        .push(
            widgets::icon_button(icons::REFRESH, fl!("refresh-now"), Some(Message::Refresh))
                .class(theme::Button::Standard),
        )
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}
