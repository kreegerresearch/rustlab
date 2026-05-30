//! Cheap path-based fingerprint for cache key building.
//!
//! Used by future builtins that take a file path (e.g. `load_audio`,
//! `readmatrix`): hashing the file's contents on every call would
//! defeat the speedup the cache exists to deliver, so we hash a
//! short tuple of metadata instead — `(canonical_path, mtime_nanos,
//! size_bytes)`. This catches every "file changed" case we care about
//! in practice (overwrites bump mtime; renames change the path) at the
//! cost of being fooled by mtime-rewinding tools. Users who care about
//! that last case use `cache clear`.
//!
//! Cross-machine portability: the canonical path is host-local, so a
//! cache file shipped between hosts will mostly miss on file-based
//! entries. That's a deliberate trade per the plan's non-goals.

use std::path::Path;
use std::time::UNIX_EPOCH;

const TAG: &[u8] = b"rustlab-cache/file-fp/v1\0";

/// Compute a stable 32-byte fingerprint for a file at `path` based on
/// `(canonical_path, mtime_nanos, size_bytes)`. Returns an `io::Error`
/// if the file can't be canonicalized or stat'd.
pub fn file_fingerprint(path: impl AsRef<Path>) -> std::io::Result<[u8; 32]> {
    let path = path.as_ref();
    let canonical = std::fs::canonicalize(path)?;
    let metadata = std::fs::metadata(&canonical)?;

    // mtime: nanoseconds since UNIX_EPOCH. If the file is somehow
    // older than the epoch (clock skew, restored backup), fall back
    // to 0 — the entry will key consistently within this session.
    let mtime_nanos: u128 = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let size_bytes = metadata.len();

    let mut h = blake3::Hasher::new();
    h.update(TAG);

    // Canonical path: length-prefixed UTF-8 (or platform bytes). On
    // Unix this round-trips; on Windows OsStr is WTF-8 — fine for
    // hashing.
    let path_bytes = canonical.as_os_str().as_encoded_bytes();
    h.update(&(path_bytes.len() as u64).to_le_bytes());
    h.update(path_bytes);

    h.update(&mtime_nanos.to_le_bytes());
    h.update(&size_bytes.to_le_bytes());

    Ok(*h.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn same_file_same_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.bin");
        std::fs::write(&p, b"hello").unwrap();
        let a = file_fingerprint(&p).unwrap();
        let b = file_fingerprint(&p).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_paths_distinct_fingerprints() {
        let dir = tempfile::tempdir().unwrap();
        let p1 = dir.path().join("a.bin");
        let p2 = dir.path().join("b.bin");
        std::fs::write(&p1, b"x").unwrap();
        std::fs::write(&p2, b"x").unwrap();
        assert_ne!(
            file_fingerprint(&p1).unwrap(),
            file_fingerprint(&p2).unwrap(),
            "same content but different path → different fingerprint"
        );
    }

    #[test]
    fn relative_and_absolute_paths_collide() {
        // Canonicalization should collapse different spellings.
        let dir = tempfile::tempdir().unwrap();
        let abs = dir.path().join("c.bin");
        std::fs::write(&abs, b"y").unwrap();

        // Build a relative path from a known parent and compare.
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let rel = std::path::Path::new("./c.bin");
        let from_rel = file_fingerprint(rel).unwrap();
        std::env::set_current_dir(cwd).unwrap();

        let from_abs = file_fingerprint(&abs).unwrap();
        assert_eq!(
            from_rel, from_abs,
            "relative and absolute spellings of the same file should match"
        );
    }

    #[test]
    fn size_change_changes_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("grow.bin");
        std::fs::write(&p, b"small").unwrap();
        let before = file_fingerprint(&p).unwrap();
        // Append rather than overwrite so size definitely changes
        // even on filesystems with coarse mtime resolution (HFS+ has
        // 1-second resolution and the two writes can land in the same
        // second).
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b" but longer").unwrap();
        f.sync_all().unwrap();
        let after = file_fingerprint(&p).unwrap();
        assert_ne!(before, after);
    }

    #[test]
    fn missing_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("does-not-exist.bin");
        assert!(file_fingerprint(&p).is_err());
    }
}
