use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};

use super::error::{Error, InvalidTimestamp, Result};

/// Временная отметка среза `DeepStateMap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotTime(DateTime<Utc>);

impl SnapshotTime {
    /// Возвращает представление времени в UTC.
    #[must_use]
    pub const fn as_datetime(&self) -> &DateTime<Utc> {
        &self.0
    }

    /// Возвращает Unix timestamp, используемый API `DeepStateMap`.
    #[must_use]
    pub const fn unix_timestamp(self) -> i64 {
        self.0.timestamp()
    }
}

impl fmt::Display for SnapshotTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<DateTime<Utc>> for SnapshotTime {
    fn from(value: DateTime<Utc>) -> Self {
        Self(value)
    }
}

impl From<SnapshotTime> for DateTime<Utc> {
    fn from(value: SnapshotTime) -> Self {
        value.0
    }
}

impl TryFrom<i64> for SnapshotTime {
    type Error = InvalidTimestamp;

    fn try_from(value: i64) -> std::result::Result<Self, Self::Error> {
        DateTime::from_timestamp(value, 0)
            .map(Self)
            .ok_or_else(|| InvalidTimestamp::new(value))
    }
}

/// Площадь одного типа территории в срезе `DeepStateMap`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Area {
    hash: String,
    #[serde(rename = "area")]
    value: f64,
    #[serde(deserialize_with = "deserialize_percent")]
    percent: f64,
    #[serde(rename = "type", alias = "area_type")]
    kind: String,
}

impl Area {
    /// Возвращает цветовой идентификатор территории из API.
    #[must_use]
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// Возвращает площадь территории.
    #[must_use]
    pub const fn area(&self) -> f64 {
        self.value
    }

    /// Возвращает долю территории в процентах.
    #[must_use]
    pub const fn percent(&self) -> f64 {
        self.percent
    }

    /// Возвращает тип территории в исходном представлении API.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// Разбирает площадь на поля без копирования строк.
    #[must_use]
    pub fn into_parts(self) -> (String, f64, f64, String) {
        (self.hash, self.value, self.percent, self.kind)
    }
}

/// Полный набор площадей для одной временной отметки.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    timestamp: SnapshotTime,
    areas: Vec<Area>,
}

impl Snapshot {
    /// Создаёт срез из уже типизированных компонентов.
    #[must_use]
    pub const fn new(timestamp: SnapshotTime, areas: Vec<Area>) -> Self {
        Self { timestamp, areas }
    }

    /// Возвращает временную отметку среза.
    #[must_use]
    pub const fn timestamp(&self) -> SnapshotTime {
        self.timestamp
    }

    /// Возвращает площади, входящие в срез.
    #[must_use]
    pub fn areas(&self) -> &[Area] {
        &self.areas
    }

    /// Разбирает срез на временную отметку и площади без копирования.
    #[must_use]
    pub fn into_parts(self) -> (SnapshotTime, Vec<Area>) {
        (self.timestamp, self.areas)
    }
}

#[derive(Debug, Deserialize)]
struct TimestampItem {
    id: i64,
}

pub(super) fn parse_timestamps(bytes: &[u8]) -> Result<Vec<SnapshotTime>> {
    let items: Vec<TimestampItem> =
        serde_json::from_slice(bytes).map_err(Error::DecodeTimestamps)?;
    items
        .into_iter()
        .map(|item| SnapshotTime::try_from(item.id).map_err(Error::InvalidTimestamp))
        .collect()
}

pub(super) fn parse_snapshot(bytes: &[u8], timestamp: SnapshotTime) -> Result<Snapshot> {
    let areas = serde_json::from_slice(bytes)
        .map_err(|source| Error::DecodeSnapshot { timestamp, source })?;
    Ok(Snapshot::new(timestamp, areas))
}

/// Преобразует строковое значение процента из API в конечное число.
fn deserialize_percent<'de, D>(deserializer: D) -> std::result::Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = <&str>::deserialize(deserializer)?;
    let value = raw.parse::<f64>().map_err(serde::de::Error::custom)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(serde::de::Error::custom("percent must be finite"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMESTAMPS_JSON: &[u8] =
        include_bytes!("../../tests/fixtures/deepstatemap/timestamps.json");
    const AREAS_JSON: &[u8] = include_bytes!("../../tests/fixtures/deepstatemap/areas.json");

    #[test]
    fn parses_timestamps_from_api_fixture() {
        let timestamps = parse_timestamps(TIMESTAMPS_JSON).expect("fixture must be valid");

        assert_eq!(timestamps.len(), 2);
        assert_eq!(timestamps[0].unix_timestamp(), 1_648_989_208);
        assert_eq!(timestamps[1].unix_timestamp(), 1_649_004_375);
    }

    #[test]
    fn parses_snapshot_from_api_fixture() {
        let timestamp = SnapshotTime::try_from(1_648_989_208).expect("timestamp must be valid");
        let snapshot = parse_snapshot(AREAS_JSON, timestamp).expect("fixture must be valid");

        assert_eq!(snapshot.timestamp(), timestamp);
        assert_eq!(snapshot.areas().len(), 4);
        assert_eq!(snapshot.areas()[0].hash(), "#a52714");
        assert_eq!(snapshot.areas()[0].area(), 63_864.643_989_695_01);
        assert_eq!(snapshot.areas()[0].percent(), 10.579);
        assert_eq!(snapshot.areas()[0].kind(), "occupied_after_24_02_2022");
    }

    #[test]
    fn converts_snapshot_time_without_losing_precision() {
        let datetime = DateTime::from_timestamp(1_648_989_208, 0).expect("timestamp must be valid");
        let timestamp = SnapshotTime::from(datetime);

        assert_eq!(DateTime::<Utc>::from(timestamp), datetime);
    }

    #[test]
    fn area_can_be_consumed_without_cloning_strings() {
        let mut areas: Vec<Area> =
            serde_json::from_slice(AREAS_JSON).expect("fixture must be valid");
        let area = areas.remove(0);

        let (hash, value, percent, kind) = area.into_parts();

        assert_eq!(hash, "#a52714");
        assert_eq!(value, 63_864.643_989_695_01);
        assert_eq!(percent, 10.579);
        assert_eq!(kind, "occupied_after_24_02_2022");
    }

    #[test]
    fn rejects_non_finite_percent() {
        let timestamp = SnapshotTime::try_from(1_648_989_208).expect("timestamp must be valid");
        let json = br##"[{"hash":"#a52714","area":1.0,"percent":"NaN","type":"test"}]"##;

        assert!(matches!(
            parse_snapshot(json, timestamp),
            Err(Error::DecodeSnapshot { .. })
        ));
    }

    #[test]
    fn rejects_timestamp_outside_supported_range() {
        let json = format!(r#"[{{"id":{}}}]"#, i64::MAX);

        assert!(matches!(
            parse_timestamps(json.as_bytes()),
            Err(Error::InvalidTimestamp(err)) if err.value() == i64::MAX
        ));
    }
}
