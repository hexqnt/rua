use std::fmt;

use reqwest::Error as HttpError;

use super::model::SnapshotTime;

/// Результат операции с API `DeepStateMap`.
pub type Result<T> = std::result::Result<T, Error>;

/// Timestamp, который невозможно представить как `DateTime<Utc>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidTimestamp(i64);

impl InvalidTimestamp {
    pub(super) const fn new(value: i64) -> Self {
        Self(value)
    }

    /// Возвращает исходное значение timestamp.
    #[must_use]
    pub const fn value(self) -> i64 {
        self.0
    }
}

impl fmt::Display for InvalidTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Timestamp is outside the supported range: {}", self.0)
    }
}

impl std::error::Error for InvalidTimestamp {}

/// Ошибка загрузки или декодирования данных `DeepStateMap`.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Не удалось загрузить список временных отметок.
    FetchTimestamps(HttpError),
    /// Ответ со списком временных отметок имеет неожиданный формат.
    DecodeTimestamps(serde_json::Error),
    /// API вернул timestamp вне диапазона `DateTime<Utc>`.
    InvalidTimestamp(InvalidTimestamp),
    /// Не удалось загрузить временной срез.
    FetchSnapshot {
        /// Временная отметка запрошенного среза.
        timestamp: SnapshotTime,
        /// Ошибка HTTP-клиента.
        source: HttpError,
    },
    /// Ответ временного среза имеет неожиданный формат.
    DecodeSnapshot {
        /// Временная отметка запрошенного среза.
        timestamp: SnapshotTime,
        /// Ошибка десериализации JSON.
        source: serde_json::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FetchTimestamps(err) => write!(f, "Failed to fetch timestamps: {err}"),
            Self::DecodeTimestamps(err) => write!(f, "Failed to decode timestamps JSON: {err}"),
            Self::InvalidTimestamp(err) => err.fmt(f),
            Self::FetchSnapshot { timestamp, source } => {
                write!(f, "Failed to fetch snapshot {timestamp}: {source}")
            }
            Self::DecodeSnapshot { timestamp, source } => {
                write!(f, "Failed to decode snapshot {timestamp}: {source}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FetchTimestamps(err) => Some(err),
            Self::DecodeTimestamps(err) => Some(err),
            Self::InvalidTimestamp(err) => Some(err),
            Self::FetchSnapshot { source, .. } => Some(source),
            Self::DecodeSnapshot { source, .. } => Some(source),
        }
    }
}
