//! Подготовка данных и генерация Plotly-графика.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::path::Path;

use chrono::{Datelike, NaiveDate};
use itertools::Itertools;
use plotly::box_plot::BoxPoints;
use plotly::color::{Rgb, Rgba};
use plotly::common::{Anchor, DashType, Fill, Font, Line, Mode, Orientation, Title, Visible};
use plotly::layout::{
    Annotation, Axis, GridPattern, ItemClick, Layout, LayoutGrid, Legend, Margin, RowOrder, Shape,
    ShapeLayer, ShapeLine, ShapeType, TicksDirection,
};
use plotly::{BoxPlot, Configuration, Plot, Scatter};

use crate::constants::{AREA_THOUSANDS_DIVISOR, DATE_FORMAT};
use crate::series::{AreaBuckets, build_occupied_series, load_area_buckets};

#[derive(Clone, Debug)]
pub struct ForecastOverlay {
    pub dates: Vec<String>,
    pub mean: Vec<f64>,
    pub lower: Vec<f64>,
    pub upper: Vec<f64>,
}

/// Сводные метрики для HTML-страницы (единицы указаны в комментариях).
#[derive(Clone, Debug)]
pub(super) struct ChartSummary {
    /// Дата последнего доступного среза (YYYY-MM-DD).
    pub latest_date: String,
    /// Текущая площадь в тыс. км².
    pub latest_area_km2: f64,
    /// Доля от площади Украины (в процентах).
    pub ukraine_percent: f64,
    /// Изменение за сутки в км² (может отсутствовать при коротком ряде).
    pub daily_change_km2: Option<f64>,
    /// Изменение за 7 дней в км² (может отсутствовать при коротком ряде).
    pub weekly_change_km2: Option<f64>,
    /// Сводка по прогнозу (если он передан).
    pub forecast: Option<ForecastSummary>,
}

/// Краткая сводка по прогнозу (в тыс. км²).
#[derive(Clone, Debug)]
pub(super) struct ForecastSummary {
    pub horizon_days: usize,
    pub end_date: String,
    pub mean_km2: f64,
    pub lower_km2: f64,
    pub upper_km2: f64,
}

pub(super) struct ChartOutput {
    pub main_plot: Plot,
    pub yoy_plot: Plot,
    pub summary: ChartSummary,
}

#[derive(Clone, Debug)]
struct PreparedChangeSeries {
    baseline: NaiveDate,
    dates: Vec<NaiveDate>,
    labels: Vec<String>,
    values: Vec<f64>,
}

#[derive(Clone, Debug)]
struct YoyYearSeries {
    year: i32,
    dates: Vec<String>,
    values: Vec<f64>,
}

#[derive(Clone, Debug)]
struct YoyEnvelope {
    dates: Vec<String>,
    lower: Vec<f64>,
    upper: Vec<f64>,
}

#[derive(Clone, Debug)]
struct YoyStdSeries {
    dates: Vec<String>,
    values: Vec<f64>,
}

#[derive(Clone, Debug, Default)]
struct MonthlyBoxSeries {
    dates: Vec<String>,
    values: Vec<f64>,
}

