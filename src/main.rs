mod plot;

use chrono::{DateTime, Utc};
use reqwest::{Client, Error};
use serde::{Deserialize, Deserializer};
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use std::{env, fmt};
use tqdm::pbar;

#[derive(Debug)]
enum FetchError {
    Request(reqwest::Error),
    NoAttempts,
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(err) => write!(f, "{err}"),
            Self::NoAttempts => f.write_str("Request attempts were not performed"),
        }
    }
}

impl std::error::Error for FetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Request(err) => Some(err),
            Self::NoAttempts => None,
        }
    }
}

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

fn draw() {
    let csv_path = Path::new("data/area_history.csv");
    let output_path = Path::new("img/area.html");
    if let Err(err) = plot::draw_area_chart(csv_path, output_path) {
        eprintln!("Не удалось построить график: {err}");
    } else {
        println!("Сохранил интерактивный график в {}", output_path.display());
    }
}

/// Функция для отправки запроса на URL и повторных попыток в случае ошибки
async fn fetch_url(
    client: &Client,
    timestamp: i64,
    max_retries: u32,
    delay: Duration,
) -> Result<String, FetchError> {
    let url = format!("https://deepstatemap.live/api/history/{timestamp}/areas");
    let mut last_error: Option<Error> = None;
    for attempt in 0..max_retries {
        match client.get(&url).send().await {
            Ok(response) => match response.error_for_status() {
                Ok(success_response) => {
                    return success_response.text().await.map_err(FetchError::Request);
                }
                Err(err) => {
                    if let Some(status) = err.status() {
                        eprintln!("Attempt {} failed: HTTP {status} - {}", attempt + 1, err);
                    } else {
                        eprintln!("Attempt {} failed: {}", attempt + 1, err);
                    }
                    last_error = Some(err);
                }
            },
            Err(err) => {
                eprintln!("Attempt {} failed: {}", attempt + 1, err);
                last_error = Some(err);
            }
        }

        if attempt + 1 < max_retries {
            if let Some(error) = &last_error {
                if let Some(status) = error.status() {
                    eprintln!("Retrying after HTTP status {status}: {error}");
                } else {
                    eprintln!("Retrying after error: {error}");
                }
            }
            tokio::time::sleep(delay).await;
        }
    }

    last_error.map_or_else(
        || Err(FetchError::NoAttempts),
        |err| Err(FetchError::Request(err)),
    )
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
                for a in &mut area {
                    a.time_index = DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap();
                }
                areas.extend(area);
                pbar.update(1).unwrap();
            }
            Err(err) => eprintln!("Failed to fetch the URL: {err:?}"),
        }
    }
    let csv_path = Path::new("data/area_history.csv");
    to_csv(areas, csv_path);
    draw();
}
