//! SQLite-backed cache store.

use crate::CacheError;
use rusqlite::{params, Connection, ErrorCode, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Schema version this binary knows how to write. Bump only when the
/// table layout changes in a way readers from an older binary couldn't
/// safely write to.
pub const SCHEMA_VERSION: u32 = 1;

/// Highest schema version this binary will read from. Same as
/// `SCHEMA_VERSION` for now; reserved for future "read newer, write
/// older" scenarios.
pub const MAX_SUPPORTED_SCHEMA_VERSION: u32 = SCHEMA_VERSION;

const SCHEMA_VERSION_KEY: &str = "version";
const RUSTLAB_VERSION_KEY: &str = "rustlab_version";

/// `Open` a cache store at `path`, creating the file and schema if
/// needed. See `Store::open` for details.
pub fn open(path: impl AsRef<Path>) -> Result<Store, CacheError> {
    Store::open(path)
}

/// Persistent function-result cache. Thread-safe through an internal
/// `Mutex<Connection>`; cheap to hold across an evaluator session.
///
/// When the on-disk schema version is newer than this binary supports,
/// the store opens in **disabled** mode — `get` always returns `None`,
/// `put` is a no-op. This matches the "silent treat-as-cold" policy in
/// the plan and lets older binaries coexist with newer ones without
/// corrupting either side.
pub struct Store {
    conn: Mutex<Connection>,
    path: PathBuf,
    /// `true` when the DB's schema version is newer than we support.
    /// All operations no-op in this state.
    disabled: bool,
}

impl Store {
    /// Open (or create) the cache at `path`.
    ///
    /// On creation: initializes the schema, enables WAL, records this
    /// binary's `rustlab_version` in `schema_meta`.
    ///
    /// On open of an existing DB: validates schema version. If the DB
    /// is on a future schema, the returned store is **disabled** (all
    /// reads miss, all writes are dropped) — no error, by design.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CacheError> {
        let path = path.as_ref().to_path_buf();

        // Make sure the parent directory exists; rusqlite won't create it.
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| CacheError::Open {
                    path: path.clone(),
                    source: rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                        Some(format!("create_dir_all({}): {e}", parent.display())),
                    ),
                })?;
            }
        }

        let conn = Connection::open(&path).map_err(|source| CacheError::Open {
            path: path.clone(),
            source,
        })?;

        // WAL + synchronous=NORMAL: the standard "fast and crash-safe
        // enough for a cache" combo. WAL is required for the
        // multi-reader / single-writer story called out in the plan.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;

        conn.execute_batch(SCHEMA_SQL)?;

        // Read the on-disk schema version. Absent → fresh DB, write it.
        // Present and ≤ ours → we can read and write.
        // Present and > ours → disabled mode.
        let on_disk_version: Option<u32> = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key = ?1",
                params![SCHEMA_VERSION_KEY],
                |row| {
                    let s: String = row.get(0)?;
                    Ok(s.parse::<u32>().unwrap_or(0))
                },
            )
            .optional()?;

        let disabled = match on_disk_version {
            None => {
                // Fresh DB — write the version.
                conn.execute(
                    "INSERT INTO schema_meta(key, value) VALUES(?1, ?2)",
                    params![SCHEMA_VERSION_KEY, SCHEMA_VERSION.to_string()],
                )?;
                false
            }
            Some(v) if v > MAX_SUPPORTED_SCHEMA_VERSION => true,
            Some(_) => false,
        };

        // Record this binary's version. Skip on disabled stores to keep
        // a newer-schema DB pristine.
        if !disabled {
            conn.execute(
                "INSERT INTO schema_meta(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![RUSTLAB_VERSION_KEY, rustlab_version()],
            )?;
        }

        Ok(Store {
            conn: Mutex::new(conn),
            path,
            disabled,
        })
    }

    /// Path the store was opened with.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `true` when the store is in silent treat-as-cold mode (DB schema
    /// is newer than this binary supports).
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Look up a cached value by `(entry_id, input_hash)`. Returns
    /// `Ok(None)` for cache miss and for disabled stores.
    pub fn get(
        &self,
        entry_id: &[u8; 32],
        input_hash: &[u8; 32],
    ) -> Result<Option<Vec<u8>>, CacheError> {
        if self.disabled {
            return Ok(None);
        }
        let conn = self.conn.lock().expect("cache mutex poisoned");
        let result: Option<Vec<u8>> = conn
            .query_row(
                "SELECT value FROM cache_entries WHERE entry_id = ?1 AND input_hash = ?2",
                params![&entry_id[..], &input_hash[..]],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result)
    }

    /// Store `value` under `(entry_id, input_hash)` AND record the
    /// function name as metadata so `cache list` can surface it later.
    /// Same transient-error handling as [`Store::put`]; the metadata
    /// insert is `INSERT OR IGNORE` so concurrent writers don't fight
    /// and the first observation wins.
    pub fn put_with_meta(
        &self,
        entry_id: &[u8; 32],
        input_hash: &[u8; 32],
        value: &[u8],
        fn_name: &str,
    ) -> Result<(), CacheError> {
        self.put(entry_id, input_hash, value)?;
        if self.disabled {
            return Ok(());
        }
        let conn = self.conn.lock().expect("cache mutex poisoned");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let result = conn.execute(
            "INSERT OR IGNORE INTO fn_metadata (entry_id, fn_name, first_seen)
             VALUES (?1, ?2, ?3)",
            params![&entry_id[..], fn_name, now],
        );
        match result {
            Ok(_) => Ok(()),
            Err(e) if is_transient_write_error(&e) => {
                // Metadata is best-effort — losing it just makes
                // `cache list` show `<unknown>` for this row.
                eprintln!("cache metadata skipped ({}): {e}", self.path.display());
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Store `value` under `(entry_id, input_hash)`. Duplicate inserts
    /// are silently ignored (a concurrent process won the race; their
    /// row is correct). Disk-full and similar transient write errors
    /// are logged to stderr and swallowed — caching is never
    /// load-bearing.
    pub fn put(
        &self,
        entry_id: &[u8; 32],
        input_hash: &[u8; 32],
        value: &[u8],
    ) -> Result<(), CacheError> {
        if self.disabled {
            return Ok(());
        }
        let conn = self.conn.lock().expect("cache mutex poisoned");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let result = conn.execute(
            "INSERT OR IGNORE INTO cache_entries
                 (entry_id, input_hash, value, bytes, rustlab_version, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &entry_id[..],
                &input_hash[..],
                value,
                value.len() as i64,
                rustlab_version(),
                now,
            ],
        );

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_transient_write_error(&e) => {
                eprintln!("cache write skipped ({}): {e}", self.path.display());
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Delete every cached entry. Keeps the schema and the DB file.
    /// Returns the number of rows removed.
    pub fn clear(&self) -> Result<usize, CacheError> {
        if self.disabled {
            return Ok(0);
        }
        let conn = self.conn.lock().expect("cache mutex poisoned");
        let removed = conn.execute("DELETE FROM cache_entries", [])?;
        Ok(removed)
    }

    /// Delete entries whose `created_at` is older than `max_age_secs`
    /// (relative to now). Returns the number of rows removed.
    pub fn prune_older_than(&self, max_age_secs: u64) -> Result<usize, CacheError> {
        if self.disabled {
            return Ok(0);
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let cutoff = now.saturating_sub(max_age_secs as i64);
        let conn = self.conn.lock().expect("cache mutex poisoned");
        // `<=` rather than `<`: with `max_age_secs = 0` the cutoff
        // equals `now`, and entries created during the current second
        // share that timestamp. `<=` makes `prune older=0s` mean
        // "drop everything" — which is the user-friendly reading.
        let removed = conn.execute(
            "DELETE FROM cache_entries WHERE created_at <= ?1",
            params![cutoff],
        )?;
        Ok(removed)
    }

    /// Number of cached entries.
    pub fn entry_count(&self) -> Result<u64, CacheError> {
        let conn = self.conn.lock().expect("cache mutex poisoned");
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM cache_entries", [], |row| row.get(0))?;
        Ok(n.max(0) as u64)
    }

    /// Sum of `bytes` across all entries (size of cached values, not
    /// including SQLite overhead).
    pub fn total_bytes(&self) -> Result<u64, CacheError> {
        let conn = self.conn.lock().expect("cache mutex poisoned");
        let n: i64 = conn.query_row(
            "SELECT COALESCE(SUM(bytes), 0) FROM cache_entries",
            [],
            |row| row.get(0),
        )?;
        Ok(n.max(0) as u64)
    }

    /// Drop oldest entries (by `created_at` ascending) until the total
    /// stored `bytes` is ≤ `max_bytes`. Returns the row count removed.
    /// Useful for `cache prune --max-size`; pairs naturally with
    /// `prune_older_than` (size-based and age-based cleanup are
    /// independent — call either, or both).
    pub fn prune_to_max_size(&self, max_bytes: u64) -> Result<usize, CacheError> {
        if self.disabled {
            return Ok(0);
        }
        let conn = self.conn.lock().expect("cache mutex poisoned");
        let current: i64 = conn
            .query_row("SELECT COALESCE(SUM(bytes), 0) FROM cache_entries", [], |row| {
                row.get(0)
            })?;
        let mut current = current.max(0) as u64;
        if current <= max_bytes {
            return Ok(0);
        }

        let mut removed: usize = 0;
        // Walk oldest → newest, deleting one row at a time until we're
        // under the cap. SQLite's per-row DELETE inside the loop costs
        // ~1 statement/row but keeps the logic obvious; cache sizes
        // are expected to be small enough that this isn't a hot path.
        let mut stmt = conn.prepare(
            "SELECT entry_id, input_hash, bytes
             FROM cache_entries
             ORDER BY created_at ASC, entry_id ASC, input_hash ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let entry_id: Vec<u8> = row.get(0)?;
            let input_hash: Vec<u8> = row.get(1)?;
            let bytes: i64 = row.get(2)?;
            Ok((entry_id, input_hash, bytes.max(0) as u64))
        })?;

        // Collect first so we can release the prepared statement
        // before issuing DELETEs (sqlite locks one stmt at a time).
        let candidates: Vec<(Vec<u8>, Vec<u8>, u64)> =
            rows.collect::<Result<_, _>>()?;
        drop(stmt);

        for (entry_id, input_hash, bytes) in candidates {
            if current <= max_bytes {
                break;
            }
            conn.execute(
                "DELETE FROM cache_entries WHERE entry_id = ?1 AND input_hash = ?2",
                params![&entry_id[..], &input_hash[..]],
            )?;
            current = current.saturating_sub(bytes);
            removed += 1;
        }
        Ok(removed)
    }

    /// List entries for `cache list` display. Returns one
    /// [`ListEntry`] per row up to `limit` (or all if `None`),
    /// ordered most-recent-first. No actual cached values are
    /// surfaced — only the keys and metadata.
    pub fn list_entries(&self, limit: Option<usize>) -> Result<Vec<ListEntry>, CacheError> {
        if self.disabled {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().expect("cache mutex poisoned");
        // ORDER newest-first so `cache list` shows what changed most
        // recently at the top. Pull the natural-key sort into the
        // query so two clients see the same ordering.
        let mut stmt = conn.prepare(
            "SELECT c.entry_id, c.input_hash, c.bytes, c.rustlab_version, c.created_at, m.fn_name
             FROM cache_entries c
             LEFT JOIN fn_metadata m ON c.entry_id = m.entry_id
             ORDER BY c.created_at DESC, c.entry_id ASC, c.input_hash ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let entry_id: Vec<u8> = row.get(0)?;
            let input_hash: Vec<u8> = row.get(1)?;
            let bytes: i64 = row.get(2)?;
            let rustlab_version: String = row.get(3)?;
            let created_at: i64 = row.get(4)?;
            let fn_name: Option<String> = row.get(5)?;
            let mut full_entry: [u8; 32] = [0u8; 32];
            for (i, b) in entry_id.iter().take(32).enumerate() {
                full_entry[i] = *b;
            }
            Ok(ListEntry {
                entry_id_short: short_hex(&entry_id),
                input_hash_short: short_hex(&input_hash),
                bytes: bytes.max(0) as u64,
                rustlab_version,
                created_at,
                fn_name,
                entry_id: full_entry,
            })
        })?;
        let mut out: Vec<ListEntry> = Vec::new();
        for row in rows {
            out.push(row?);
            if let Some(n) = limit {
                if out.len() >= n {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Read a `schema_meta` value by key. Returns `None` if the key is
    /// absent; useful in tests and for diagnostic CLIs.
    pub fn schema_meta(&self, key: &str) -> Result<Option<String>, CacheError> {
        let conn = self.conn.lock().expect("cache mutex poisoned");
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    }
}

/// Schema DDL — kept in one batch so a fresh DB lands in a single
/// transaction. `WITHOUT ROWID` keeps the table tight for the (BLOB,
/// BLOB) primary key. No secondary indexes — prune scans the whole
/// table, which is fine at <100k expected rows.
///
/// `fn_metadata` is a sibling table that maps each `entry_id` (which
/// is opaque BLAKE3 bytes) to the function name that produced it. The
/// dispatcher records `(entry_id, fn_name)` on the first write to a
/// new entry; subsequent writes `INSERT OR IGNORE`. Adding the table
/// to an existing DB is idempotent — `CREATE TABLE IF NOT EXISTS`
/// makes the upgrade transparent. Old rows in `cache_entries` without
/// matching metadata get `<unknown>` displayed by `cache list`.
const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS cache_entries (
  entry_id        BLOB NOT NULL,
  input_hash      BLOB NOT NULL,
  value           BLOB NOT NULL,
  bytes           INTEGER NOT NULL,
  rustlab_version TEXT NOT NULL,
  created_at      INTEGER NOT NULL,
  PRIMARY KEY (entry_id, input_hash)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS fn_metadata (
  entry_id   BLOB PRIMARY KEY,
  fn_name    TEXT NOT NULL,
  first_seen INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS schema_meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
";

fn rustlab_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// One row from [`Store::list_entries`]. Shortened hex of the two
/// keys (8 chars each — enough to disambiguate at expected cache
/// sizes), the function name from `fn_metadata` (`None` for legacy
/// rows that predate the metadata table), the full 32-byte
/// `entry_id` so callers can compare against currently-loaded user
/// functions for a "loaded" status, plus size + metadata. No cached
/// value is exposed.
#[derive(Debug, Clone)]
pub struct ListEntry {
    pub entry_id_short: String,
    pub input_hash_short: String,
    pub bytes: u64,
    pub rustlab_version: String,
    pub created_at: i64,
    pub fn_name: Option<String>,
    pub entry_id: [u8; 32],
}

fn short_hex(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(8);
    for b in bytes.iter().take(4) {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// `true` for SQLite errors we treat as "don't break the user's
/// session, just skip the write." Disk-full and read-only-DB are the
/// main two; everything else propagates so the caller (or tests) can
/// surface it.
fn is_transient_write_error(e: &rusqlite::Error) -> bool {
    if let rusqlite::Error::SqliteFailure(ffi_err, _) = e {
        matches!(
            ffi_err.code,
            ErrorCode::DiskFull | ErrorCode::ReadOnly | ErrorCode::CannotOpen
        )
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_db(name: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(name);
        (dir, path)
    }

    #[test]
    fn open_creates_db_and_schema() {
        let (_dir, path) = tmp_db("cache.db");
        let store = Store::open(&path).expect("open");
        assert!(path.exists(), "db file should exist after open");

        // Schema version recorded.
        let version = store.schema_meta(SCHEMA_VERSION_KEY).unwrap();
        assert_eq!(version.as_deref(), Some("1"));

        // Rustlab version recorded.
        let rl = store.schema_meta(RUSTLAB_VERSION_KEY).unwrap();
        assert_eq!(rl.as_deref(), Some(rustlab_version()));
    }

    #[test]
    fn put_get_roundtrip() {
        let (_dir, path) = tmp_db("rt.db");
        let store = Store::open(&path).unwrap();
        let entry_id = [1u8; 32];
        let input_hash = [2u8; 32];

        assert_eq!(store.get(&entry_id, &input_hash).unwrap(), None);
        store.put(&entry_id, &input_hash, b"hello").unwrap();
        assert_eq!(
            store.get(&entry_id, &input_hash).unwrap().as_deref(),
            Some(&b"hello"[..])
        );
        assert_eq!(store.entry_count().unwrap(), 1);
        assert_eq!(store.total_bytes().unwrap(), 5);
    }

    #[test]
    fn reopen_preserves_entries() {
        let (_dir, path) = tmp_db("reopen.db");
        let entry_id = [7u8; 32];
        let input_hash = [9u8; 32];
        {
            let store = Store::open(&path).unwrap();
            store.put(&entry_id, &input_hash, b"persists").unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(
            store.get(&entry_id, &input_hash).unwrap().as_deref(),
            Some(&b"persists"[..])
        );
    }

    #[test]
    fn duplicate_put_is_ignored_not_an_error() {
        let (_dir, path) = tmp_db("dup.db");
        let store = Store::open(&path).unwrap();
        let entry_id = [3u8; 32];
        let input_hash = [4u8; 32];
        store.put(&entry_id, &input_hash, b"first").unwrap();
        // OR IGNORE: second insert succeeds, original value wins.
        store.put(&entry_id, &input_hash, b"second").unwrap();
        assert_eq!(
            store.get(&entry_id, &input_hash).unwrap().as_deref(),
            Some(&b"first"[..]),
            "OR IGNORE keeps the first writer's value"
        );
        assert_eq!(store.entry_count().unwrap(), 1);
    }

    #[test]
    fn distinct_keys_dont_collide() {
        let (_dir, path) = tmp_db("distinct.db");
        let store = Store::open(&path).unwrap();
        let id_a = [10u8; 32];
        let id_b = [11u8; 32];
        let hash = [99u8; 32];
        store.put(&id_a, &hash, b"A").unwrap();
        store.put(&id_b, &hash, b"B").unwrap();
        assert_eq!(store.get(&id_a, &hash).unwrap().as_deref(), Some(&b"A"[..]));
        assert_eq!(store.get(&id_b, &hash).unwrap().as_deref(), Some(&b"B"[..]));
        assert_eq!(store.entry_count().unwrap(), 2);
    }

    #[test]
    fn clear_wipes_entries_but_keeps_meta() {
        let (_dir, path) = tmp_db("clear.db");
        let store = Store::open(&path).unwrap();
        store.put(&[1; 32], &[1; 32], b"x").unwrap();
        store.put(&[2; 32], &[2; 32], b"y").unwrap();
        assert_eq!(store.clear().unwrap(), 2);
        assert_eq!(store.entry_count().unwrap(), 0);
        // Meta keys still present.
        assert_eq!(
            store.schema_meta(SCHEMA_VERSION_KEY).unwrap().as_deref(),
            Some("1")
        );
    }

    #[test]
    fn prune_older_than_drops_old_rows_only() {
        let (_dir, path) = tmp_db("prune.db");
        let store = Store::open(&path).unwrap();
        store.put(&[1; 32], &[1; 32], b"x").unwrap();
        // Backdate this row by 100 seconds via direct SQL.
        {
            let conn = store.conn.lock().unwrap();
            conn.execute("UPDATE cache_entries SET created_at = created_at - 100", [])
                .unwrap();
        }
        store.put(&[2; 32], &[2; 32], b"y").unwrap();
        assert_eq!(store.entry_count().unwrap(), 2);
        // Prune everything older than 50 s → only the backdated row.
        assert_eq!(store.prune_older_than(50).unwrap(), 1);
        assert_eq!(store.entry_count().unwrap(), 1);
    }

    #[test]
    fn newer_schema_disables_store_silently() {
        let (_dir, path) = tmp_db("future.db");
        {
            // Pretend a future binary wrote schema version 999.
            let store = Store::open(&path).unwrap();
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "UPDATE schema_meta SET value = '999' WHERE key = ?1",
                params![SCHEMA_VERSION_KEY],
            )
            .unwrap();
        }
        // Reopen — should succeed, in disabled mode.
        let store = Store::open(&path).unwrap();
        assert!(store.is_disabled());
        // All ops no-op silently.
        store.put(&[1; 32], &[1; 32], b"ignored").unwrap();
        assert_eq!(store.get(&[1; 32], &[1; 32]).unwrap(), None);
        assert_eq!(store.entry_count().unwrap(), 0);
    }

    #[test]
    fn rustlab_version_updates_on_reopen() {
        let (_dir, path) = tmp_db("rlver.db");
        {
            let store = Store::open(&path).unwrap();
            // Forge an older version string.
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "UPDATE schema_meta SET value = '0.0.1' WHERE key = ?1",
                params![RUSTLAB_VERSION_KEY],
            )
            .unwrap();
        }
        // Reopen with the real binary; the open path should overwrite.
        let store = Store::open(&path).unwrap();
        assert_eq!(
            store.schema_meta(RUSTLAB_VERSION_KEY).unwrap().as_deref(),
            Some(rustlab_version()),
            "open() should overwrite the stored rustlab_version"
        );
    }

    #[test]
    fn put_records_rustlab_version_per_row() {
        let (_dir, path) = tmp_db("perrow.db");
        let store = Store::open(&path).unwrap();
        store.put(&[1; 32], &[1; 32], b"x").unwrap();
        let conn = store.conn.lock().unwrap();
        let v: String = conn
            .query_row(
                "SELECT rustlab_version FROM cache_entries LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(v, rustlab_version());
    }

    #[test]
    fn list_entries_returns_short_hex_keys() {
        let (_dir, path) = tmp_db("list.db");
        let store = Store::open(&path).unwrap();
        store.put(&[0xab; 32], &[0xcd; 32], b"v1").unwrap();
        store.put(&[0xef; 32], &[0x12; 32], b"v22").unwrap();
        let mut rows = store.list_entries(None).unwrap();
        rows.sort_by(|a, b| a.entry_id_short.cmp(&b.entry_id_short));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].entry_id_short.len(), 8);
        assert_eq!(rows[0].input_hash_short.len(), 8);
        assert_eq!(rows[0].entry_id_short, "abababab");
        assert_eq!(rows[1].entry_id_short, "efefefef");
        let bytes_total: u64 = rows.iter().map(|r| r.bytes).sum();
        assert_eq!(bytes_total, 5);
    }

    #[test]
    fn list_entries_respects_limit() {
        let (_dir, path) = tmp_db("limit.db");
        let store = Store::open(&path).unwrap();
        for i in 0..5u8 {
            store.put(&[i; 32], &[i; 32], &[i]).unwrap();
        }
        assert_eq!(store.list_entries(Some(3)).unwrap().len(), 3);
        assert_eq!(store.list_entries(None).unwrap().len(), 5);
    }

    #[test]
    fn prune_to_max_size_drops_oldest_first() {
        let (_dir, path) = tmp_db("maxsz.db");
        let store = Store::open(&path).unwrap();
        // Three rows: 100, 200, 300 bytes; total 600.
        for (i, n) in [100usize, 200, 300].iter().enumerate() {
            let id = [i as u8 + 1; 32];
            store.put(&id, &id, &vec![0u8; *n]).unwrap();
        }
        assert_eq!(store.total_bytes().unwrap(), 600);

        // Backdate the first row so it's the oldest by created_at.
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "UPDATE cache_entries SET created_at = created_at - 100
                 WHERE entry_id = ?1",
                params![&[1u8; 32][..]],
            )
            .unwrap();
        }

        // Cap at 350 → must drop 100-byte (oldest) and 200-byte rows,
        // leaving the 300-byte row.
        let removed = store.prune_to_max_size(350).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(store.total_bytes().unwrap(), 300);
        assert_eq!(store.entry_count().unwrap(), 1);

        // Cap already satisfied → no-op.
        assert_eq!(store.prune_to_max_size(1000).unwrap(), 0);
    }

    #[test]
    fn put_with_meta_records_fn_name_in_listing() {
        let (_dir, path) = tmp_db("meta.db");
        let store = Store::open(&path).unwrap();
        store
            .put_with_meta(&[1u8; 32], &[1u8; 32], b"v1", "expensive")
            .unwrap();
        store
            .put_with_meta(&[1u8; 32], &[2u8; 32], b"v2", "expensive")
            .unwrap();
        store
            .put_with_meta(&[2u8; 32], &[3u8; 32], b"v3", "tiny")
            .unwrap();
        let rows = store.list_entries(None).unwrap();
        let names: std::collections::BTreeSet<String> = rows
            .iter()
            .filter_map(|r| r.fn_name.clone())
            .collect();
        assert_eq!(
            names,
            std::collections::BTreeSet::from(["expensive".to_string(), "tiny".to_string()])
        );
        // Both entries for `expensive` share the same entry_id and so
        // share the same single metadata row — INSERT OR IGNORE keeps
        // the first observation.
        let expensive_count = rows
            .iter()
            .filter(|r| r.fn_name.as_deref() == Some("expensive"))
            .count();
        assert_eq!(expensive_count, 2, "both input variants of expensive present");
        for row in &rows {
            assert_eq!(row.entry_id.len(), 32, "full entry_id captured");
        }
    }

    #[test]
    fn legacy_row_without_metadata_shows_none_fn_name() {
        let (_dir, path) = tmp_db("legacy.db");
        let store = Store::open(&path).unwrap();
        // Use the metadata-free `put` to simulate a row written by an
        // older binary that predates the `fn_metadata` table.
        store.put(&[9u8; 32], &[9u8; 32], b"old").unwrap();
        let rows = store.list_entries(None).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].fn_name.is_none(), "legacy row has no metadata");
    }

    #[test]
    #[cfg(unix)]
    fn read_only_directory_surfaces_clear_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let ro = dir.path().join("ro");
        std::fs::create_dir(&ro).unwrap();
        // Strip write permission. Note: running as root bypasses this;
        // CI usually doesn't run tests as root.
        let mut perms = std::fs::metadata(&ro).unwrap().permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&ro, perms).unwrap();

        let path = ro.join("cache.db");
        let err = match Store::open(&path) {
            Ok(_) => panic!("open should fail on RO dir"),
            Err(e) => e,
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("cannot open cache"),
            "error message should be clear: {msg}"
        );
    }
}