const MAX_PLOT_POINTS: usize = 900;
const UKRAINE_AREA_SQ_KM: f64 = 603_550.0;
const CHANGE_SMOOTH_WINDOW: usize = 5;
const CHANGE_SMOOTH_MIN_PERIODS: usize = 3;
const CHANGE_BASELINE: (i32, u32, u32) = (2022, 11, 23);
const AXIS_MAIN_X: &str = "x1";
const AXIS_MAIN_Y: &str = "y1";
const AXIS_CHANGE_X: &str = "x2";
const AXIS_CHANGE_Y: &str = "y2";
const AXIS_YOY_X: &str = "x";
const AXIS_YOY_Y: &str = "y";
const AXIS_YOY_BOX_X: &str = "x2";
const AXIS_YOY_BOX_Y: &str = "y2";
const AXIS_REF_X: &str = "x";
const AXIS_REF_Y: &str = "y";
const AXIS_REF_PAPER: &str = "paper";
const AXIS_REF_PIXEL: &str = "pixel";
const LABEL_ACTUAL: &str = "Факт";
const LABEL_FORECAST: &str = "Прогноз";
const LABEL_CONFIDENCE: &str = "95%";
const LABEL_AVG_CHANGE: &str = "Ср. изменение";
const LABEL_YOY_BAND: &str = "Исторический min/max";
const LABEL_YOY_STDDEV: &str = "σ";
const LABEL_BOX_DAILY_CHANGE: &str = "Суточное сглаженное изменение";
const LABEL_UKRAINE: &str = "Украины";
const UNIT_THOUSAND_KM2: &str = "тыс. км²";
const UNIT_KM2_PER_DAY: &str = "км²/сутки";
const HOVER_FORMAT_KM2_PER_DAY: &str = ".1f";
const HOVER_FORMAT_DAY_MONTH: &str = "%d.%m";
const FONT_FAMILY: &str = "PT Sans, Arial, sans-serif";
const TICK_FORMAT_MONTH_YEAR: &str = "%b\n%Y";
const DATE_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
const FONT_SIZE_BASE: usize = 12;
const FONT_SIZE_AXIS_TITLE: usize = 13;
const FONT_SIZE_AXIS_TICK: usize = 11;
const FONT_SIZE_ANNOTATION: usize = 11;
const LINE_WIDTH_MAIN: f64 = 2.6;
const LINE_WIDTH_FORECAST: f64 = 2.2;
const LINE_WIDTH_CHANGE: f64 = 1.6;
const LINE_WIDTH_YOY: f64 = 1.8;
const LINE_WIDTH_MARKER: f64 = 1.0;
const ARROW_HEAD: u8 = 2;
const ARROW_SIZE: f64 = 0.9;
const ARROW_WIDTH: f64 = 1.0;
const ANNOTATION_OFFSET_X: f64 = 20.0;
const ANNOTATION_OFFSET_Y: f64 = -34.0;
const FORECAST_OFFSET_X: f64 = -20.0;
const FORECAST_OFFSET_Y: f64 = -34.0;
const CHANGE_OFFSET_X: f64 = 12.0;
const UKRAINE_LABEL_X: f64 = 0.99;
const UKRAINE_LABEL_Y: f64 = 0.99;
const LEGEND_X: f64 = 0.5;
const LEGEND_Y: f64 = 1.02;
const LEGEND_FONT_SIZE: usize = 12;
const LEGEND_BORDER_WIDTH: usize = 1;
const MARGIN_LEFT: usize = 80;
const MARGIN_RIGHT: usize = 40;
const MARGIN_TOP: usize = 70;
const MARGIN_BOTTOM: usize = 60;
const MARGIN_PAD: usize = 8;
const TICK_LENGTH: usize = 6;
const AXIS_GRID_WIDTH: usize = 1;
const GRID_ROWS: usize = 2;
const GRID_COLS: usize = 1;
const GRID_Y_GAP: f64 = 0.08;
const X_MAIN_TICKS_COUNT: usize = 14;
const X2_TICKS_COUNT: usize = 14;
const Y_MAIN_TICKS_COUNT: usize = 11;
const Y_CHANGE_TICKS_COUNT: usize = 11;
const X_YOY_TICKS_COUNT: usize = 16;
const Y_YOY_TICKS_COUNT: usize = 12;
const Y_YOY_MONTH_TICKS_COUNT: usize = 10;
const YOY_ANCHOR_YEAR: i32 = 2000;
// Кастомные якоря для box-plot по месяцам (день, час, минута) в anchor-year.
// Значения соответствуют спецификации пользователя: 15.5 -> 15 12:00, 14.125 -> 14 03:00.
const YOY_BOX_MONTH_ANCHORS: [(u32, u32, u32); 12] = [
    (15, 12, 0), // Jan: 15.5
    (14, 3, 0),  // Feb: 14.125
    (15, 12, 0), // Mar: 15.5
    (15, 0, 0),  // Apr: 15
    (15, 12, 0), // May: 15.5
    (15, 0, 0),  // Jun: 15
    (15, 12, 0), // Jul: 15.5
    (15, 12, 0), // Aug: 15.5
    (15, 0, 0),  // Sep: 15
    (15, 12, 0), // Oct: 15.5
    (15, 0, 0),  // Nov: 15
    (15, 12, 0), // Dec: 15.5
];
const COLOR_AREA: (u8, u8, u8) = (36, 100, 166);
const COLOR_AREA_TRANSPARENT: (u8, u8, u8, f64) = (36, 100, 166, 0.0);
const COLOR_AREA_BAND: (u8, u8, u8, f64) = (36, 100, 166, 0.2);
const COLOR_CHANGE_FILL: (u8, u8, u8, f64) = (220, 82, 60, 0.25);
const COLOR_CHANGE_LINE: (u8, u8, u8) = (200, 67, 46);
const COLOR_YOY_LINE: (u8, u8, u8) = (36, 100, 166);
const COLOR_YOY_STDDEV: (u8, u8, u8) = (200, 67, 46);
const COLOR_YOY_STDDEV_FILL: (u8, u8, u8, f64) = (200, 67, 46, 0.22);
const YOY_LINE_ALPHA_MIN: f64 = 0.25;
const YOY_LINE_ALPHA_MAX: f64 = 1.0;
const COLOR_YOY_BAND: (u8, u8, u8, f64) = (120, 120, 120, 0.22);
const COLOR_YOY_BAND_LINE: (u8, u8, u8, f64) = (120, 120, 120, 0.0);
const COLOR_ARROW: (u8, u8, u8, f64) = (80, 80, 80, 0.6);
const COLOR_MARKER_LINE: (u8, u8, u8, f64) = (80, 80, 80, 0.35);
const COLOR_TEXT_BASE: (u8, u8, u8) = (40, 40, 40);
const COLOR_TEXT_ANNOTATION: (u8, u8, u8) = (32, 32, 32);
const COLOR_PANEL_BG: (u8, u8, u8, f64) = (255, 255, 255, 0.85);
const COLOR_PANEL_BORDER: (u8, u8, u8, f64) = (200, 200, 200, 0.75);
const COLOR_LABEL_BG: (u8, u8, u8, f64) = (255, 255, 255, 0.8);
const COLOR_LABEL_BORDER: (u8, u8, u8, f64) = (200, 200, 200, 0.7);
const COLOR_AXIS_TICK: (u8, u8, u8, f64) = (0, 0, 0, 0.45);
const COLOR_AXIS_LINE: (u8, u8, u8, f64) = (0, 0, 0, 0.35);
const COLOR_AXIS_GRID_LIGHT: (u8, u8, u8, f64) = (0, 0, 0, 0.06);
const COLOR_AXIS_GRID_MEDIUM: (u8, u8, u8, f64) = (0, 0, 0, 0.08);
const COLOR_LEGEND_BG: (u8, u8, u8, f64) = (255, 255, 255, 0.75);
const COLOR_LEGEND_BORDER: (u8, u8, u8, f64) = (210, 210, 210, 0.8);

fn rgb(color: (u8, u8, u8)) -> Rgb {
    Rgb::new(color.0, color.1, color.2)
}

fn rgba(color: (u8, u8, u8, f64)) -> Rgba {
    Rgba::new(color.0, color.1, color.2, color.3)
}

fn usize_to_f64(value: usize, context: &str) -> f64 {
    u32::try_from(value).map_or_else(
        |_| {
            tracing::warn!(
                value,
                context,
                "usize value exceeds u32::MAX; clamping conversion to f64",
            );
            f64::from(u32::MAX)
        },
        f64::from,
    )
}

/// Строит Plotly-график (и возвращает последний уровень площади) без прогноза.
pub(super) fn build_area_chart(
    csv_path: &Path,
    forecast: Option<&ForecastOverlay>,
) -> Result<ChartOutput, Box<dyn Error>> {
    let buckets = load_area_buckets(csv_path)?;
    build_area_chart_from_buckets(&buckets, forecast)
}

