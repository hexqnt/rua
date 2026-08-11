//! Вспомогательные функции для загрузки данных и записи CSV.

use std::io::{BufWriter, IsTerminal};
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::{StreamExt, stream};
use serde::{Deserialize, Deserializer};
use tqdm::pbar;
use tracing::{info, warn};

use crate::fetch;

const CSV_HEADER: [&str; 5] = ["time_index", "hash", "area", "percent", "area_type"];
const MAX_CONCURRENT_FETCHES: usize = 16;

#[allow(clippy::struct_field_names)]
#[derive(Debug, Deserialize)]
pub struct Area {
    #[serde(skip_deserializing)]
    time_index: DateTime<Utc>,
    hash: String,
    area: f64,
    #[serde(deserialize_with = "str_to_f64")]
    percent: f64,
    #[serde(rename = "type", alias = "area_type")]
    area_type: String,
}

/// Преобразует строковое значение процента из API в `f64`.
fn str_to_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &str = Deserialize::deserialize(deserializer)?;
    s.parse::<f64>().map_err(serde::de::Error::custom)
}

/// Записывает точки площадей в CSV, создавая директорию при необходимости.
pub fn to_csv(areas: &[Area], file_path: &Path) -> Result<(), String> {
    if let Some(parent) = file_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create {}: {err}", parent.display()))?;
    }
    let file = std::fs::File::create(file_path)
        .map_err(|err| format!("Failed to create CSV {}: {err}", file_path.display()))?;
    let mut writer = csv::Writer::from_writer(BufWriter::new(file));
    writer.write_record(CSV_HEADER).map_err(|err| {
        format!(
            "Failed to write CSV header to {}: {err}",
            file_path.display()
        )
    })?;
    for area in areas {
        let time_index = area.time_index.to_string();
        writer
            .serialize((
                &time_index,
                area.hash.as_str(),
                area.area,
                area.percent,
                area.area_type.as_str(),
            ))
            .map_err(|err| {
                format!(
                    "Failed to serialize CSV row to {}: {err}",
                    file_path.display()
                )
            })?;
    }
    writer
        .flush()
        .map_err(|err| format!("Failed to flush CSV {}: {err}", file_path.display()))?;
    Ok(())
}

/// Загружает все доступные срезы и проставляет `time_index` из timestamp.
pub async fn fetch_areas(
    client: &reqwest::Client,
    max_retries: u32,
    delay: Duration,
) -> Result<Vec<Area>, String> {
    // Сначала получаем список временных отметок, по которым запрашиваем площади.
    info!("Fetching timestamps...");
    let timestamps_json = fetch::get_timestamps(client)
        .await
        .map_err(|err| format!("Failed to fetch timestamps: {err}"))?;
    let timestamps: Vec<fetch::AreaItem> = serde_json::from_slice(&timestamps_json)
        .map_err(|err| format!("Failed to deserialize timestamps JSON: {err}"))?;

    // Затем скачиваем площади по каждой отметке.
    let mut areas = Vec::new();
    let mut progress = std::io::stderr()
        .is_terminal()
        .then(|| pbar(Some(timestamps.len())));
    let requests = stream::iter(timestamps).map(|area_item| async move {
        let timestamp = area_item.id;
        let areas_json = fetch::fetch_url(client, timestamp, max_retries, delay)
            .await
            .map_err(|err| format!("Failed to fetch areas for timestamp {timestamp}: {err}"))?;
        let mut items: Vec<Area> = serde_json::from_slice(&areas_json).map_err(|err| {
            format!("Failed to deserialize areas for timestamp {timestamp}: {err}")
        })?;
        let time_index = DateTime::<Utc>::from_timestamp(timestamp, 0)
            .ok_or_else(|| format!("Timestamp is outside the supported range: {timestamp}"))?;
        for item in &mut items {
            item.time_index = time_index;
        }
        Ok::<Vec<Area>, String>(items)
    });
    let mut batches = requests.buffer_unordered(MAX_CONCURRENT_FETCHES);

    while let Some(result) = batches.next().await {
        let mut batch = result?;
        areas.append(&mut batch);
        if let Some(progress) = &mut progress
            && let Err(err) = progress.update(1)
        {
            warn!(error = %err, "Failed to update progress bar");
        }
    }

    areas.sort_by_key(|area| area.time_index);
    Ok(areas)
}
