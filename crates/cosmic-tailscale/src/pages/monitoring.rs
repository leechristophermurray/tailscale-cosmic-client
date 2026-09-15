//! Monitoring: hardware health from a Beszel hub, beside the tailnet machines
//! it belongs to.
//!
//! The hub is reached over the tailnet, so it stays off the public internet.
//! Everything here degrades on its own: with no hub configured the page is a
//! sign-in form, and the rest of the application is unaffected either way.

use beszel_client::{ContainerStats, Stats, SystemRecord};
use cosmic::iced::{Alignment, Length};
use cosmic::{Apply, Element, theme, widget};

use crate::app::beszel::{BeszelState, HubConnection};
use crate::app::message::Message;
use crate::app::state::State;
use crate::fl;
use crate::ui::{Tone, chart, format, icons, widgets};

pub fn view(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let mut column = widget::Column::new()
        .push(hub_card(state))
        .spacing(spacing.space_m);

    if state.beszel.connection.is_connected() {
        column = column
            .push(monitored_card(state))
            .push(detail_card(&state.beszel))
            .push(unmonitored_card(state));
    }

    if let Some(pending) = &state.beszel.pending_install {
        column = column.push(install_confirmation(pending));
    }

    widget::scrollable(column).height(Length::Fill).into()
}

// ---------------------------------------------------------------------------
// Hub connection
// ---------------------------------------------------------------------------

fn hub_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let beszel = &state.beszel;

    let mut column = widget::Column::new()
        .push(widgets::section_header(icons::MONITORING, fl!("beszel-hub")))
        .spacing(spacing.space_s);

    column = column.push(match &beszel.connection {
        HubConnection::Connected => widgets::status_label(
            fl!("beszel-connected", version = beszel.hub_version.as_str()),
            Tone::Positive,
        ),
        HubConnection::Connecting => {
            widgets::status_label(fl!("beszel-connecting"), Tone::Caution)
        }
        HubConnection::NeedsSignIn => {
            widgets::status_label(fl!("beszel-needs-signin"), Tone::Critical)
        }
        HubConnection::Unreachable(reason) => widgets::status_label(
            fl!("beszel-unreachable", reason = reason.as_str()),
            Tone::Critical,
        ),
        HubConnection::Failed(reason) => widgets::status_label(
            fl!("beszel-failed", reason = reason.as_str()),
            Tone::Critical,
        ),
        HubConnection::Unconfigured => {
            widgets::status_label(fl!("beszel-unconfigured"), Tone::Neutral)
        }
    });

    if matches!(beszel.connection, HubConnection::Unconfigured) {
        column = column.push(widget::text::body(fl!("beszel-unconfigured-detail")));
    }

    if beszel.connection.is_connected() {
        column = column.push(
            widget::button::text(fl!("beszel-sign-out"))
                .on_press(Message::BeszelSignOut)
                .class(theme::Button::Text),
        );
    } else {
        column = column.push(sign_in_form(beszel));
    }

    column
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

fn sign_in_form(beszel: &BeszelState) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let ready = !beszel.url_input.trim().is_empty()
        && !beszel.user_input.trim().is_empty()
        && !beszel.password_input.is_empty()
        && !beszel.connection.is_busy();

    let mut column = widget::Column::new()
        .push(
            widget::text_input::text_input(fl!("beszel-url-hint"), &beszel.url_input)
                .label(fl!("beszel-url"))
                .on_input(Message::BeszelUrlChanged)
                .width(Length::Fill),
        )
        .push(
            widget::Row::new()
                .push(
                    widget::text_input::text_input("", &beszel.user_input)
                        .label(fl!("beszel-user"))
                        .on_input(Message::BeszelUserChanged)
                        .width(Length::FillPortion(1)),
                )
                .push(
                    widget::text_input::secure_input(
                        "",
                        &beszel.password_input,
                        None,
                        true,
                    )
                    .label(fl!("beszel-password"))
                    .on_input(Message::BeszelPasswordChanged)
                    .width(Length::FillPortion(1)),
                )
                .spacing(spacing.space_xs)
                .width(Length::Fill),
        )
        .spacing(spacing.space_xs);

    // Say where the password came from, so a pre-filled box does not look like
    // the application invented one.
    if beszel.password_stored {
        column = column.push(widget::text::caption(fl!("beszel-password-stored")));
    }

    column
        .push(
            widget::button::suggested(if beszel.connection.is_busy() {
                fl!("beszel-connecting")
            } else {
                fl!("beszel-connect")
            })
            .on_press_maybe(ready.then_some(Message::BeszelConnect)),
        )
        .into()
}

