//! Подготовка данных и генерация Plotly-графика.

use std::error::Error;
use std::path::Path;

use chrono::NaiveDate;
use itertools::Itertools;
use plotly::color::{Rgb, Rgba};
use plotly::common::{Anchor, DashType, Fill, Font, Line, Mode, Orientation, Title};
use plotly::layout::{
    Annotation, Axis, GridPattern, ItemClick, Layout, LayoutGrid, Legend, Margin, RowOrder, Shape,
    ShapeLayer, ShapeLine, ShapeType, TicksDirection,
};
use plotly::{Configuration, Plot, Scatter};

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
    pub plot: Plot,
    pub summary: ChartSummary,
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
const AXIS_REF_X: &str = "x";
const AXIS_REF_Y: &str = "y";
const AXIS_REF_PAPER: &str = "paper";
const AXIS_REF_PIXEL: &str = "pixel";
const LABEL_ACTUAL: &str = "Факт";
const LABEL_FORECAST: &str = "Прогноз";
const LABEL_CONFIDENCE: &str = "95%";
const LABEL_AVG_CHANGE: &str = "Ср. изменение";
const LABEL_UKRAINE: &str = "Украины";
const UNIT_THOUSAND_KM2: &str = "тыс. км²";
const UNIT_KM2_PER_DAY: &str = "км²/сутки";
const FONT_FAMILY: &str = "PT Sans, Arial, sans-serif";
const TICK_FORMAT_MONTH_YEAR: &str = "%b\n%Y";
const FONT_SIZE_BASE: usize = 12;
const FONT_SIZE_AXIS_TITLE: usize = 13;
const FONT_SIZE_AXIS_TICK: usize = 11;
const FONT_SIZE_ANNOTATION: usize = 11;
const LINE_WIDTH_MAIN: f64 = 2.6;
const LINE_WIDTH_FORECAST: f64 = 2.2;
const LINE_WIDTH_CHANGE: f64 = 1.6;
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
const X2_TICKS_COUNT: usize = 8;
const COLOR_AREA: (u8, u8, u8) = (36, 100, 166);
const COLOR_AREA_TRANSPARENT: (u8, u8, u8, f64) = (36, 100, 166, 0.0);
const COLOR_AREA_BAND: (u8, u8, u8, f64) = (36, 100, 166, 0.2);
const COLOR_CHANGE_FILL: (u8, u8, u8, f64) = (220, 82, 60, 0.25);
const COLOR_CHANGE_LINE: (u8, u8, u8) = (200, 67, 46);
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

/// Строит Plotly-график (и возвращает последний уровень площади) без прогноза.
pub(super) fn build_area_chart(
    csv_path: &Path,
    forecast: Option<ForecastOverlay>,
) -> Result<ChartOutput, Box<dyn Error>> {
    let buckets = load_area_buckets(csv_path)?;
    build_area_chart_from_buckets(&buckets, forecast)
}

pub(super) fn build_area_chart_from_buckets(
    buckets: &AreaBuckets,
    forecast: Option<ForecastOverlay>,
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
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid fallback date"));
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

    let daily_changes = daily_change_series(&occupied_area);
    let smoothed_daily = centered_moving_average(
        &daily_changes,
        CHANGE_SMOOTH_WINDOW,
        CHANGE_SMOOTH_MIN_PERIODS,
    );
    // Показываем суточные изменения только после базовой даты.
    let threshold =
        NaiveDate::from_ymd_opt(CHANGE_BASELINE.0, CHANGE_BASELINE.1, CHANGE_BASELINE.2)
            .expect("valid threshold date");
    let (change_dates, change_values) = dates
        .iter()
        .zip(smoothed_daily)
        .filter_map(|(date, value)| value.map(|v| (*date, v)))
        .filter(|(date, _)| *date >= threshold)
        .map(|(date, value)| (date.format(DATE_FORMAT).to_string(), value))
        .unzip::<_, _, Vec<_>, Vec<_>>();

    let (area_dates_plot, area_km2_plot) =
        downsample_min_max(&area_dates, &area_km2, MAX_PLOT_POINTS);
    let (change_dates_plot, change_values_plot) =
        downsample_min_max(&change_dates, &change_values, MAX_PLOT_POINTS / 2);

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

    let forecast_ref = forecast.as_ref();
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
    if let (Some(last_date), Some(last_value)) = (change_dates.last(), change_values.last()) {
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

    let reference_dates = [threshold];
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
        plot,
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

/// Находит последний локальный максимум за год для потенциальной пометки на графике.
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
                Some(sum / count as f64)
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
    let bucket_size = (interior_len as f64 / bucket_count as f64).ceil() as usize;

    if bucket_size == 0 {
        indices.push(len - 1);
    } else {
        let mut start = 1usize;
        while start < len - 1 {
            let end = (start + bucket_size).min(len - 1);
            let mut min_idx = start;
            let mut max_idx = start;
            let mut min_val = y[start];
            let mut max_val = y[start];

            for idx in (start + 1)..end {
                let val = y[idx];
                if val < min_val {
                    min_val = val;
                    min_idx = idx;
                }
                if val > max_val {
                    max_val = val;
                    max_idx = idx;
                }
            }

            if min_idx == max_idx {
                indices.push(min_idx);
            } else if min_idx < max_idx {
                indices.push(min_idx);
                indices.push(max_idx);
            } else {
                indices.push(max_idx);
                indices.push(min_idx);
            }

            start = end;
        }

        indices.push(len - 1);
    }

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