#[allow(clippy::too_many_lines)]
pub(super) fn build_area_chart_from_buckets(
    buckets: &AreaBuckets,
    forecast: Option<&ForecastOverlay>,
) -> Result<ChartOutput, Box<dyn Error>> {
    let (dates, occupied_area) = build_occupied_series(buckets)?;
    let area_dates = dates
        .iter()
        .map(|date| date.format(DATE_FORMAT).to_string())
        .collect_vec();
    let area_km2 = occupied_area
        .iter()
        .map(|value| value / AREA_THOUSANDS_DIVISOR)
        .collect_vec();
    let latest_area_km2 = area_km2.last().copied().unwrap_or_default();
    let latest_area_sq_km = latest_area_km2 * AREA_THOUSANDS_DIVISOR;
    let ukraine_percent = if UKRAINE_AREA_SQ_KM > 0.0 {
        latest_area_sq_km / UKRAINE_AREA_SQ_KM * 100.0
    } else {
        0.0
    };
    let latest_date = dates
        .last()
        .copied()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no dates available"))?;
    let latest_date_label = latest_date.format(DATE_FORMAT).to_string();
    let daily_change_km2 = if occupied_area.len() >= 2 {
        Some(occupied_area[occupied_area.len() - 1] - occupied_area[occupied_area.len() - 2])
    } else {
        None
    };
    let weekly_change_km2 = if occupied_area.len() >= 8 {
        Some(occupied_area[occupied_area.len() - 1] - occupied_area[occupied_area.len() - 8])
    } else {
        None
    };

    let change_series = prepare_change_series(&dates, &occupied_area)?;

    let (area_dates_plot, area_km2_plot) =
        downsample_min_max(&area_dates, &area_km2, MAX_PLOT_POINTS);
    let (change_dates_plot, change_values_plot) = downsample_min_max(
        &change_series.labels,
        &change_series.values,
        MAX_PLOT_POINTS / 2,
    );

    let mut plot = Plot::new();
    plot.add_trace(
        Scatter::new(area_dates_plot, area_km2_plot)
            .mode(Mode::Lines)
            .line(
                Line::new()
                    .color(rgb(COLOR_AREA))
                    .width(LINE_WIDTH_MAIN)
                    .simplify(true),
            )
            .name(LABEL_ACTUAL)
            .x_axis(AXIS_MAIN_X)
            .y_axis(AXIS_MAIN_Y),
    );

    let forecast_ref = forecast;
    if let Some(forecast) = forecast_ref
        && !forecast.dates.is_empty()
    {
        plot.add_trace(
            Scatter::new(forecast.dates.clone(), forecast.lower.clone())
                .mode(Mode::Lines)
                .line(Line::new().color(rgba(COLOR_AREA_TRANSPARENT)))
                .show_legend(false)
                .x_axis(AXIS_MAIN_X)
                .y_axis(AXIS_MAIN_Y),
        );
        plot.add_trace(
            Scatter::new(forecast.dates.clone(), forecast.upper.clone())
                .mode(Mode::Lines)
                .fill(Fill::ToNextY)
                .fill_color(rgba(COLOR_AREA_BAND))
                .line(Line::new().color(rgba(COLOR_AREA_TRANSPARENT)))
                .show_legend(false)
                .name(LABEL_CONFIDENCE)
                .x_axis(AXIS_MAIN_X)
                .y_axis(AXIS_MAIN_Y),
        );
        plot.add_trace(
            Scatter::new(forecast.dates.clone(), forecast.mean.clone())
                .mode(Mode::Lines)
                .line(
                    Line::new()
                        .color(rgb(COLOR_AREA))
                        .width(LINE_WIDTH_FORECAST)
                        .dash(DashType::Dash),
                )
                .name(LABEL_FORECAST)
                .x_axis(AXIS_MAIN_X)
                .y_axis(AXIS_MAIN_Y),
        );
    }

    plot.add_trace(
        Scatter::new(change_dates_plot, change_values_plot)
            .mode(Mode::Lines)
            .fill(Fill::ToZeroY)
            .fill_color(rgba(COLOR_CHANGE_FILL))
            .line(
                Line::new()
                    .color(rgb(COLOR_CHANGE_LINE))
                    .width(LINE_WIDTH_CHANGE)
                    .simplify(true),
            )
            .name(LABEL_AVG_CHANGE)
            .x_axis(AXIS_CHANGE_X)
            .y_axis(AXIS_CHANGE_Y),
    );

    let mut annotations = Vec::new();
    if let (Some(last_date), Some(last_value)) = (area_dates.last(), area_km2.last()) {
        annotations.push(
            Annotation::new()
                .text(format!("{last_value:.1} {UNIT_THOUSAND_KM2}"))
                .x(last_date.clone())
                .y(*last_value)
                .x_ref(AXIS_REF_X)
                .y_ref(AXIS_REF_Y)
                .x_anchor(Anchor::Left)
                .y_anchor(Anchor::Bottom)
                .ax(ANNOTATION_OFFSET_X)
                .ay(ANNOTATION_OFFSET_Y)
                .ax_ref(AXIS_REF_PIXEL)
                .ay_ref(AXIS_REF_PIXEL)
                .show_arrow(true)
                .arrow_head(ARROW_HEAD)
                .arrow_size(ARROW_SIZE)
                .arrow_width(ARROW_WIDTH)
                .arrow_color(rgba(COLOR_ARROW))
                .font(
                    Font::new()
                        .size(FONT_SIZE_ANNOTATION)
                        .color(rgb(COLOR_TEXT_ANNOTATION)),
                )
                .background_color(rgba(COLOR_PANEL_BG))
                .border_color(rgba(COLOR_PANEL_BORDER))
                .border_width(LINE_WIDTH_MARKER),
        );
    }
    if let Some(forecast) = forecast_ref
        && let (Some(last_date), Some(last_mean)) = (forecast.dates.last(), forecast.mean.last())
    {
        annotations.push(
            Annotation::new()
                .text(format!("{last_mean:.1} {UNIT_THOUSAND_KM2}"))
                .x(last_date.clone())
                .y(*last_mean)
                .x_ref(AXIS_REF_X)
                .y_ref(AXIS_REF_Y)
                .x_anchor(Anchor::Right)
                .y_anchor(Anchor::Bottom)
                .ax(FORECAST_OFFSET_X)
                .ay(FORECAST_OFFSET_Y)
                .ax_ref(AXIS_REF_PIXEL)
                .ay_ref(AXIS_REF_PIXEL)
                .show_arrow(true)
                .arrow_head(ARROW_HEAD)
                .arrow_size(ARROW_SIZE)
                .arrow_width(ARROW_WIDTH)
                .arrow_color(rgba(COLOR_ARROW))
                .font(
                    Font::new()
                        .size(FONT_SIZE_ANNOTATION)
                        .color(rgb(COLOR_TEXT_ANNOTATION)),
                )
                .background_color(rgba(COLOR_PANEL_BG))
                .border_color(rgba(COLOR_PANEL_BORDER))
                .border_width(LINE_WIDTH_MARKER),
        );
    }
    annotations.push(
        Annotation::new()
            .text(format!("{ukraine_percent:.2}% {LABEL_UKRAINE}"))
            .x(UKRAINE_LABEL_X)
            .y(UKRAINE_LABEL_Y)
            .x_ref(AXIS_REF_PAPER)
            .y_ref(AXIS_REF_PAPER)
            .x_anchor(Anchor::Right)
            .y_anchor(Anchor::Top)
            .show_arrow(false)
            .font(Font::new().size(FONT_SIZE_BASE).color(rgb(COLOR_TEXT_BASE)))
            .background_color(rgba(COLOR_LABEL_BG))
            .border_color(rgba(COLOR_LABEL_BORDER))
            .border_width(LINE_WIDTH_MARKER),
    );
    if let (Some(last_date), Some(last_value)) =
        (change_series.labels.last(), change_series.values.last())
    {
        annotations.push(
            Annotation::new()
                .text(format!("◀ {last_value:+.1} {UNIT_KM2_PER_DAY}"))
                .x(last_date.clone())
                .y(*last_value)
                .x_ref(AXIS_CHANGE_X)
                .y_ref(AXIS_CHANGE_Y)
                .x_anchor(Anchor::Left)
                .y_anchor(Anchor::Middle)
                .x_shift(CHANGE_OFFSET_X)
                .show_arrow(false)
                .font(
                    Font::new()
                        .size(FONT_SIZE_ANNOTATION)
                        .color(rgb(COLOR_TEXT_ANNOTATION)),
                )
                .background_color(rgba(COLOR_PANEL_BG))
                .border_color(rgba(COLOR_PANEL_BORDER))
                .border_width(LINE_WIDTH_MARKER),
        );
    }

    let reference_dates = [change_series.baseline];
    let marker_shapes: Vec<Shape> = reference_dates
        .iter()
        .map(|date| {
            let date_str = date.format(DATE_FORMAT).to_string();
            Shape::new()
                .shape_type(ShapeType::Line)
                .layer(ShapeLayer::Below)
                .x_ref(AXIS_REF_X)
                .y_ref(AXIS_REF_PAPER)
                .x0(date_str.clone())
                .x1(date_str)
                .y0(0)
                .y1(1)
                .line(
                    ShapeLine::new()
                        .color(rgba(COLOR_MARKER_LINE))
                        .width(LINE_WIDTH_MARKER)
                        .dash(DashType::Dash),
                )
        })
        .collect();

    let layout = Layout::new()
        .font(
            Font::new()
                .family(FONT_FAMILY)
                .size(FONT_SIZE_BASE)
                .color(rgb(COLOR_TEXT_BASE)),
        )
        .auto_size(true)
        .margin(
            Margin::new()
                .left(MARGIN_LEFT)
                .right(MARGIN_RIGHT)
                .top(MARGIN_TOP)
                .bottom(MARGIN_BOTTOM)
                .pad(MARGIN_PAD),
        )
        .grid(
            LayoutGrid::new()
                .rows(GRID_ROWS)
                .columns(GRID_COLS)
                .pattern(GridPattern::Independent)
                .y_gap(GRID_Y_GAP)
                .row_order(RowOrder::TopToBottom),
        )
        .show_legend(true)
        .legend(
            Legend::new()
                .orientation(Orientation::Horizontal)
                .item_click(ItemClick::False)
                .item_double_click(ItemClick::False)
                .x(LEGEND_X)
                .x_anchor(Anchor::Center)
                .y(LEGEND_Y)
                .y_anchor(Anchor::Bottom)
                .font(Font::new().size(LEGEND_FONT_SIZE))
                .background_color(rgba(COLOR_LEGEND_BG))
                .border_color(rgba(COLOR_LEGEND_BORDER))
                .border_width(LEGEND_BORDER_WIDTH),
        )
        .annotations(annotations)
        .shapes(marker_shapes)
        .x_axis(
            Axis::new()
                .title(Title::new())
                .show_tick_labels(false)
                .n_ticks(X_MAIN_TICKS_COUNT)
                .ticks(TicksDirection::Outside)
                .tick_length(TICK_LENGTH)
                .tick_color(rgba(COLOR_AXIS_TICK))
                .show_line(true)
                .line_color(rgba(COLOR_AXIS_LINE))
                .grid_color(rgba(COLOR_AXIS_GRID_LIGHT))
                .grid_width(AXIS_GRID_WIDTH)
                .auto_margin(true),
        )
        .y_axis(
            Axis::new()
                .title(
                    Title::with_text(UNIT_THOUSAND_KM2)
                        .font(Font::new().size(FONT_SIZE_AXIS_TITLE)),
                )
                .n_ticks(Y_MAIN_TICKS_COUNT)
                .tick_font(Font::new().size(FONT_SIZE_AXIS_TICK))
                .ticks(TicksDirection::Outside)
                .tick_length(TICK_LENGTH)
                .tick_color(rgba(COLOR_AXIS_TICK))
                .separate_thousands(true)
                .show_line(true)
                .line_color(rgba(COLOR_AXIS_LINE))
                .grid_color(rgba(COLOR_AXIS_GRID_MEDIUM))
                .grid_width(AXIS_GRID_WIDTH)
                .auto_margin(true),
        )
        .x_axis2(
            Axis::new()
                .matches("x")
                .tick_format(TICK_FORMAT_MONTH_YEAR)
                .n_ticks(X2_TICKS_COUNT)
                .tick_font(Font::new().size(FONT_SIZE_AXIS_TICK))
                .ticks(TicksDirection::Outside)
                .tick_length(TICK_LENGTH)
                .tick_color(rgba(COLOR_AXIS_TICK))
                .show_line(true)
                .line_color(rgba(COLOR_AXIS_LINE))
                .grid_color(rgba(COLOR_AXIS_GRID_LIGHT))
                .grid_width(AXIS_GRID_WIDTH)
                .auto_margin(true),
        )
        .y_axis2(
            Axis::new()
                .title(
                    Title::with_text(UNIT_KM2_PER_DAY).font(Font::new().size(FONT_SIZE_AXIS_TITLE)),
                )
                .hover_format(HOVER_FORMAT_KM2_PER_DAY)
                .n_ticks(Y_CHANGE_TICKS_COUNT)
                .tick_font(Font::new().size(FONT_SIZE_AXIS_TICK))
                .ticks(TicksDirection::Outside)
                .tick_length(TICK_LENGTH)
                .tick_color(rgba(COLOR_AXIS_TICK))
                .separate_thousands(true)
                .show_line(true)
                .line_color(rgba(COLOR_AXIS_LINE))
                .grid_color(rgba(COLOR_AXIS_GRID_MEDIUM))
                .grid_width(AXIS_GRID_WIDTH)
                .auto_margin(true),
        );

    plot.set_layout(layout);
    plot.set_configuration(Configuration::new().responsive(true));
    let yoy_plot = build_yoy_chart(&change_series.dates, &change_series.values);

    let forecast_summary = forecast_ref.and_then(|forecast| {
        if forecast.dates.is_empty() {
            return None;
        }
        let end_date = forecast.dates.last().cloned().unwrap_or_default();
        let mean_km2 = *forecast.mean.last().unwrap_or(&0.0);
        let lower_km2 = *forecast.lower.last().unwrap_or(&0.0);
        let upper_km2 = *forecast.upper.last().unwrap_or(&0.0);
        Some(ForecastSummary {
            horizon_days: forecast.dates.len(),
            end_date,
            mean_km2,
            lower_km2,
            upper_km2,
        })
    });

    Ok(ChartOutput {
        main_plot: plot,
        yoy_plot,
        summary: ChartSummary {
            latest_date: latest_date_label,
            latest_area_km2,
            ukraine_percent,
            daily_change_km2,
            weekly_change_km2,
            forecast: forecast_summary,
        },
    })
}

