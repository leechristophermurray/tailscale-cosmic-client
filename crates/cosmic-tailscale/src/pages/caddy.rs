//! Caddy: manage a reverse proxy running on a tailnet peer.
//!
//! Caddy is configured through a JSON admin API, not a config file, so this
//! page edits routes structurally and Caddy applies them live. The admin API
//! normally binds to the peer's localhost, so reaching it means either the
//! peer has rebound it to its tailnet address or we tunnel in over Tailscale
//! SSH — neither exposes the admin port to the wider network.

use cosmic::iced::{Alignment, Length};
use cosmic::{Apply, Element, theme, widget};

use crate::app::caddy::{CaddyState, Connection};
use crate::app::message::Message;
use crate::fl;
use crate::app::state::State;
use crate::ui::{Tone, icons, widgets};

pub fn view(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    widget::scrollable(
        widget::Column::new()
            .push(target_card(state))
            .push(routes_card(state))
            .push(add_route_card(state))
            .spacing(spacing.space_m),
    )
    .height(Length::Fill)
    .into()
}

/// Pick which peer to manage, and show how we are reaching its admin API.
fn target_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let caddy = &state.caddy;

    // Only online peers that accept Tailscale SSH are candidates: without SSH
    // there is no way to tunnel if the admin API is not directly reachable.
    let candidates = state.caddy_candidates();

    let mut column = widget::Column::new()
        .push(widgets::section_header(icons::SERVICES, fl!("caddy-server")))
        .spacing(spacing.space_s);

    if candidates.is_empty() {
        return column
            .push(widget::text::body(fl!("caddy-no-candidates")))
            .apply(widget::container)
            .padding(spacing.space_m)
            .width(Length::Fill)
            .class(theme::Container::Card)
            .into();
    }

    let labels: Vec<String> = candidates
        .iter()
        .map(|peer| peer.display_name().to_string())
        .collect();

    let selected = caddy
        .target
        .as_ref()
        .and_then(|id| candidates.iter().position(|p| &p.id == id));

    column = column.push(
        widget::Row::new()
            .push(widget::text::caption(fl!("caddy-machine")))
            .push(widget::dropdown(
                labels,
                selected,
                Message::CaddyTargetSelected,
            ))
            .push(widget::Space::new().width(Length::Fill))
            .push(
                widgets::icon_button(
                    icons::REFRESH,
                    if caddy.is_busy() {
                        fl!("caddy-connecting")
                    } else {
                        fl!("caddy-connect")
                    },
                    (!caddy.is_busy() && caddy.target.is_some())
                        .then_some(Message::CaddyConnect),
                )
                .class(theme::Button::Suggested),
            )
            .spacing(spacing.space_xs)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    );

    column = column.push(connection_status(caddy));

    column
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

fn connection_status(caddy: &CaddyState) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let (tone, headline, detail) = match &caddy.connection {
        Connection::Idle => (
            Tone::Neutral,
            fl!("caddy-not-connected"),
            fl!("caddy-not-connected-detail"),
        ),
        Connection::Connecting => (
            Tone::Caution,
            fl!("caddy-connecting"),
            fl!("caddy-probing"),
        ),
        Connection::Direct(endpoint) => (
            Tone::Positive,
            fl!("caddy-connected-direct"),
            fl!("caddy-connected-direct-detail", endpoint = endpoint.to_string()),
        ),
        Connection::Tunnelled(endpoint) => (
            Tone::Positive,
            fl!("caddy-connected-tunnel"),
            fl!("caddy-connected-tunnel-detail", endpoint = endpoint.to_string()),
        ),
        Connection::Failed(error) => (Tone::Critical, fl!("caddy-unreachable"), error.clone()),
    };

    widget::Column::new()
        .push(widgets::status_label(headline, tone))
        .push(widget::text::caption(detail))
        .spacing(spacing.space_xxxs)
        .apply(widget::container)
        .padding(spacing.space_s)
        .width(Length::Fill)
        .class(theme::Container::Primary)
        .into()
}

