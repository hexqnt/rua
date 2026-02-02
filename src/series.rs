//! Общие функции для загрузки CSV и построения временного ряда занятых территорий.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;

const AREA_TYPE_OCCUPIED: &str = "occupied_after_24_02_2022";
const AREA_TYPE_OTHER: &str = "other_territories";
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
#[derive(Debug, Deserialize)]
struct CsvRow {
    time_index: String,
    hash: String,
    area: f64,
    percent: f64,
    #[serde(alias = "type", alias = "area_type")]
    area_type: String,
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
        (self.count > 0).then_some(self.sum / self.count as f64)
    }
}

/// Дневные бакеты, сгруппированные по категориям площадей.
#[derive(Default)]
pub(crate) struct AreaBuckets {
    ru: DailyBuckets,
    ua: DailyBuckets,
}

impl AreaBuckets {
    /// Добавляет строку CSV в соответствующий дневной бакет.
    fn ingest_with_date(&mut self, row: CsvRow, date: NaiveDate) -> Result<(), Box<dyn Error>> {
        match row.area_type.as_str() {
            AREA_TYPE_OCCUPIED => {
                self.ru.entry(date).or_default().add(row.area);
            }
            AREA_TYPE_OTHER if row.hash == UA_HASH => {
                self.ua.entry(date).or_default().add(row.area);
            }
            _ => {} // Игнорируем прочие категории.
        }

        Ok(())
    }
}

/// Читает CSV и раскладывает значения по дневным бакетам.
pub(crate) fn load_area_buckets(csv_path: &Path) -> Result<AreaBuckets, Box<dyn Error>> {
    let mut reader = csv::Reader::from_path(csv_path)?;
    let mut hint = None;
    reader
        .deserialize::<CsvRow>()
        .try_fold(AreaBuckets::default(), |mut acc, row| {
            let row = row?;
            let datetime = parse_time_index_with_hint(&row.time_index, &mut hint)
                .map_err(|err| format!("failed to parse time_index '{}': {err}", row.time_index))?;
            acc.ingest_with_date(row, datetime.date_naive())?;
            Ok::<_, Box<dyn Error>>(acc)
        })
}

/// Строит непрерывный ряд занятых территорий, вычитая и интерполируя RU/UA.
pub(crate) fn build_occupied_series(
    buckets: &AreaBuckets,
) -> Result<(Vec<NaiveDate>, Vec<f64>), Box<dyn Error>> {
    let first_date = buckets
        .ru
        .keys()
        .chain(buckets.ua.keys())
        .min()
        .copied()
        .ok_or_else(|| ERROR_NO_DATA.to_string())?;
    let last_date = buckets
        .ru
        .keys()
        .chain(buckets.ua.keys())
        .max()
        .copied()
        .ok_or_else(|| ERROR_NO_DATA.to_string())?;

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

/// Парсит разные форматы времени из API и приводит их к UTC.
fn parse_time_index_with_hint(
    raw: &str,
    hint: &mut Option<TimeFormatHint>,
) -> Result<DateTime<Utc>, String> {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return Err(ERROR_EMPTY_TIME_INDEX.into());
    }

    if let Some(hint) = *hint {
        if let Some(parsed) = hint.parse(trimmed) {
            return Ok(parsed);
        }
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
                            let ratio = (offset as f64 + 1.0) / gap as f64;
                            values[slot] = Some(start_val + (end_val - start_val) * ratio);
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
