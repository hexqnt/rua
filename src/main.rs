mod constants;
mod data;
mod fetch;
mod model;
mod report;
mod series;

use clap::{ArgAction, CommandFactory, Parser, Subcommand};
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
use clap_complete::{Shell, generate};
use tracing_subscriber::EnvFilter;

const APP_ABOUT: &str = "RUA - Dynamic transition of territory in the Russian-Ukrainian conflict";
const DEFAULT_OUTPUT_HTML: &str = "dist/index.html";
const DEFAULT_HISTORY_CSV: &str = "dist/history.csv";
const DEFAULT_FORECAST_CSV: &str = "dist/forecast.csv";
const CSV_ARCHIVE_EXTENSION: &str = "gz";
const DEFAULT_FORECAST_HORIZON_DAYS: usize = 365;
const DEFAULT_MODEL_CONFIG: &str = "config/model.toml";
const FETCH_MAX_RETRIES: u32 = 10;
const FETCH_DELAY_SECS: u64 = 2;

#[derive(Parser, Debug)]
#[command(name = "rua", about = APP_ABOUT)]
struct Args {
    /// Архивировать CSV в .csv.gz и использовать архивы в HTML.
    /// Исходные CSV удаляются после успешной архивации.
    #[arg(long = "archive-csv", global = true)]
    archive_csv: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Полный режим: скачать данные, обучить модель и сгенерировать HTML с прогнозом.
    Run {
        /// Куда сохранить HTML.
        #[arg(
            short = 'o',
            long = "output-html",
            value_name = "PATH",
            default_value = DEFAULT_OUTPUT_HTML
        )]
        output_html: PathBuf,
        /// Не минифицировать HTML (по умолчанию минифицируется).
        #[arg(
            long = "no-minify-html",
            default_value_t = true,
            action = ArgAction::SetFalse
        )]
        minify_html: bool,
        /// Куда сохранить CSV с историческими данными.
        #[arg(
            long = "output-history-csv",
            value_name = "PATH",
            default_value = DEFAULT_HISTORY_CSV
        )]
        output_history_csv: PathBuf,
        /// Куда сохранить CSV с прогнозом.
        #[arg(
            long = "output-forecast-csv",
            value_name = "PATH",
            default_value = DEFAULT_FORECAST_CSV
        )]
        output_forecast_csv: PathBuf,
        /// Горизонт прогноза (в днях).
        #[arg(
            long = "horizon-days",
            value_name = "DAYS",
            default_value_t = default_horizon_days()
        )]
        horizon_days: NonZeroUsize,
        /// TOML-файл с параметрами модели.
        #[arg(
            long = "model-config",
            value_name = "PATH",
            default_value = DEFAULT_MODEL_CONFIG
        )]
        model_config: PathBuf,
    },
    /// Скачать данные и сохранить CSV.
    Download {
        /// Куда сохранить CSV.
        #[arg(
            short = 'o',
            long = "output-csv",
            value_name = "PATH",
            default_value = DEFAULT_HISTORY_CSV
        )]
        output_csv: PathBuf,
    },
    /// Обучить модель и сохранить прогноз в CSV.
    Forecast {
        /// CSV с историческими данными.
        #[arg(
            short = 'c',
            long = "csv",
            value_name = "PATH",
            default_value = DEFAULT_HISTORY_CSV
        )]
        csv: PathBuf,
        /// Куда сохранить CSV с прогнозом.
        #[arg(
            long = "output-csv",
            value_name = "PATH",
            default_value = DEFAULT_FORECAST_CSV
        )]
        output_csv: PathBuf,
        /// Горизонт прогноза (в днях).
        #[arg(
            long = "horizon-days",
            value_name = "DAYS",
            default_value_t = default_horizon_days()
        )]
        horizon_days: NonZeroUsize,
        /// TOML-файл с параметрами модели.
        #[arg(
            long = "model-config",
            value_name = "PATH",
            default_value = DEFAULT_MODEL_CONFIG
        )]
        model_config: PathBuf,
    },
    /// Сгенерировать HTML-страницу на основе CSV и прогноза.
    Render {
        /// CSV с историческими данными.
        #[arg(
            short = 'c',
            long = "csv",
            value_name = "PATH",
            default_value = DEFAULT_HISTORY_CSV
        )]
        csv: PathBuf,
        /// CSV с прогнозом (обязателен).
        #[arg(long = "forecast-csv", value_name = "PATH")]
        forecast_csv: PathBuf,
        /// Куда сохранить HTML.
        #[arg(
            short = 'o',
            long = "output-html",
            value_name = "PATH",
            default_value = DEFAULT_OUTPUT_HTML
        )]
        output_html: PathBuf,
        /// Не минифицировать HTML (по умолчанию минифицируется).
        #[arg(
            long = "no-minify-html",
            default_value_t = true,
            action = ArgAction::SetFalse
        )]
        minify_html: bool,
    },
    /// Сгенерировать файлы автодополнения для shell.
    Completions {
        /// Целевой shell.
        #[arg(value_enum)]
        shell: Shell,
        /// Куда сохранить файл (если не указано — stdout).
        #[arg(short = 'o', long = "output", value_name = "PATH")]
        output: Option<PathBuf>,
    },
}

