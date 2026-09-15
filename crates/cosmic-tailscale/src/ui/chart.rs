//! Time-series charts, drawn natively with an `iced` canvas.
//!
//! # Why the colours work the way they do
//!
//! Single-series charts use the desktop's accent. There is exactly one mark, so
//! there are no pairs to keep apart — only contrast against the surface, which
//! is the one thing an accent colour is guaranteed to have.
//!
//! Multi-series charts do **not** use the accent. A user's accent is arbitrary:
//! this machine's light accent is a dark desaturated teal that sits below both
//! the lightness band and the chroma floor a categorical slot must clear. It is
//! perfectly legible alone and unfit to sit beside two siblings. So series
//! identity comes from a fixed, validated three-slot palette instead, which also
//! means a series keeps its colour when the user changes their theme.
//!
//! The three slots are blue/orange/aqua, validated all-pairs against COSMIC's
//! real card surfaces in both modes: worst CVD ΔE 9.2 light / 9.4 dark against a
//! target of 8, worst normal-vision ΔE 24.0 / 20.9 against a floor of 15. On the
//! light surface orange and aqua fall below 3:1 contrast, which obliges visible
//! labels rather than colour alone — hence a legend on every multi-series chart
//! and a labelled endpoint, never identity by colour-matching.

use cosmic::iced::widget::canvas::{self, Frame, Path, Stroke, Text};
use cosmic::iced::{Color, Length, Point, Rectangle, mouse};
use cosmic::widget;
use cosmic::{Element, Renderer, Theme};

/// Mark specs, fixed across every chart here.
const LINE_WIDTH: f32 = 2.0;
const AREA_OPACITY: f32 = 0.10;
const GRID_WIDTH: f32 = 1.0;
const END_MARKER_RADIUS: f32 = 4.0;
const SURFACE_RING: f32 = 2.0;

/// Room for the y-axis labels and the x-axis band, so the plot never eats them.
const AXIS_LEFT: f32 = 44.0;
const AXIS_BOTTOM: f32 = 16.0;
const PLOT_TOP: f32 = 8.0;
const PLOT_RIGHT: f32 = 8.0;

/// The fixed categorical slots, by theme mode. Never cycled, never extended —
/// no chart here carries more than three series.
const SLOTS_LIGHT: [Color; 3] = [
    rgb(0x2a, 0x78, 0xd6), // blue
    rgb(0xeb, 0x68, 0x34), // orange
    rgb(0x1b, 0xaf, 0x7a), // aqua
];
const SLOTS_DARK: [Color; 3] = [
    rgb(0x39, 0x87, 0xe5),
    rgb(0xd9, 0x59, 0x26),
    rgb(0x19, 0x9e, 0x70),
];

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

/// Which colour a series wears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesColor {
    /// The desktop accent. Only for a chart with a single series.
    Accent,
    /// A fixed categorical slot, 0-2.
    Slot(usize),
    /// Context behind an emphasised series: recessive, never labelled.
    Muted,
}

/// One line on a chart.
pub struct Series {
    pub label: String,
    pub color: SeriesColor,
    /// Oldest first, so the newest value is the right-hand end.
    pub points: Vec<f64>,
    /// Fill the area under the line. Only sensible for a lone series.
    pub filled: bool,
}

impl Series {
    #[must_use]
    pub fn new(label: impl Into<String>, color: SeriesColor, points: Vec<f64>) -> Self {
        Self {
            label: label.into(),
            color,
            points,
            filled: false,
        }
    }

    #[must_use]
    pub fn filled(mut self) -> Self {
        self.filled = true;
        self
    }
}

/// How to render the y-axis values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// A percentage. The axis keeps its zero baseline but scales its ceiling to
    /// the data, with a floor so a quiet machine is not drawn as dramatic.
    Percent,
    /// Bytes per second, axis scaled to the data.
    Rate,
    /// A bare number, axis scaled to the data.
    Number,
    /// Degrees Celsius.
    Celsius,
}

