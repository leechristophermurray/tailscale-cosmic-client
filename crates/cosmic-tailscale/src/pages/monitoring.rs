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
        .push(widgets::section_header(
            icons::MONITORING,
            fl!("beszel-hub"),
        ))
        .spacing(spacing.space_s);

    column = column.push(match &beszel.connection {
        HubConnection::Connected => widgets::status_label(
            fl!("beszel-connected", version = beszel.hub_version.as_str()),
            Tone::Positive,
        ),
        HubConnection::Connecting => widgets::status_label(fl!("beszel-connecting"), Tone::Caution),
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
            widget::Row::new()
                .push(
                    widget::Column::new()
                        .push(widget::text::body(fl!("beszel-alert-notifications")))
                        .push(widget::text::caption(fl!(
                            "beszel-alert-notifications-detail"
                        )))
                        .spacing(spacing.space_xxxs)
                        .width(Length::Fill),
                )
                .push(
                    widget::toggler(!state.config.beszel_alerts_muted)
                        .on_toggle(Message::SetBeszelAlertNotifications),
                )
                .spacing(spacing.space_s)
                .align_y(cosmic::iced::Alignment::Center),
        );
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
                    widget::text_input::secure_input("", &beszel.password_input, None, true)
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
        fl!(
            "beszel-agent-version",
            version = system.info.agent_version.as_str()
        )
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
        .class(theme::Button::ListItem(
            [theme::active().cosmic().corner_radii.radius_s[0]; 4],
        ))
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
        .push(opens_files(
            widgets::stat_card(
                icons::SERVICES,
                fl!("beszel-disk"),
                format!("{:.0}%", stats.disk_pct),
                fl!(
                    "beszel-disk-detail",
                    used = format!("{:.0} GiB", stats.disk_used),
                    total = format!("{:.0} GiB", stats.disk_total)
                ),
            ),
            &system.id,
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
            .into();
    }

    let mut column = widget::Column::new()
        .push(period_picker(beszel))
        .spacing(spacing.space_m);

    // Panels sit two to a row, in the order `chart_panels` returns them.
    let system_id = beszel.selected.as_deref().unwrap_or_default();
    let mut panels = chart_panels(history).into_iter();
    while let Some(left) = panels.next() {
        column = column.push(chart_row(left, panels.next(), system_id));
    }

    if let Some(temperature) = temperature_panel(history) {
        column = column.push(
            widget::Column::new()
                .push(widgets::section_label(temperature.title))
                .push(chart::view(
                    temperature.chart.height(120.0),
                    temperature.caption,
                ))
                .spacing(spacing.space_xxs)
                .width(Length::Fill),
        );
    }

    column.into()
}

/// One chart and the title above it.
///
/// Building these apart from drawing them is what makes the colour rules
/// testable: which series wears the accent, which a fixed slot, and how many
/// there are, can all be checked without a renderer.
pub(crate) struct Panel {
    pub title: String,
    pub chart: chart::Chart,
    pub caption: Option<String>,
}

impl Panel {
    fn new(title: String, chart: chart::Chart) -> Self {
        Self {
            title,
            chart,
            caption: None,
        }
    }
}

/// The time-series panels, in display order.
pub(crate) fn chart_panels(history: &[beszel_client::StatsRecord]) -> Vec<Panel> {
    let series = |pick: fn(&beszel_client::StatsRecord) -> f64| -> Vec<f64> {
        history.iter().map(pick).collect()
    };

    // A single series wears the desktop accent, filled, with no legend — the
    // title already says what it is.
    let single = |label: String, points: Vec<f64>| {
        chart::Chart::new(
            vec![chart::Series::new(label, chart::SeriesColor::Accent, points).filled()],
            chart::Scale::Percent,
        )
    };

    #[allow(clippy::cast_precision_loss)] // byte rates for display
    let rate = |value: u64| value as f64;

    vec![
        Panel::new(
            fl!("beszel-cpu"),
            single(fl!("beszel-cpu"), series(|r| r.stats.cpu)),
        ),
        Panel::new(
            fl!("beszel-memory"),
            single(fl!("beszel-memory"), series(|r| r.stats.memory_pct)),
        ),
        Panel::new(
            fl!("beszel-disk"),
            single(fl!("beszel-disk"), series(|r| r.stats.disk_pct)),
        ),
        // Two series in the same unit, so one axis — never a second y-scale.
        Panel::new(
            fl!("beszel-disk-io"),
            chart::Chart::new(
                vec![
                    chart::Series::new(
                        fl!("beszel-read"),
                        chart::SeriesColor::Slot(0),
                        history.iter().map(|r| rate(r.stats.disk_io[0])).collect(),
                    ),
                    chart::Series::new(
                        fl!("beszel-write"),
                        chart::SeriesColor::Slot(1),
                        history.iter().map(|r| rate(r.stats.disk_io[1])).collect(),
                    ),
                ],
                chart::Scale::Rate,
            ),
        ),
        Panel::new(
            fl!("beszel-bandwidth"),
            chart::Chart::new(
                vec![
                    chart::Series::new(
                        fl!("beszel-sent"),
                        chart::SeriesColor::Slot(0),
                        history
                            .iter()
                            .map(|r| rate(r.stats.bandwidth_sent()))
                            .collect(),
                    ),
                    chart::Series::new(
                        fl!("beszel-received"),
                        chart::SeriesColor::Slot(1),
                        history
                            .iter()
                            .map(|r| rate(r.stats.bandwidth_received()))
                            .collect(),
                    ),
                ],
                chart::Scale::Rate,
            ),
        ),
        Panel::new(
            fl!("beszel-load"),
            chart::Chart::new(
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
            ),
        ),
    ]
}

/// Make a disk title open the machine's files on double-click.
///
/// The machine is mounted first if it is not already; the hint says so, since
/// nothing else on the page suggests a title can be clicked.
fn opens_files<'a>(
    content: impl Into<Element<'a, Message>>,
    system_id: &str,
) -> Element<'a, Message> {
    widget::tooltip(
        widget::mouse_area(content)
            .on_double_click(Message::OpenSystemFiles(system_id.to_string())),
        widget::text::caption(fl!("files-open-hint")),
        widget::tooltip::Position::Top,
    )
    .into()
}

