use bytes::Bytes;
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
}

pub fn build_client() -> Client {
    match env::var(HTTPS_PROXY_ENV) {
        Ok(val) => {
            info!(proxy = %val, "Using HTTPS proxy");
            match reqwest::Proxy::https(&val) {
                Ok(proxy) => match Client::builder().proxy(proxy).build() {
                    Ok(client) => client,
                    Err(err) => {
                        warn!(error = %err, "Failed to build client with proxy, falling back to direct client");
                        Client::new()
                    }
                },
                Err(err) => {
                    warn!(error = %err, "Invalid HTTPS_PROXY, falling back to direct client");
                    Client::new()
                }
            }
        }
        Err(env::VarError::NotPresent) => Client::new(),
        Err(err) => {
            warn!(error = %err, "Couldn't interpret HTTPS_PROXY");
            Client::new()
        }
    }
}

/// Логирует ошибку запроса с уровнем `warn`, включая HTTP-статус если доступен.
fn warn_request_failed(attempt: u32, error: &Error) {
    if let Some(status) = error.status() {
        warn!(attempt, status = %status, error = %error, "HTTP request failed");
    } else {
        warn!(attempt, error = %error, "HTTP request failed");
    }
}

/// Логирует попытку повтора с уровнем `warn`, включая HTTP-статус если доступен.
fn warn_retrying(attempt: u32, error: &Error) {
    if let Some(status) = error.status() {
        warn!(attempt, status = %status, error = %error, "Retrying request");
    } else {
        warn!(attempt, error = %error, "Retrying request");
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
        let attempt_number = attempt + 1;
        match client.get(&url).send().await {
            Ok(response) => match response.error_for_status() {
                Ok(success_response) => {
                    return success_response.bytes().await.map_err(FetchError::Request);
                }
                Err(err) => {
                    warn_request_failed(attempt_number, &err);
                    last_error = Some(err);
                }
            },
            Err(err) => {
                warn_request_failed(attempt_number, &err);
                last_error = Some(err);
            }
        }

        if attempt + 1 < max_retries {
            if let Some(ref error) = last_error {
                warn_retrying(attempt_number, error);
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