// ---------------------------------------------------------------------------
// Monitored machines
// ---------------------------------------------------------------------------

fn monitored_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let beszel = &state.beszel;

    let mut column = widget::Column::new()
        .push(widgets::section_label(fl!("beszel-monitored")))
        .spacing(spacing.space_xs);

    if beszel.systems.is_empty() {
        column = column.push(widget::text::body(fl!("beszel-none-monitored")));
    } else {
        for system in &beszel.systems {
            column = column.push(system_row(state, system));
        }
    }

    column
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

fn system_row<'a>(state: &'a State, system: &'a SystemRecord) -> Element<'a, Message> {
    let spacing = theme::spacing();
    let selected = state.beszel.selected.as_deref() == Some(system.id.as_str());

    let tone = if system.status.is_up() {
        Tone::Positive
    } else {
        Tone::Neutral
    };

    // The agent version only matters when it disagrees with the hub, which is a
    // plausible cause of metrics going missing.
    let version = if system.info.agent_version.is_empty()
        || system.info.agent_version == state.beszel.hub_version
    {
        fl!("beszel-agent-version", version = system.info.agent_version.as_str())
    } else {
        fl!(
            "beszel-agent-outdated",
            version = system.info.agent_version.as_str(),
            hub = state.beszel.hub_version.as_str()
        )
    };

    let metrics = widget::Row::new()
        .push(metric(fl!("beszel-cpu"), system.info.cpu))
        .push(metric(fl!("beszel-memory"), system.info.memory_pct))
        .push(metric(fl!("beszel-disk"), system.info.disk_pct))
        .spacing(spacing.space_s)
        .align_y(Alignment::Center);

    let content = widget::Row::new()
        .push(icons::named(icons::MONITORING, 20))
        .push(
            widget::Column::new()
                .push(
                    widget::Row::new()
                        .push(widget::text::body(system.name.clone()))
                        .push(widgets::pill(system.status.label(), tone))
                        .spacing(spacing.space_xxs)
                        .align_y(Alignment::Center),
                )
                .push(widget::text::caption(format!(
                    "{} · {}",
                    fl!("beszel-uptime", uptime = system.info.uptime_human()),
                    version
                )))
                .spacing(spacing.space_xxxs)
                .width(Length::Fill),
        )
        .push(metrics)
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
        .on_press(Message::BeszelSelectSystem(system.id.clone()))
        .into()
}

/// A compact percentage readout that turns amber then red as it fills.
fn metric<'a>(label: String, percent: f64) -> Element<'a, Message> {
    let tone = match percent {
        p if p >= 90.0 => Tone::Critical,
        p if p >= 75.0 => Tone::Caution,
        _ => Tone::Positive,
    };

    widget::Column::new()
        .push(widget::text::caption(label))
        .push(widgets::status_label(format!("{percent:.0}%"), tone))
        .spacing(theme::spacing().space_xxxs)
        .align_x(Alignment::End)
        .into()
}

// ---------------------------------------------------------------------------
// Selected machine detail
// ---------------------------------------------------------------------------

fn detail_card(beszel: &BeszelState) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let Some(system) = beszel.selected_system() else {
        return widget::Space::new().width(0).height(0).into();
    };

    let mut column = widget::Column::new()
        .push(widget::text::title3(system.name.clone()))
        .spacing(spacing.space_s);

    let Some(stats) = beszel.selected_stats() else {
        return column
            .push(widget::text::body(fl!("beszel-no-stats")))
            .apply(widget::container)
            .padding(spacing.space_m)
            .width(Length::Fill)
            .class(theme::Container::Card)
            .into();
    };

    column = column.push(stat_row(system, stats));
    column = column.push(charts(beszel));

    if !stats.zfs_pools.is_empty() {
        column = column.push(pools(stats));
    }

    if !stats.temperatures.is_empty() {
        column = column.push(sensors(stats));
    }

    column = column.push(containers(beszel.selected_containers()));

    column
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

