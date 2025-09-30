use chrono::{DateTime, Utc};
use reqwest::{Client, Error};
use serde::{Deserialize, Deserializer};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::{env, io};
// use std::thread::sleep;
use std::time::Duration;
use tqdm::pbar;

#[derive(Debug, Deserialize)]
struct Area {
    #[serde(skip_deserializing)]
    time_index: DateTime<Utc>,
    hash: String,
    area: f64,
    #[serde(deserialize_with = "str_to_f64")]
    percent: f64,
    #[serde(rename = "type", alias = "area_type")]
    area_type: String,
}

#[derive(Debug, Deserialize)]
struct AreaItem {
    id: i64,
    // #[serde(rename = "description")]
    // description_ua: String,
    // #[serde(rename = "descriptionEn")]
    // description_en: String,
    #[serde(rename = "updatedAt")]
    updated_at: DateTime<Utc>,
    datetime: String,
    status: bool,
    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
}

/// Функция для преобразования строки в f64
fn str_to_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &str = Deserialize::deserialize(deserializer)?;
    s.parse::<f64>().map_err(serde::de::Error::custom)
}

/// Функция для чтения CSV в вектор структур Area
fn read_csv_to_areas(file_path: &str) -> io::Result<Vec<Area>> {
    // Открываем CSV-файл
    let file = File::open(file_path)?;
    let mut rdr = csv::Reader::from_reader(file);

    // Вектор для хранения результатов
    let mut areas = Vec::new();

    // Читаем записи из CSV
    for result in rdr.deserialize() {
        let mut record: Area = result?;
        // Устанавливаем time_index вручную, так как он пропускается при десериализации
        record.time_index = Utc::now(); // Замените на нужную логику для time_index
        areas.push(record);
    }

    Ok(areas)
}

fn draw() {
    println!("Drawing...");
    let file_path = "data/area_history.csv";
    match read_csv_to_areas(file_path) {
        Ok(areas) => {
            for area in areas {
                println!("{:?}", area);
            }
        }
        Err(e) => eprintln!("Ошибка при чтении CSV: {}", e),
    }
}

/// Функция для отправки запроса на URL и повторных попыток в случае ошибки
async fn fetch_url(
    client: &Client,
    timestamp: i64,
    max_retries: u32,
    delay: Duration,
) -> Result<String, Error> {
    let url = format!("https://deepstatemap.live/api/history/{timestamp}/areas");
    let mut last_error: Option<Error> = None;
    for attempt in 0..max_retries {
        match client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    return response.text().await;
                } else {
                    eprintln!("Attempt {} failed: HTTP {}", attempt + 1, response.status());
                }
            }
            Err(err) => {
                eprintln!("Attempt {} failed: {:?}", attempt + 1, err);
                last_error = Some(err);
            }
        }
        if attempt < max_retries - 1 {
            tokio::time::sleep(delay).await;
        }
    }
    Err(last_error.unwrap())
}

/// Функция для получения временных меток
async fn get_timestamps(client: &Client) -> Result<String, Error> {
    // let client = reqwest::blocking::Client::new();
    let time_history_url = "https://deepstatemap.live/api/history/public";
    match client.get(time_history_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                return response.text().await;
            }
            Err(response.error_for_status().unwrap_err())
        }
        Err(err) => Err(err),
    }
}

/// Функция для записи данных о территории в CSV
fn to_csv(areas: Vec<Area>, file_path: &Path) {
    let mut file = std::fs::File::create(file_path).unwrap();
    let head_str = "time_index,hash,area,percent,area_type\n";
    file.write_all(head_str.as_bytes()).unwrap();
    for area in areas {
        let line = format!(
            "{},{},{},{},{}\n",
            area.time_index, area.hash, area.area, area.percent, area.area_type
        );
        file.write_all(line.as_bytes()).unwrap();
    }
}

#[tokio::main]
async fn main() {
    // draw();
    // return;
    println!("RUA - Dynamic transition of territory in the Russian-Ukrainian conflict");
    let max_retries = 10;
    let delay = Duration::from_secs(2);

    let client = match env::var("HTTPS_PROXY") {
        Ok(val) => {
            println!("HTTPS_PROXY: {val:?}");
            let proxy = reqwest::Proxy::https(val).unwrap();
            Client::builder().proxy(proxy).build().unwrap()
        }
        Err(e) => {
            println!("couldn't interpret HTTPS_PROXY: {e}");
            Client::new()
        }
    };

    // Загрузка временных меток
    println!("Fetching timestamps...");
    let json_data = match get_timestamps(&client).await {
        Ok(data) => data,
        Err(err) => {
            eprintln!("Failed to fetch timestamps: {err}");
            return;
        }
    };
    let json_data = get_timestamps(&client).await.unwrap();
    let result: Vec<AreaItem> =
        serde_json::from_str(&json_data).expect("Failed to deserialize JSON");

    // Загрузка площадей
    let mut areas = Vec::with_capacity(5000);
    let mut pbar = pbar(Some(result.len()));

    for area_item in result {
        let timestamp = area_item.id;

        match fetch_url(&client, timestamp, max_retries, delay).await {
            Ok(content) => {
                let mut area: Vec<Area> =
                    serde_json::from_str(&content).expect("Failed to deserialize JSON");
                for a in area.iter_mut() {
                    a.time_index = DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap();
                }
                areas.extend(area);
                pbar.update(1).unwrap();
            }
            Err(err) => eprintln!("Failed to fetch the URL: {:?}", err),
        }
    }
    to_csv(areas, Path::new("data/area_history.csv"));
}