fn prepare_change_series(
    dates: &[NaiveDate],
    occupied_area: &[f64],
) -> Result<PreparedChangeSeries, io::Error> {
    let daily_changes = daily_change_series(occupied_area);
    let smoothed_daily = centered_moving_average(
        &daily_changes,
        CHANGE_SMOOTH_WINDOW,
        CHANGE_SMOOTH_MIN_PERIODS,
    );
    // Показываем суточные изменения только после базовой даты.
    let baseline = NaiveDate::from_ymd_opt(CHANGE_BASELINE.0, CHANGE_BASELINE.1, CHANGE_BASELINE.2)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid change baseline date")
        })?;

    let (filtered_dates, values) = dates
        .iter()
        .zip(smoothed_daily)
        .filter_map(|(date, value)| value.map(|v| (*date, v)))
        .filter(|(date, _)| *date >= baseline)
        .unzip::<_, _, Vec<_>, Vec<_>>();
    let labels = filtered_dates
        .iter()
        .map(|date| date.format(DATE_FORMAT).to_string())
        .collect();

    Ok(PreparedChangeSeries {
        baseline,
        dates: filtered_dates,
        labels,
        values,
    })
}

fn build_yoy_chart(change_dates: &[NaiveDate], change_values: &[f64]) -> Plot {
    let mut plot = Plot::new();

    if let Some(envelope) = build_yoy_envelope(change_dates, change_values) {
        plot.add_trace(
            Scatter::new(envelope.dates.clone(), envelope.lower)
                .mode(Mode::Lines)
                .line(Line::new().color(rgba(COLOR_YOY_BAND_LINE)))
                .show_legend(false)
                .x_axis(AXIS_YOY_X)
                .y_axis(AXIS_YOY_Y),
        );
        plot.add_trace(
            Scatter::new(envelope.dates, envelope.upper)
                .mode(Mode::Lines)
                .fill(Fill::ToNextY)
                .fill_color(rgba(COLOR_YOY_BAND))
                .line(Line::new().color(rgba(COLOR_YOY_BAND_LINE)))
                .name(LABEL_YOY_BAND)
                .x_axis(AXIS_YOY_X)
                .y_axis(AXIS_YOY_Y),
        );
    }

    if let Some(std_series) = build_yoy_stddev_series(change_dates, change_values) {
        plot.add_trace(
            Scatter::new(std_series.dates, std_series.values)
                .mode(Mode::Lines)
                .hover_template("Дата: %{x|%d.%m}<br>СКО: %{y:.1f} км²/сутки<extra></extra>")
                .fill(Fill::ToZeroY)
                .fill_color(rgba(COLOR_YOY_STDDEV_FILL))
                .line(
                    Line::new()
                        .width(LINE_WIDTH_YOY)
                        .dash(DashType::Solid)
                        .color(rgb(COLOR_YOY_STDDEV)),
                )
                .name(LABEL_YOY_STDDEV)
                .visible(Visible::LegendOnly)
                .x_axis(AXIS_YOY_X)
                .y_axis(AXIS_YOY_Y),
        );
    }

    let year_series = build_yoy_series_by_year(change_dates, change_values);
    let year_count = year_series.len();
    for (idx, year_series) in year_series.into_iter().enumerate() {
        let alpha = yoy_line_alpha(idx, year_count);
        plot.add_trace(
            Scatter::new(year_series.dates, year_series.values)
                .mode(Mode::Lines)
                .hover_template(
                    "Дата: %{x|%d.%m}.%{fullData.name}<br>Значение: %{y:.1f} км²/сутки<extra></extra>",
                )
                .line(
                    Line::new()
                        .width(LINE_WIDTH_YOY)
                        .simplify(true)
                        .color(rgba((
                            COLOR_YOY_LINE.0,
                            COLOR_YOY_LINE.1,
                            COLOR_YOY_LINE.2,
                            alpha,
                        ))),
                )
                .name(year_series.year.to_string())
                .x_axis(AXIS_YOY_X)
                .y_axis(AXIS_YOY_Y),
        );
    }

    let monthly_box_series = build_monthly_box_series(change_dates, change_values);
    if !monthly_box_series.values.is_empty() {
        plot.add_trace(
            BoxPlot::new_xy(monthly_box_series.dates, monthly_box_series.values)
                .name(LABEL_BOX_DAILY_CHANGE)
                .show_legend(false)
                .box_points(BoxPoints::False)
                .fill_color(rgba(COLOR_CHANGE_FILL))
                .line(
                    Line::new()
                        .color(rgb(COLOR_CHANGE_LINE))
                        .width(LINE_WIDTH_CHANGE),
                )
                .x_axis(AXIS_YOY_BOX_X)
                .y_axis(AXIS_YOY_BOX_Y),
        );
    }

    plot.set_layout(build_yoy_layout());
    plot.set_configuration(Configuration::new().responsive(true));
    plot
}