const fn default_horizon_days() -> NonZeroUsize {
    NonZeroUsize::new(DEFAULT_FORECAST_HORIZON_DAYS)
        .expect("DEFAULT_FORECAST_HORIZON_DAYS must be non-zero")
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
struct ModelConfigFile {
    #[serde(default)]
    model: ModelKind,
    trend_filter: Option<TrendFilterFile>,
}

#[derive(Debug, Deserialize)]
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

fn default_resolved_config() -> ResolvedModelConfig {
    ResolvedModelConfig {
        kind: ModelKind::TrendFilter,
        trend_filter: model::TrendFilterConfig::default(),
    }
}

fn load_model_config(path: &Path) -> Result<ResolvedModelConfig, String> {
    if !path.exists() {
        if path == Path::new(DEFAULT_MODEL_CONFIG) {
            tracing::info!(
                "Model config {} not found, using built-in defaults",
                path.display()
            );
            return Ok(default_resolved_config());
        }
        return Err(format!("Model config {} does not exist", path.display()));
    }

    let raw = fs::read_to_string(path)
        .map_err(|err| format!("Failed to read model config {}: {err}", path.display()))?;
    let config: ModelConfigFile = toml::from_str(&raw)
        .map_err(|err| format!("Failed to parse model config {}: {err}", path.display()))?;

    match config.model {
        ModelKind::TrendFilter => {
            let trend_filter = resolve_trend_filter_config(config.trend_filter);
            Ok(ResolvedModelConfig {
                kind: ModelKind::TrendFilter,
                trend_filter,
            })
        }
        ModelKind::Llt => {
            if config.trend_filter.is_some() {
                tracing::warn!("trend_filter section ignored for LLT model");
            }
            Ok(ResolvedModelConfig {
                kind: ModelKind::Llt,
                trend_filter: model::TrendFilterConfig::default(),
            })
        }
    }
}

fn generate_completions(shell: Shell, output: Option<PathBuf>) -> Result<(), String> {
    let mut cmd = Args::command();
    let bin_name = cmd.get_name().to_string();
    if let Some(path) = output {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create {}: {err}", parent.display()))?;
        }
        let mut file = File::create(&path)
            .map_err(|err| format!("Failed to create {}: {err}", path.display()))?;
        generate(shell, &mut cmd, bin_name, &mut file);
    } else {
        let mut stdout = std::io::stdout();
        generate(shell, &mut cmd, bin_name, &mut stdout);
    }
    Ok(())
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
    let archive_csv = args.archive_csv;
    match args.command {
        Command::Completions { shell, output } => {
            if let Err(err) = generate_completions(shell, output) {
                eprintln!("{err}");
            }
            return;
        }
        Command::Run {
            output_html,
            minify_html,
            output_history_csv,
            output_forecast_csv,
            horizon_days,
            model_config: model_config_path,
        } => {
            init_logging();
            headline(APP_ABOUT);
            let model_config = match load_model_config(&model_config_path) {
                Ok(config) => config,
                Err(err) => {
                    error(&err);
                    return;
                }
            };
            tracing::info!(
                mode = "run",
                model = %model_config.kind,
                archive_csv,
                horizon_days = horizon_days.get(),
                model_config_path = %model_config_path.display(),
                output_history_csv = %output_history_csv.display(),
                output_forecast_csv = %output_forecast_csv.display(),
                output_html = %output_html.display(),
                minify_html,
                "Starting full pipeline"
            );
            let download_links = match build_download_links(
                &output_history_csv,
                &output_forecast_csv,
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
                output_history_csv.display()
            ));
            if let Err(err) = download_to_csv(&output_history_csv).await {
                error(&err);
                return;
            }
            if archive_csv {
                match archive_csv_file(&output_history_csv) {
                    Ok(path) => success(&format!("Saved archive to {}", path.display())),
                    Err(err) => {
                        error(&err);
                        return;
                    }
                }
            }

            let buckets = match series::load_area_buckets(&output_history_csv) {
                Ok(buckets) => buckets,
                Err(err) => {
                    error(&format!("Failed to read history CSV: {err}"));
                    return;
                }
            };
            if archive_csv && let Err(err) = remove_csv_file(&output_history_csv) {
                error(&err);
                return;
            }

            let forecast = match train_forecast_from_buckets(&buckets, horizon_days, &model_config)
            {
                Ok(forecast) => forecast,
                Err(err) => {
                    error(&format!("Failed to train forecast model: {err}"));
                    return;
                }
            };

            if let Err(err) = model::write_forecast_csv(&forecast, &output_forecast_csv) {
                error(&format!("Failed to write forecast CSV: {err}"));
                return;
            }
            if archive_csv {
                match archive_csv_file(&output_forecast_csv) {
                    Ok(path) => success(&format!("Saved archive to {}", path.display())),
                    Err(err) => {
                        error(&err);
                        return;
                    }
                }
                if let Err(err) = remove_csv_file(&output_forecast_csv) {
                    error(&err);
                    return;
                }
            }

            let overlay = build_forecast_overlay(&forecast);
            if let Err(err) = report::draw_area_chart_with_forecast_from_buckets(
                &buckets,
                &output_html,
                Some(&overlay),
                Some(download_links),
                minify_html,
            ) {
                error(&format!("Failed to render forecast chart: {err}"));
                return;
            }

            success(&format!(
                "Saved forecast to {} and {}",
                if archive_csv {
                    archive_path_for(&output_forecast_csv).map_or_else(
                        |_| output_forecast_csv.display().to_string(),
                        |path| path.display().to_string(),
                    )
                } else {
                    output_forecast_csv.display().to_string()
                },
                output_html.display()
            ));
        }
        Command::Download { output_csv } => {
            init_logging();
            headline(APP_ABOUT);
            tracing::info!(
                mode = "download",
                archive_csv,
                output_csv = %output_csv.display(),
                "Downloading history data"
            );
            info(&format!("Saving CSV to {}", output_csv.display()));
            if let Err(err) = download_to_csv(&output_csv).await {
                error(&err);
                return;
            }
            if archive_csv {
                match archive_csv_file(&output_csv) {
                    Ok(path) => {
                        success(&format!("Saved archive to {}", path.display()));
                        if let Err(err) = remove_csv_file(&output_csv) {
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
                success(&format!("Saved CSV to {}", output_csv.display()));
            }
        }
        Command::Forecast {
            csv,
            output_csv,
            horizon_days,
            model_config: model_config_path,
        } => {
            init_logging();
            headline(APP_ABOUT);
            let model_config = match load_model_config(&model_config_path) {
                Ok(config) => config,
                Err(err) => {
                    error(&err);
                    return;
                }
            };
            tracing::info!(
                mode = "forecast",
                model = %model_config.kind,
                archive_csv,
                horizon_days = horizon_days.get(),
                model_config_path = %model_config_path.display(),
                input_csv = %csv.display(),
                output_csv = %output_csv.display(),
                "Training forecast model"
            );
            let forecast = match train_forecast_from_csv(&csv, horizon_days, &model_config) {
                Ok(forecast) => forecast,
                Err(err) => {
                    error(&format!("Failed to train forecast model: {err}"));
                    return;
                }
            };

            if let Err(err) = model::write_forecast_csv(&forecast, &output_csv) {
                error(&format!("Failed to write forecast CSV: {err}"));
                return;
            }
            if archive_csv {
                match archive_csv_file(&output_csv) {
                    Ok(path) => {
                        success(&format!("Saved archive to {}", path.display()));
                        if let Err(err) = remove_csv_file(&output_csv) {
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
                    archive_path_for(&output_csv).map_or_else(
                        |_| output_csv.display().to_string(),
                        |path| path.display().to_string(),
                    )
                } else {
                    output_csv.display().to_string()
                }
            ));
        }
        Command::Render {
            csv,
            forecast_csv,
            output_html,
            minify_html,
        } => {
            init_logging();
            headline(APP_ABOUT);
            tracing::info!(
                mode = "render",
                archive_csv,
                input_csv = %csv.display(),
                forecast_csv = %forecast_csv.display(),
                output_html = %output_html.display(),
                minify_html,
                "Rendering HTML report"
            );
            let download_links = match build_download_links(&csv, &forecast_csv, archive_csv) {
                Ok(links) => links,
                Err(err) => {
                    error(&err);
                    return;
                }
            };
            if archive_csv {
                match archive_csv_file(&csv) {
                    Ok(path) => success(&format!("Saved archive to {}", path.display())),
                    Err(err) => {
                        error(&err);
                        return;
                    }
                }
                match archive_csv_file(&forecast_csv) {
                    Ok(path) => success(&format!("Saved archive to {}", path.display())),
                    Err(err) => {
                        error(&err);
                        return;
                    }
                }
            }
            let overlay = match load_forecast_overlay(&forecast_csv) {
                Ok(overlay) => overlay,
                Err(err) => {
                    error(&format!("Failed to read forecast CSV: {err}"));
                    return;
                }
            };

            if let Err(err) = report::draw_area_chart_with_forecast(
                &csv,
                &output_html,
                Some(&overlay),
                Some(download_links),
                minify_html,
            ) {
                error(&format!("Failed to render forecast chart: {err}"));
                return;
            }
            if archive_csv {
                if let Err(err) = remove_csv_file(&csv) {
                    error(&err);
                    return;
                }
                if let Err(err) = remove_csv_file(&forecast_csv) {
                    error(&err);
                    return;
                }
            }
            success(&format!("Saved HTML to {}", output_html.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ModelConfigFile, ModelKind};

    #[test]
    fn model_kind_supports_aliases() {
        let kebab: ModelConfigFile =
            toml::from_str("model = \"trend-filter\"").expect("kebab-case model should parse");
        assert_eq!(kebab.model, ModelKind::TrendFilter);

        let alias: ModelConfigFile =
            toml::from_str("model = \"trend_filter\"").expect("alias model should parse");
        assert_eq!(alias.model, ModelKind::TrendFilter);

        let llt: ModelConfigFile =
            toml::from_str("model = \"llt\"").expect("LLT model should parse");
        assert_eq!(llt.model, ModelKind::Llt);
    }

    #[test]
    fn rejects_invalid_trend_filter_values_during_parse() {
        let err = toml::from_str::<ModelConfigFile>(
            "model = \"trend-filter\"\n[trend_filter]\ndamping = 1.5",
        )
        .expect_err("damping out of range should fail parse");
        assert!(err.to_string().contains("0..=1"));

        let err = toml::from_str::<ModelConfigFile>(
            "model = \"trend-filter\"\n[trend_filter]\nepsilon = 0.0",
        )
        .expect_err("non-positive epsilon should fail parse");
        assert!(err.to_string().contains("> 0"));
    }
}
