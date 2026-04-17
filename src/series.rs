//! Общие функции для загрузки CSV и построения временного ряда занятых территорий.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Deserializer};

const AREA_TYPE_OCCUPIED: &str = "occupied_after_24_02_2022";
const AREA_TYPE_OTHER: &str = "other_territories";
const AREA_TYPE_UNSPECIFIED: &str = "unspecified";
const UA_HASH: &str = "#01579b";

const TIME_FORMAT_TZ: &str = "%Y-%m-%d %H:%M:%S %Z";
const TIME_FORMAT_OFFSET: &str = "%Y-%m-%d %H:%M:%S %z";
const TIME_FORMAT_NAIVE: &str = "%Y-%m-%d %H:%M:%S";
const UTC_SUFFIX: &str = " UTC";

const ERROR_EMPTY_TIME_INDEX: &str = "empty time index";
const ERROR_UNRECOGNIZED_TIME: &str = "unrecognized time format";
const ERROR_NO_DATA: &str = "No data available";

type DailyBuckets = BTreeMap<NaiveDate, DailyAccumulator>;

#[derive(Clone, Copy, Debug)]
enum TimeFormatHint {
    Tz,
    Offset,
    Rfc3339,
    NaiveUtcSuffix,
    Naive,
}

impl TimeFormatHint {
    fn parse(self, raw: &str) -> Option<DateTime<Utc>> {
        match self {
            Self::Tz => DateTime::parse_from_str(raw, TIME_FORMAT_TZ)
                .ok()
                .map(|dt| dt.with_timezone(&Utc)),
            Self::Offset => DateTime::parse_from_str(raw, TIME_FORMAT_OFFSET)
                .ok()
                .map(|dt| dt.with_timezone(&Utc)),
            Self::Rfc3339 => DateTime::parse_from_rfc3339(raw)
                .ok()
                .map(|dt| dt.with_timezone(&Utc)),
            Self::NaiveUtcSuffix => raw
                .strip_suffix(UTC_SUFFIX)
                .and_then(|stripped| {
                    NaiveDateTime::parse_from_str(stripped.trim_end(), TIME_FORMAT_NAIVE).ok()
                })
                .map(|naive| Utc.from_utc_datetime(&naive)),
            Self::Naive => NaiveDateTime::parse_from_str(raw, TIME_FORMAT_NAIVE)
                .ok()
                .map(|naive| Utc.from_utc_datetime(&naive)),
        }
    }
}

/// Строка из CSV-экспорта API (поля используются выборочно).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AreaKind {
    RuOccupied,
    UaOtherTerritories,
    Unspecified,
    Other,
}

fn deserialize_area_kind<'de, D>(deserializer: D) -> Result<AreaKind, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    Ok(match raw.as_str() {
        AREA_TYPE_OCCUPIED => AreaKind::RuOccupied,
        AREA_TYPE_OTHER => AreaKind::UaOtherTerritories,
        AREA_TYPE_UNSPECIFIED => AreaKind::Unspecified,
        _ => AreaKind::Other,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HashKind {
    Ua,
    Other,
}

fn deserialize_hash_kind<'de, D>(deserializer: D) -> Result<HashKind, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    Ok(if raw == UA_HASH {
        HashKind::Ua
    } else {
        HashKind::Other
    })
}

#[derive(Debug, Deserialize)]
struct CsvRow {
    time_index: String,
    #[serde(rename = "hash", deserialize_with = "deserialize_hash_kind")]
    hash_kind: HashKind,
    area: f64,
    #[allow(dead_code)]
    percent: f64,
    #[serde(
        alias = "type",
        alias = "area_type",
        deserialize_with = "deserialize_area_kind"
    )]
    area_kind: AreaKind,
}

/// Аккумулятор для усреднения значений внутри одного дня.
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
        (self.count > 0).then_some(self.sum / f64::from(self.count))
    }
}

/// Дневные бакеты, сгруппированные по категориям площадей.
#[derive(Default)]
pub struct AreaBuckets {
    ru: DailyBuckets,
    ua: DailyBuckets,
    unspecified: DailyBuckets,
}