fn build_yoy_layout() -> Layout {
    Layout::new()
        .font(
            Font::new()
                .family(FONT_FAMILY)
                .size(FONT_SIZE_BASE)
                .color(rgb(COLOR_TEXT_BASE)),
        )
        .auto_size(true)
        .margin(
            Margin::new()
                .left(MARGIN_LEFT)
                .right(MARGIN_RIGHT)
                .top(MARGIN_TOP)
                .bottom(MARGIN_BOTTOM)
                .pad(MARGIN_PAD),
        )
        .grid(
            LayoutGrid::new()
                .rows(2)
                .columns(1)
                .pattern(GridPattern::Independent)
                .y_gap(GRID_Y_GAP)
                .row_order(RowOrder::TopToBottom),
        )
        .show_legend(true)
        .legend(build_yoy_legend())
        .x_axis(build_yoy_primary_x_axis())
        .y_axis(build_yoy_primary_y_axis())
        .x_axis2(build_yoy_secondary_x_axis())
        .y_axis2(build_yoy_secondary_y_axis())
}

fn build_yoy_legend() -> Legend {
    Legend::new()
        .orientation(Orientation::Horizontal)
        .x(LEGEND_X)
        .x_anchor(Anchor::Center)
        .y(LEGEND_Y)
        .y_anchor(Anchor::Bottom)
        .font(Font::new().size(FONT_SIZE_AXIS_TICK))
        .background_color(rgba(COLOR_LEGEND_BG))
        .border_color(rgba(COLOR_LEGEND_BORDER))
        .border_width(LEGEND_BORDER_WIDTH)
}

