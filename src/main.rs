mod config;
mod constants;
mod data;
mod model;
mod report;
mod series;

use clap::Parser;
use flate2::Compression;
use flate2::write::GzEncoder;
use std::fs::{self, File};
use std::io::IsTerminal;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use crate::config::{
    AppConfig, ModeConfig, ModelKind, ResolvedModelConfig, load_app_config, resolve_runtime_path,
};
use crate::constants::AREA_THOUSANDS_DIVISOR;
use crate::data::to_csv;
use crate::series::AreaBuckets;
use rua::deepstatemap::Client as DeepStateMapClient;
use tracing_subscriber::EnvFilter;

const APP_ABOUT: &str = "RUA - Dynamic transition of territory in the Russian-Ukrainian conflict";
const CSV_ARCHIVE_EXTENSION: &str = "gz";

#[derive(Parser, Debug)]
#[command(name = "rua", about = APP_ABOUT)]
struct Args {
    /// TOML-файл с параметрами запуска.
    #[arg(long = "config", value_name = "PATH")]
    config: PathBuf,
}

fn build_forecast_overlay(forecast: &model::Forecast) -> report::ForecastOverlay {
    let to_thousand_km2 = |values: &[f64]| {
        values
            .iter()
            .map(|value| value / AREA_THOUSANDS_DIVISOR)
            .collect()
    };

    report::ForecastOverlay {
        dates: forecast.dates.iter().map(ToString::to_string).collect(),
        mean: to_thousand_km2(&forecast.mean),
        lower: to_thousand_km2(&forecast.lower),
        upper: to_thousand_km2(&forecast.upper),
    }
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
    let client = DeepStateMapClient::default();
    let snapshots = data::fetch_all_with_progress(&client)
        .await
        .map_err(|err| err.to_string())?;
    to_csv(&snapshots, output_csv)?;
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