/// Two panels side by side. A trailing odd panel takes the full width.
fn chart_row<'a>(left: Panel, right: Option<Panel>, system_id: &str) -> Element<'a, Message> {
    let spacing = theme::spacing();

    let panel = |panel: Panel| {
        let body: Element<'a, Message> = if panel.chart.is_empty() {
            // A "no data" note beats an axis with nothing on it.
            widget::text::caption(fl!("beszel-history-thin")).into()
        } else {
            chart::view(panel.chart, panel.caption)
        };

        let is_disk = panel.title == fl!("beszel-disk") || panel.title == fl!("beszel-disk-io");
        let title = widgets::section_label(panel.title);
        let title = if is_disk {
            opens_files(title, system_id)
        } else {
            title
        };

        widget::Column::new()
            .push(title)
            .push(body)
            .spacing(spacing.space_xxs)
            .width(Length::FillPortion(1))
    };

    let mut row = widget::Row::new()
        .push(panel(left))
        .spacing(spacing.space_m)
        .width(Length::Fill);

    if let Some(right) = right {
        row = row.push(panel(right));
    }

    row.into()
}

/// Temperatures, as emphasis rather than one colour per sensor.
///
/// A machine can report eight or more sensors. That many categorical hues are
/// indistinguishable under colour-vision deficiency and bury the only number
/// that matters, so the hottest sensor wears the accent and the rest recede
/// into context.
pub(crate) fn temperature_panel(history: &[beszel_client::StatsRecord]) -> Option<Panel> {
    let latest = history.last()?;
    let (hottest, peak) = latest.stats.peak_temperature()?;
    let hottest = hottest.to_string();

    let mut series: Vec<chart::Series> = latest
        .stats
        .temperatures
        .keys()
        .map(|name| {
            let color = if *name == hottest {
                chart::SeriesColor::Accent
            } else {
                chart::SeriesColor::Muted
            };
            chart::Series::new(name.clone(), color, sensor_history(history, name))
        })
        .collect();

    // Draw the emphasised line last so it sits above the context.
    series.sort_by_key(|s| s.color == chart::SeriesColor::Accent);

    Some(Panel {
        title: fl!("beszel-temperature"),
        chart: chart::Chart::new(series, chart::Scale::Celsius),
        caption: Some(fl!(
            "beszel-temp-caption",
            sensor = hottest.as_str(),
            celsius = format!("{peak:.0}"),
            others = latest.stats.temperatures.len().saturating_sub(1)
        )),
    })
}