fn stat_row<'a>(system: &'a SystemRecord, stats: &'a Stats) -> Element<'a, Message> {
    let spacing = theme::spacing();

    let memory = fl!(
        "beszel-memory-detail",
        used = format!("{:.1} GiB", stats.memory_used),
        total = format!("{:.1} GiB", stats.memory_total),
        // Cache and ZFS ARC are reported beside used memory, not inside it.
        cache = format!("{:.1} GiB", stats.memory_reclaimable())
    );

    let load = system.info.threads.to_string();

    widget::Row::new()
        .push(widgets::stat_card(
            icons::MONITORING,
            fl!("beszel-cpu"),
            format!("{:.1}%", stats.cpu),
            system.info.cpu_model.clone(),
        ))
        .push(widgets::stat_card(
            icons::MONITORING,
            fl!("beszel-memory"),
            format!("{:.0}%", stats.memory_pct),
            memory,
        ))
        .push(widgets::stat_card(
            icons::SERVICES,
            fl!("beszel-disk"),
            format!("{:.0}%", stats.disk_pct),
            fl!(
                "beszel-disk-detail",
                used = format!("{:.0} GiB", stats.disk_used),
                total = format!("{:.0} GiB", stats.disk_total)
            ),
        ))
        .push(widgets::stat_card(
            icons::MONITORING,
            fl!("beszel-load"),
            format!("{:.2}", stats.load_average[0]),
            fl!(
                "beszel-load-detail",
                one = format!("{:.2}", stats.load_average[0]),
                five = format!("{:.2}", stats.load_average[1]),
                fifteen = format!("{:.2}", stats.load_average[2]),
                threads = load
            ),
        ))
        .spacing(spacing.space_xs)
        .width(Length::Fill)
        .into()
}

/// The time-series panels, all reading the same slice.
///
/// One period control sits above every chart rather than one per card: a filter
/// inside a chart makes the cards disagree about what they are showing.
fn charts(beszel: &BeszelState) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let history = &beszel.history;

    if history.len() < 2 {
        return widget::Column::new()
            .push(period_picker(beszel))
            .push(widget::text::caption(fl!("beszel-history-thin")))
            .spacing(spacing.space_xs)
            .into()
            ;
    }

    let series = |pick: fn(&beszel_client::StatsRecord) -> f64| -> Vec<f64> {
        history.iter().map(pick).collect()
    };

    // Single series: the desktop accent, filled. No legend — the title says
    // what it is.
    let cpu = chart::Chart::new(
        vec![
            chart::Series::new(
                fl!("beszel-cpu"),
                chart::SeriesColor::Accent,
                series(|r| r.stats.cpu),
            )
            .filled(),
        ],
        chart::Scale::Percent,
    );

    let memory = chart::Chart::new(
        vec![
            chart::Series::new(
                fl!("beszel-memory"),
                chart::SeriesColor::Accent,
                series(|r| r.stats.memory_pct),
            )
            .filled(),
        ],
        chart::Scale::Percent,
    );

    let disk = chart::Chart::new(
        vec![
            chart::Series::new(
                fl!("beszel-disk"),
                chart::SeriesColor::Accent,
                series(|r| r.stats.disk_pct),
            )
            .filled(),
        ],
        chart::Scale::Percent,
    );

    // Two series, same units, one axis — never a second y-scale.
    let disk_io = chart::Chart::new(
        vec![
            chart::Series::new(
                fl!("beszel-read"),
                chart::SeriesColor::Slot(0),
                series(|r| r.stats.disk_io[0] as f64),
            ),
            chart::Series::new(
                fl!("beszel-write"),
                chart::SeriesColor::Slot(1),
                series(|r| r.stats.disk_io[1] as f64),
            ),
        ],
        chart::Scale::Rate,
    );

    let bandwidth = chart::Chart::new(
        vec![
            chart::Series::new(
                fl!("beszel-sent"),
                chart::SeriesColor::Slot(0),
                series(|r| r.stats.bandwidth_sent() as f64),
            ),
            chart::Series::new(
                fl!("beszel-received"),
                chart::SeriesColor::Slot(1),
                series(|r| r.stats.bandwidth_received() as f64),
            ),
        ],
        chart::Scale::Rate,
    );

    let load = chart::Chart::new(
        vec![
            chart::Series::new(
                fl!("beszel-load-1"),
                chart::SeriesColor::Slot(0),
                series(|r| r.stats.load_average[0]),
            ),
            chart::Series::new(
                fl!("beszel-load-5"),
                chart::SeriesColor::Slot(1),
                series(|r| r.stats.load_average[1]),
            ),
            chart::Series::new(
                fl!("beszel-load-15"),
                chart::SeriesColor::Slot(2),
                series(|r| r.stats.load_average[2]),
            ),
        ],
        chart::Scale::Number,
    );

    let mut column = widget::Column::new()
        .push(period_picker(beszel))
        .push(chart_row(fl!("beszel-cpu"), cpu, fl!("beszel-memory"), memory))
        .push(chart_row(
            fl!("beszel-disk"),
            disk,
            fl!("beszel-disk-io"),
            disk_io,
        ))
        .push(chart_row(
            fl!("beszel-bandwidth"),
            bandwidth,
            fl!("beszel-load"),
            load,
        ))
        .spacing(spacing.space_m);

    if let Some(temperature) = temperature_chart(history) {
        column = column.push(temperature);
    }

    column.into()
}

