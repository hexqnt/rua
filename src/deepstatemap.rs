//! Типизированный клиент для загрузки истории `DeepStateMap`.
//!
//! Модуль предоставляет как точечную загрузку отдельных временных срезов, так и
//! конкурентную загрузку полной истории. Выбор способа хранения остаётся за
//! вызывающим кодом.
//!
//! # Пример
//!
//! ```no_run
//! use rua::deepstatemap::Client;
//!
//! # async fn load() -> rua::deepstatemap::Result<()> {
//! let client = Client::default();
//! let snapshots = client.fetch_all().await?;
//! # let _ = snapshots;
//! # Ok(())
//! # }
//! ```

mod client;
mod error;
mod model;

pub use client::{Client, FetchOptions, SnapshotStream};
pub use error::{Error, InvalidTimestamp, Result};
pub use model::{Area, Snapshot, SnapshotTime};
