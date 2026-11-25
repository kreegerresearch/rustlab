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
/// watch. Quoted glob patterns are expanded first ([`expand_watch_args`]).
pub fn resolve_watch_inputs(inputs: &[PathBuf]) -> Result<WatchSet> {
    let expanded = expand_watch_args(inputs)?;
    let inputs = expanded.as_slice();
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

/// Folders a `**` walk never enters, besides any hidden (`.`) folder.
const SKIPPED_DIRS: &[&str] = &["target", "node_modules"];

/// Expand quoted glob patterns among the `watch` arguments.
///
/// An argument is a pattern when it contains `*`, `?`, or `[` and does
/// not name an existing path (so a file called `a[1].md` stays literal).
/// `*`, `?`, and `[...]` (`[!...]` / `[^...]` negate, `a-z` ranges)
/// match within one path segment; a whole `**` segment matches any
/// number of folders. A pattern's matches are `.md` / `.rlab` files only,
/// sorted by path, and take the pattern's place in the argument list, so
/// order between arguments is kept. Folders and other files are skipped.
/// Wildcards do not match hidden names unless the pattern segment starts
/// with `.`. A `**` walk skips hidden folders, `target`, and
/// `node_modules`, and does not enter symlinked folders, so a link back
/// up the tree cannot loop. A pattern with no matches is an error that
/// names it. Literal arguments pass through unchanged; shell-expanded
/// globs arrive as literals.
pub fn expand_watch_args(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::with_capacity(inputs.len());
    for arg in inputs {
        let text = arg.to_string_lossy();
        if !has_glob_meta(&text) || arg.exists() {
            out.push(arg.clone());
            continue;
        }
        let (base, segments) = split_pattern(&text);
        let mut found = Vec::new();
        walk_pattern(&base, &segments, &mut found);
        if found.is_empty() {
            anyhow::bail!("pattern {text} matched no .md or .rlab files");
        }
        found.sort();
        found.dedup();
        out.extend(found);
    }
    Ok(out)
}

/// True when `arg` has a glob metacharacter (`*`, `?`, `[`).
pub fn has_glob_meta(arg: &str) -> bool {
    arg.contains(['*', '?', '['])
}

fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// Split a pattern into the literal folder before its first wildcard
/// segment (kept verbatim, so `/abs/`, `C:\x\`, and `./` survive) and
/// the segments from there on.
fn split_pattern(text: &str) -> (PathBuf, Vec<String>) {
    let mut start = 0;
    let mut cut = 0;
    for (i, c) in text.char_indices() {
        if is_separator(c) {
            if has_glob_meta(&text[start..i]) {
                break;
            }
            start = i + c.len_utf8();
            cut = start;
        }
    }
    let base = PathBuf::from(&text[..cut]);
    let segments = text[cut..]
        .split(is_separator)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    (base, segments)
}

fn is_notebook_file(path: &Path) -> bool {
    (has_extension(path, "md") || is_script(path)) && path.is_file()
}

/// Entries of `dir` (`.` when empty) as `(name, path, is_real_dir)`.
/// `is_real_dir` does not follow symlinks.
fn entries(dir: &Path) -> Vec<(String, PathBuf, bool)> {
    let read = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let Ok(rd) = std::fs::read_dir(read) else {
        return Vec::new();
    };
    rd.filter_map(|e| e.ok())
        .map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let real_dir = e.file_type().is_ok_and(|t| t.is_dir());
            (name, dir.join(e.file_name()), real_dir)
        })
        .collect()
}

fn walk_pattern(dir: &Path, segments: &[String], out: &mut Vec<PathBuf>) {
    let Some((seg, rest)) = segments.split_first() else {
        return;
    };
    if seg == "**" {
        // A trailing `**` means every notebook below.
        let star = ["*".to_string()];
        let rest: &[String] = if rest.is_empty() { &star } else { rest };
        walk_pattern(dir, rest, out);
        for (name, path, real_dir) in entries(dir) {
            if real_dir && !name.starts_with('.') && !SKIPPED_DIRS.contains(&name.as_str()) {
                walk_pattern(&path, segments, out);
            }
        }
        return;
    }
    let mut visit = |path: PathBuf| {
        if rest.is_empty() {
            if is_notebook_file(&path) {
                out.push(path);
            }
        } else if path.is_dir() {
            walk_pattern(&path, rest, out);
        }
    };
    if !has_glob_meta(seg) {
        visit(dir.join(seg));
        return;
    }
    for (name, path, _) in entries(dir) {
        if name.starts_with('.') && !seg.starts_with('.') {
            continue;
        }
        if glob_match(seg, &name) {
            visit(path);
        }
    }
}

/// Match one path segment against a pattern segment.
fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0, 0);
    // Where the last `*` resumes: (pattern index after it, name index).
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() {
            match p[pi] {
                '*' => {
                    while pi < p.len() && p[pi] == '*' {
                        pi += 1;
                    }
                    star = Some((pi, ni));
                    continue;
                }
                '?' => {
                    pi += 1;
                    ni += 1;
                    continue;
                }
                '[' => match class_match(&p[pi..], n[ni]) {
                    Some((true, len)) => {
                        pi += len;
                        ni += 1;
                        continue;
                    }
                    Some((false, _)) => {}
                    // An unclosed `[` is a literal.
                    None if n[ni] == '[' => {
                        pi += 1;
                        ni += 1;
                        continue;
                    }
                    None => {}
                },
                c if c == n[ni] => {
                    pi += 1;
                    ni += 1;
                    continue;
                }
                _ => {}
            }
        }
        match star {
            Some((sp, sn)) => {
                pi = sp;
                ni = sn + 1;
                star = Some((sp, sn + 1));
            }
            None => return false,
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// `[...]` at the start of `p` against `c`: `Some((matched, length))`,
/// or `None` when the class is not closed.
fn class_match(p: &[char], c: char) -> Option<(bool, usize)> {
    let mut i = 1;
    let negate = matches!(p.get(1), Some('!') | Some('^'));
    if negate {
        i += 1;
    }
    let first = i;
    let mut hit = false;
    while i < p.len() {
        if p[i] == ']' && i > first {
            return Some((hit != negate, i + 1));
        }
        if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
            hit |= p[i] <= c && c <= p[i + 2];
            i += 3;
        } else {
            hit |= p[i] == c;
            i += 1;
        }
    }
    None
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