/// Two charts side by side, each with its own title.
fn chart_row<'a>(
    left_title: String,
    left: chart::Chart,
    right_title: String,
    right: chart::Chart,
) -> Element<'a, Message> {
    let spacing = theme::spacing();

    let panel = |title: String, chart: chart::Chart| {
        let body: Element<'a, Message> = if chart.is_empty() {
            // A flat "no data" line beats an axis with nothing on it.
            widget::text::caption(fl!("beszel-history-thin")).into()
        } else {
            chart::view(chart, None)
        };

        widget::Column::new()
            .push(widgets::section_label(title))
            .push(body)
            .spacing(spacing.space_xxs)
            .width(Length::FillPortion(1))
    };

    widget::Row::new()
        .push(panel(left_title, left))
        .push(panel(right_title, right))
        .spacing(spacing.space_m)
        .width(Length::Fill)
        .into()
}

/// Temperatures, as emphasis rather than eight colours.
///
/// This machine reports eight sensors. Eight categorical hues would be
/// indistinguishable under colour-vision deficiency and would bury the only
/// number that matters, so the hottest sensor is drawn in the accent and the
/// rest recede into context.
fn temperature_chart<'a>(
    history: &[beszel_client::StatsRecord],
) -> Option<Element<'a, Message>> {
    let spacing = theme::spacing();

    let latest = history.last()?;
    if latest.stats.temperatures.is_empty() {
        return None;
    }

    let (hottest, peak) = latest.stats.peak_temperature()?;
    let hottest = hottest.to_string();

    let mut series = Vec::new();

    for name in latest.stats.temperatures.keys() {
        let points: Vec<f64> = history
            .iter()
            .map(|record| {
                record
                    .stats
                    .temperatures
                    .get(name)
                    .copied()
                    .unwrap_or(0.0)
            })
            .collect();

        let is_hottest = *name == hottest;
        series.push(chart::Series::new(
            name.clone(),
            if is_hottest {
                chart::SeriesColor::Accent
            } else {
                chart::SeriesColor::Muted
            },
            points,
        ));
    }

    // Draw the emphasised line last so it sits above the context.
    series.sort_by_key(|s| s.color == chart::SeriesColor::Accent);

    let caption = fl!(
        "beszel-temp-caption",
        sensor = hottest.as_str(),
        celsius = format!("{peak:.0}"),
        others = latest.stats.temperatures.len().saturating_sub(1)
    );

    Some(
        widget::Column::new()
            .push(widgets::section_label(fl!("beszel-temperature")))
            .push(chart::view(
                chart::Chart::new(series, chart::Scale::Celsius).height(120.0),
                Some(caption),
            ))
            .spacing(spacing.space_xxs)
            .width(Length::Fill)
            .into(),
    )
}

/// The one time-range control, above every chart it scopes.
fn period_picker(beszel: &BeszelState) -> Element<'_, Message> {
    let labels: Vec<String> = crate::app::beszel::PERIODS
        .iter()
        .map(|period| period.label().to_string())
        .collect();

    let selected = crate::app::beszel::PERIODS
        .iter()
        .position(|period| *period == beszel.period);

    widget::Row::new()
        .push(widget::text::caption(fl!("beszel-period")))
        .push(widget::dropdown(
            labels,
            selected,
            Message::BeszelPeriodChanged,
        ))
        .spacing(theme::spacing().space_xs)
        .align_y(Alignment::Center)
        .into()
}

fn pools(stats: &Stats) -> Element<'_, Message> {
    let spacing = theme::spacing();
    let mut column = widget::Column::new()
        .push(widgets::section_label(fl!("beszel-pools")))
        .spacing(spacing.space_xxs);

    for (name, pool) in &stats.zfs_pools {
        let tone = if pool.is_healthy() {
            Tone::Positive
        } else {
            Tone::Critical
        };

        column = column.push(
            widget::Row::new()
                .push(widgets::dot(tone))
                .push(widget::text::body(name.clone()).width(Length::Fill))
                .push(widget::text::caption(format!(
                    "{:.0} / {:.0} GiB ({:.0}%)",
                    pool.used,
                    pool.total,
                    pool.used_pct()
                )))
                .push(widgets::pill(pool.health.clone(), tone))
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center)
                .width(Length::Fill),
        );
    }

    column.into()
}

