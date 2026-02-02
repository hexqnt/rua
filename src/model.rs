#![allow(dead_code)]

use std::error::Error;
use std::fs;
use std::path::Path;

use argmin::core::{CostFunction, Error as ArgminError, Executor, Gradient, State};
use argmin::solver::linesearch::MoreThuenteLineSearch;
use argmin::solver::quasinewton::LBFGS;
use chrono::{Duration, NaiveDate};
use serde::Deserialize;

use crate::constants::DATE_FORMAT;
use crate::series::{AreaBuckets, build_occupied_series, load_area_buckets};

const DEFAULT_MAX_ITERS: u64 = 400;
const DEFAULT_HISTORY: usize = 10;
const DEFAULT_SCALE: f64 = 1000.0;
const DEFAULT_TOL_GRAD: f64 = 1e-8;
const DEFAULT_TOL_COST: f64 = 1e-10;
const DEFAULT_TF_LAMBDA: f64 = 5.0;
const DEFAULT_TF_EPS: f64 = 1e-3;
const DEFAULT_TF_HUBER_DELTA: f64 = 0.0;
const DEFAULT_TF_DAMPING: f64 = 1.0;
const HETERO_WINDOW: usize = 21;
const HETERO_MIN_SCALE: f64 = 0.6;
const HETERO_MAX_SCALE: f64 = 2.5;
const MIN_SIGMA: f64 = 1e-6;
const LARGE_COST: f64 = 1e30;
const CONFIDENCE_Z: f64 = 1.96;
const TRAINING_START: (i32, u32, u32) = (2022, 11, 22);

