//! Вспомогательные функции для загрузки данных и записи CSV.

use std::io::BufWriter;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::{StreamExt, stream};
use serde::{Deserialize, Deserializer};
use tqdm::pbar;
use tracing::{info, warn};

use crate::fetch;

const CSV_HEADER: &str = "time_index,hash,area,percent,area_type\n";
const FETCH_AREAS_CAPACITY: usize = 5000;
const FETCH_CONCURRENCY: usize = 4;

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
pub fn to_csv(areas: Vec<Area>, file_path: &Path) -> Result<(), String> {
    if let Some(parent) = file_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create {}: {err}", parent.display()))?;
    }
    let file = std::fs::File::create(file_path)
        .map_err(|err| format!("Failed to create CSV {}: {err}", file_path.display()))?;
    let mut writer = csv::Writer::from_writer(BufWriter::new(file));
    writer
        .write_record(CSV_HEADER.trim_end().split(','))
        .map_err(|err| {
            format!(
                "Failed to write CSV header to {}: {err}",
                file_path.display()
            )
        })?;
    for area in areas {
        writer
            .write_record([
                area.time_index.to_string(),
                area.hash,
                area.area.to_string(),
                area.percent.to_string(),
                area.area_type,
            ])
            .map_err(|err| format!("Failed to write CSV row to {}: {err}", file_path.display()))?;
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
    let json_data = fetch::get_timestamps(client)
        .await
        .map_err(|err| format!("Failed to fetch timestamps: {err}"))?;
    let result: Vec<fetch::AreaItem> = serde_json::from_slice(&json_data)
        .map_err(|err| format!("Failed to deserialize JSON: {err}"))?;

    // Затем скачиваем площади по каждой отметке.
    let mut areas = Vec::with_capacity(FETCH_AREAS_CAPACITY);
    let mut pbar = pbar(Some(result.len()));
    let stream = stream::iter(result.into_iter()).map(|area_item| async move {
        let timestamp = area_item.id;
        let content = fetch::fetch_url(client, timestamp, max_retries, delay)
            .await
            .map_err(|err| format!("Failed to fetch URL: {err:?}"))?;
        let mut area: Vec<Area> = serde_json::from_slice(&content)
            .map_err(|err| format!("Failed to deserialize JSON: {err}"))?;
        for a in &mut area {
            a.time_index = DateTime::<Utc>::from_timestamp(timestamp, 0)
                .ok_or_else(|| "Failed to build timestamp".to_string())?;
        }
        Ok::<Vec<Area>, String>(area)
    });
    let mut buffered = stream.buffer_unordered(FETCH_CONCURRENCY);

    while let Some(result) = buffered.next().await {
        match result {
            Ok(mut area) => {
                areas.append(&mut area);
            }
            Err(err) => warn!(error = %err, "Failed to fetch the URL"),
        }
        if let Err(err) = pbar.update(1) {
            warn!(error = %err, "Failed to update progress bar");
        }
    }

    Ok(areas)
}
