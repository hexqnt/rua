mod constants;
mod data;
mod fetch;
mod model;
mod report;
mod series;

use chrono::NaiveDate;
use clap::Parser;
use flate2::Compression;
use flate2::write::GzEncoder;
use serde::{Deserialize, Deserializer};
use std::fmt;
use std::fs::{self, File};
use std::io::IsTerminal;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::constants::{AREA_THOUSANDS_DIVISOR, DATE_FORMAT};
use crate::data::{fetch_areas, to_csv};
use crate::series::AreaBuckets;
use tracing_subscriber::EnvFilter;

const APP_ABOUT: &str = "RUA - Dynamic transition of territory in the Russian-Ukrainian conflict";
const DEFAULT_OUTPUT_HTML: &str = "dist/index.html";
const DEFAULT_HISTORY_CSV: &str = "dist/history.csv";
const DEFAULT_FORECAST_CSV: &str = "dist/forecast.csv";
const CSV_ARCHIVE_EXTENSION: &str = "gz";
const DEFAULT_FORECAST_HORIZON_DAYS: usize = 365;
const FETCH_MAX_RETRIES: u32 = 10;
const FETCH_DELAY_SECS: u64 = 2;

#[derive(Parser, Debug)]
#[command(name = "rua", about = APP_ABOUT)]
struct Args {
    /// TOML-файл с параметрами запуска.
    #[arg(long = "config", value_name = "PATH")]
    config: PathBuf,
}

const fn default_horizon_days() -> NonZeroUsize {
    NonZeroUsize::new(DEFAULT_FORECAST_HORIZON_DAYS)
        .expect("DEFAULT_FORECAST_HORIZON_DAYS must be non-zero")
}

const fn default_minify_html() -> bool {
    true
}

fn default_output_html() -> PathBuf {
    PathBuf::from(DEFAULT_OUTPUT_HTML)
}

fn default_history_csv() -> PathBuf {
    PathBuf::from(DEFAULT_HISTORY_CSV)
}

fn default_forecast_csv() -> PathBuf {
    PathBuf::from(DEFAULT_FORECAST_CSV)
}

fn default_gray_zone_start() -> NaiveDate {
    report::ChartRenderConfig::default().gray_zone_start
}

