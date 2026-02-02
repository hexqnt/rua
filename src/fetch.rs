use bytes::Bytes;
use chrono::{DateTime, Utc};
use reqwest::{Client, Error};
use serde::Deserialize;
use std::time::Duration;
use std::{env, fmt};
use tracing::{info, warn};

const HTTPS_PROXY_ENV: &str = "HTTPS_PROXY";
const HISTORY_API_BASE: &str = "https://deepstatemap.live/api/history";
const HISTORY_PUBLIC_URL: &str = "https://deepstatemap.live/api/history/public";

#[derive(Debug)]
pub enum FetchError {
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
pub struct AreaItem {
    pub id: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    pub datetime: String,
    pub status: bool,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
}

pub fn build_client() -> Client {
    match env::var(HTTPS_PROXY_ENV) {
        Ok(val) => {
            info!(proxy = %val, "Using HTTPS proxy");
            let proxy = reqwest::Proxy::https(val).unwrap();
            Client::builder().proxy(proxy).build().unwrap()
        }
        Err(e) => {
            warn!(error = %e, "Couldn't interpret HTTPS_PROXY");
            Client::new()
        }
    }
}

/// Запрашивает историю площадей по timestamp и повторяет попытки при сетевых/HTTP ошибках.
pub async fn fetch_url(
    client: &Client,
    timestamp: i64,
    max_retries: u32,
    delay: Duration,
) -> Result<Bytes, FetchError> {
    let url = format!("{HISTORY_API_BASE}/{timestamp}/areas");
    let mut last_error: Option<Error> = None;
    for attempt in 0..max_retries {
        match client.get(&url).send().await {
            Ok(response) => match response.error_for_status() {
                Ok(success_response) => {
                    return success_response.bytes().await.map_err(FetchError::Request);
                }
                Err(err) => {
                    if let Some(status) = err.status() {
                        warn!(
                            attempt = attempt + 1,
                            status = %status,
                            error = %err,
                            "HTTP request failed"
                        );
                    } else {
                        warn!(
                            attempt = attempt + 1,
                            error = %err,
                            "HTTP request failed"
                        );
                    }
                    last_error = Some(err);
                }
            },
            Err(err) => {
                warn!(
                    attempt = attempt + 1,
                    error = %err,
                    "HTTP request failed"
                );
                last_error = Some(err);
            }
        }

        if attempt + 1 < max_retries {
            if let Some(error) = &last_error {
                if let Some(status) = error.status() {
                    warn!(
                        attempt = attempt + 1,
                        status = %status,
                        error = %error,
                        "Retrying request"
                    );
                } else {
                    warn!(
                        attempt = attempt + 1,
                        error = %error,
                        "Retrying request"
                    );
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

/// Получает список доступных временных отметок из публичного API.
pub async fn get_timestamps(client: &Client) -> Result<Bytes, Error> {
    match client.get(HISTORY_PUBLIC_URL).send().await {
        Ok(response) => {
            if response.status().is_success() {
                return response.bytes().await;
            }
            Err(response.error_for_status().unwrap_err())
        }
        Err(err) => Err(err),
    }
}
