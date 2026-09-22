//! Optional filesystem path jail for notebook (and other sandboxed) execution.
//!
//! When a jail root is installed via [`set_path_jail`], every path checked
//! through [`check_path`] / [`check_path_buf`] must resolve inside that root
//! after canonicalization. Relative paths are resolved against the process
//! cwd (the notebook executor already `chdir`s to the notebook directory).
//!
//! Outside a notebook — REPL, `rustlab run`, library callers — the jail is
//! unset and path checks are no-ops (the path is returned unchanged).

use std::cell::RefCell;
use std::path::{Component, Path, PathBuf};

thread_local! {
    static PATH_JAIL: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Install (or clear) the path jail for the current thread.
///
/// Pass `Some(canonical_root)` to enable; `None` to disable. The notebook
/// executor sets this to the directory containing the notebook before
/// executing code blocks.
pub fn set_path_jail(root: Option<PathBuf>) {
    PATH_JAIL.with(|j| *j.borrow_mut() = root);
}

/// Current jail root, if any.
pub fn path_jail() -> Option<PathBuf> {
    PATH_JAIL.with(|j| j.borrow().clone())
}

/// RAII guard that restores the previous jail when dropped.
pub struct PathJailGuard {
    prev: Option<PathBuf>,
}

impl PathJailGuard {
    /// Set `root` as the jail for the remainder of this guard's lifetime.
    pub fn new(root: Option<PathBuf>) -> Self {
        let prev = path_jail();
        set_path_jail(root);
        Self { prev }
    }
}

impl Drop for PathJailGuard {
    fn drop(&mut self) {
        set_path_jail(self.prev.take());
    }
}

/// Resolve `path` and verify it lies inside the active jail (if any).
///
/// Returns the canonical path on success. When no jail is set, returns
/// `PathBuf::from(path)` without touching the filesystem.
pub fn check_path(path: &str) -> Result<PathBuf, String> {
    check_path_buf(Path::new(path))
}

/// [`check_path`] for a [`Path`].
pub fn check_path_buf(path: &Path) -> Result<PathBuf, String> {
    let Some(jail) = path_jail() else {
        return Ok(path.to_path_buf());
    };
    let jail = jail
        .canonicalize()
        .map_err(|e| format!("path jail root {}: {e}", jail.display()))?;

    // Reject absolute escapes and `..` walks that leave the jail before we
    // even hit the filesystem (covers non-existent targets).
    if path_has_parent_escape(path) {
        return Err(format!(
            "path escapes notebook directory: {}",
            path.display()
        ));
    }

    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cannot read cwd: {e}"))?
            .join(path)
    };

    let resolved = resolve_existing_prefix(&abs)?;
    if !resolved.starts_with(&jail) {
        return Err(format!(
            "path escapes notebook directory: {}",
            path.display()
        ));
    }
    Ok(resolved)
}

/// True if any component is `..` (after skipping `.`). Absolute paths that
/// don't start under the jail are caught later via canonicalize.
fn path_has_parent_escape(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::ParentDir))
}

/// Canonicalize as much of `path` as exists, then re-append the missing
/// suffix so new files (e.g. `save("out.npy")`) can be checked too.
fn resolve_existing_prefix(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()));
    }
    let mut suffix = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        if let Some(name) = cur.file_name() {
            suffix.push(name.to_os_string());
        } else {
            break;
        }
        cur.pop();
        if cur.as_os_str().is_empty() {
            break;
        }
        if cur.exists() {
            let mut canon = cur
                .canonicalize()
                .map_err(|e| format!("{}: {e}", cur.display()))?;
            for part in suffix.into_iter().rev() {
                canon.push(part);
            }
            return Ok(canon);
        }
    }
    Err(format!(
        "path escapes notebook directory: {}",
        path.display()
    ))
}

/// Same check for embed / notebook-crate callers that already know the jail
/// root (no thread-local). `candidate` is joined against `host_dir` if
/// relative. Returns the canonical path inside `jail_root`.
pub fn check_under_root(candidate: &Path, jail_root: &Path) -> Result<PathBuf, String> {
    let jail = jail_root
        .canonicalize()
        .map_err(|e| format!("path jail root {}: {e}", jail_root.display()))?;

    if path_has_parent_escape(candidate) {
        return Err(format!(
            "path escapes notebook directory: {}",
            candidate.display()
        ));
    }

    let resolved = if candidate.exists() {
        candidate
            .canonicalize()
            .map_err(|e| format!("{}: {e}", candidate.display()))?
    } else {
        resolve_existing_prefix(candidate)?
    };

    if !resolved.starts_with(&jail) {
        return Err(format!(
            "path escapes notebook directory: {}",
            candidate.display()
        ));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn no_jail_is_passthrough() {
        let _g = PathJailGuard::new(None);
        let p = check_path("../../etc/passwd").unwrap();
        assert_eq!(p, PathBuf::from("../../etc/passwd"));
    }

    #[test]
    fn rejects_parent_escape() {
        let dir = tempdir().unwrap();
        let jail = dir.path().canonicalize().unwrap();
        let _g = PathJailGuard::new(Some(jail));
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let err = check_path("../secret.txt").unwrap_err();
        assert!(err.contains("escapes notebook directory"), "{err}");
        std::env::set_current_dir(prev).unwrap();
    }

    #[test]
    fn allows_in_jail_relative() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("data.csv"), "1,2\n").unwrap();
        let jail = dir.path().canonicalize().unwrap();
        let _g = PathJailGuard::new(Some(jail.clone()));
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let got = check_path("data.csv").unwrap();
        assert_eq!(got, jail.join("data.csv"));
        std::env::set_current_dir(prev).unwrap();
    }

    #[test]
    fn allows_new_file_in_jail() {
        let dir = tempdir().unwrap();
        let jail = dir.path().canonicalize().unwrap();
        let _g = PathJailGuard::new(Some(jail.clone()));
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let got = check_path("out.npy").unwrap();
        assert_eq!(got, jail.join("out.npy"));
        std::env::set_current_dir(prev).unwrap();
    }

    #[test]
    fn check_under_root_rejects_symlink_escape() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret.md"), "nope").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                outside.path().join("secret.md"),
                dir.path().join("link.md"),
            )
            .unwrap();
            let err = check_under_root(&dir.path().join("link.md"), dir.path()).unwrap_err();
            assert!(err.contains("escapes"), "{err}");
        }
    }
}