#[derive(Clone, Copy, Debug)]
pub struct ModelConfig {
    pub max_iters: u64,
    pub history: usize,
    pub scale: f64,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            max_iters: DEFAULT_MAX_ITERS,
            history: DEFAULT_HISTORY,
            scale: DEFAULT_SCALE,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FittedModel {
    pub sigma_level: f64,
    pub sigma_trend: f64,
    pub sigma_obs: f64,
    pub state: [f64; 2],
    pub cov: [[f64; 2]; 2],
    pub last_date: NaiveDate,
    pub scale: f64,
    pub nll: f64,
    pub last_weight: f64,
}

#[derive(Clone, Debug)]
pub struct Forecast {
    pub dates: Vec<NaiveDate>,
    pub mean: Vec<f64>,
    pub lower: Vec<f64>,
    pub upper: Vec<f64>,
    pub variance: Vec<f64>,
}

#[derive(Clone, Copy, Debug)]
pub struct TrendFilterConfig {
    pub lambda: f64,
    pub epsilon: f64,
    pub huber_delta: f64,
    pub damping: f64,
    pub max_iters: u64,
    pub history: usize,
    pub scale: f64,
}

impl Default for TrendFilterConfig {
    fn default() -> Self {
        Self {
            lambda: DEFAULT_TF_LAMBDA,
            epsilon: DEFAULT_TF_EPS,
            huber_delta: DEFAULT_TF_HUBER_DELTA,
            damping: DEFAULT_TF_DAMPING,
            max_iters: DEFAULT_MAX_ITERS,
            history: DEFAULT_HISTORY,
            scale: DEFAULT_SCALE,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TrendFilterModel {
    trend: Vec<f64>,
    last_date: NaiveDate,
    scale: f64,
    resid_std: f64,
    slope_std: f64,
    damping: f64,
}

#[derive(Debug, Deserialize)]
struct ForecastRow {
    date: String,
    mean: f64,
    lower: f64,
    upper: f64,
    variance: f64,
}

pub fn write_forecast_csv(forecast: &Forecast, output_path: &Path) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let mut writer = csv::Writer::from_path(output_path)?;
    writer.write_record(["date", "mean", "lower", "upper", "variance"])?;

    for idx in 0..forecast.dates.len() {
        let date = forecast.dates[idx].format(DATE_FORMAT).to_string();
        writer.write_record([
            date,
            format!("{:.6}", forecast.mean[idx]),
            format!("{:.6}", forecast.lower[idx]),
            format!("{:.6}", forecast.upper[idx]),
            format!("{:.6}", forecast.variance[idx]),
        ])?;
    }

    writer.flush()?;
    Ok(())
}

pub fn read_forecast_csv(input_path: &Path) -> Result<Forecast, Box<dyn Error>> {
    let mut reader = csv::Reader::from_path(input_path)?;
    let mut dates = Vec::new();
    let mut mean = Vec::new();
    let mut lower = Vec::new();
    let mut upper = Vec::new();
    let mut variance = Vec::new();

    for row in reader.deserialize::<ForecastRow>() {
        let row = row?;
        let date = NaiveDate::parse_from_str(&row.date, DATE_FORMAT)?;
        dates.push(date);
        mean.push(row.mean);
        lower.push(row.lower);
        upper.push(row.upper);
        variance.push(row.variance);
    }

    if dates.is_empty() {
        return Err("forecast CSV is empty".into());
    }

    Ok(Forecast {
        dates,
        mean,
        lower,
        upper,
        variance,
    })
}

pub fn train_from_csv(csv_path: &Path, config: ModelConfig) -> Result<FittedModel, Box<dyn Error>> {
    let (dates, values) = load_target_series(csv_path)?;
    train_from_series(&dates, &values, config)
}

pub fn train_from_buckets(
    buckets: &AreaBuckets,
    config: ModelConfig,
) -> Result<FittedModel, Box<dyn Error>> {
    let (dates, values) = load_target_series_from_buckets(buckets)?;
    train_from_series(&dates, &values, config)
}

pub fn train_trend_filter_from_csv(
    csv_path: &Path,
    config: TrendFilterConfig,
) -> Result<TrendFilterModel, Box<dyn Error>> {
    let (dates, values) = load_target_series(csv_path)?;
    train_trend_filter_from_series(&dates, &values, config)
}

pub fn train_trend_filter_from_buckets(
    buckets: &AreaBuckets,
    config: TrendFilterConfig,
) -> Result<TrendFilterModel, Box<dyn Error>> {
    let (dates, values) = load_target_series_from_buckets(buckets)?;
    train_trend_filter_from_series(&dates, &values, config)
}

pub fn train_from_series(
    dates: &[NaiveDate],
    values: &[f64],
    config: ModelConfig,
) -> Result<FittedModel, Box<dyn Error>> {
    if dates.is_empty() || values.is_empty() || dates.len() != values.len() {
        return Err("dates/values must be non-empty and the same length".into());
    }
    if values.len() < 2 {
        return Err("need at least 2 observations".into());
    }

    let scale = if config.scale > 0.0 {
        config.scale
    } else {
        DEFAULT_SCALE
    };
    let series: Vec<f64> = values.iter().map(|v| v / scale).collect();
    let init = initial_params(&series);
    let weights = volatility_weights(&series);
    let last_weight = weights.last().copied().unwrap_or(1.0);
    let problem = LocalLinearTrendProblem::new(series, weights);
    let linesearch = MoreThuenteLineSearch::new().with_c(1e-4, 0.9)?;
    let solver = LBFGS::new(linesearch, config.history)
        .with_tolerance_grad(DEFAULT_TOL_GRAD)?
        .with_tolerance_cost(DEFAULT_TOL_COST)?;

    let result = Executor::new(problem, solver)
        .configure(|state| state.param(init).max_iters(config.max_iters))
        .run()?;

    let best = result
        .state
        .get_param()
        .ok_or("no parameters returned from optimizer")?
        .clone();
    let (sigma_level, sigma_trend, sigma_obs) = unpack_params(&best);
    let filter_series: Vec<f64> = values.iter().map(|v| v / scale).collect();
    let filter_weights = volatility_weights(&filter_series);
    let filter = kalman_filter(
        &filter_series,
        sigma_level,
        sigma_trend,
        sigma_obs,
        Some(&filter_weights),
    );
    let last_date = *dates.last().expect("non-empty dates");

    Ok(FittedModel {
        sigma_level,
        sigma_trend,
        sigma_obs,
        state: filter.state,
        cov: filter.cov,
        last_date,
        scale,
        nll: filter.nll,
        last_weight,
    })
}

pub fn train_trend_filter_from_series(
    dates: &[NaiveDate],
    values: &[f64],
    config: TrendFilterConfig,
) -> Result<TrendFilterModel, Box<dyn Error>> {
    if dates.is_empty() || values.is_empty() || dates.len() != values.len() {
        return Err("dates/values must be non-empty and the same length".into());
    }
    if values.len() < 3 {
        return Err("need at least 3 observations".into());
    }

    let scale = if config.scale > 0.0 {
        config.scale
    } else {
        DEFAULT_SCALE
    };
    let series: Vec<f64> = values.iter().map(|v| v / scale).collect();

    let lambda = config.lambda.max(0.0);
    let epsilon = config.epsilon.max(1e-9);
    let diffs: Vec<f64> = series.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let diff_std = stddev(&diffs);
    let huber_delta = if config.huber_delta > 0.0 {
        config.huber_delta
    } else {
        (1.5 * diff_std).max(1e-3)
    };

    let problem = TrendFilterProblem::new(series.clone(), lambda, epsilon, huber_delta);
    let init = series.clone();
    let linesearch = MoreThuenteLineSearch::new().with_c(1e-4, 0.9)?;
    let solver = LBFGS::new(linesearch, config.history)
        .with_tolerance_grad(DEFAULT_TOL_GRAD)?
        .with_tolerance_cost(DEFAULT_TOL_COST)?;

    let result = Executor::new(problem, solver)
        .configure(|state| state.param(init).max_iters(config.max_iters))
        .run()?;

    let trend = result
        .state
        .get_param()
        .ok_or("no parameters returned from optimizer")?
        .clone();

    let residuals: Vec<f64> = series
        .iter()
        .zip(trend.iter())
        .map(|(y, f)| y - f)
        .collect();
    let mut resid_std = stddev(&residuals);
    let floor_resid = (0.25 * diff_std).max(1e-3);
    if !resid_std.is_finite() || resid_std < floor_resid {
        resid_std = floor_resid;
    }

    let slopes: Vec<f64> = trend.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let mut slope_std = stddev(&slopes);
    let floor_slope = (0.10 * diff_std).max(1e-4);
    if !slope_std.is_finite() || slope_std < floor_slope {
        slope_std = floor_slope;
    }
    let last_date = *dates.last().expect("non-empty dates");

    Ok(TrendFilterModel {
        trend,
        last_date,
        scale,
        resid_std,
        slope_std,
        damping: config.damping,
    })
}

impl FittedModel {
    pub fn forecast(&self, horizon_days: usize) -> Forecast {
        let mut dates = Vec::with_capacity(horizon_days);
        let mut mean = Vec::with_capacity(horizon_days);
        let mut lower = Vec::with_capacity(horizon_days);
        let mut upper = Vec::with_capacity(horizon_days);
        let mut variance = Vec::with_capacity(horizon_days);

        let weight = self.last_weight.clamp(HETERO_MIN_SCALE, HETERO_MAX_SCALE);
        let q_level = self.sigma_level * self.sigma_level * weight * weight;
        let q_trend = self.sigma_trend * self.sigma_trend * weight * weight;
        let r = self.sigma_obs * self.sigma_obs * weight * weight;

        let mut state = self.state;
        let mut cov = self.cov;

        for step in 1..=horizon_days {
            predict_state(&mut state, &mut cov, q_level, q_trend);
            let y_mean = state[0];
            let y_var = (cov[0][0] + r).max(0.0);
            let std = y_var.sqrt();

            let day = self.last_date + Duration::days(step as i64);
            dates.push(day);
            mean.push(y_mean * self.scale);
            variance.push(y_var * self.scale * self.scale);
            lower.push(CONFIDENCE_Z.mul_add(-std, y_mean) * self.scale);
            upper.push(CONFIDENCE_Z.mul_add(std, y_mean) * self.scale);
        }

        Forecast {
            dates,
            mean,
            lower,
            upper,
            variance,
        }
    }
}

impl TrendFilterModel {
    pub fn forecast(&self, horizon_days: usize) -> Forecast {
        let mut dates = Vec::with_capacity(horizon_days);
        let mut mean = Vec::with_capacity(horizon_days);
        let mut lower = Vec::with_capacity(horizon_days);
        let mut upper = Vec::with_capacity(horizon_days);
        let mut variance = Vec::with_capacity(horizon_days);

        let last = self.trend.last().copied().unwrap_or(0.0);
        let slope = if self.trend.len() > 1 {
            last - self.trend[self.trend.len() - 2]
        } else {
            0.0
        };
        let resid_var = self.resid_std * self.resid_std;
        let slope_var = self.slope_std * self.slope_std;
        let phi = self.damping.clamp(0.0, 1.0);

        for step in 1..=horizon_days {
            let step_f = step as f64;
            let sum_phi = if (phi - 1.0).abs() < 1e-12 {
                step_f
            } else {
                (1.0 - phi.powf(step_f)) / (1.0 - phi)
            };
            let mean_scaled = last + slope * sum_phi;
            let day = self.last_date + Duration::days(step as i64);
            let var = (sum_phi * sum_phi).mul_add(slope_var, resid_var).max(0.0);
            let std = var.sqrt();

            dates.push(day);
            mean.push(mean_scaled * self.scale);
            variance.push(var * self.scale * self.scale);
            lower.push(CONFIDENCE_Z.mul_add(-std, mean_scaled) * self.scale);
            upper.push(CONFIDENCE_Z.mul_add(std, mean_scaled) * self.scale);
        }

        Forecast {
            dates,
            mean,
            lower,
            upper,
            variance,
        }
    }
}

#[derive(Clone)]
struct TrendFilterProblem {
    y: Vec<f64>,
    lambda: f64,
    epsilon: f64,
    huber_delta: f64,
}

impl TrendFilterProblem {
    const fn new(y: Vec<f64>, lambda: f64, epsilon: f64, huber_delta: f64) -> Self {
        Self {
            y,
            lambda,
            epsilon,
            huber_delta,
        }
    }
}

impl CostFunction for TrendFilterProblem {
    type Param = Vec<f64>;
    type Output = f64;

    fn cost(&self, param: &Self::Param) -> Result<Self::Output, ArgminError> {
        if param.len() != self.y.len() {
            return Ok(LARGE_COST);
        }
        let mut cost = 0.0;
        let delta = self.huber_delta.max(1e-6);
        for (value, target) in param.iter().zip(self.y.iter()) {
            let diff = value - target;
            cost += huber_loss(diff, delta);
        }
        if param.len() >= 3 && self.lambda > 0.0 {
            let eps2 = self.epsilon * self.epsilon;
            for idx in 2..param.len() {
                let d2 = 2.0f64.mul_add(-param[idx - 1], param[idx]) + param[idx - 2];
                cost += self.lambda * d2.mul_add(d2, eps2).sqrt();
            }
        }
        Ok(cost)
    }
}

impl Gradient for TrendFilterProblem {
    type Param = Vec<f64>;
    type Gradient = Vec<f64>;

    fn gradient(&self, param: &Self::Param) -> Result<Self::Gradient, ArgminError> {
        if param.len() != self.y.len() {
            return Ok(vec![0.0; param.len()]);
        }
        let mut grad = vec![0.0; param.len()];
        let delta = self.huber_delta.max(1e-6);
        for (idx, (value, target)) in param.iter().zip(self.y.iter()).enumerate() {
            grad[idx] += huber_grad(value - target, delta);
        }
        if param.len() >= 3 && self.lambda > 0.0 {
            let eps2 = self.epsilon * self.epsilon;
            for idx in 2..param.len() {
                let d2 = 2.0f64.mul_add(-param[idx - 1], param[idx]) + param[idx - 2];
                let denom = d2.mul_add(d2, eps2).sqrt();
                let g = if denom > 0.0 {
                    self.lambda * d2 / denom
                } else {
                    0.0
                };
                grad[idx] += g;
                grad[idx - 1] += -2.0 * g;
                grad[idx - 2] += g;
            }
        }
        Ok(grad)
    }
}

#[derive(Clone)]
struct LocalLinearTrendProblem {
    y: Vec<f64>,
    weights: Vec<f64>,
}

impl LocalLinearTrendProblem {
    const fn new(y: Vec<f64>, weights: Vec<f64>) -> Self {
        Self { y, weights }
    }

    fn nll(&self, param: &[f64]) -> f64 {
        if param.len() != 3 {
            return LARGE_COST;
        }
        let (sigma_level, sigma_trend, sigma_obs) = unpack_params(param);
        if !sigma_level.is_finite() || !sigma_trend.is_finite() || !sigma_obs.is_finite() {
            return LARGE_COST;
        }
        let result = kalman_filter(
            &self.y,
            sigma_level,
            sigma_trend,
            sigma_obs,
            Some(&self.weights),
        );
        if result.nll.is_finite() {
            result.nll
        } else {
            LARGE_COST
        }
    }
}

impl CostFunction for LocalLinearTrendProblem {
    type Param = Vec<f64>;
    type Output = f64;

    fn cost(&self, param: &Self::Param) -> Result<Self::Output, ArgminError> {
        Ok(self.nll(param))
    }
}

impl Gradient for LocalLinearTrendProblem {
    type Param = Vec<f64>;
    type Gradient = Vec<f64>;

    fn gradient(&self, param: &Self::Param) -> Result<Self::Gradient, ArgminError> {
        let mut grad = vec![0.0; param.len()];
        for i in 0..param.len() {
            let step = 1e-4 * (1.0 + param[i].abs());
            let mut plus = param.clone();
            let mut minus = param.clone();
            plus[i] += step;
            minus[i] -= step;
            let f_plus = self.nll(&plus);
            let f_minus = self.nll(&minus);
            grad[i] = (f_plus - f_minus) / (2.0 * step);
        }
        Ok(grad)
    }
}

struct FilterResult {
    nll: f64,
    state: [f64; 2],
    cov: [[f64; 2]; 2],
}

fn kalman_filter(
    y: &[f64],
    sigma_level: f64,
    sigma_trend: f64,
    sigma_obs: f64,
    weights: Option<&[f64]>,
) -> FilterResult {
    if y.is_empty() {
        return FilterResult {
            nll: LARGE_COST,
            state: [0.0, 0.0],
            cov: [[0.0, 0.0], [0.0, 0.0]],
        };
    }

    let sigma_level = sigma_level.max(MIN_SIGMA);
    let sigma_trend = sigma_trend.max(MIN_SIGMA);
    let sigma_obs = sigma_obs.max(MIN_SIGMA);

    let q_level = sigma_level * sigma_level;
    let q_trend = sigma_trend * sigma_trend;
    let r = sigma_obs * sigma_obs;

    let mut state = [y[0], 0.0];
    let mut cov = [[1.0e4, 0.0], [0.0, 1.0e2]];
    let mut nll = 0.0;

    for (idx, &obs) in y.iter().enumerate() {
        let weight = weights
            .and_then(|series| series.get(idx).copied())
            .unwrap_or(1.0)
            .clamp(HETERO_MIN_SCALE, HETERO_MAX_SCALE);
        let q_level_t = q_level * weight * weight;
        let q_trend_t = q_trend * weight * weight;
        let r_t = r * weight * weight;

        predict_state(&mut state, &mut cov, q_level_t, q_trend_t);

        let y_pred = state[0];
        let innovation = obs - y_pred;
        let s = cov[0][0] + r_t;
        if s <= 0.0 || !s.is_finite() {
            return FilterResult {
                nll: LARGE_COST,
                state,
                cov,
            };
        }

        let k0 = cov[0][0] / s;
        let k1 = cov[1][0] / s;

        state[0] += k0 * innovation;
        state[1] += k1 * innovation;

        let p00 = cov[0][0];
        let p01 = cov[0][1];
        let p11 = cov[1][1];

        cov[0][0] = (1.0 - k0) * p00;
        cov[0][1] = (1.0 - k0) * p01;
        cov[1][0] = cov[0][1];
        cov[1][1] = p11 - k1 * p01;

        nll += 0.5 * ((2.0 * std::f64::consts::PI * s).ln() + (innovation * innovation) / s);
    }

    FilterResult { nll, state, cov }
}

fn predict_state(state: &mut [f64; 2], cov: &mut [[f64; 2]; 2], q_level: f64, q_trend: f64) {
    state[0] += state[1];

    let p00 = cov[0][0];
    let p01 = cov[0][1];
    let p11 = cov[1][1];

    let p00_pred = 2.0f64.mul_add(p01, p00) + p11 + q_level;
    let p01_pred = p01 + p11;
    let p11_pred = p11 + q_trend;

    cov[0][0] = p00_pred;
    cov[0][1] = p01_pred;
    cov[1][0] = p01_pred;
    cov[1][1] = p11_pred;
}

fn unpack_params(param: &[f64]) -> (f64, f64, f64) {
    if param.len() < 3 {
        return (MIN_SIGMA, MIN_SIGMA, MIN_SIGMA);
    }
    let sigma_level = param[0].exp().max(MIN_SIGMA);
    let sigma_trend = param[1].exp().max(MIN_SIGMA);
    let sigma_obs = param[2].exp().max(MIN_SIGMA);
    (sigma_level, sigma_trend, sigma_obs)
}

fn initial_params(series: &[f64]) -> Vec<f64> {
    let diffs: Vec<f64> = series.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let diff_std = stddev(&diffs).max(1e-3);
    let sigma_obs = diff_std;
    let sigma_level = diff_std * 0.5;
    let sigma_trend = diff_std * 0.1;
    vec![sigma_level.ln(), sigma_trend.ln(), sigma_obs.ln()]
}

fn stddev(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 1.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let var = values
        .iter()
        .map(|value| {
            let diff = value - mean;
            diff * diff
        })
        .sum::<f64>()
        / values.len() as f64;
    var.sqrt().max(1e-6)
}

fn huber_loss(diff: f64, delta: f64) -> f64 {
    let abs = diff.abs();
    if abs <= delta {
        0.5 * diff * diff
    } else {
        delta * 0.5f64.mul_add(-delta, abs)
    }
}

fn huber_grad(diff: f64, delta: f64) -> f64 {
    let abs = diff.abs();
    if abs <= delta {
        diff
    } else if diff > 0.0 {
        delta
    } else {
        -delta
    }
}

fn volatility_weights(series: &[f64]) -> Vec<f64> {
    let n = series.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![1.0];
    }

    let mut diffs = Vec::with_capacity(n);
    diffs.push(0.0);
    for idx in 1..n {
        diffs.push((series[idx] - series[idx - 1]).abs());
    }

    let mut vol = vec![0.0; n];
    for idx in 1..n {
        let start = idx.saturating_sub(HETERO_WINDOW - 1).max(1);
        let mut sum = 0.0;
        let mut count = 0;
        for j in start..=idx {
            sum += diffs[j];
            count += 1;
        }
        vol[idx] = if count > 0 { sum / count as f64 } else { 0.0 };
    }
    vol[0] = vol[1];

    let mut sample = vol[1..].to_vec();
    let median = if sample.is_empty() {
        1.0
    } else {
        let mid = sample.len() / 2;
        sample.select_nth_unstable_by(mid, |a, b| {
            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
        });
        sample[mid]
    };

    if !median.is_finite() || median <= 1e-9 {
        return vec![1.0; n];
    }

    vol.into_iter()
        .map(|v| {
            let ratio = v / median;
            ratio.clamp(HETERO_MIN_SCALE, HETERO_MAX_SCALE)
        })
        .collect()
}

fn load_target_series(csv_path: &Path) -> Result<(Vec<NaiveDate>, Vec<f64>), Box<dyn Error>> {
    let buckets = load_area_buckets(csv_path)?;
    load_target_series_from_buckets(&buckets)
}

fn load_target_series_from_buckets(
    buckets: &AreaBuckets,
) -> Result<(Vec<NaiveDate>, Vec<f64>), Box<dyn Error>> {
    let (dates, values) = build_occupied_series(buckets)?;
    let cutoff = NaiveDate::from_ymd_opt(TRAINING_START.0, TRAINING_START.1, TRAINING_START.2)
        .ok_or("invalid training start date")?;
    let mut filtered_dates = Vec::with_capacity(dates.len());
    let mut filtered_values = Vec::with_capacity(values.len());
    for (date, value) in dates.into_iter().zip(values.into_iter()) {
        if date >= cutoff {
            filtered_dates.push(date);
            filtered_values.push(value);
        }
    }
    if filtered_dates.is_empty() {
        return Err(format!("no training data after {}", cutoff.format(DATE_FORMAT)).into());
    }
    Ok((filtered_dates, filtered_values))
}