/// One sensor's readings across the history, with gaps filled honestly.
///
/// A sensor can be absent from a sample — an agent restarted, a drive spun
/// down. Filling that gap with zero draws a plunge to 0 °C that never
/// happened. Instead a gap holds the last known reading, and a gap at the
/// start takes the first one that exists.
pub(crate) fn sensor_history(history: &[beszel_client::StatsRecord], sensor: &str) -> Vec<f64> {
    let readings: Vec<Option<f64>> = history
        .iter()
        .map(|record| record.stats.temperatures.get(sensor).copied())
        .collect();

    let first_known = readings
        .iter()
        .flatten()
        .next()
        .copied()
        .unwrap_or_default();

    let mut last = first_known;
    readings
        .into_iter()
        .map(|reading| {
            if let Some(value) = reading {
                last = value;
            }
            last
        })
        .collect()
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
        .push(widget::scrollable(row).horizontal().width(Length::Fill))
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
            .push(widget::text::caption(format!(
                "{:.0} MiB",
                container.memory
            )))
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
fn install_confirmation(pending: &crate::app::beszel::PendingInstall) -> Element<'_, Message> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::chart::{Scale, SeriesColor};
    use beszel_client::StatsRecord;

    fn record(json: &str) -> StatsRecord {
        serde_json::from_str(&format!(
            r#"{{"id":"r","system":"s","type":"1m","created":"x","stats":{json}}}"#
        ))
        .expect("stats record decodes")
    }

    fn history() -> Vec<StatsRecord> {
        (0..5)
            .map(|i| {
                record(&format!(
                    r#"{{"cpu":{i}.0,"mp":10.0,"dp":60.0,"dio":[{r},{w}],"b":[1,2],
                        "la":[1.0,2.0,3.0],"t":{{"core_0":50.0,"nvme":{hot}}}}}"#,
                    r = i * 100,
                    w = i * 200,
                    hot = 60 + i
                ))
            })
            .collect()
    }

    fn panel<'a>(panels: &'a [Panel], title: &str) -> &'a Panel {
        panels
            .iter()
            .find(|p| p.title == title)
            .unwrap_or_else(|| panic!("no {title} panel"))
    }

    #[test]
    fn every_panel_is_present_in_order() {
        let titles: Vec<String> = chart_panels(&history())
            .into_iter()
            .map(|p| p.title)
            .collect();
        assert_eq!(
            titles,
            [
                fl!("beszel-cpu"),
                fl!("beszel-memory"),
                fl!("beszel-disk"),
                fl!("beszel-disk-io"),
                fl!("beszel-bandwidth"),
                fl!("beszel-load"),
            ]
        );
    }

    /// Single-series charts use the desktop accent, filled.
    #[test]
    fn single_series_charts_wear_the_accent() {
        let panels = chart_panels(&history());

        for title in [fl!("beszel-cpu"), fl!("beszel-memory"), fl!("beszel-disk")] {
            let chart = &panel(&panels, &title).chart;
            assert_eq!(chart.series().len(), 1, "{title}");
            assert_eq!(chart.series()[0].color, SeriesColor::Accent, "{title}");
            assert!(chart.series()[0].filled, "{title} should be filled");
            assert_eq!(chart.scale(), Scale::Percent, "{title}");
        }
    }

    /// The accent never appears beside a sibling series. A user's accent is
    /// arbitrary and can fail the checks that keep series distinguishable, so
    /// multi-series charts use the fixed, validated slots — in order, never
    /// cycled.
    #[test]
    fn multi_series_charts_never_use_the_accent() {
        for panel in chart_panels(&history()) {
            let series = panel.chart.series();
            if series.len() < 2 {
                continue;
            }

            for (index, s) in series.iter().enumerate() {
                assert_eq!(
                    s.color,
                    SeriesColor::Slot(index),
                    "{} series {index} should use slot {index}",
                    panel.title
                );
                assert!(
                    !s.filled,
                    "{}: overlapping fills would hide each other",
                    panel.title
                );
            }
        }
    }

    #[test]
    fn multi_series_charts_have_the_right_series_and_units() {
        let panels = chart_panels(&history());

        assert_eq!(
            panel(&panels, &fl!("beszel-disk-io")).chart.series().len(),
            2
        );
        assert_eq!(
            panel(&panels, &fl!("beszel-disk-io")).chart.scale(),
            Scale::Rate
        );
        assert_eq!(
            panel(&panels, &fl!("beszel-bandwidth"))
                .chart
                .series()
                .len(),
            2
        );
        assert_eq!(panel(&panels, &fl!("beszel-load")).chart.series().len(), 3);
        assert_eq!(
            panel(&panels, &fl!("beszel-load")).chart.scale(),
            Scale::Number
        );
    }

    /// Charts read left to right, oldest first, one point per sample.
    #[test]
    fn series_follow_the_history_in_order() {
        let history = history();
        let panels = chart_panels(&history);

        let cpu = &panel(&panels, &fl!("beszel-cpu")).chart.series()[0].points;
        assert_eq!(cpu, &[0.0, 1.0, 2.0, 3.0, 4.0]);

        let write = &panel(&panels, &fl!("beszel-disk-io")).chart.series()[1].points;
        assert_eq!(write, &[0.0, 200.0, 400.0, 600.0, 800.0]);

        for panel in &panels {
            for s in panel.chart.series() {
                assert_eq!(
                    s.points.len(),
                    history.len(),
                    "{}: {}",
                    panel.title,
                    s.label
                );
            }
        }
    }

    /// Emphasis: exactly one sensor in the accent — the hottest — and the rest
    /// as muted context, drawn underneath it.
    #[test]
    fn temperature_emphasises_only_the_hottest_sensor() {
        let panel = temperature_panel(&history()).expect("sensors present");
        let series = panel.chart.series();

        assert_eq!(series.len(), 2);
        let accented: Vec<&str> = series
            .iter()
            .filter(|s| s.color == SeriesColor::Accent)
            .map(|s| s.label.as_str())
            .collect();
        assert_eq!(accented, ["nvme"], "the hottest sensor is emphasised");
        assert!(
            series
                .iter()
                .filter(|s| s.color != SeriesColor::Accent)
                .all(|s| s.color == SeriesColor::Muted)
        );

        // Last in the list means drawn last, on top of the context lines.
        assert_eq!(series.last().map(|s| s.color), Some(SeriesColor::Accent));
        assert_eq!(panel.chart.scale(), Scale::Celsius);
        assert!(panel.caption.expect("caption").contains("nvme"));
    }

    #[test]
    fn eight_sensors_still_get_one_accent() {
        let history = vec![
            record(r#"{"t":{"a":40,"b":41,"c":42,"d":43,"e":44,"f":45,"g":70,"h":46}}"#),
            record(r#"{"t":{"a":40,"b":41,"c":42,"d":43,"e":44,"f":45,"g":71,"h":46}}"#),
        ];
        let panel = temperature_panel(&history).expect("sensors present");
        let accents = panel
            .chart
            .series()
            .iter()
            .filter(|s| s.color == SeriesColor::Accent)
            .count();

        assert_eq!(panel.chart.series().len(), 8);
        assert_eq!(accents, 1, "never one hue per sensor");
    }

    #[test]
    fn no_sensors_means_no_temperature_panel() {
        assert!(temperature_panel(&[record(r#"{"cpu":1.0}"#)]).is_none());
        assert!(temperature_panel(&[]).is_none());
    }

    /// A sensor missing from a sample must not be drawn as a plunge to 0 °C.
    #[test]
    fn missing_sensor_readings_hold_the_last_known_value() {
        let history = vec![
            record(r#"{"t":{"other":1}}"#), // leading gap
            record(r#"{"t":{"nvme":35.0}}"#),
            record(r#"{"t":{"other":1}}"#), // gap in the middle
            record(r#"{"t":{"nvme":38.0}}"#),
        ];

        assert_eq!(sensor_history(&history, "nvme"), [35.0, 35.0, 35.0, 38.0]);
    }
}