fn sensors(stats: &Stats) -> Element<'_, Message> {
    let spacing = theme::spacing();

    // A machine can report a dozen cores; the hottest is the one that matters.
    let hottest = stats.peak_temperature();

    let mut row = widget::Row::new().spacing(spacing.space_xs);

    for (name, celsius) in &stats.temperatures {
        let tone = match *celsius {
            c if c >= 85.0 => Tone::Critical,
            c if c >= 70.0 => Tone::Caution,
            _ => Tone::Neutral,
        };

        let is_hottest = hottest.is_some_and(|(peak, _)| peak == name);
        let label = format!("{name} {celsius:.0}°C");

        row = row.push(if is_hottest {
            widgets::pill(label, tone)
        } else {
            widget::text::caption(label).into()
        });
    }

    widget::Column::new()
        .push(widgets::section_label(fl!("beszel-sensors")))
        .push(
            widget::scrollable(row)
                .horizontal()
                .width(Length::Fill),
        )
        .spacing(spacing.space_xxs)
        .into()
}

fn containers(containers: &[ContainerStats]) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let mut column = widget::Column::new()
        .push(widgets::section_label(fl!("beszel-containers")))
        .spacing(spacing.space_xxs);

    if containers.is_empty() {
        return column
            .push(widget::text::caption(fl!("beszel-no-containers")))
            .into();
    }

    for container in containers.iter().take(12) {
        let mut row = widget::Row::new()
            .push(widget::text::body(container.name.clone()).width(Length::Fill))
            .push(widget::text::caption(format!("{:.1}%", container.cpu)))
            .push(widget::text::caption(format!("{:.0} MiB", container.memory)))
            .spacing(spacing.space_s)
            .align_y(Alignment::Center);

        if container.update_available {
            row = row.push(widgets::pill(fl!("beszel-container-update"), Tone::Caution));
        }

        column = column.push(row);
    }

    column.into()
}

// ---------------------------------------------------------------------------
// Machines the hub does not know about
// ---------------------------------------------------------------------------

fn unmonitored_card(state: &State) -> Element<'_, Message> {
    let spacing = theme::spacing();

    let peers = state.filtered_peers();
    let unmonitored = state.beszel.unmonitored(&peers);

    if unmonitored.is_empty() {
        return widget::Space::new().width(0).height(0).into();
    }

    let mut column = widget::Column::new()
        .push(widgets::section_label(fl!("beszel-unmonitored")))
        .push(widget::text::caption(fl!("beszel-unmonitored-detail")))
        .spacing(spacing.space_xs);

    for peer in unmonitored {
        let installing = state.beszel.is_installing(peer);

        column = column.push(
            widget::Row::new()
                .push(icons::named(icons::for_os(&peer.os), 16))
                .push(
                    widget::Column::new()
                        .push(widget::text::body(peer.display_name().to_string()))
                        .push(widget::text::caption(format::os_name(&peer.os)))
                        .spacing(spacing.space_xxxs)
                        .width(Length::Fill),
                )
                .push(
                    widget::button::standard(if installing {
                        fl!("beszel-installing")
                    } else {
                        fl!("beszel-install-agent")
                    })
                    .on_press_maybe(
                        (!installing).then(|| Message::BeszelProposeInstall(peer.id.clone())),
                    ),
                )
                .spacing(spacing.space_xs)
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

/// The confirmation shown before anything runs on a remote machine.
///
/// This installs software as root over SSH. The exact command is shown, on the
/// named host, and nothing happens until the user agrees to that specific one —
/// there is deliberately no way to approve a batch.
fn install_confirmation(
    pending: &crate::app::beszel::PendingInstall,
) -> Element<'_, Message> {
    let spacing = theme::spacing();

    widget::Column::new()
        .push(widget::text::title4(fl!(
            "beszel-install-title",
            host = pending.host.as_str()
        )))
        .push(widget::text::body(fl!(
            "beszel-install-explain",
            host = pending.host.as_str()
        )))
        .push(
            widget::text::monotext(pending.command.clone())
                .size(12.0)
                .apply(widget::container)
                .padding(spacing.space_s)
                .width(Length::Fill)
                .class(theme::Container::Primary),
        )
        .push(
            widget::Row::new()
                .push(
                    widget::button::destructive(fl!(
                        "beszel-install-confirm",
                        host = pending.host.as_str()
                    ))
                    .on_press(Message::BeszelConfirmInstall),
                )
                .push(
                    widget::button::standard(fl!("beszel-install-cancel"))
                        .on_press(Message::BeszelCancelInstall),
                )
                .spacing(spacing.space_xs),
        )
        .spacing(spacing.space_s)
        .apply(widget::container)
        .padding(spacing.space_m)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}