impl AreaBuckets {
    /// Добавляет строку CSV в соответствующий дневной бакет.
    fn ingest_with_date(&mut self, row: &CsvRow, date: NaiveDate) {
        match (row.area_kind, row.hash_kind) {
            (AreaKind::RuOccupied, _) => {
                self.ru.entry(date).or_default().add(row.area);
            }
            (AreaKind::UaOtherTerritories, HashKind::Ua) => {
                self.ua.entry(date).or_default().add(row.area);
            }
            (AreaKind::Unspecified, _) => {
                self.unspecified.entry(date).or_default().add(row.area);
            }
            _ => {} // Игнорируем прочие категории.
        }
    }
}

/// Непрерывные дневные ряды для занятых территорий и слоя `unspecified`.
#[derive(Clone, Debug)]
pub struct OccupiedUnspecifiedSeries {
    pub dates: Vec<NaiveDate>,
    pub occupied: Vec<f64>,
    pub unspecified: Vec<f64>,
}

/// Читает CSV и раскладывает значения по дневным бакетам.
pub fn load_area_buckets(csv_path: &Path) -> Result<AreaBuckets, Box<dyn Error>> {
    let mut reader = csv::Reader::from_path(csv_path)?;
    let mut hint = None;
    reader
        .deserialize::<CsvRow>()
        .try_fold(AreaBuckets::default(), |mut acc, row| {
            let row = row?;
            let datetime = parse_time_index_with_hint(&row.time_index, &mut hint)
                .map_err(|err| format!("failed to parse time_index '{}': {err}", row.time_index))?;
            acc.ingest_with_date(&row, datetime.date_naive());
            Ok::<_, Box<dyn Error>>(acc)
        })
}

/// Строит непрерывный ряд занятых территорий, вычитая и интерполируя RU/UA.
pub fn build_occupied_series(
    buckets: &AreaBuckets,
) -> Result<(Vec<NaiveDate>, Vec<f64>), Box<dyn Error>> {
    let series = build_occupied_and_unspecified_series(buckets)?;
    Ok((series.dates, series.occupied))
}

/// Строит непрерывные ряды `occupied` и `unspecified` на общей шкале дат.
pub fn build_occupied_and_unspecified_series(
    buckets: &AreaBuckets,
) -> Result<OccupiedUnspecifiedSeries, Box<dyn Error>> {
    let first_date = buckets
        .ru
        .keys()
        .chain(buckets.ua.keys())
        .chain(buckets.unspecified.keys())
        .min()
        .copied()
        .ok_or_else(|| ERROR_NO_DATA.to_string())?;
    let last_date = buckets
        .ru
        .keys()
        .chain(buckets.ua.keys())
        .chain(buckets.unspecified.keys())
        .max()
        .copied()
        .ok_or_else(|| ERROR_NO_DATA.to_string())?;

    let span_days = (last_date - first_date).num_days();
    if span_days < 0 {
        tracing::warn!(
            first_date = %first_date,
            last_date = %last_date,
            span_days,
            "Negative date span while building series; returning error",
        );
        return Err("negative date span".into());
    }
    let dates: Vec<_> = (0..=span_days)
        .map(|offset| first_date + Duration::days(offset))
        .collect();

    let ru_values = interpolate_series(&dates, &buckets.ru);
    let ua_values = interpolate_series(&dates, &buckets.ua);
    let unspecified_values = interpolate_series(&dates, &buckets.unspecified);
    let occupied_area = ru_values
        .iter()
        .zip(ua_values)
        .map(|(ru, ua)| ru - ua)
        .collect();

    Ok(OccupiedUnspecifiedSeries {
        dates,
        occupied: occupied_area,
        unspecified: unspecified_values,
    })
}

