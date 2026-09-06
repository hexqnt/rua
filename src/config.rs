//! Загрузка TOML-конфигурации и преобразование в параметры запуска CLI.

use std::fmt;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::{Deserialize, Deserializer};

use crate::{model, report};

const DEFAULT_OUTPUT_HTML: &str = "dist/index.html";
const DEFAULT_HISTORY_CSV: &str = "dist/history.csv";
const DEFAULT_FORECAST_CSV: &str = "dist/forecast.csv";
const DEFAULT_FORECAST_HORIZON_DAYS: usize = 365;

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
pub enum Mode {
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
pub enum ModelKind {
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
pub enum ModeConfig {
    Run(RunConfig),
    Download(DownloadConfig),
    Forecast(ForecastConfig),
    Render(RenderConfig),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunConfig {
    pub output_html: PathBuf,
    pub minify_html: bool,
    pub output_history_csv: PathBuf,
    pub output_forecast_csv: PathBuf,
    pub horizon_days: NonZeroUsize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadConfig {
    pub output_csv: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastConfig {
    pub csv: PathBuf,
    pub output_csv: PathBuf,
    pub horizon_days: NonZeroUsize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderConfig {
    pub csv: PathBuf,
    pub forecast_csv: PathBuf,
    pub output_html: PathBuf,
    pub minify_html: bool,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub mode: Mode,
    pub archive_csv: bool,
    pub mode_config: ModeConfig,
    pub model: ResolvedModelConfig,
    pub chart: report::ChartRenderConfig,
}

#[derive(Debug, Clone)]
pub struct ResolvedModelConfig {
    pub kind: ModelKind,
    pub trend_filter: model::TrendFilterConfig,
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

pub fn resolve_runtime_path(path: &Path) -> Result<PathBuf, String> {
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

    let mode_config = match config.mode {
        Mode::Run => ModeConfig::Run(RunConfig {
            output_html: resolve_runtime_path_from(&config.run.output_html, cwd),
            minify_html: config.run.minify_html,
            output_history_csv: resolve_runtime_path_from(&config.run.output_history_csv, cwd),
            output_forecast_csv: resolve_runtime_path_from(&config.run.output_forecast_csv, cwd),
            horizon_days: config.run.horizon_days,
        }),
        Mode::Download => ModeConfig::Download(DownloadConfig {
            output_csv: resolve_runtime_path_from(&config.download.output_csv, cwd),
        }),
        Mode::Forecast => ModeConfig::Forecast(ForecastConfig {
            csv: resolve_runtime_path_from(&config.forecast.csv, cwd),
            output_csv: resolve_runtime_path_from(&config.forecast.output_csv, cwd),
            horizon_days: config.forecast.horizon_days,
        }),
        Mode::Render => {
            let forecast_csv = config.render.forecast_csv.ok_or_else(|| {
                "Field render.forecast_csv is required when mode = \"render\"".to_string()
            })?;
            ModeConfig::Render(RenderConfig {
                csv: resolve_runtime_path_from(&config.render.csv, cwd),
                forecast_csv: resolve_runtime_path_from(&forecast_csv, cwd),
                output_html: resolve_runtime_path_from(&config.render.output_html, cwd),
                minify_html: config.render.minify_html,
            })
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

pub fn load_app_config(path: &Path) -> Result<AppConfig, String> {
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