fn build_yoy_axis_x_base() -> Axis {
    Axis::new()
        .tick_format("%b")
        .n_ticks(X_YOY_TICKS_COUNT)
        .tick_font(Font::new().size(FONT_SIZE_AXIS_TICK))
        .ticks(TicksDirection::Outside)
        .tick_length(TICK_LENGTH)
        .tick_color(rgba(COLOR_AXIS_TICK))
        .show_line(true)
        .line_color(rgba(COLOR_AXIS_LINE))
        .grid_color(rgba(COLOR_AXIS_GRID_LIGHT))
        .grid_width(AXIS_GRID_WIDTH)
        .auto_margin(true)
}

fn build_yoy_primary_x_axis() -> Axis {
    build_yoy_axis_x_base()
        .title(Title::new())
        .show_tick_labels(false)
        .hover_format(HOVER_FORMAT_DAY_MONTH)
}

fn build_yoy_secondary_x_axis() -> Axis {
    build_yoy_axis_x_base().matches(AXIS_YOY_X)
}

fn build_yoy_axis_y_base() -> Axis {
    Axis::new()
        .title(Title::with_text(UNIT_KM2_PER_DAY).font(Font::new().size(FONT_SIZE_AXIS_TITLE)))
        .hover_format(HOVER_FORMAT_KM2_PER_DAY)
        .tick_font(Font::new().size(FONT_SIZE_AXIS_TICK))
        .ticks(TicksDirection::Outside)
        .tick_length(TICK_LENGTH)
        .tick_color(rgba(COLOR_AXIS_TICK))
        .separate_thousands(true)
        .show_line(true)
        .line_color(rgba(COLOR_AXIS_LINE))
        .grid_color(rgba(COLOR_AXIS_GRID_MEDIUM))
        .grid_width(AXIS_GRID_WIDTH)
        .auto_margin(true)
}

fn build_yoy_primary_y_axis() -> Axis {
    build_yoy_axis_y_base().n_ticks(Y_YOY_TICKS_COUNT)
}

fn build_yoy_secondary_y_axis() -> Axis {
    build_yoy_axis_y_base().n_ticks(Y_YOY_MONTH_TICKS_COUNT)
}

fn build_monthly_box_series(change_dates: &[NaiveDate], change_values: &[f64]) -> MonthlyBoxSeries {
    if change_dates.is_empty() || change_dates.len() != change_values.len() {
        return MonthlyBoxSeries::default();
    }

    let (dates, values) = change_dates
        .iter()
        .zip(change_values.iter().copied())
        .map(|(date, value)| (normalize_to_yoy_month(date.month()), value))
        .unzip();

    MonthlyBoxSeries { dates, values }
}

fn normalize_to_yoy_month(month: u32) -> String {
    let month_index = month
        .checked_sub(1)
        .and_then(|value| usize::try_from(value).ok())
        .expect("month must fit into usize");
    let (day, hour, minute) = *YOY_BOX_MONTH_ANCHORS
        .get(month_index)
        .expect("month must be in [1..=12]");
    let date_time = NaiveDate::from_ymd_opt(YOY_ANCHOR_YEAR, month, day)
        .expect("YOY anchor year must support configured month/day values")
        .and_hms_opt(hour, minute, 0)
        .expect("configured box-plot month time must be representable");
    date_time.format(DATE_TIME_FORMAT).to_string()
}

fn build_yoy_series_by_year(
    change_dates: &[NaiveDate],
    change_values: &[f64],
) -> Vec<YoyYearSeries> {
    let mut grouped: BTreeMap<i32, Vec<(String, f64)>> = BTreeMap::new();

    for (date, value) in change_dates.iter().zip(change_values.iter()) {
        grouped
            .entry(date.year())
            .or_default()
            .push((normalize_to_yoy_day(*date), *value));
    }

    grouped
        .into_iter()
        .map(|(year, points)| {
            let (dates, values) = points.into_iter().unzip::<_, _, Vec<_>, Vec<_>>();
            YoyYearSeries {
                year,
                dates,
                values,
            }
        })
        .collect()
}

fn build_yoy_envelope(change_dates: &[NaiveDate], change_values: &[f64]) -> Option<YoyEnvelope> {
    let mut envelope = BTreeMap::<NaiveDate, (f64, f64)>::new();

    for (date, value) in change_dates.iter().zip(change_values.iter()) {
        let normalized = normalized_yoy_date(*date);
        envelope
            .entry(normalized)
            .and_modify(|(lower, upper)| {
                *lower = lower.min(*value);
                *upper = upper.max(*value);
            })
            .or_insert((*value, *value));
    }

    if envelope.is_empty() {
        return None;
    }

    let mut dates = Vec::with_capacity(envelope.len());
    let mut lower = Vec::with_capacity(envelope.len());
    let mut upper = Vec::with_capacity(envelope.len());
    for (date, (min_value, max_value)) in envelope {
        dates.push(date.format(DATE_FORMAT).to_string());
        lower.push(min_value);
        upper.push(max_value);
    }

    Some(YoyEnvelope {
        dates,
        lower,
        upper,
    })
}

