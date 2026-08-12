use std::env;
use std::num::{NonZeroU32, NonZeroUsize};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use futures::stream::FusedStream;
use futures::{Stream, StreamExt, stream};
use reqwest::{Client as HttpClient, Error as HttpError};
use tracing::{info, warn};

use super::error::{Error, Result};
use super::model::{Snapshot, SnapshotTime, parse_snapshot, parse_timestamps};

const HTTPS_PROXY_ENV: &str = "HTTPS_PROXY";
const HISTORY_API_BASE: &str = "https://deepstatemap.live/api/history";
const HISTORY_PUBLIC_URL: &str = "https://deepstatemap.live/api/history/public";
const DEFAULT_MAX_ATTEMPTS: u32 = 10;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(2);
const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 16;

/// Параметры загрузки временных срезов.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FetchOptions {
    /// Максимальное количество попыток загрузки одного среза.
    pub max_attempts: NonZeroU32,
    /// Пауза между повторными попытками.
    pub retry_delay: Duration,
    /// Максимальное количество одновременно загружаемых срезов.
    pub max_concurrent_requests: NonZeroUsize,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            max_attempts: NonZeroU32::new(DEFAULT_MAX_ATTEMPTS)
                .expect("DEFAULT_MAX_ATTEMPTS must be non-zero"),
            retry_delay: DEFAULT_RETRY_DELAY,
            max_concurrent_requests: NonZeroUsize::new(DEFAULT_MAX_CONCURRENT_REQUESTS)
                .expect("DEFAULT_MAX_CONCURRENT_REQUESTS must be non-zero"),
        }
    }
}

pin_project_lite::pin_project! {
    /// Асинхронный поток срезов, загружаемых с ограниченным параллелизмом.
    ///
    /// Срезы выдаются по мере готовности и поэтому не обязаны быть упорядочены по времени.
    pub struct SnapshotStream<S> {
        #[pin]
        inner: S,
        total: usize,
        remaining: usize,
    }
}

impl<S> SnapshotStream<S> {
    /// Возвращает полное количество запрошенных срезов.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.total
    }

    /// Возвращает количество ещё не выданных срезов.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.remaining
    }
}

impl<S> Stream for SnapshotStream<S>
where
    S: Stream<Item = Result<Snapshot>>,
{
    type Item = Result<Snapshot>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.as_mut().project();
        if *this.remaining == 0 {
            return Poll::Ready(None);
        }

        let poll = this.inner.as_mut().poll_next(cx);
        match &poll {
            Poll::Ready(Some(_)) => *this.remaining -= 1,
            Poll::Ready(None) => *this.remaining = 0,
            Poll::Pending => {}
        }
        poll
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<S> FusedStream for SnapshotStream<S>
where
    S: Stream<Item = Result<Snapshot>>,
{
    fn is_terminated(&self) -> bool {
        self.remaining == 0
    }
}

/// Клиент API истории `DeepStateMap`.
#[derive(Clone, Debug)]
pub struct Client {
    http: HttpClient,
    options: FetchOptions,
}

impl Client {
    /// Создаёт клиент на основе настроенного HTTP-клиента.
    #[must_use]
    pub const fn new(http: HttpClient, options: FetchOptions) -> Self {
        Self { http, options }
    }

    /// Создаёт клиент с учётом переменной окружения `HTTPS_PROXY`.
    ///
    /// При некорректной настройке proxy записывает предупреждение и использует
    /// стандартный HTTP-клиент, сохраняя поведение CLI.
    #[must_use]
    pub fn from_env(options: FetchOptions) -> Self {
        Self::new(build_http_client(), options)
    }

    /// Возвращает параметры загрузки.
    #[must_use]
    pub const fn options(&self) -> FetchOptions {
        self.options
    }

    /// Загружает список доступных временных отметок.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку при неуспешном HTTP-запросе, некорректном JSON или
    /// timestamp вне поддерживаемого диапазона.
    pub async fn timestamps(&self) -> Result<Vec<SnapshotTime>> {
        info!("Fetching timestamps...");
        let bytes = request_bytes(&self.http, HISTORY_PUBLIC_URL)
            .await
            .map_err(Error::FetchTimestamps)?;
        parse_timestamps(&bytes)
    }

    /// Загружает один временной срез с настроенными повторными попытками.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку, если все попытки HTTP-запроса завершились неуспешно
    /// либо ответ не удалось декодировать.
    pub async fn snapshot(&self, timestamp: SnapshotTime) -> Result<Snapshot> {
        let bytes = self.fetch_snapshot_bytes(timestamp).await?;
        parse_snapshot(&bytes, timestamp)
    }

    /// Создаёт поток всех доступных срезов с настроенным ограничением параллелизма.
    ///
    /// # Errors
    ///
    /// Возвращает ошибку, если не удалось получить список временных отметок.
    /// Ошибки отдельных срезов возвращаются элементами потока.
    pub async fn snapshots(
        &self,
    ) -> Result<SnapshotStream<impl Stream<Item = Result<Snapshot>> + Send + '_>> {
        let timestamps = self.timestamps().await?;
        let total = timestamps.len();
        let requests = stream::iter(timestamps)
            .map(move |timestamp| async move { self.snapshot(timestamp).await });
        let inner = requests.buffer_unordered(self.options.max_concurrent_requests.get());

        Ok(SnapshotStream {
            inner,
            total,
            remaining: total,
        })
    }

    /// Загружает все доступные срезы и сортирует их по времени.
    ///
    /// # Errors
    ///
    /// Возвращает первую ошибку загрузки или декодирования списка временных
    /// отметок либо отдельного среза.
    pub async fn fetch_all(&self) -> Result<Vec<Snapshot>> {
        let mut stream = self.snapshots().await?;
        let mut snapshots = Vec::with_capacity(stream.total());
        while let Some(snapshot) = stream.next().await {
            snapshots.push(snapshot?);
        }
        snapshots.sort_unstable_by_key(Snapshot::timestamp);
        Ok(snapshots)
    }

    async fn fetch_snapshot_bytes(&self, timestamp: SnapshotTime) -> Result<Bytes> {
        let url = format!("{HISTORY_API_BASE}/{}/areas", timestamp.unix_timestamp());
        let max_attempts = self.options.max_attempts.get();
        let mut attempt = 1;

        loop {
            match request_bytes(&self.http, &url).await {
                Ok(bytes) => return Ok(bytes),
                Err(source) if attempt >= max_attempts => {
                    return Err(Error::FetchSnapshot { timestamp, source });
                }
                Err(source) => warn_retrying(timestamp, attempt, max_attempts, &source),
            }

            tokio::time::sleep(self.options.retry_delay).await;
            attempt += 1;
        }
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::from_env(FetchOptions::default())
    }
}

