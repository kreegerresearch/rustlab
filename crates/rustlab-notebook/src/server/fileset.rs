//! What `watch` was asked to serve: one file, one directory, or a set of
//! files rooted at their common ancestor directory.
//!
//! A file set is served like a directory whose listing is exactly the
//! files named on the command line, in that order. Identities and URLs
//! are paths relative to the common ancestor (`ch2/filters.md` →
//! `/n/ch2/filters`), so the drawer shows the real folder tree. The
//! ancestor is only a naming root: it is not watched recursively and it
//! is not the file-I/O jail (see [`default_jail_root`]).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::http::ServerState;

/// Resolved `watch` inputs. Paths are canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchSet {
    /// One file (`watch analysis.md`).
    Single(PathBuf),
    /// One directory (`watch notebooks/`).
    Dir(PathBuf),
    /// Two or more distinct files. `root` is their deepest common
    /// ancestor directory; `files` keeps command-line order.
    Files { root: PathBuf, files: Vec<PathBuf> },
}

impl WatchSet {
    /// Collection root for identities and the drawer. `None` for one file.
    pub fn collection_root(&self) -> Option<&Path> {
        match self {
            WatchSet::Single(_) => None,
            WatchSet::Dir(root) | WatchSet::Files { root, .. } => Some(root),
        }
    }
}

/// A `.rlab` script (any case). `watch` serves it as a read-only page:
/// a heading with the file name and one code cell holding the script.
pub fn is_script(path: &Path) -> bool {
    has_extension(path, "rlab")
}

fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

/// Resolve the `watch` positional paths.
///
/// One path keeps today's behaviour (a file is single-file watch, a
/// directory is directory watch). Several paths must all be `.md` or
/// `.rlab` files; a directory among them is refused, because a directory
/// listing hides partials and `index.md` while a named file is always
/// served, and mixing the two rules would be guesswork. Every missing
/// path is reported at once. The same file named twice (any spelling or
/// symlink) is served once; if that leaves one file it is single-file
/// watch.
pub fn resolve_watch_inputs(inputs: &[PathBuf]) -> Result<WatchSet> {
    if inputs.is_empty() {
        anyhow::bail!("watch needs a notebook file, a directory, or several files");
    }
    let missing: Vec<String> = inputs
        .iter()
        .filter(|p| !p.exists())
        .map(|p| format!("{} does not exist", p.display()))
        .collect();
    if !missing.is_empty() {
        anyhow::bail!("{}", missing.join("\n"));
    }
    if let [only] = inputs {
        let canon =
            std::fs::canonicalize(only).with_context(|| format!("resolving {}", only.display()))?;
        return Ok(if canon.is_dir() {
            WatchSet::Dir(canon)
        } else {
            WatchSet::Single(canon)
        });
    }

    let mut files: Vec<PathBuf> = Vec::with_capacity(inputs.len());
    for path in inputs {
        let canon =
            std::fs::canonicalize(path).with_context(|| format!("resolving {}", path.display()))?;
        if canon.is_dir() {
            anyhow::bail!(
                "{} is a directory; pass a directory alone, or list the files in it",
                path.display()
            );
        }
        if !(has_extension(&canon, "md") || is_script(&canon)) {
            anyhow::bail!(
                "{} is not a .md notebook or a .rlab script; watch serves only those",
                path.display()
            );
        }
        if files.contains(&canon) {
            eprintln!(
                "[watch] {} is listed more than once; serving it once",
                path.display()
            );
            continue;
        }
        files.push(canon);
    }
    if files.len() == 1 {
        return Ok(WatchSet::Single(files.remove(0)));
    }
    let root = common_ancestor(&files)
        .context("the files share no common folder (different drives?); watch them separately")?;
    Ok(WatchSet::Files { root, files })
}

/// Expand quoted glob patterns among the `watch` arguments.
pub fn expand_watch_args(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    Ok(inputs.to_vec())
}

pub(crate) fn has_glob_meta(_arg: &str) -> bool {
    false
}

fn glob_match(_pattern: &str, _name: &str) -> bool {
    false
}