fn build_yoy_stddev_series(
    change_dates: &[NaiveDate],
    change_values: &[f64],
) -> Option<YoyStdSeries> {
    if change_dates.is_empty() || change_dates.len() != change_values.len() {
        return None;
    }

    let mut grouped = BTreeMap::<NaiveDate, Vec<f64>>::new();
    for (date, value) in change_dates.iter().zip(change_values.iter().copied()) {
        grouped
            .entry(normalized_yoy_date(*date))
            .or_default()
            .push(value);
    }

    let mut dates = Vec::new();
    let mut values = Vec::new();
    for (date, day_values) in grouped {
        if day_values.len() < 2 {
            continue;
        }
        dates.push(date.format(DATE_FORMAT).to_string());
        values.push(stddev(&day_values));
    }

    if dates.is_empty() {
        None
    } else {
        Some(YoyStdSeries { dates, values })
    }
}

fn stddev(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / usize_to_f64(values.len(), "stddev");
    let variance = values
        .iter()
        .map(|value| {
            let delta = *value - mean;
            delta * delta
        })
        .sum::<f64>()
        / usize_to_f64(values.len(), "stddev");
    variance.sqrt()
}

fn normalize_to_yoy_day(date: NaiveDate) -> String {
    normalized_yoy_date(date).format(DATE_FORMAT).to_string()
}

fn normalized_yoy_date(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(YOY_ANCHOR_YEAR, date.month(), date.day())
        .expect("YOY anchor year must support all month/day combinations")
}

fn yoy_line_alpha(index: usize, total: usize) -> f64 {
    if total <= 1 {
        return YOY_LINE_ALPHA_MAX;
    }
    let position = usize_to_f64(index, "yoy_line_alpha");
    let span = usize_to_f64(total - 1, "yoy_line_alpha");
    (YOY_LINE_ALPHA_MAX - YOY_LINE_ALPHA_MIN).mul_add(position / span, YOY_LINE_ALPHA_MIN)
}

