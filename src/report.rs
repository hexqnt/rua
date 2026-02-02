//! Утилиты для построения графика динамики занятых территорий.

mod chart;
mod data;
mod page;

use std::error::Error;
use std::fs;
use std::path::Path;

use crate::series::AreaBuckets;
use chrono::Utc;

pub use chart::ForecastOverlay;

/// Собирает и сохраняет Plotly-график без прогноза (как в прежней Python-версии).
pub fn draw_area_chart(csv_path: &Path, output_html: &Path) -> Result<(), Box<dyn Error>> {
    draw_area_chart_with_forecast(csv_path, output_html, None)
}

pub fn draw_area_chart_with_forecast(
    csv_path: &Path,
    output_html: &Path,
    forecast: Option<ForecastOverlay>,
) -> Result<(), Box<dyn Error>> {
    let chart::ChartOutput { plot, summary } = chart::build_area_chart(csv_path, forecast)?;
    render_plot(plot, summary, output_html)
}

pub fn draw_area_chart_with_forecast_from_buckets(
    buckets: &AreaBuckets,
    output_html: &Path,
    forecast: Option<ForecastOverlay>,
) -> Result<(), Box<dyn Error>> {
    let chart::ChartOutput { plot, summary } =
        chart::build_area_chart_from_buckets(buckets, forecast)?;
    render_plot(plot, summary, output_html)
}

fn render_plot(
    plot: plotly::Plot,
    summary: chart::ChartSummary,
    output_html: &Path,
) -> Result<(), Box<dyn Error>> {
    // Создаём директорию для HTML, если её ещё нет.
    if let Some(parent) = output_html.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let generated_at = Utc::now();
    let page = page::render_plot_page(&plot, &summary, generated_at);
    fs::write(output_html, page)?;
    Ok(())
}