impl Scale {
    fn format(self, value: f64) -> String {
        match self {
            Self::Percent => format!("{value:.0}%"),
            Self::Rate => super::format::rate(value),
            Self::Celsius => format!("{value:.0}°"),
            Self::Number => {
                if value >= 10.0 {
                    format!("{value:.0}")
                } else {
                    format!("{value:.1}")
                }
            }
        }
    }
}

/// A time-series chart.
pub struct Chart {
    series: Vec<Series>,
    scale: Scale,
    height: f32,
}

impl Chart {
    #[must_use]
    pub fn new(series: Vec<Series>, scale: Scale) -> Self {
        Self {
            series,
            scale,
            height: 96.0,
        }
    }

    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// True when there is nothing worth drawing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.series.iter().all(|s| s.points.len() < 2)
    }

    /// The upper bound of the y-axis, rounded to something a person would pick.
    ///
    /// The baseline always stays at zero — truncating *that* is what makes a
    /// molehill look like a mountain. The ceiling scales to the data, because a
    /// machine sitting at 18% drawn against a fixed 0-100 axis is a flat line
    /// in six pixels, which says less than no chart at all. Both bounds are
    /// labelled, so the range a reader is looking at is never implicit.
    fn ceiling(&self) -> f64 {
        let peak = self
            .series
            .iter()
            .flat_map(|s| s.points.iter().copied())
            .fold(0.0_f64, f64::max);

        if self.scale == Scale::Percent {
            // Headroom above the peak so the line is not pinned to the top,
            // then a floor so small numbers keep a sense of proportion.
            return (peak * 1.25).clamp(25.0, 100.0).ceil();
        }

        if self.scale == Scale::Celsius {
            // Silicon idles in the fifties, so a 0-100 axis spends half its
            // height on temperatures no component will ever report.
            return (peak * 1.25).max(40.0).ceil();
        }

        if peak <= 0.0 {
            return 1.0;
        }

        // Round up to 1, 2 or 5 times a power of ten, so the axis lands on a
        // number worth reading rather than 0.37.
        let magnitude = 10.0_f64.powf(peak.log10().floor());
        let normalised = peak / magnitude;
        let step = if normalised <= 1.0 {
            1.0
        } else if normalised <= 2.0 {
            2.0
        } else if normalised <= 5.0 {
            5.0
        } else {
            10.0
        };
        step * magnitude
    }
}

/// Render a chart with its legend, when it has more than one series.
pub fn view<'a, Message: 'a>(chart: Chart, caption: Option<String>) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();

    // A single series needs no legend: the title already names what is plotted,
    // and a one-swatch box would just restate it.
    let needs_legend = chart
        .series
        .iter()
        .filter(|s| s.color != SeriesColor::Muted)
        .count()
        > 1;

    let legend = needs_legend.then(|| {
        let mut row = widget::Row::new().spacing(spacing.space_s);
        for series in &chart.series {
            if series.color == SeriesColor::Muted {
                continue;
            }
            row = row.push(
                widget::Row::new()
                    .push(swatch(series.color))
                    // Identity comes from the swatch beside the text, never
                    // from colouring the text itself.
                    .push(widget::text::caption(series.label.clone()))
                    .spacing(spacing.space_xxxs)
                    .align_y(cosmic::iced::Alignment::Center),
            );
        }
        row
    });

    let height = chart.height;
    let canvas = canvas::Canvas::new(chart)
        .width(Length::Fill)
        .height(Length::Fixed(height));

    let mut column = widget::Column::new()
        .push(canvas)
        .spacing(spacing.space_xxxs);

    if let Some(legend) = legend {
        column = column.push(legend);
    }
    if let Some(caption) = caption {
        column = column.push(widget::text::caption(caption));
    }

    column.into()
}

