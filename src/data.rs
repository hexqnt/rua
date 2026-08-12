//! Вспомогательные функции CLI для загрузки данных и записи CSV.

use std::io::{BufWriter, IsTerminal, Write};
use std::path::Path;

use futures::StreamExt;
use rua::deepstatemap::{Client, Error, Snapshot};
use tqdm::pbar;
use tracing::warn;

const CSV_HEADER: [&str; 5] = ["time_index", "hash", "area", "percent", "area_type"];

/// Загружает срезы и отображает прогресс при работе в терминале.
pub async fn fetch_all_with_progress(client: &Client) -> Result<Vec<Snapshot>, Error> {
    let mut stream = client.snapshots().await?;
    let mut progress = std::io::stderr()
        .is_terminal()
        .then(|| pbar(Some(stream.total())));
    let mut snapshots = Vec::with_capacity(stream.total());

    while let Some(snapshot) = stream.next().await {
        snapshots.push(snapshot?);
        if let Some(progress) = &mut progress
            && let Err(err) = progress.update(1)
        {
            warn!(error = %err, "Failed to update progress bar");
        }
    }

    snapshots.sort_unstable_by_key(Snapshot::timestamp);
    Ok(snapshots)
}

/// Записывает точки площадей в CSV, создавая директорию при необходимости.
pub fn to_csv(snapshots: &[Snapshot], file_path: &Path) -> Result<(), String> {
    if let Some(parent) = file_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create {}: {err}", parent.display()))?;
    }
    let file = std::fs::File::create(file_path)
        .map_err(|err| format!("Failed to create CSV {}: {err}", file_path.display()))?;
    write_csv(snapshots, BufWriter::new(file))
        .map_err(|err| format!("Failed to write CSV {}: {err}", file_path.display()))
}

fn write_csv<W>(snapshots: &[Snapshot], target: W) -> Result<(), csv::Error>
where
    W: Write,
{
    let mut writer = csv::Writer::from_writer(target);
    writer.write_record(CSV_HEADER)?;
    for snapshot in snapshots {
        let time_index = snapshot.timestamp().to_string();
        for area in snapshot.areas() {
            writer.serialize((
                &time_index,
                area.hash(),
                area.area(),
                area.percent(),
                area.kind(),
            ))?;
        }
    }
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rua::deepstatemap::{Area, SnapshotTime};

    use super::*;

    const AREAS_JSON: &[u8] = include_bytes!("../tests/fixtures/deepstatemap/areas.json");

    #[test]
    fn csv_format_remains_compatible() {
        let timestamp = SnapshotTime::try_from(1_648_989_208).expect("timestamp must be valid");
        let areas: Vec<Area> = serde_json::from_slice(AREAS_JSON).expect("fixture must be valid");
        let snapshot = Snapshot::new(timestamp, areas);
        let mut output = Vec::new();

        write_csv(&[snapshot], &mut output).expect("CSV serialization must succeed");

        let output = String::from_utf8(output).expect("CSV must be UTF-8");
        let mut rows = output.lines();
        assert_eq!(rows.next(), Some("time_index,hash,area,percent,area_type"));
        assert_eq!(
            rows.next(),
            Some(
                "2022-04-03 12:33:28 UTC,#a52714,63864.64398969501,10.579,occupied_after_24_02_2022"
            )
        );
        assert_eq!(rows.count(), 3);
    }
}
