//! Optional filesystem path jail for notebook (and other sandboxed) execution.
//!
//! When a jail is installed via [`set_path_jail`] / [`PathJailGuard`], every
//! path checked through [`check_path`] / [`check_path_buf`] must resolve
//! inside the jail's `root` after lexical normalisation and canonicalisation
//! of the existing prefix. Relative paths resolve against the jail's `base`
//! (captured when the jail is installed — normally the notebook's own
//! directory), **not** against the process cwd at check time: the cwd is
//! process-global and may be moved by another thread between install and
//! check, which would otherwise make an in-jail relative path look like an
//! escape.
//!
//! Outside a notebook — REPL, `rustlab run`, library callers — the jail is
//! unset and path checks are no-ops (the path is returned unchanged).

use std::cell::RefCell;
use std::path::{Component, Path, PathBuf};

/// An installed jail: `root` bounds every checked path; `base` is what
/// relative paths resolve against. `base` is normally inside `root` (the
/// notebook directory inside its collection), but nothing requires it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathJail {
    pub root: PathBuf,
    pub base: PathBuf,
}

thread_local! {
    static PATH_JAIL: RefCell<Option<PathJail>> = const { RefCell::new(None) };
}

/// Install (or clear) the path jail for the current thread.
///
/// Pass `Some(jail)` to enable; `None` to disable. The notebook executor
/// installs a jail before executing code blocks.
pub fn set_path_jail(jail: Option<PathJail>) {
    PATH_JAIL.with(|j| *j.borrow_mut() = jail);
}

/// Current jail, if any.
pub fn path_jail() -> Option<PathJail> {
    PATH_JAIL.with(|j| j.borrow().clone())
}

/// RAII guard that restores the previous jail when dropped.
pub struct PathJailGuard {
    prev: Option<PathJail>,
}

impl PathJailGuard {
    /// Jail to `root` for the remainder of this guard's lifetime; relative
    /// paths resolve against the process cwd *as captured now*. `None`
    /// clears the jail.
    pub fn new(root: Option<PathBuf>) -> Self {
        let base = std::env::current_dir().ok();
        Self::with_base(root, base)
    }

    /// Jail to `root`, resolving relative paths against `base`. A missing
    /// `base` falls back to `root`.
    pub fn with_base(root: Option<PathBuf>, base: Option<PathBuf>) -> Self {
        let prev = path_jail();
        let jail = root.map(|root| {
            let base = base.unwrap_or_else(|| root.clone());
            PathJail { root, base }
        });
        set_path_jail(jail);
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
/// Returns the resolved path on success. When no jail is set, returns
/// `PathBuf::from(path)` without touching the filesystem.
pub fn check_path(path: &str) -> Result<PathBuf, String> {
    check_path_buf(Path::new(path))
}

/// [`check_path`] for a [`Path`].
pub fn check_path_buf(path: &Path) -> Result<PathBuf, String> {
    let Some(jail) = path_jail() else {
        return Ok(path.to_path_buf());
    };
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        jail.base.join(path)
    };
    resolve_inside(&abs, &jail.root).map_err(|_| escape_error(path))
}

/// Same check for callers that already know the jail root (no
/// thread-local). `candidate` must be absolute or is taken relative to the
/// process cwd. Returns the resolved path inside `jail_root`.
pub fn check_under_root(candidate: &Path, jail_root: &Path) -> Result<PathBuf, String> {
    let abs = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cannot read cwd: {e}"))?
            .join(candidate)
    };
    resolve_inside(&abs, jail_root).map_err(|_| escape_error(candidate))
}

fn escape_error(path: &Path) -> String {
    format!("path escapes notebook directory: {}", path.display())
}

/// Core check: lexically normalise `abs` (so `a/../b` stays inside and
/// `../x` climbs out *before* touching the filesystem), canonicalise the
/// longest existing prefix (resolving symlinks), re-append the missing
/// suffix, and require the result to start with the canonical root.
fn resolve_inside(abs: &Path, root: &Path) -> Result<PathBuf, ()> {
    let root = root.canonicalize().map_err(|_| ())?;
    let normalised = normalize_lexically(abs).ok_or(())?;
    let resolved = resolve_existing_prefix(&normalised).ok_or(())?;
    if resolved.starts_with(&root) {
        Ok(resolved)
    } else {
        Err(())
    }
}