/// Парсит разные форматы времени из API и приводит их к UTC.
fn parse_time_index_with_hint(
    raw: &str,
    hint: &mut Option<TimeFormatHint>,
) -> Result<DateTime<Utc>, String> {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return Err(ERROR_EMPTY_TIME_INDEX.into());
    }

    if let Some(hint) = *hint
        && let Some(parsed) = hint.parse(trimmed)
    {
        return Ok(parsed);
    }

    if let Ok(dt) = DateTime::parse_from_str(trimmed, TIME_FORMAT_TZ) {
        *hint = Some(TimeFormatHint::Tz);
        return Ok(dt.with_timezone(&Utc));
    }

    if let Ok(dt) = DateTime::parse_from_str(trimmed, TIME_FORMAT_OFFSET) {
        *hint = Some(TimeFormatHint::Offset);
        return Ok(dt.with_timezone(&Utc));
    }

    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        *hint = Some(TimeFormatHint::Rfc3339);
        return Ok(dt.with_timezone(&Utc));
    }

    if let Some(stripped) = trimmed.strip_suffix(UTC_SUFFIX)
        && let Ok(naive) = NaiveDateTime::parse_from_str(stripped.trim_end(), TIME_FORMAT_NAIVE)
    {
        *hint = Some(TimeFormatHint::NaiveUtcSuffix);
        return Ok(Utc.from_utc_datetime(&naive));
    }

    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, TIME_FORMAT_NAIVE) {
        *hint = Some(TimeFormatHint::Naive);
        return Ok(Utc.from_utc_datetime(&naive));
    }

    Err(ERROR_UNRECOGNIZED_TIME.into())
}