fn default_avg_change_start() -> NaiveDate {
    report::ChartRenderConfig::default().avg_change_start
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Mode {
    Run,
    Download,
    Forecast,
    Render,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Run => f.write_str("run"),
            Self::Download => f.write_str("download"),
            Self::Forecast => f.write_str("forecast"),
            Self::Render => f.write_str("render"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppConfigFile {
    mode: Mode,
    #[serde(default)]
    archive_csv: bool,
    #[serde(default)]
    run: RunConfigFile,
    #[serde(default)]
    download: DownloadConfigFile,
    #[serde(default)]
    forecast: ForecastConfigFile,
    #[serde(default)]
    render: RenderConfigFile,
    #[serde(default)]
    chart: ChartConfigFile,
    #[serde(default)]
    model: ModelKind,
    #[serde(default)]
    trend_filter: Option<TrendFilterFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunConfigFile {
    #[serde(default = "default_output_html")]
    output_html: PathBuf,
    #[serde(default = "default_minify_html")]
    minify_html: bool,
    #[serde(default = "default_history_csv")]
    output_history_csv: PathBuf,
    #[serde(default = "default_forecast_csv")]
    output_forecast_csv: PathBuf,
    #[serde(default = "default_horizon_days")]
    horizon_days: NonZeroUsize,
}

impl Default for RunConfigFile {
    fn default() -> Self {
        Self {
            output_html: default_output_html(),
            minify_html: default_minify_html(),
            output_history_csv: default_history_csv(),
            output_forecast_csv: default_forecast_csv(),
            horizon_days: default_horizon_days(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DownloadConfigFile {
    #[serde(default = "default_history_csv")]
    output_csv: PathBuf,
}

impl Default for DownloadConfigFile {
    fn default() -> Self {
        Self {
            output_csv: default_history_csv(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ForecastConfigFile {
    #[serde(default = "default_history_csv")]
    csv: PathBuf,
    #[serde(default = "default_forecast_csv")]
    output_csv: PathBuf,
    #[serde(default = "default_horizon_days")]
    horizon_days: NonZeroUsize,
}

impl Default for ForecastConfigFile {
    fn default() -> Self {
        Self {
            csv: default_history_csv(),
            output_csv: default_forecast_csv(),
            horizon_days: default_horizon_days(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RenderConfigFile {
    #[serde(default = "default_history_csv")]
    csv: PathBuf,
    #[serde(default)]
    forecast_csv: Option<PathBuf>,
    #[serde(default = "default_output_html")]
    output_html: PathBuf,
    #[serde(default = "default_minify_html")]
    minify_html: bool,
}

impl Default for RenderConfigFile {
    fn default() -> Self {
        Self {
            csv: default_history_csv(),
            forecast_csv: None,
            output_html: default_output_html(),
            minify_html: default_minify_html(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChartConfigFile {
    #[serde(default = "default_gray_zone_start")]
    gray_zone_start: NaiveDate,
    #[serde(default = "default_avg_change_start")]
    avg_change_start: NaiveDate,
}

impl Default for ChartConfigFile {
    fn default() -> Self {
        Self {
            gray_zone_start: default_gray_zone_start(),
            avg_change_start: default_avg_change_start(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
enum ModelKind {
    #[default]
    #[serde(alias = "trend_filter")]
    TrendFilter,
    Llt,
}

impl fmt::Display for ModelKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TrendFilter => f.write_str("trend-filter"),
            Self::Llt => f.write_str("llt"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct NonNegativeFinite(f64);

impl NonNegativeFinite {
    const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
struct PositiveFinite(f64);

impl PositiveFinite {
    const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
struct UnitIntervalFinite(f64);

impl UnitIntervalFinite {
    const fn get(self) -> f64 {
        self.0
    }
}

fn parse_non_negative<'de, D>(deserializer: D) -> Result<Option<NonNegativeFinite>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<f64>::deserialize(deserializer)?;
    raw.map(|value| {
        if value.is_finite() && value >= 0.0 {
            Ok(NonNegativeFinite(value))
        } else {
            Err(serde::de::Error::custom("must be a finite value >= 0"))
        }
    })
    .transpose()
}

fn parse_positive<'de, D>(deserializer: D) -> Result<Option<PositiveFinite>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<f64>::deserialize(deserializer)?;
    raw.map(|value| {
        if value.is_finite() && value > 0.0 {
            Ok(PositiveFinite(value))
        } else {
            Err(serde::de::Error::custom("must be a finite value > 0"))
        }
    })
    .transpose()
}

fn parse_unit_interval<'de, D>(deserializer: D) -> Result<Option<UnitIntervalFinite>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<f64>::deserialize(deserializer)?;
    raw.map(|value| {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(UnitIntervalFinite(value))
        } else {
            Err(serde::de::Error::custom(
                "must be a finite value within 0..=1",
            ))
        }
    })
    .transpose()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrendFilterFile {
    #[serde(default, deserialize_with = "parse_non_negative")]
    lambda: Option<NonNegativeFinite>,
    #[serde(default, deserialize_with = "parse_positive")]
    epsilon: Option<PositiveFinite>,
    #[serde(alias = "huber")]
    #[serde(default, deserialize_with = "parse_non_negative")]
    huber_delta: Option<NonNegativeFinite>,
    #[serde(default, deserialize_with = "parse_unit_interval")]
    damping: Option<UnitIntervalFinite>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ModeConfig {
    Run(RunConfig),
    Download(DownloadConfig),
    Forecast(ForecastConfig),
    Render(RenderConfig),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunConfig {
    output_html: PathBuf,
    minify_html: bool,
    output_history_csv: PathBuf,
    output_forecast_csv: PathBuf,
    horizon_days: NonZeroUsize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DownloadConfig {
    output_csv: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForecastConfig {
    csv: PathBuf,
    output_csv: PathBuf,
    horizon_days: NonZeroUsize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderConfig {
    csv: PathBuf,
    forecast_csv: PathBuf,
    output_html: PathBuf,
    minify_html: bool,
}

#[derive(Debug, Clone)]
struct AppConfig {
    mode: Mode,
    archive_csv: bool,
    mode_config: ModeConfig,
    model: ResolvedModelConfig,
    chart: report::ChartRenderConfig,
}

#[derive(Debug, Clone)]
struct ResolvedModelConfig {
    kind: ModelKind,
    trend_filter: model::TrendFilterConfig,
}

fn build_forecast_overlay(forecast: &model::Forecast) -> report::ForecastOverlay {
    report::ForecastOverlay {
        dates: forecast
            .dates
            .iter()
            .map(|date| date.format(DATE_FORMAT).to_string())
            .collect(),
        mean: forecast
            .mean
            .iter()
            .map(|v| v / AREA_THOUSANDS_DIVISOR)
            .collect(),
        lower: forecast
            .lower
            .iter()
            .map(|v| v / AREA_THOUSANDS_DIVISOR)
            .collect(),
        upper: forecast
            .upper
            .iter()
            .map(|v| v / AREA_THOUSANDS_DIVISOR)
            .collect(),
    }
}

fn resolve_trend_filter_config(overrides: Option<TrendFilterFile>) -> model::TrendFilterConfig {
    let mut cfg = model::TrendFilterConfig::default();
    if let Some(overrides) = overrides {
        if let Some(lambda) = overrides.lambda {
            cfg.lambda = lambda.get();
        }
        if let Some(epsilon) = overrides.epsilon {
            cfg.epsilon = epsilon.get();
        }
        if let Some(huber_delta) = overrides.huber_delta {
            cfg.huber_delta = huber_delta.get();
        }
        if let Some(damping) = overrides.damping {
            cfg.damping = damping.get();
        }
    }
    cfg
}

fn resolve_model_config(
    kind: ModelKind,
    overrides: Option<TrendFilterFile>,
) -> ResolvedModelConfig {
    match kind {
        ModelKind::TrendFilter => ResolvedModelConfig {
            kind: ModelKind::TrendFilter,
            trend_filter: resolve_trend_filter_config(overrides),
        },
        ModelKind::Llt => {
            if overrides.is_some() {
                tracing::warn!("trend_filter section ignored for LLT model");
            }
            ResolvedModelConfig {
                kind: ModelKind::Llt,
                trend_filter: model::TrendFilterConfig::default(),
            }
        }
    }
}

fn parse_app_config(raw: &str, path: &Path) -> Result<AppConfigFile, String> {
    toml::from_str(raw).map_err(|err| format!("Failed to parse config {}: {err}", path.display()))
}

fn resolve_runtime_path_from(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn resolve_runtime_path(path: &Path) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir()
        .map_err(|err| format!("Failed to resolve current working directory: {err}"))?;
    Ok(resolve_runtime_path_from(path, &cwd))
}

fn resolve_app_config(config: AppConfigFile, cwd: &Path) -> Result<AppConfig, String> {
    let model = resolve_model_config(config.model, config.trend_filter);
    let chart = report::ChartRenderConfig {
        gray_zone_start: config.chart.gray_zone_start,
        avg_change_start: config.chart.avg_change_start,
    };

    let run = RunConfig {
        output_html: resolve_runtime_path_from(&config.run.output_html, cwd),
        minify_html: config.run.minify_html,
        output_history_csv: resolve_runtime_path_from(&config.run.output_history_csv, cwd),
        output_forecast_csv: resolve_runtime_path_from(&config.run.output_forecast_csv, cwd),
        horizon_days: config.run.horizon_days,
    };

    let download = DownloadConfig {
        output_csv: resolve_runtime_path_from(&config.download.output_csv, cwd),
    };

    let forecast = ForecastConfig {
        csv: resolve_runtime_path_from(&config.forecast.csv, cwd),
        output_csv: resolve_runtime_path_from(&config.forecast.output_csv, cwd),
        horizon_days: config.forecast.horizon_days,
    };

    let render_mode = if let Some(path) = config.render.forecast_csv {
        Some(RenderConfig {
            csv: resolve_runtime_path_from(&config.render.csv, cwd),
            forecast_csv: resolve_runtime_path_from(&path, cwd),
            output_html: resolve_runtime_path_from(&config.render.output_html, cwd),
            minify_html: config.render.minify_html,
        })
    } else {
        None
    };

    let mode_config = match config.mode {
        Mode::Run => ModeConfig::Run(run),
        Mode::Download => ModeConfig::Download(download),
        Mode::Forecast => ModeConfig::Forecast(forecast),
        Mode::Render => {
            let Some(render) = render_mode else {
                return Err(
                    "Field render.forecast_csv is required when mode = \"render\"".to_string(),
                );
            };
            ModeConfig::Render(render)
        }
    };

    Ok(AppConfig {
        mode: config.mode,
        archive_csv: config.archive_csv,
        mode_config,
        model,
        chart,
    })
}

fn load_app_config(path: &Path) -> Result<AppConfig, String> {
    if !path.exists() {
        return Err(format!("Config {} does not exist", path.display()));
    }

    let raw = fs::read_to_string(path)
        .map_err(|err| format!("Failed to read config {}: {err}", path.display()))?;
    let parsed = parse_app_config(&raw, path)?;
    let cwd = std::env::current_dir()
        .map_err(|err| format!("Failed to resolve current working directory: {err}"))?;
    resolve_app_config(parsed, &cwd)
}

fn train_forecast_from_csv(
    csv_path: &Path,
    horizon_days: NonZeroUsize,
    model_config: &ResolvedModelConfig,
) -> Result<model::Forecast, String> {
    match model_config.kind {
        ModelKind::TrendFilter => {
            model::train_trend_filter_from_csv(csv_path, model_config.trend_filter)
                .map(|fitted| fitted.forecast(horizon_days.get()))
                .map_err(|err| err.to_string())
        }
        ModelKind::Llt => model::train_from_csv(csv_path, model::ModelConfig::default())
            .map(|fitted| fitted.forecast(horizon_days.get()))
            .map_err(|err| err.to_string()),
    }
}

fn train_forecast_from_buckets(
    buckets: &AreaBuckets,
    horizon_days: NonZeroUsize,
    model_config: &ResolvedModelConfig,
) -> Result<model::Forecast, String> {
    match model_config.kind {
        ModelKind::TrendFilter => {
            model::train_trend_filter_from_buckets(buckets, model_config.trend_filter)
                .map(|fitted| fitted.forecast(horizon_days.get()))
                .map_err(|err| err.to_string())
        }
        ModelKind::Llt => model::train_from_buckets(buckets, model::ModelConfig::default())
            .map(|fitted| fitted.forecast(horizon_days.get()))
            .map_err(|err| err.to_string()),
    }
}

fn load_forecast_overlay(forecast_csv: &Path) -> Result<report::ForecastOverlay, String> {
    model::read_forecast_csv(forecast_csv)
        .map(|forecast| build_forecast_overlay(&forecast))
        .map_err(|err| err.to_string())
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("rua=info"));
    let ansi = std::io::stdout().is_terminal();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(ansi)
        .compact()
        .init();
}

fn headline(message: &str) {
    tracing::info!(status = "start", "{message}");
}

fn info(message: &str) {
    tracing::info!(status = "info", "{message}");
}

fn success(message: &str) {
    tracing::info!(status = "ok", "{message}");
}

fn error(message: &str) {
    tracing::error!(status = "err", "{message}");
}

async fn download_to_csv(output_csv: &Path) -> Result<(), String> {
    let delay = Duration::from_secs(FETCH_DELAY_SECS);
    let client = fetch::build_client();
    let areas = fetch_areas(&client, FETCH_MAX_RETRIES, delay).await?;
    to_csv(areas, output_csv)?;
    Ok(())
}

fn file_name_for(path: &Path) -> Result<String, String> {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .ok_or_else(|| format!("Path {} has no file name", path.display()))
}

fn archive_path_for(csv_path: &Path) -> Result<PathBuf, String> {
    let file_name = file_name_for(csv_path)?;
    let archive_name = format!("{file_name}.{CSV_ARCHIVE_EXTENSION}");
    let mut archive_path = csv_path.to_path_buf();
    archive_path.set_file_name(archive_name);
    Ok(archive_path)
}

fn archive_csv_file(csv_path: &Path) -> Result<PathBuf, String> {
    let archive_path = archive_path_for(csv_path)?;
    if let Some(parent) = archive_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create {}: {err}", parent.display()))?;
    }
    let mut input = File::open(csv_path)
        .map_err(|err| format!("Failed to open CSV {}: {err}", csv_path.display()))?;
    let output = File::create(&archive_path)
        .map_err(|err| format!("Failed to create archive {}: {err}", archive_path.display()))?;
    let mut encoder = GzEncoder::new(output, Compression::default());
    std::io::copy(&mut input, &mut encoder)
        .map_err(|err| format!("Failed to write archive {}: {err}", archive_path.display()))?;
    encoder.finish().map_err(|err| {
        format!(
            "Failed to finalize archive {}: {err}",
            archive_path.display()
        )
    })?;
    Ok(archive_path)
}

fn remove_csv_file(csv_path: &Path) -> Result<(), String> {
    fs::remove_file(csv_path)
        .map_err(|err| format!("Failed to remove CSV {}: {err}", csv_path.display()))
}

fn download_name(csv_path: &Path, archive: bool) -> Result<String, String> {
    if archive {
        let archive_path = archive_path_for(csv_path)?;
        file_name_for(&archive_path)
    } else {
        file_name_for(csv_path)
    }
}

fn build_download_links(
    history_csv: &Path,
    forecast_csv: &Path,
    archive: bool,
) -> Result<report::DownloadLinks, String> {
    Ok(report::DownloadLinks {
        history: download_name(history_csv, archive)?,
        forecast: download_name(forecast_csv, archive)?,
    })
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() {
    let args = Args::parse();
    init_logging();
    headline(APP_ABOUT);

    let config_path = match resolve_runtime_path(&args.config) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };
    let app_config = match load_app_config(&config_path) {
        Ok(config) => config,
        Err(err) => {
            error(&err);
            return;
        }
    };

    let AppConfig {
        mode,
        archive_csv,
        mode_config,
        model: model_config,
        chart: chart_config,
    } = app_config;

    tracing::info!(
        mode = %mode,
        archive_csv,
        config_path = %config_path.display(),
        model = %model_config.kind,
        gray_zone_start = %chart_config.gray_zone_start,
        avg_change_start = %chart_config.avg_change_start,
        "Loaded configuration"
    );

    match mode_config {
        ModeConfig::Run(config) => {
            tracing::info!(
                mode = "run",
                model = %model_config.kind,
                archive_csv,
                horizon_days = config.horizon_days.get(),
                output_history_csv = %config.output_history_csv.display(),
                output_forecast_csv = %config.output_forecast_csv.display(),
                output_html = %config.output_html.display(),
                minify_html = config.minify_html,
                "Starting full pipeline"
            );
            let download_links = match build_download_links(
                &config.output_history_csv,
                &config.output_forecast_csv,
                archive_csv,
            ) {
                Ok(links) => links,
                Err(err) => {
                    error(&err);
                    return;
                }
            };
            info(&format!(
                "Saving history CSV to {}",
                config.output_history_csv.display()
            ));
            if let Err(err) = download_to_csv(&config.output_history_csv).await {
                error(&err);
                return;
            }
            if archive_csv {
                match archive_csv_file(&config.output_history_csv) {
                    Ok(path) => success(&format!("Saved archive to {}", path.display())),
                    Err(err) => {
                        error(&err);
                        return;
                    }
                }
            }

            let buckets = match series::load_area_buckets(&config.output_history_csv) {
                Ok(buckets) => buckets,
                Err(err) => {
                    error(&format!("Failed to read history CSV: {err}"));
                    return;
                }
            };
            if archive_csv && let Err(err) = remove_csv_file(&config.output_history_csv) {
                error(&err);
                return;
            }

            let forecast =
                match train_forecast_from_buckets(&buckets, config.horizon_days, &model_config) {
                    Ok(forecast) => forecast,
                    Err(err) => {
                        error(&format!("Failed to train forecast model: {err}"));
                        return;
                    }
                };

            if let Err(err) = model::write_forecast_csv(&forecast, &config.output_forecast_csv) {
                error(&format!("Failed to write forecast CSV: {err}"));
                return;
            }
            if archive_csv {
                match archive_csv_file(&config.output_forecast_csv) {
                    Ok(path) => success(&format!("Saved archive to {}", path.display())),
                    Err(err) => {
                        error(&err);
                        return;
                    }
                }
                if let Err(err) = remove_csv_file(&config.output_forecast_csv) {
                    error(&err);
                    return;
                }
            }

            let overlay = build_forecast_overlay(&forecast);
            if let Err(err) = report::draw_area_chart_with_forecast_from_buckets_and_config(
                &buckets,
                &config.output_html,
                Some(&overlay),
                chart_config,
                Some(download_links),
                config.minify_html,
            ) {
                error(&format!("Failed to render forecast chart: {err}"));
                return;
            }

            success(&format!(
                "Saved forecast to {} and {}",
                if archive_csv {
                    archive_path_for(&config.output_forecast_csv).map_or_else(
                        |_| config.output_forecast_csv.display().to_string(),
                        |path| path.display().to_string(),
                    )
                } else {
                    config.output_forecast_csv.display().to_string()
                },
                config.output_html.display()
            ));
        }
        ModeConfig::Download(config) => {
            tracing::info!(
                mode = "download",
                archive_csv,
                output_csv = %config.output_csv.display(),
                "Downloading history data"
            );
            info(&format!("Saving CSV to {}", config.output_csv.display()));
            if let Err(err) = download_to_csv(&config.output_csv).await {
                error(&err);
                return;
            }
            if archive_csv {
                match archive_csv_file(&config.output_csv) {
                    Ok(path) => {
                        success(&format!("Saved archive to {}", path.display()));
                        if let Err(err) = remove_csv_file(&config.output_csv) {
                            error(&err);
                            return;
                        }
                    }
                    Err(err) => {
                        error(&err);
                        return;
                    }
                }
            }
            if !archive_csv {
                success(&format!("Saved CSV to {}", config.output_csv.display()));
            }
        }
        ModeConfig::Forecast(config) => {
            tracing::info!(
                mode = "forecast",
                model = %model_config.kind,
                archive_csv,
                horizon_days = config.horizon_days.get(),
                input_csv = %config.csv.display(),
                output_csv = %config.output_csv.display(),
                "Training forecast model"
            );
            let forecast =
                match train_forecast_from_csv(&config.csv, config.horizon_days, &model_config) {
                    Ok(forecast) => forecast,
                    Err(err) => {
                        error(&format!("Failed to train forecast model: {err}"));
                        return;
                    }
                };

            if let Err(err) = model::write_forecast_csv(&forecast, &config.output_csv) {
                error(&format!("Failed to write forecast CSV: {err}"));
                return;
            }
            if archive_csv {
                match archive_csv_file(&config.output_csv) {
                    Ok(path) => {
                        success(&format!("Saved archive to {}", path.display()));
                        if let Err(err) = remove_csv_file(&config.output_csv) {
                            error(&err);
                            return;
                        }
                    }
                    Err(err) => {
                        error(&err);
                        return;
                    }
                }
            }
            success(&format!(
                "Saved forecast to {}",
                if archive_csv {
                    archive_path_for(&config.output_csv).map_or_else(
                        |_| config.output_csv.display().to_string(),
                        |path| path.display().to_string(),
                    )
                } else {
                    config.output_csv.display().to_string()
                }
            ));
        }
        ModeConfig::Render(config) => {
            tracing::info!(
                mode = "render",
                archive_csv,
                input_csv = %config.csv.display(),
                forecast_csv = %config.forecast_csv.display(),
                output_html = %config.output_html.display(),
                minify_html = config.minify_html,
                "Rendering HTML report"
            );
            let download_links =
                match build_download_links(&config.csv, &config.forecast_csv, archive_csv) {
                    Ok(links) => links,
                    Err(err) => {
                        error(&err);
                        return;
                    }
                };
            if archive_csv {
                match archive_csv_file(&config.csv) {
                    Ok(path) => success(&format!("Saved archive to {}", path.display())),
                    Err(err) => {
                        error(&err);
                        return;
                    }
                }
                match archive_csv_file(&config.forecast_csv) {
                    Ok(path) => success(&format!("Saved archive to {}", path.display())),
                    Err(err) => {
                        error(&err);
                        return;
                    }
                }
            }
            let overlay = match load_forecast_overlay(&config.forecast_csv) {
                Ok(overlay) => overlay,
                Err(err) => {
                    error(&format!("Failed to read forecast CSV: {err}"));
                    return;
                }
            };

            if let Err(err) = report::draw_area_chart_with_forecast_and_config(
                &config.csv,
                &config.output_html,
                Some(&overlay),
                chart_config,
                Some(download_links),
                config.minify_html,
            ) {
                error(&format!("Failed to render forecast chart: {err}"));
                return;
            }
            if archive_csv {
                if let Err(err) = remove_csv_file(&config.csv) {
                    error(&err);
                    return;
                }
                if let Err(err) = remove_csv_file(&config.forecast_csv) {
                    error(&err);
                    return;
                }
            }
            success(&format!("Saved HTML to {}", config.output_html.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppConfigFile, DownloadConfig, Mode, ModeConfig, ModelKind, RenderConfig,
        resolve_app_config,
    };
    use chrono::NaiveDate;
    use std::path::Path;

    #[test]
    fn model_kind_supports_aliases() {
        let kebab: AppConfigFile = toml::from_str("mode = \"run\"\nmodel = \"trend-filter\"")
            .expect("kebab-case model should parse");
        assert_eq!(kebab.model, ModelKind::TrendFilter);

        let alias: AppConfigFile = toml::from_str("mode = \"run\"\nmodel = \"trend_filter\"")
            .expect("alias model should parse");
        assert_eq!(alias.model, ModelKind::TrendFilter);

        let llt: AppConfigFile =
            toml::from_str("mode = \"run\"\nmodel = \"llt\"").expect("LLT model should parse");
        assert_eq!(llt.model, ModelKind::Llt);
    }

    #[test]
    fn rejects_invalid_trend_filter_values_during_parse() {
        let err = toml::from_str::<AppConfigFile>(
            "mode = \"run\"\nmodel = \"trend-filter\"\n[trend_filter]\ndamping = 1.5",
        )
        .expect_err("damping out of range should fail parse");
        assert!(err.to_string().contains("0..=1"));

        let err = toml::from_str::<AppConfigFile>(
            "mode = \"run\"\nmodel = \"trend-filter\"\n[trend_filter]\nepsilon = 0.0",
        )
        .expect_err("non-positive epsilon should fail parse");
        assert!(err.to_string().contains("> 0"));
    }

    #[test]
    fn parses_minimal_default_config() {
        let config: AppConfigFile =
            toml::from_str("mode = \"run\"").expect("minimal config with mode should parse");
        let resolved = resolve_app_config(config, Path::new("workspace"))
            .expect("default config should resolve");

        assert_eq!(resolved.mode, Mode::Run);
        assert!(!resolved.archive_csv);
        assert_eq!(
            resolved.chart,
            crate::report::ChartRenderConfig::default(),
            "default chart render config must be applied"
        );
        match resolved.mode_config {
            ModeConfig::Run(run) => {
                assert_eq!(
                    run.output_html,
                    Path::new("workspace").join("dist/index.html")
                );
                assert_eq!(
                    run.output_history_csv,
                    Path::new("workspace").join("dist/history.csv")
                );
                assert_eq!(
                    run.output_forecast_csv,
                    Path::new("workspace").join("dist/forecast.csv")
                );
            }
            _ => panic!("expected run mode config"),
        }
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = toml::from_str::<AppConfigFile>("mode = \"run\"\nunknown = 1")
            .expect_err("unknown top-level field should fail parse");
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_zero_horizon_days() {
        let err =
            toml::from_str::<AppConfigFile>("mode = \"forecast\"\n[forecast]\nhorizon_days = 0")
                .expect_err("zero horizon should fail parse");
        assert!(err.to_string().to_lowercase().contains("nonzero"));
    }

    #[test]
    fn render_mode_requires_forecast_csv() {
        let config: AppConfigFile = toml::from_str("mode = \"render\"")
            .expect("render config without required field still parses");
        let err = resolve_app_config(config, Path::new("workspace"))
            .expect_err("render mode without forecast csv should fail resolve");
        assert!(err.contains("render.forecast_csv"));
    }

    #[test]
    fn resolves_paths_from_cwd() {
        let config: AppConfigFile =
            toml::from_str("mode = \"download\"\n[download]\noutput_csv = \"out/history.csv\"")
                .expect("download config should parse");
        let resolved =
            resolve_app_config(config, Path::new("repo")).expect("config should resolve");

        assert_eq!(resolved.mode, Mode::Download);
        assert_eq!(
            resolved.mode_config,
            ModeConfig::Download(DownloadConfig {
                output_csv: Path::new("repo").join("out/history.csv"),
            })
        );
    }

    #[test]
    fn resolves_render_paths_from_cwd() {
        let config: AppConfigFile = toml::from_str(
            "mode = \"render\"\n[render]\ncsv = \"dist/history.csv\"\nforecast_csv = \"dist/forecast.csv\"\noutput_html = \"dist/custom.html\"",
        )
        .expect("render config should parse");
        let resolved =
            resolve_app_config(config, Path::new("repo")).expect("config should resolve");

        assert_eq!(resolved.mode, Mode::Render);
        assert_eq!(
            resolved.mode_config,
            ModeConfig::Render(RenderConfig {
                csv: Path::new("repo").join("dist/history.csv"),
                forecast_csv: Path::new("repo").join("dist/forecast.csv"),
                output_html: Path::new("repo").join("dist/custom.html"),
                minify_html: true,
            })
        );
    }

    #[test]
    fn resolves_chart_dates_from_config() {
        let config: AppConfigFile = toml::from_str(
            "mode = \"run\"\n[chart]\ngray_zone_start = \"2023-02-05\"\navg_change_start = \"2022-12-01\"",
        )
        .expect("chart config should parse");
        let resolved =
            resolve_app_config(config, Path::new("repo")).expect("config should resolve");

        assert_eq!(
            resolved.chart.gray_zone_start,
            NaiveDate::from_ymd_opt(2023, 2, 5).expect("valid date")
        );
        assert_eq!(
            resolved.chart.avg_change_start,
            NaiveDate::from_ymd_opt(2022, 12, 1).expect("valid date")
        );
    }
}