/// A small colour key for the legend.
fn swatch<'a, Message: 'a>(color: SeriesColor) -> Element<'a, Message> {
    widget::container(widget::Space::new().width(0).height(0))
        .width(Length::Fixed(10.0))
        .height(Length::Fixed(3.0))
        .class(cosmic::theme::Container::custom(move |theme| {
            cosmic::iced::widget::container::Style {
                background: Some(resolve(color, theme).into()),
                border: cosmic::iced::Border {
                    radius: 1.5.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }))
        .into()
}

/// Turn a series colour into the concrete colour for the active theme.
fn resolve(color: SeriesColor, theme: &Theme) -> Color {
    let cosmic = theme.cosmic();

    match color {
        SeriesColor::Accent => cosmic.accent_color().into(),
        SeriesColor::Slot(index) => {
            let slots = if theme.theme_type.is_dark() {
                SLOTS_DARK
            } else {
                SLOTS_LIGHT
            };
            slots[index % slots.len()]
        }
        SeriesColor::Muted => {
            let mut color: Color = cosmic.on_bg_color().into();
            color.a *= 0.30;
            color
        }
    }
}

impl<Message> canvas::Program<Message, Theme, Renderer> for Chart {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let cosmic = theme.cosmic();

        let surface: Color = cosmic.bg_color().into();
        // Recessive: one step off the surface, hairline, solid — never dashed.
        let mut grid_color: Color = cosmic.on_bg_color().into();
        grid_color.a *= 0.12;
        let mut label_color: Color = cosmic.on_bg_color().into();
        label_color.a *= 0.55;

        let plot = Rectangle {
            x: AXIS_LEFT,
            y: PLOT_TOP,
            width: (bounds.width - AXIS_LEFT - PLOT_RIGHT).max(1.0),
            height: (bounds.height - PLOT_TOP - AXIS_BOTTOM).max(1.0),
        };

        let ceiling = self.ceiling();

        // Gridlines at 0, half and full, with the value beside each. These carry
        // the numbers that are deliberately not printed on every point.
        for fraction in [0.0_f32, 0.5, 1.0] {
            let y = plot.y + plot.height * (1.0 - fraction);

            frame.stroke(
                &Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y)),
                Stroke::default()
                    .with_color(grid_color)
                    .with_width(GRID_WIDTH),
            );

            frame.fill_text(Text {
                content: self.scale.format(ceiling * f64::from(fraction)),
                position: Point::new(plot.x - 6.0, y),
                color: label_color,
                size: 10.0.into(),
                align_x: cosmic::iced::alignment::Horizontal::Right.into(),
                align_y: cosmic::iced::alignment::Vertical::Center,
                ..Text::default()
            });
        }

        for series in &self.series {
            if series.points.len() < 2 {
                continue;
            }

            let color = resolve(series.color, theme);
            let step = plot.width / (series.points.len() - 1) as f32;

            let point_at = |index: usize, value: f64| {
                let ratio = if ceiling > 0.0 {
                    (value / ceiling).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                Point::new(
                    plot.x + step * index as f32,
                    plot.y + plot.height * (1.0 - ratio as f32),
                )
            };

            // The area wash goes down first so the line sits on top of it.
            if series.filled {
                let area = Path::new(|builder| {
                    builder.move_to(Point::new(plot.x, plot.y + plot.height));
                    for (index, value) in series.points.iter().enumerate() {
                        builder.line_to(point_at(index, *value));
                    }
                    builder.line_to(Point::new(plot.x + plot.width, plot.y + plot.height));
                    builder.close();
                });

                frame.fill(
                    &area,
                    Color {
                        a: AREA_OPACITY,
                        ..color
                    },
                );
            }

            let line = Path::new(|builder| {
                for (index, value) in series.points.iter().enumerate() {
                    let point = point_at(index, *value);
                    if index == 0 {
                        builder.move_to(point);
                    } else {
                        builder.line_to(point);
                    }
                }
            });

            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(color)
                    .with_width(LINE_WIDTH)
                    .with_line_join(canvas::LineJoin::Round)
                    .with_line_cap(canvas::LineCap::Round),
            );

            // Mark the newest value. Context series stay unmarked — the point of
            // emphasis is that only one line is being pointed at.
            if series.color != SeriesColor::Muted {
                let last = series.points.len() - 1;
                let end = point_at(last, series.points[last]);

                // A ring in the surface colour keeps the marker legible where
                // lines cross, without drawing a border around the mark.
                frame.fill(
                    &Path::circle(end, END_MARKER_RADIUS + SURFACE_RING / 2.0),
                    surface,
                );
                frame.fill(&Path::circle(end, END_MARKER_RADIUS), color);
            }
        }

        vec![frame.into_geometry()]
    }
}