/// Линейно интерполирует пропуски по дням и делает ряд непрерывным.
fn interpolate_series(dates: &[NaiveDate], source: &DailyBuckets) -> Vec<f64> {
    let mut values: Vec<Option<f64>> = dates
        .iter()
        .map(|date| source.get(date).and_then(DailyAccumulator::mean))
        .collect();

    // Заполняем пропуски интерполяцией между известными точками.
    let mut last_known = None;
    for idx in 0..values.len() {
        if values[idx].is_some() {
            if let Some(start) = last_known {
                if let (Some(start_val), Some(end_val)) = (values[start], values[idx]) {
                    let gap = idx - start;
                    if gap > 1 {
                        for (offset, slot) in ((start + 1)..idx).enumerate() {
                            let numerator: f64 = u32::try_from(offset + 1).map_or_else(
                                |_| {
                                    tracing::warn!(
                                        offset = offset + 1,
                                        "Interpolation offset exceeds u32::MAX; clamping ratio",
                                    );
                                    f64::from(u32::MAX)
                                },
                                f64::from,
                            );
                            let denominator: f64 = u32::try_from(gap).map_or_else(
                                |_| {
                                    tracing::warn!(
                                        gap,
                                        "Interpolation gap exceeds u32::MAX; clamping ratio",
                                    );
                                    f64::from(u32::MAX)
                                },
                                f64::from,
                            );
                            let ratio = numerator / denominator;
                            values[slot] =
                                Some(f64::mul_add(end_val - start_val, ratio, start_val));
                        }
                    }
                }
            } else {
                // В начале ряда тянем первое известное значение назад.
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

#[cfg(test)]
mod tests {
    use super::{build_occupied_and_unspecified_series, build_occupied_series, load_area_buckets};
    use chrono::NaiveDate;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    const EPS: f64 = 1e-9;

    fn write_temp_csv(contents: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before UNIX_EPOCH")
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!("rua_series_test_{timestamp}_{counter}.csv"));
        std::fs::write(&path, contents).expect("failed to write test csv");
        path
    }

    fn remove_temp_csv(path: &Path) {
        let _ = std::fs::remove_file(path);
    }

    fn assert_vec_close(actual: &[f64], expected: &[f64]) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "series length mismatch: actual={} expected={}",
            actual.len(),
            expected.len()
        );
        for (idx, (actual_value, expected_value)) in actual.iter().zip(expected).enumerate() {
            assert!(
                (actual_value - expected_value).abs() < EPS,
                "mismatch at index {idx}: actual={actual_value}, expected={expected_value}"
            );
        }
    }

    #[test]
    fn load_area_buckets_parses_unspecified_independent_of_hash() {
        let csv = "time_index,hash,area,percent,area_type\n\
2024-02-01 00:00:00 UTC,#01579b,12.5,0.0,unspecified\n\
2024-02-01 00:00:00 UTC,#a52714,100.0,0.0,occupied_after_24_02_2022\n\
2024-02-01 00:00:00 UTC,#01579b,20.0,0.0,other_territories\n";
        let path = write_temp_csv(csv);
        let buckets = load_area_buckets(&path).expect("failed to load area buckets");
        remove_temp_csv(&path);

        let date = NaiveDate::from_ymd_opt(2024, 2, 1).expect("valid date");
        let unspecified = buckets
            .unspecified
            .get(&date)
            .and_then(super::DailyAccumulator::mean)
            .expect("missing unspecified bucket");
        assert!(
            (unspecified - 12.5).abs() < EPS,
            "unexpected unspecified value: {unspecified}"
        );
    }

    #[test]
    fn build_occupied_series_ignores_unspecified_values() {
        let csv = "time_index,hash,area,percent,area_type\n\
2024-03-01 00:00:00 UTC,#a52714,100.0,0.0,occupied_after_24_02_2022\n\
2024-03-01 00:00:00 UTC,#01579b,20.0,0.0,other_territories\n\
2024-03-01 00:00:00 UTC,#bcaaa4,30.0,0.0,unspecified\n\
2024-03-02 00:00:00 UTC,#a52714,110.0,0.0,occupied_after_24_02_2022\n\
2024-03-02 00:00:00 UTC,#01579b,25.0,0.0,other_territories\n\
2024-03-02 00:00:00 UTC,#bcaaa4,40.0,0.0,unspecified\n";
        let path = write_temp_csv(csv);
        let buckets = load_area_buckets(&path).expect("failed to load area buckets");
        remove_temp_csv(&path);

        let (dates, occupied) = build_occupied_series(&buckets).expect("failed to build series");
        assert_eq!(
            dates,
            vec![
                NaiveDate::from_ymd_opt(2024, 3, 1).expect("valid date"),
                NaiveDate::from_ymd_opt(2024, 3, 2).expect("valid date"),
            ]
        );
        assert_vec_close(&occupied, &[80.0, 85.0]);
    }

    #[test]
    fn build_occupied_and_unspecified_series_uses_common_interpolated_dates() {
        let csv = "time_index,hash,area,percent,area_type\n\
2024-04-01 00:00:00 UTC,#a52714,100.0,0.0,occupied_after_24_02_2022\n\
2024-04-01 00:00:00 UTC,#01579b,20.0,0.0,other_territories\n\
2024-04-01 00:00:00 UTC,#bcaaa4,10.0,0.0,unspecified\n\
2024-04-03 00:00:00 UTC,#a52714,160.0,0.0,occupied_after_24_02_2022\n\
2024-04-03 00:00:00 UTC,#01579b,50.0,0.0,other_territories\n\
2024-04-03 00:00:00 UTC,#bcaaa4,40.0,0.0,unspecified\n";
        let path = write_temp_csv(csv);
        let buckets = load_area_buckets(&path).expect("failed to load area buckets");
        remove_temp_csv(&path);

        let series = build_occupied_and_unspecified_series(&buckets)
            .expect("failed to build occupied/unspecified series");
        assert_eq!(
            series.dates,
            vec![
                NaiveDate::from_ymd_opt(2024, 4, 1).expect("valid date"),
                NaiveDate::from_ymd_opt(2024, 4, 2).expect("valid date"),
                NaiveDate::from_ymd_opt(2024, 4, 3).expect("valid date"),
            ]
        );
        assert_vec_close(&series.occupied, &[80.0, 95.0, 110.0]);
        assert_vec_close(&series.unspecified, &[10.0, 25.0, 40.0]);

        let upper = series
            .occupied
            .iter()
            .zip(&series.unspecified)
            .map(|(occupied, unspecified)| occupied + unspecified)
            .collect::<Vec<_>>();
        assert_vec_close(&upper, &[90.0, 120.0, 150.0]);
    }
}