/// The routes Caddy is currently serving.
fn routes_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let caddy = &state.caddy;

    if !caddy.connection.is_connected() {
        return widget::Space::new().width(0).height(0).into();
    }

    let header = widget::Row::new()
        .push(widgets::section_header(icons::FORWARD, fl!("caddy-routes")))
        .push(widget::Space::new().width(Length::Fill))
        .push(widget::text::caption(fl!(
            "caddy-route-count",
            count = caddy.sites.len()
        )))
        .align_y(Alignment::Center)
        .width(Length::Fill);

    let mut column = widget::Column::new().push(header).spacing(spacing.space_s);

    if caddy.sites.is_empty() {
        column = column.push(widget::text::body(fl!("caddy-no-routes")));
    } else {
        for site in &caddy.sites {
            column = column.push(site_row(site));
        }
    }

    column
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

fn site_row(site: &caddy_admin::Site) -> Element<'static, Message> {
    let spacing = theme::spacing();

    let title = match &site.path {
        Some(path) if path != "/" => format!("{}{path}", site.host),
        _ => site.host.clone(),
    };

    // A `.ts.net` host gets its certificate from Tailscale automatically, which
    // is the whole reason to run Caddy on a tailnet node.
    let https = site.host.ends_with(".ts.net");

    let mut heading = widget::Row::new()
        .push(widget::text::monotext(title.clone()).size(13.0))
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center);

    if https {
        heading = heading.push(widgets::pill(fl!("caddy-automatic-https"), Tone::Positive));
    }

    // Only routes this client created carry an `@id`, and only those can be
    // removed by id. A Caddyfile-compiled site has none — so rather than show a
    // Remove button that cannot work, say where the site came from.
    let trailing: Element<'static, Message> = match site.id.clone() {
        Some(id) => widget::button::text(fl!("remove"))
            .on_press(Message::CaddyDeleteRoute(id))
            .class(theme::Button::Text)
            .into(),
        None => widget::text::caption(fl!("caddy-from-caddyfile")).into(),
    };

    widget::Column::new()
        .push(
            heading
                .push(widget::Space::new().width(Length::Fill))
                .push(
                    widget::button::icon(widget::icon::from_name(icons::COPY).size(14))
                        .on_press(Message::CopyToClipboard(title))
                        .class(theme::Button::Icon),
                )
                .push(trailing),
        )
        .push(
            widget::Row::new()
                .push(icons::named(icons::FORWARD, 14))
                .push(widget::text::monotext(site.target.clone()).size(12.0))
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

/// Add a reverse proxy: a hostname and where to send its traffic.
fn add_route_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let caddy = &state.caddy;

    if !caddy.connection.is_connected() {
        return widget::Space::new().width(0).height(0).into();
    }

    let ready = !caddy.new_host.trim().is_empty() && !caddy.new_upstream.trim().is_empty();

    widget::Column::new()
        .push(widgets::section_header(icons::ADD, fl!("caddy-add-route")))
        .push(
            widget::Row::new()
                .push(
                    widget::text_input::text_input("app.your-tailnet.ts.net", &caddy.new_host)
                        .label(fl!("caddy-hostname"))
                        .on_input(Message::CaddyHostChanged)
                        .width(Length::FillPortion(1)),
                )
                .push(
                    widget::text_input::text_input("127.0.0.1:8080", &caddy.new_upstream)
                        .label(fl!("caddy-forward-to"))
                        .on_input(Message::CaddyUpstreamChanged)
                        .width(Length::FillPortion(1)),
                )
                .spacing(spacing.space_xs)
                .width(Length::Fill),
        )
        .push(widget::text::caption(fl!("caddy-https-note")))
        .push(
            widget::button::suggested(fl!("caddy-add"))
                .on_press_maybe((ready && !caddy.is_busy()).then_some(Message::CaddyAddRoute)),
        )
        .spacing(spacing.space_s)
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}