/// Deepest directory that contains every path in `files`.
fn common_ancestor(files: &[PathBuf]) -> Option<PathBuf> {
    let mut root = files.first()?.parent()?.to_path_buf();
    for file in &files[1..] {
        while !file.starts_with(&root) {
            if !root.pop() {
                return None;
            }
        }
    }
    Some(root)
}

/// Path jail for notebook file I/O.
///
/// `--jail-root` wins. A directory is one trust unit, so it jails to
/// itself. One file, and every file of a set, is jailed to its own
/// directory — the same rule as running `watch` on that file alone.
/// The common ancestor of a set is not used: it can be far wider than
/// any folder the user meant to expose (`~/a/x.md ~/b/y.md` would jail
/// to the home directory).
pub fn default_jail_root(set: &WatchSet, explicit: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(p) = explicit {
        let canon = std::fs::canonicalize(p)
            .with_context(|| format!("resolving --jail-root {}", p.display()))?;
        return Ok(Some(canon));
    }
    Ok(match set {
        WatchSet::Dir(root) => Some(root.clone()),
        WatchSet::Single(_) | WatchSet::Files { .. } => None,
    })
}

/// Page the browser opens first: the first file named on the command
/// line for a set, `/` otherwise (the index, or the redirect to the lone
/// notebook).
pub fn initial_path(set: &WatchSet, state: &ServerState) -> String {
    if !matches!(set, WatchSet::Files { .. }) {
        return "/".to_string();
    }
    let rel_of = state.rel_by_slug();
    state
        .order
        .first()
        .and_then(|slug| rel_of.get(slug.as_str()))
        .map(|rel| format!("/n/{}", crate::render::watch_public_rel(rel)))
        .unwrap_or_else(|| "/".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_ancestor_walks_up_to_the_shared_folder() {
        let files = vec![
            PathBuf::from("/a/b/c/x.md"),
            PathBuf::from("/a/b/y.md"),
            PathBuf::from("/a/b/d/e/z.rlab"),
        ];
        assert_eq!(common_ancestor(&files), Some(PathBuf::from("/a/b")));
        let siblings = vec![PathBuf::from("/a/x.md"), PathBuf::from("/a/y.md")];
        assert_eq!(common_ancestor(&siblings), Some(PathBuf::from("/a")));
        let apart = vec![PathBuf::from("/a/x.md"), PathBuf::from("/b/y.md")];
        assert_eq!(common_ancestor(&apart), Some(PathBuf::from("/")));
    }

    #[test]
    fn glob_segment_matching() {
        assert!(glob_match("*.md", "intro.md"));
        assert!(glob_match("*.md", ".md"));
        assert!(!glob_match("*.md", "intro.rlab"));
        assert!(glob_match("ch?.md", "ch1.md"));
        assert!(!glob_match("ch?.md", "ch10.md"));
        assert!(glob_match("[a-c]*.md", "b.md"));
        assert!(!glob_match("[a-c]*.md", "d.md"));
        assert!(glob_match("[!a]*.md", "b.md"));
        assert!(!glob_match("[!a]*.md", "a.md"));
        assert!(glob_match("[^a]*.md", "b.md"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("a*b*c", "aXbYc"));
        assert!(!glob_match("a*b*c", "aXbY"));
        assert!(glob_match("lesson-*.md", "lesson-ünï.md"));
        // An unclosed `[` is a literal.
        assert!(glob_match("a[b.md", "a[b.md"));
    }

    #[test]
    fn glob_detection() {
        assert!(has_glob_meta("course/**/*.md"));
        assert!(has_glob_meta("ch?.md"));
        assert!(has_glob_meta("[ab].md"));
        assert!(!has_glob_meta("course/intro.md"));
    }

    #[test]
    fn script_extension_is_case_insensitive() {
        assert!(is_script(Path::new("demo.rlab")));
        assert!(is_script(Path::new("DEMO.RLAB")));
        assert!(!is_script(Path::new("demo.md")));
        assert!(!is_script(Path::new("rlab")));
    }
}
