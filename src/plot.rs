//! Plot utilities for visualising the dynamics of the occupied territory.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use itertools::Itertools;
use plotly::common::{Fill, Mode, Orientation, Title};
use plotly::layout::{Axis, GridPattern, ItemClick, Layout, LayoutGrid, Legend, RowOrder};
use plotly::{Plot, Scatter};
use serde::Deserialize;

type DailyBuckets = BTreeMap<NaiveDate, DailyAccumulator>;

/// Raw row from the CSV export.
#[derive(Debug, Deserialize)]
struct CsvRow {
    time_index: String,
    hash: String,
    area: f64,
    percent: f64,
    #[serde(alias = "type", alias = "area_type")]
    area_type: String,
}

/// Accumulates values for a single day.
#[derive(Default, Clone, Copy)]
struct DailyAccumulator {
    sum: f64,
    count: u32,
}

impl DailyAccumulator {
    fn add(&mut self, value: f64) {
        self.sum += value;
        self.count += 1;
    }

    fn mean(&self) -> Option<f64> {
        (self.count > 0).then_some(self.sum / self.count as f64)
    }
}

/// Build and persist Plotly figure identical to the former Python visualisation (without forecast).
pub fn draw_area_chart(csv_path: &Path, output_html: &Path) -> Result<(), Box<dyn Error>> {
    let mut reader = csv::Reader::from_path(csv_path)?;
    let buckets =
        reader
            .deserialize::<CsvRow>()
            .try_fold(AreaBuckets::default(), |mut acc, row| {
                let row = row?;
                acc.ingest(row)?;
                Ok::<_, Box<dyn Error>>(acc)
            })?;

    let (dates, occupied_area) = build_occupied_series(&buckets)?;
    let area_dates = dates
        .iter()
        .map(|date| date.format("%Y-%m-%d").to_string())
        .collect_vec();
    let area_km2 = occupied_area
        .iter()
        .map(|value| value / 1000.0)
        .collect_vec();

    let daily_changes = daily_change_series(&occupied_area);
    let smoothed_daily = centered_moving_average(&daily_changes, 5, 3);
    // Display daily changes only after the cited baseline date.
    let threshold = NaiveDate::from_ymd_opt(2022, 11, 23).expect("valid threshold date");
    let (change_dates, change_values) = dates
        .iter()
        .zip(smoothed_daily)
        .filter_map(|(date, value)| value.map(|v| (*date, v)))
        .filter(|(date, _)| *date >= threshold)
        .map(|(date, value)| (date.format("%Y-%m-%d").to_string(), value))
        .unzip::<_, _, Vec<_>, Vec<_>>();

    let mut plot = Plot::new();
    plot.add_trace(
        Scatter::new(area_dates.clone(), area_km2.clone())
            .mode(Mode::Lines)
            .name("Факт")
            .x_axis("x1")
            .y_axis("y1"),
    );

    plot.add_trace(
        Scatter::new(change_dates, change_values)
            .mode(Mode::Lines)
            .fill(Fill::ToZeroY)
            .name("Ср. изменение")
            .x_axis("x2")
            .y_axis("y2"),
    );

    let layout = Layout::new()
        .title(Title::with_text(
            "Территория подконтрольная РФ с начала СВО",
        ))
        .grid(
            LayoutGrid::new()
                .rows(2)
                .columns(1)
                .pattern(GridPattern::Independent)
                .row_order(RowOrder::TopToBottom),
        )
        .show_legend(true)
        .legend(
            Legend::new()
                .orientation(Orientation::Horizontal)
                .item_click(ItemClick::False)
                .item_double_click(ItemClick::False),
        )
        .x_axis(Axis::new().title(Title::new()))
        .y_axis(
            Axis::new()
                .title(Title::with_text("тыс. км²"))
                .grid_color("rgba(0,0,0,0.1)"),
        )
        .x_axis2(Axis::new().matches("x"))
        .y_axis2(
            Axis::new()
                .title(Title::with_text("км²/сутки"))
                .grid_color("rgba(0,0,0,0.1)"),
        );

    plot.set_layout(layout);

    // Ensure the target directory exists before writing the HTML artefact.
    if let Some(parent) = output_html.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    plot.write_html(output_html);
    Ok(())
}