/// Находит последний локальный максимум за год для потенциальной пометки на графике.
#[allow(dead_code)]
fn peak_annotation(values: &[f64]) -> Option<(usize, f64)> {
    const OFFSET: usize = 60;
    if values.len() <= OFFSET {
        return None;
    }
    let start = values.len().saturating_sub(365 + OFFSET);
    let end = values.len().saturating_sub(OFFSET);

    values
        .iter()
        .enumerate()
        .take(end)
        .skip(start)
        .max_by(|(_, left), (_, right)| {
            left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(idx, value)| (idx, *value))
}

/// Считает посуточные изменения, сохраняя длину ряда (первое значение = 0).
fn daily_change_series(values: &[f64]) -> Vec<f64> {
    if values.is_empty() {
        return Vec::new();
    }

    std::iter::once(0.0)
        .chain(
            values
                .iter()
                .tuple_windows()
                .map(|(prev, curr)| curr - prev),
        )
        .collect()
}

/// Центрированное скользящее среднее; возвращает `None`, если окно недозаполнено.
fn centered_moving_average(values: &[f64], window: usize, min_periods: usize) -> Vec<Option<f64>> {
    if window == 0 || values.is_empty() {
        return vec![None; values.len()];
    }
    let radius = window / 2;

    values
        .iter()
        .enumerate()
        .map(|(idx, _)| {
            let start = idx.saturating_sub(radius);
            let end = (idx + radius).min(values.len() - 1);
            let count = end - start + 1;
            if count < min_periods {
                None
            } else {
                let sum: f64 = values[start..=end].iter().copied().sum();
                Some(sum / usize_to_f64(count, "centered_moving_average"))
            }
        })
        .collect()
}

/// Даунсемплит ряд, сохраняя минимум/максимум в бакетах, чтобы ускорить отрисовку.
fn downsample_min_max<X: Clone>(x: &[X], y: &[f64], max_points: usize) -> (Vec<X>, Vec<f64>) {
    if x.len() <= max_points || x.len() != y.len() || max_points < 3 {
        return (x.to_vec(), y.to_vec());
    }

    let len = y.len();
    let mut indices = Vec::with_capacity(max_points);
    indices.push(0);

    let interior_len = len.saturating_sub(2);
    let max_pairs = max_points.saturating_sub(2);
    let bucket_count = (max_pairs / 2).max(1);
    let bucket_size = interior_len.div_ceil(bucket_count);

    if bucket_size != 0 {
        let mut start = 1usize;
        while start < len - 1 {
            let end = (start + bucket_size).min(len - 1);
            let mut min_idx = start;
            let mut max_idx = start;
            let mut min_val = y[start];
            let mut max_val = y[start];

            for (idx, &val) in y.iter().enumerate().take(end).skip(start + 1) {
                if val < min_val {
                    min_val = val;
                    min_idx = idx;
                }
                if val > max_val {
                    max_val = val;
                    max_idx = idx;
                }
            }

            match min_idx.cmp(&max_idx) {
                Ordering::Equal => indices.push(min_idx),
                Ordering::Less => {
                    indices.push(min_idx);
                    indices.push(max_idx);
                }
                Ordering::Greater => {
                    indices.push(max_idx);
                    indices.push(min_idx);
                }
            }

            start = end;
        }
    }
    indices.push(len - 1);

    indices.sort_unstable();
    indices.dedup();

    let mut out_x = Vec::with_capacity(indices.len());
    let mut out_y = Vec::with_capacity(indices.len());
    for idx in indices {
        out_x.push(x[idx].clone());
        out_y.push(y[idx]);
    }

    (out_x, out_y)
}

#[cfg(test)]
mod tests {
    use super::{
        YOY_LINE_ALPHA_MAX, YOY_LINE_ALPHA_MIN, build_monthly_box_series, build_yoy_envelope,
        build_yoy_series_by_year, build_yoy_stddev_series, normalize_to_yoy_month, yoy_line_alpha,
    };
    use chrono::NaiveDate;

    #[test]
    fn yoy_grouping_normalizes_dates_to_anchor_year() {
        let change_dates = vec![
            NaiveDate::from_ymd_opt(2023, 1, 2).expect("valid date"),
            NaiveDate::from_ymd_opt(2023, 12, 31).expect("valid date"),
            NaiveDate::from_ymd_opt(2024, 2, 29).expect("valid date"),
        ];
        let change_values = vec![1.5, -2.0, 0.25];

        let grouped = build_yoy_series_by_year(&change_dates, &change_values);
        assert_eq!(grouped.len(), 2);

        assert_eq!(grouped[0].year, 2023);
        assert_eq!(grouped[0].dates, vec!["2000-01-02", "2000-12-31"]);
        assert_eq!(grouped[0].values, vec![1.5, -2.0]);

        assert_eq!(grouped[1].year, 2024);
        assert_eq!(grouped[1].dates, vec!["2000-02-29"]);
        assert_eq!(grouped[1].values, vec![0.25]);
    }

    #[test]
    fn envelope_is_computed_per_calendar_day() {
        let change_dates = vec![
            NaiveDate::from_ymd_opt(2023, 1, 1).expect("valid date"),
            NaiveDate::from_ymd_opt(2024, 1, 1).expect("valid date"),
            NaiveDate::from_ymd_opt(2024, 1, 2).expect("valid date"),
            NaiveDate::from_ymd_opt(2025, 1, 2).expect("valid date"),
        ];
        let change_values = vec![10.0, -5.0, 7.0, 3.0];

        let envelope =
            build_yoy_envelope(&change_dates, &change_values).expect("envelope should exist");
        assert_eq!(envelope.dates, vec!["2000-01-01", "2000-01-02"]);
        assert_eq!(envelope.lower, vec![-5.0, 3.0]);
        assert_eq!(envelope.upper, vec![10.0, 7.0]);
    }

    #[test]
    fn yoy_stddev_is_computed_per_calendar_day() {
        let change_dates = vec![
            NaiveDate::from_ymd_opt(2023, 1, 1).expect("valid date"),
            NaiveDate::from_ymd_opt(2024, 1, 1).expect("valid date"),
            NaiveDate::from_ymd_opt(2023, 1, 2).expect("valid date"),
            NaiveDate::from_ymd_opt(2024, 1, 2).expect("valid date"),
            NaiveDate::from_ymd_opt(2025, 1, 2).expect("valid date"),
        ];
        let change_values = vec![1.0, 5.0, 2.0, 4.0, 6.0];

        let stddev = build_yoy_stddev_series(&change_dates, &change_values)
            .expect("stddev series should exist");
        assert_eq!(stddev.dates, vec!["2000-01-01", "2000-01-02"]);
        assert!((stddev.values[0] - 2.0).abs() < 1e-12);
        assert!((stddev.values[1] - (8.0_f64 / 3.0).sqrt()).abs() < 1e-12);
    }

    #[test]
    fn yoy_stddev_requires_at_least_two_years_per_day() {
        let change_dates = vec![
            NaiveDate::from_ymd_opt(2023, 1, 1).expect("valid date"),
            NaiveDate::from_ymd_opt(2023, 1, 2).expect("valid date"),
        ];
        let change_values = vec![1.0, 2.0];

        assert!(build_yoy_stddev_series(&change_dates, &change_values).is_none());
    }

    #[test]
    fn yoy_line_alpha_is_strongest_for_newest_year() {
        let oldest = yoy_line_alpha(0, 5);
        let middle = yoy_line_alpha(2, 5);
        let newest = yoy_line_alpha(4, 5);

        assert!((oldest - YOY_LINE_ALPHA_MIN).abs() < f64::EPSILON);
        assert!(oldest < middle);
        assert!(middle < newest);
        assert!((newest - YOY_LINE_ALPHA_MAX).abs() < f64::EPSILON);
        assert!((yoy_line_alpha(0, 1) - YOY_LINE_ALPHA_MAX).abs() < f64::EPSILON);
    }

    #[test]
    fn monthly_box_series_groups_by_calendar_month_and_normalizes_x() {
        let change_dates = vec![
            NaiveDate::from_ymd_opt(2024, 1, 15).expect("valid date"),
            NaiveDate::from_ymd_opt(2024, 2, 1).expect("valid date"),
            NaiveDate::from_ymd_opt(2025, 1, 20).expect("valid date"),
        ];
        let change_values = vec![1.5, -2.0, 3.0];

        let monthly = build_monthly_box_series(&change_dates, &change_values);
        assert_eq!(
            monthly.dates,
            vec![
                "2000-01-15 12:00:00",
                "2000-02-14 03:00:00",
                "2000-01-15 12:00:00",
            ]
        );
        assert_eq!(monthly.values, change_values);
    }

    #[test]
    fn monthly_box_series_keeps_daily_values_without_monthly_aggregation() {
        let change_dates = vec![
            NaiveDate::from_ymd_opt(2024, 3, 1).expect("valid date"),
            NaiveDate::from_ymd_opt(2024, 3, 2).expect("valid date"),
            NaiveDate::from_ymd_opt(2024, 3, 3).expect("valid date"),
        ];
        let change_values = vec![10.0, -4.0, 2.0];

        let monthly = build_monthly_box_series(&change_dates, &change_values);
        assert_eq!(
            monthly.dates,
            vec![
                "2000-03-15 12:00:00",
                "2000-03-15 12:00:00",
                "2000-03-15 12:00:00",
            ]
        );
        assert_eq!(monthly.values, change_values);
    }

    #[test]
    fn monthly_box_series_includes_partial_month_edges() {
        let change_dates = vec![
            NaiveDate::from_ymd_opt(2024, 1, 30).expect("valid date"),
            NaiveDate::from_ymd_opt(2024, 1, 31).expect("valid date"),
            NaiveDate::from_ymd_opt(2024, 2, 1).expect("valid date"),
        ];
        let change_values = vec![1.0, 2.0, 3.0];

        let monthly = build_monthly_box_series(&change_dates, &change_values);
        assert_eq!(
            monthly.dates,
            vec![
                "2000-01-15 12:00:00",
                "2000-01-15 12:00:00",
                "2000-02-14 03:00:00",
            ]
        );
        assert_eq!(monthly.values, change_values);
    }

    #[test]
    fn monthly_box_series_uses_configured_month_anchor_coordinates() {
        let expected = vec![
            "2000-01-15 12:00:00",
            "2000-02-14 03:00:00",
            "2000-03-15 12:00:00",
            "2000-04-15 00:00:00",
            "2000-05-15 12:00:00",
            "2000-06-15 00:00:00",
            "2000-07-15 12:00:00",
            "2000-08-15 12:00:00",
            "2000-09-15 00:00:00",
            "2000-10-15 12:00:00",
            "2000-11-15 00:00:00",
            "2000-12-15 12:00:00",
        ];

        let actual = (1..=12).map(normalize_to_yoy_month).collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}
