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

const DEFAULT_HISTORY_CSV_LINK: &str = "history.csv";
const DEFAULT_FORECAST_CSV_LINK: &str = "forecast.csv";

#[derive(Clone, Debug)]
pub struct DownloadLinks {
    pub history: String,
    pub forecast: String,
}

impl Default for DownloadLinks {
    fn default() -> Self {
        Self {
            history: DEFAULT_HISTORY_CSV_LINK.to_string(),
            forecast: DEFAULT_FORECAST_CSV_LINK.to_string(),
        }
    }
}

/// Собирает и сохраняет Plotly-график без прогноза (как в прежней Python-версии).
#[allow(dead_code)]
pub fn draw_area_chart(csv_path: &Path, output_html: &Path) -> Result<(), Box<dyn Error>> {
    draw_area_chart_with_forecast(csv_path, output_html, None, None, false)
}

pub fn draw_area_chart_with_forecast(
    csv_path: &Path,
    output_html: &Path,
    forecast: Option<&ForecastOverlay>,
    download_links: Option<DownloadLinks>,
    minify_html: bool,
) -> Result<(), Box<dyn Error>> {
    let chart::ChartOutput { plot, summary } = chart::build_area_chart(csv_path, forecast)?;
    render_plot(&plot, &summary, output_html, download_links, minify_html)
}

pub fn draw_area_chart_with_forecast_from_buckets(
    buckets: &AreaBuckets,
    output_html: &Path,
    forecast: Option<&ForecastOverlay>,
    download_links: Option<DownloadLinks>,
    minify_html: bool,
) -> Result<(), Box<dyn Error>> {
    let chart::ChartOutput { plot, summary } =
        chart::build_area_chart_from_buckets(buckets, forecast)?;
    render_plot(&plot, &summary, output_html, download_links, minify_html)
}

fn render_plot(
    plot: &plotly::Plot,
    summary: &chart::ChartSummary,
    output_html: &Path,
    download_links: Option<DownloadLinks>,
    minify_html: bool,
) -> Result<(), Box<dyn Error>> {
    // Создаём директорию для HTML, если её ещё нет.
    if let Some(parent) = output_html.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let generated_at = Utc::now();
    let links = download_links.unwrap_or_default();
    let page = page::render_plot_page(plot, summary, generated_at, &links);
    if minify_html {
        let cfg = minify_html::Cfg::new();
        let minified = minify_html::minify(page.as_bytes(), &cfg);
        fs::write(output_html, minified)?;
    } else {
        fs::write(output_html, page)?;
    }
    Ok(())
}