/// Parse various time formats used by the upstream API into UTC.
fn parse_time_index(raw: &str) -> Result<DateTime<Utc>, String> {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return Err("empty time index".into());
    }

    if let Ok(dt) = DateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S %Z") {
        return Ok(dt.with_timezone(&Utc));
    }

    if let Ok(dt) = DateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S %z") {
        return Ok(dt.with_timezone(&Utc));
    }

    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(dt.with_timezone(&Utc));
    }

    if let Some(stripped) = trimmed.strip_suffix(" UTC")
        && let Ok(naive) = NaiveDateTime::parse_from_str(stripped.trim_end(), "%Y-%m-%d %H:%M:%S")
    {
        return Ok(Utc.from_utc_datetime(&naive));
    }

    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return Ok(Utc.from_utc_datetime(&naive));
    }

    Err("unrecognized time format".into())
}

fn build_occupied_series(
    buckets: &AreaBuckets,
) -> Result<(Vec<NaiveDate>, Vec<f64>), Box<dyn Error>> {
    let first_date = buckets
        .ru
        .keys()
        .chain(buckets.ua.keys())
        .min()
        .copied()
        .ok_or_else(|| "No data available".to_string())?;
    let last_date = buckets
        .ru
        .keys()
        .chain(buckets.ua.keys())
        .max()
        .copied()
        .ok_or_else(|| "No data available".to_string())?;

    let span_days = (last_date - first_date).num_days() as usize;
    let dates: Vec<_> = (0..=span_days)
        .map(|offset| first_date + Duration::days(offset as i64))
        .collect();

    let ru_values = interpolate_series(&dates, &buckets.ru);
    let ua_values = interpolate_series(&dates, &buckets.ua);
    let occupied_area = ru_values
        .iter()
        .zip(ua_values)
        .map(|(ru, ua)| ru - ua)
        .collect();

    Ok((dates, occupied_area))
}

/// Linearly interpolate missing days to obtain a dense daily series.
fn interpolate_series(dates: &[NaiveDate], source: &DailyBuckets) -> Vec<f64> {
    let mut values: Vec<Option<f64>> = dates
        .iter()
        .map(|date| source.get(date).and_then(DailyAccumulator::mean))
        .collect();

    // Forward/Backward fill with linear interpolation between known points.
    let mut last_known = None;
    for idx in 0..values.len() {
        if values[idx].is_some() {
            if let Some(start) = last_known {
                if let (Some(start_val), Some(end_val)) = (values[start], values[idx]) {
                    let gap = idx - start;
                    if gap > 1 {
                        for (offset, slot) in ((start + 1)..idx).enumerate() {
                            let ratio = (offset as f64 + 1.0) / gap as f64;
                            values[slot] = Some(start_val + (end_val - start_val) * ratio);
                        }
                    }
                }
            } else {
                // Leading empty chunk – fill with the first observed value.
                let first_value = values[idx];
                for placeholder in values.iter_mut().take(idx) {
                    *placeholder = first_value;
                }
            }
            last_known = Some(idx);
        }
    }

    if let Some(last) = last_known {
        let tail_value = values[last];
        for value in values.iter_mut().skip(last + 1) {
            *value = tail_value;
        }
    }

    let fallback = values.iter().flatten().copied().next().unwrap_or_default();
    values
        .into_iter()
        .map(|value| value.unwrap_or(fallback))
        .collect()
}

/// Extract the most recent local maximum (within the last year) to highlight on the chart.
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

/// Compute day-on-day changes, keeping the first value aligned with the original series.
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

/// Rolling mean centred around each point; returns `None` when the window is under-populated.
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

/// Daily buckets grouped by category.
#[derive(Default)]
struct AreaBuckets {
    ru: DailyBuckets,
    ua: DailyBuckets,
}

impl AreaBuckets {
    /// Add a single CSV row to the corresponding bucket.
    fn ingest(&mut self, row: CsvRow) -> Result<(), Box<dyn Error>> {
        let datetime = parse_time_index(&row.time_index)
            .map_err(|err| format!("failed to parse time_index '{}': {err}", row.time_index))?;
        let date = datetime.date_naive();

        match row.area_type.as_str() {
            "occupied_after_24_02_2022" => {
                self.ru.entry(date).or_default().add(row.area);
            }
            "other_territories" if row.hash == "#01579b" => {
                self.ua.entry(date).or_default().add(row.area);
            }
            _ => {} // Ignore other categories
        }

        Ok(())
    }
}