/// Collapse `.` and `..` components without consulting the filesystem.
/// Returns `None` if `..` would climb above the path's root.
fn normalize_lexically(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                // Popping a RootDir/Prefix means we climbed above `/`.
                if !out.pop() || out.as_os_str().is_empty() {
                    return None;
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

/// Canonicalize as much of `path` as exists, then re-append the missing
/// suffix so new files (e.g. `save("out.npy")`) can be checked too. `path`
/// must already be lexically normalised (no `..`).
fn resolve_existing_prefix(path: &Path) -> Option<PathBuf> {
    if path.exists() {
        return path.canonicalize().ok();
    }
    let mut suffix = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        let name = cur.file_name()?.to_os_string();
        suffix.push(name);
        if !cur.pop() || cur.as_os_str().is_empty() {
            return None;
        }
        if cur.exists() {
            let mut canon = cur.canonicalize().ok()?;
            for part in suffix.into_iter().rev() {
                canon.push(part);
            }
            return Some(canon);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn jailed(dir: &Path) -> PathJailGuard {
        let canon = dir.canonicalize().unwrap();
        PathJailGuard::with_base(Some(canon.clone()), Some(canon))
    }

    #[test]
    fn no_jail_is_passthrough() {
        let _g = PathJailGuard::new(None);
        let p = check_path("../../etc/passwd").unwrap();
        assert_eq!(p, PathBuf::from("../../etc/passwd"));
    }

    #[test]
    fn rejects_parent_escape() {
        let dir = tempdir().unwrap();
        let _g = jailed(dir.path());
        let err = check_path("../secret.txt").unwrap_err();
        assert!(err.contains("escapes notebook directory"), "{err}");
    }

    #[test]
    fn rejects_absolute_outside_jail() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let _g = jailed(dir.path());
        let target = outside.path().join("x.csv").to_string_lossy().into_owned();
        assert!(check_path(&target).is_err());
    }

    #[test]
    fn allows_in_jail_relative() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("data.csv"), "1,2\n").unwrap();
        let jail = dir.path().canonicalize().unwrap();
        let _g = jailed(dir.path());
        let got = check_path("data.csv").unwrap();
        assert_eq!(got, jail.join("data.csv"));
    }

    #[test]
    fn allows_new_file_in_jail() {
        let dir = tempdir().unwrap();
        let jail = dir.path().canonicalize().unwrap();
        let _g = jailed(dir.path());
        let got = check_path("out.npy").unwrap();
        assert_eq!(got, jail.join("out.npy"));
    }

    #[test]
    fn allows_dotdot_that_stays_inside() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("data.csv"), "1\n").unwrap();
        let jail = dir.path().canonicalize().unwrap();
        let _g = jailed(dir.path());
        let got = check_path("sub/../data.csv").unwrap();
        assert_eq!(got, jail.join("data.csv"));
    }

    #[test]
    fn relative_paths_resolve_against_base_not_cwd() {
        // A nested notebook (base = <root>/sub) may read <root>/data.csv via
        // `../data.csv` when the jail root is the collection root, and a
        // later cwd change elsewhere in the process must not affect the
        // verdict.
        let dir = tempdir().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(dir.path().join("data.csv"), "1\n").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let _g = PathJailGuard::with_base(Some(root.clone()), Some(sub.canonicalize().unwrap()));
        assert_eq!(check_path("../data.csv").unwrap(), root.join("data.csv"));
        assert_eq!(check_path("local.csv").unwrap(), root.join("sub/local.csv"));
        assert!(check_path("../../outside.csv").is_err());
    }

    #[test]
    fn rejects_dotdot_through_missing_dir() {
        // `nope/` does not exist; lexical normalisation still climbs.
        let dir = tempdir().unwrap();
        let _g = jailed(dir.path());
        assert!(check_path("nope/../../escape.txt").is_err());
        assert!(check_path("nope/../ok.txt").is_ok());
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

    #[test]
    fn check_under_root_accepts_inside_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "x").unwrap();
        let got = check_under_root(&dir.path().join("a.md"), dir.path()).unwrap();
        assert!(got.ends_with("a.md"));
    }
}