async fn request_bytes(http: &HttpClient, url: &str) -> std::result::Result<Bytes, HttpError> {
    let response = http.get(url).send().await?.error_for_status()?;
    response.bytes().await
}

fn build_http_client() -> HttpClient {
    match env::var(HTTPS_PROXY_ENV) {
        Ok(value) => {
            info!("Using HTTPS proxy");
            reqwest::Proxy::https(&value)
                .and_then(|proxy| HttpClient::builder().proxy(proxy).build())
                .unwrap_or_else(|err| {
                    warn!(error = %err, "Invalid HTTPS_PROXY, falling back to default client");
                    HttpClient::new()
                })
        }
        Err(env::VarError::NotPresent) => HttpClient::new(),
        Err(err) => {
            warn!(error = %err, "Couldn't interpret HTTPS_PROXY");
            HttpClient::new()
        }
    }
}

fn warn_retrying(timestamp: SnapshotTime, attempt: u32, max_attempts: u32, error: &HttpError) {
    let next_attempt = attempt + 1;
    if let Some(status) = error.status() {
        warn!(
            attempt,
            next_attempt,
            max_attempts,
            timestamp = %timestamp,
            status = %status,
            error = %error,
            "Snapshot request failed; retrying",
        );
    } else {
        warn!(
            attempt,
            next_attempt,
            max_attempts,
            timestamp = %timestamp,
            error = %error,
            "Snapshot request failed; retrying",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deepstatemap::model::parse_snapshot;

    const AREAS_JSON: &[u8] = include_bytes!("../../tests/fixtures/deepstatemap/areas.json");

    #[test]
    fn default_options_preserve_cli_fetch_behavior() {
        let options = FetchOptions::default();

        assert_eq!(options.max_attempts.get(), 10);
        assert_eq!(options.retry_delay, Duration::from_secs(2));
        assert_eq!(options.max_concurrent_requests.get(), 16);
    }

    #[test]
    fn snapshot_stream_tracks_remaining_items() {
        let timestamp = SnapshotTime::try_from(1_648_989_208).expect("timestamp must be valid");
        let snapshot = parse_snapshot(AREAS_JSON, timestamp).expect("fixture must be valid");
        let inner = stream::iter([Ok(snapshot.clone()), Ok(snapshot)]);
        let mut snapshots = SnapshotStream {
            inner,
            total: 2,
            remaining: 2,
        };

        futures::executor::block_on(async {
            assert_eq!(snapshots.total(), 2);
            assert_eq!(snapshots.remaining(), 2);
            assert_eq!(snapshots.size_hint(), (2, Some(2)));

            assert!(snapshots.next().await.is_some());
            assert_eq!(snapshots.remaining(), 1);
            assert!(!snapshots.is_terminated());

            assert!(snapshots.next().await.is_some());
            assert_eq!(snapshots.remaining(), 0);
            assert!(snapshots.is_terminated());
            assert!(snapshots.next().await.is_none());
        });
    }
}
