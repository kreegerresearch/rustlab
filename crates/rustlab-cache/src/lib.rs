//! Persistent function-result cache for rustlab.
//!
//! Phase 1 — storage layer. Opens a SQLite database in WAL mode, exposes
//! a small `get`/`put` API keyed on `(entry_id, input_hash)`, records the
//! `rustlab_version` that wrote each row, and degrades safely when the
//! store can't be written (read-only filesystem, disk full, schema newer
//! than this binary knows). Higher layers (fingerprinting, evaluator
//! dispatch, CLI) land in Phase 2+.
//!
//! Design summary: `dev/plans/persistent_function_cache.md`.

mod duration;
mod file_fp;
mod store;

pub use duration::{parse_duration_secs, DurationParseError};
pub use file_fp::file_fingerprint;
pub use store::{open, ListEntry, Store, MAX_SUPPORTED_SCHEMA_VERSION, SCHEMA_VERSION};

/// Errors surfaced by the cache. Almost everything else degrades to a
/// silent skip — caching is an optimization, never load-bearing.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// The chosen path can't be opened for writing (e.g. read-only
    /// filesystem, missing parent directory, permission denied).
    #[error("cannot open cache at {path}: {source}")]
    Open {
        path: std::path::PathBuf,
        #[source]
        source: rusqlite::Error,
    },

    /// Any other SQLite error from a query we ran. The caller decides
    /// whether to log and swallow.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}
