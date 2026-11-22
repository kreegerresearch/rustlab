//! Git diff for interactive `notebook watch`.
//!
//! Off unless `watch --git-diff` or `[notebook] git_diff` is set. When off,
//! nothing here spawns git. When on, every invocation is a dashed plumbing
//! binary (`git-rev-parse`, `git-cat-file`, `git-ls-tree`, `git-for-each-ref`,
//! `git-hash-object`, `git-ls-files`) with config and environment overrides.
//! Name-status is computed in-process from blob ids. The working copy is
//! `git-ls-files` (`--exclude-standard`), not a directory walk. CRLF is
//! folded to LF here; clean filters are not run. There is no shell and
//! no network. Refs are character-checked before they are placed in argv.
//!
//! The stored notebook HTML stays the working-copy render. A request with
//! `?diff=<ref>` (and optional `?base=<ref>`) stamps that copy, or replaces
//! `<main>` with the newer ref's text when both sides are refs.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};

use crate::parse::{Block, CalloutKind, CodeDirectives, MermaidDirectives};
use crate::render::{escape_html, markdown_to_html};
use crate::server::diff::split_blocks;

const MAX_BYTES: usize = 512 * 1024;
const MAX_UNITS: usize = 1500;
const MAX_LINES: usize = 400;

static DIFF_DEFAULT: std::sync::OnceLock<Mutex<Option<String>>> = std::sync::OnceLock::new();
static BLOB_CACHE: Mutex<VecDeque<(String, String)>> = Mutex::new(VecDeque::new());

/// Default ref from `watch --diff`, installed only when git diff is enabled.
/// Used when the request has no `diff` query.
pub fn install_diff_default(v: Option<String>) {
    if let Some(slot) = DIFF_DEFAULT.get() {
        *slot.lock().expect("diff default") = v;
    } else {
        let _ = DIFF_DEFAULT.set(Mutex::new(v));
    }
}

pub fn diff_default() -> Option<String> {
    DIFF_DEFAULT
        .get()
        .and_then(|m| m.lock().ok().map(|g| g.clone()))?
}

/// Query string of a watch page. Absent keys stay `None`.
///
/// `changes` picks a preset (`uncommitted`, `branch`, `last`, `custom`,
/// `off`). With `custom`, `base` is the before side and `compare` the
/// after side (absent = working copy), GitHub's words. Without
/// `changes`, the 0.3.12 names apply: `diff` is the before side and
/// `base` the after side.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WatchQuery {
    pub changes: Option<String>,
    pub diff: Option<String>,
    pub base: Option<String>,
    pub compare: Option<String>,
    pub cm: Option<bool>,
    /// `source` for the unified `.md` diff; anything else is the page.
    pub view: Option<String>,
}

pub fn parse_query(q: &str) -> WatchQuery {
    let mut out = WatchQuery::default();
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut it = pair.splitn(2, '=');
        let key = it.next().unwrap_or("");
        let val = percent_decode(it.next().unwrap_or(""));
        match key {
            "changes" => out.changes = Some(val),
            "diff" => out.diff = Some(val),
            "base" => out.base = Some(val),
            "compare" => out.compare = Some(val),
            "view" => out.view = Some(val),
            "cm" => out.cm = Some(matches!(val.as_str(), "1" | "on" | "true")),
            _ => {}
        }
    }
    out
}

/// What a diff page compares. The before side is a commit (or `index`);
/// the after side is the working copy unless `compare` names a ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Compare {
    /// Working copy vs `HEAD`.
    Uncommitted,
    /// Working copy vs the merge base of `HEAD` and the default branch.
    Branch,
    /// `HEAD~1` vs `HEAD`.
    Last,
    Custom {
        base: String,
        compare: Option<String>,
    },
    /// A `?changes=` value this version does not know.
    Unknown(String),
}

impl Compare {
    /// The `?changes=` value, also stamped on `<body>` for the menu.
    pub fn key(&self) -> &'static str {
        match self {
            Compare::Uncommitted => "uncommitted",
            Compare::Branch => "branch",
            Compare::Last => "last",
            Compare::Custom { .. } | Compare::Unknown(_) => "custom",
        }
    }
}

fn non_empty(s: Option<&String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Before side `base`, after side `compare`. `HEAD` against the working
/// copy is the Uncommitted preset, so old `?diff=HEAD` links light it.
fn custom(base: String, compare: Option<String>) -> Compare {
    if base == "HEAD" && compare.is_none() {
        Compare::Uncommitted
    } else {
        Compare::Custom { base, compare }
    }
}

impl WatchQuery {
    /// `?view=source`.
    pub fn source_view(&self) -> bool {
        self.view.as_deref() == Some("source")
    }
}

/// `None` means the plain page. `?changes=off` (or the old `?diff=off`)
/// forces that. With no diff keys the CLI `--diff` ref is the before side.
pub fn resolve_compare(q: &WatchQuery, cli: Option<&str>) -> Option<Compare> {
    if let Some(c) = q.changes.as_deref() {
        return match c {
            "off" => None,
            "uncommitted" => Some(Compare::Uncommitted),
            "branch" => Some(Compare::Branch),
            "last" => Some(Compare::Last),
            "custom" => Some(custom(
                non_empty(q.base.as_ref()).unwrap_or_else(|| "HEAD".into()),
                non_empty(q.compare.as_ref()),
            )),
            other => Some(Compare::Unknown(other.to_string())),
        };
    }
    match q.diff.as_deref() {
        Some("off") => None,
        Some(d) => Some(custom(d.trim().to_string(), non_empty(q.base.as_ref()))),
        None => cli.map(|d| custom(d.to_string(), None)),
    }
}

/// Character check shared by `?diff=`, `?base=`, and the branch list.
/// `index` is the staged blob. A range (`..`) is rejected here.
/// `+` is allowed because git branch names may contain it.
pub fn ref_ok(s: &str) -> bool {
    if s.is_empty() || s.len() > 256 || s.starts_with('-') || s.contains("..") {
        return false;
    }
    if s == "index" {
        return true;
    }
    s.bytes().all(|b| {
        matches!(
            b,
            b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'.'
                | b'_'
                | b'/'
                | b'^'
                | b'~'
                | b'@'
                | b'{'
                | b'}'
                | b'+'
                | b'-'
        )
    })
}

fn ref_problem(s: &str) -> Option<&'static str> {
    if s.contains("..") {
        Some("one ref; the page is the working copy.")
    } else if !ref_ok(s) {
        Some("unknown ref")
    } else {
        None
    }
}

/// Repo-relative notebook path. Rejects `..`, absolute paths, and a
/// notebook that sits outside the jail or the repository. A deleted
/// notebook still resolves from its parent directory plus its file name,
/// so a two-ref compare does not require the worktree file to exist.
pub fn safe_rel(notebook: &Path, jail: Option<&Path>, repo: &Path) -> Result<String, ()> {
    let repo = std::fs::canonicalize(repo).map_err(|_| ())?;
    let jail = match jail {
        Some(j) => Some(std::fs::canonicalize(j).map_err(|_| ())?),
        None => None,
    };
    let nb = if notebook.exists() {
        std::fs::canonicalize(notebook).map_err(|_| ())?
    } else {
        let name = notebook.file_name().ok_or(())?;
        let parent = notebook.parent().filter(|p| p.exists()).ok_or(())?;
        let parent = std::fs::canonicalize(parent).map_err(|_| ())?;
        parent.join(name)
    };
    if let Some(jail) = &jail {
        if !nb.starts_with(jail) {
            return Err(());
        }
    }
    let rel = nb.strip_prefix(&repo).map_err(|_| ())?;
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(());
    }
    let s = rel.to_string_lossy().replace('\\', "/");
    if rel_ok(&s) {
        Ok(s)
    } else {
        Err(())
    }
}

fn rel_ok(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && !rel.contains('\0')
        && !rel.contains(':')
        && rel
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
}

/// `cat-file blob` arguments. `cat-file` has no `--no-textconv`; the
/// blob is the raw object, so diff drivers never see it.
pub fn blob_args(rev: &str, rel: &str) -> Vec<String> {
    let spec = if rev == "index" {
        format!(":{rel}")
    } else {
        format!("{rev}:{rel}")
    };
    vec!["blob".into(), "--end-of-options".into(), spec]
}

pub fn verify_args(rev: &str) -> Vec<String> {
    vec![
        "--verify".into(),
        "--end-of-options".into(),
        format!("{rev}^{{commit}}"),
    ]
}

pub fn for_each_ref_args() -> Vec<String> {
    vec![
        "--format=%(refname)%00%(HEAD)%00%(refname:short)%00%(committerdate:unix)".into(),
        "refs/heads".into(),
        "refs/tags".into(),
        "refs/remotes".into(),
    ]
}

/// `-c` equivalents applied as `GIT_CONFIG_KEY_*`. Dashed plumbing
/// binaries do not parse `-c`. `safe.directory` is intentionally absent:
/// a repository that fails the ownership check is refused.
pub fn config_overrides() -> &'static [(&'static str, &'static str)] {
    &[
        ("core.fsmonitor", "false"),
        ("core.hooksPath", "/dev/null"),
        ("core.pager", "cat"),
        ("diff.external", ""),
        ("protocol.allow", "never"),
        ("core.sshCommand", "false"),
        ("credential.helper", ""),
    ]
}

/// Dashed binaries this module is willing to exec. Anything else is refused.
pub fn plumbing_tools() -> &'static [&'static str] {
    &[
        "rev-parse",
        "cat-file",
        "ls-tree",
        "for-each-ref",
        "hash-object",
        "ls-files",
        "merge-base",
    ]
}

const OWNERSHIP_MSG: &str = "refusing this repository (ownership check failed)";
const WALK_CAP: usize = 2000;

#[cfg(test)]
fn argv_clean(args: &[String]) -> bool {
    const BAD: &[char] = &[';', '|', '&', '$', '`', '\n', '<', '>'];
    args.iter().all(|a| !a.chars().any(|c| BAD.contains(&c)))
}

#[derive(Debug)]
enum GitFail {
    Missing,
    NotRepo,
    Unknown,
    Dubious,
}

thread_local! {
    static GIT_SPAWNS: Cell<u64> = const { Cell::new(0) };
}

/// How many git processes this thread has spawned. Tests use it to prove
/// the disabled path never starts git.
pub fn git_spawns() -> u64 {
    GIT_SPAWNS.with(|c| c.get())
}

fn note_spawn() {
    GIT_SPAWNS.with(|c| c.set(c.get().saturating_add(1)));
}

pub fn ownership_refused(stderr: &[u8]) -> bool {
    let s = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    s.contains("dubious ownership") || s.contains("safe.directory")
}

fn oid_ok(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn exec_dir() -> Result<PathBuf, GitFail> {
    static CACHE: OnceLock<Result<PathBuf, ()>> = OnceLock::new();
    match CACHE.get_or_init(|| probe_exec_dir().map_err(|_| ())) {
        Ok(p) => Ok(p.clone()),
        Err(()) => Err(GitFail::Missing),
    }
}

/// `git --exec-path` outside the notebook repo, so a repo `exec.path`
/// or alias cannot redirect the probe.
fn probe_exec_dir() -> Result<PathBuf, GitFail> {
    let mut cmd = Command::new("git");
    cmd.arg("--exec-path");
    let tmp = std::env::temp_dir();
    if tmp.is_dir() {
        cmd.current_dir(&tmp);
    }
    scrub_git_env(&mut cmd);
    note_spawn();
    let out = match cmd.output() {
        Ok(out) => out,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(GitFail::Missing),
        Err(_) => return Err(GitFail::Unknown),
    };
    if !out.status.success() {
        return Err(GitFail::Missing);
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        Err(GitFail::Missing)
    } else {
        Ok(PathBuf::from(s))
    }
}

fn scrub_git_env(cmd: &mut Command) {
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_CONFIG",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_NAMESPACE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_EXEC_PATH",
        // Inherited `0` hides worktrees whose git dir is on another
        // filesystem. Discovery still starts at the notebook's `.git`.
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    ] {
        cmd.env_remove(key);
    }
    for i in 0..64 {
        cmd.env_remove(format!("GIT_CONFIG_KEY_{i}"));
        cmd.env_remove(format!("GIT_CONFIG_VALUE_{i}"));
    }
    cmd.env("GIT_CONFIG_NOSYSTEM", "1");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("GIT_PAGER", "cat");
    cmd.env("GIT_OPTIONAL_LOCKS", "0");
    let overrides = config_overrides();
    cmd.env("GIT_CONFIG_COUNT", overrides.len().to_string());
    for (i, (k, v)) in overrides.iter().enumerate() {
        cmd.env(format!("GIT_CONFIG_KEY_{i}"), *k);
        cmd.env(format!("GIT_CONFIG_VALUE_{i}"), *v);
    }
}

fn run_plumbing(repo: &Path, tool: &str, args: &[String]) -> Result<Output, GitFail> {
    run_plumbing_in(repo, tool, args, None)
}

fn run_plumbing_in(
    repo: &Path,
    tool: &str,
    args: &[String],
    stdin_bytes: Option<&[u8]>,
) -> Result<Output, GitFail> {
    if !plumbing_tools().contains(&tool) {
        return Err(GitFail::Unknown);
    }
    let bin = exec_dir()?.join(format!("git-{tool}"));
    if !bin.is_file() {
        return Err(GitFail::Missing);
    }
    let mut cmd = Command::new(&bin);
    cmd.args(args).current_dir(repo);
    scrub_git_env(&mut cmd);
    if stdin_bytes.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    note_spawn();
    // `spawn` inherits stdout/stderr unless they are piped, and
    // `wait_with_output` only reads the piped handles. An unpiped
    // success looks like an empty repository.
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(GitFail::Missing),
        Err(_) => return Err(GitFail::Unknown),
    };
    if let Some(bytes) = stdin_bytes {
        if let Some(mut sin) = child.stdin.take() {
            let _ = sin.write_all(bytes);
        }
    }
    let out = child.wait_with_output().map_err(|_| GitFail::Unknown)?;
    if !out.status.success() && ownership_refused(&out.stderr) {
        return Err(GitFail::Dubious);
    }
    Ok(out)
}

fn repo_root(start: &Path) -> Result<PathBuf, GitFail> {
    let out = run_plumbing(start, "rev-parse", &["--show-toplevel".into()])?;
    if !out.status.success() {
        return Err(GitFail::NotRepo);
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        Err(GitFail::NotRepo)
    } else {
        Ok(PathBuf::from(s))
    }
}

/// Git directories to watch so a commit or checkout refreshes an open
/// diff even when the notebook file itself did not change. Empty when
/// `start` is not a repository. Does not execute repository config.
pub fn git_watch_paths(start: &Path) -> Vec<PathBuf> {
    let Ok(root) = repo_root(start) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for arg in ["--git-dir", "--git-common-dir"] {
        let Ok(got) = run_plumbing(&root, "rev-parse", &[arg.into()]) else {
            continue;
        };
        if !got.status.success() {
            continue;
        }
        let s = String::from_utf8_lossy(&got.stdout).trim().to_string();
        if s.is_empty() {
            continue;
        }
        let path = PathBuf::from(&s);
        let path = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        if path.is_dir() && !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

fn verify(root: &Path, rev: &str) -> Result<String, GitFail> {
    let out = run_plumbing(root, "rev-parse", &verify_args(rev))?;
    if !out.status.success() {
        return Err(GitFail::Unknown);
    }
    let oid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if oid_ok(&oid) {
        Ok(oid)
    } else {
        Err(GitFail::Unknown)
    }
}

fn short(oid: &str) -> String {
    oid.chars().take(7).collect()
}

/// Merge base of two verified commit ids. `git-merge-base` gets only
/// the two hex ids, so nothing in argv can read as an option.
fn merge_base(root: &Path, a: &str, b: &str) -> Result<String, GitFail> {
    if !oid_ok(a) || !oid_ok(b) {
        return Err(GitFail::Unknown);
    }
    let out = run_plumbing(root, "merge-base", &[a.to_string(), b.to_string()])?;
    if !out.status.success() {
        return Err(GitFail::Unknown);
    }
    let oid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if oid_ok(&oid) {
        Ok(oid)
    } else {
        Err(GitFail::Unknown)
    }
}

/// `main`, else `master`, else the branch `origin/HEAD` points at.
fn default_branch(root: &Path) -> Result<Option<String>, GitFail> {
    for name in ["main", "master"] {
        match verify(root, &format!("refs/heads/{name}")) {
            Ok(_) => return Ok(Some(name.to_string())),
            Err(GitFail::Missing) => return Err(GitFail::Missing),
            Err(GitFail::Dubious) => return Err(GitFail::Dubious),
            Err(_) => {}
        }
    }
    let out = run_plumbing(
        root,
        "rev-parse",
        // A fixed refname, so no `--end-of-options` (rev-parse would echo it).
        &["--abbrev-ref".into(), "refs/remotes/origin/HEAD".into()],
    )?;
    if !out.status.success() {
        return Ok(None);
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() || name == "refs/remotes/origin/HEAD" || !ref_ok(&name) {
        return Ok(None);
    }
    Ok(Some(name))
}

/// The checked-out branch, `None` when `HEAD` is detached.
fn current_branch(root: &Path) -> Result<Option<String>, GitFail> {
    let out = run_plumbing(
        root,
        "rev-parse",
        &["--abbrev-ref".into(), "HEAD".into()],
    )?;
    if !out.status.success() {
        return unborn_branch(root);
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() || name == "HEAD" || !ref_ok(&name) {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}

/// Is `branch` the default branch, or the remote-tracking default it
/// follows (`origin/main` while on `main`)?
fn on_default(current: Option<&str>, default: &str) -> bool {
    match current {
        Some(cur) => cur == default || default.strip_prefix("origin/") == Some(cur),
        None => false,
    }
}

struct NameChange {
    status: char,
    path: String,
    old_path: Option<String>,
}

fn commit_blobs(root: &Path, oid: &str) -> Result<BTreeMap<String, String>, GitFail> {
    if !oid_ok(oid) {
        return Err(GitFail::Unknown);
    }
    let out = run_plumbing(
        root,
        "ls-tree",
        &[
            "-r".into(),
            "-z".into(),
            "--end-of-options".into(),
            oid.to_string(),
        ],
    )?;
    if !out.status.success() {
        return Err(GitFail::Unknown);
    }
    Ok(parse_ls_tree(&out.stdout))
}

fn parse_ls_tree(buf: &[u8]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for rec in buf.split(|b| *b == 0) {
        if rec.is_empty() {
            continue;
        }
        let Some(tab) = rec.iter().position(|b| *b == b'\t') else {
            continue;
        };
        let meta = String::from_utf8_lossy(&rec[..tab]);
        let path = String::from_utf8_lossy(&rec[tab + 1..]).into_owned();
        let mut bits = meta.split_whitespace();
        let mode = bits.next().unwrap_or("");
        let kind = bits.next().unwrap_or("");
        let oid = bits.next().unwrap_or("");
        if kind != "blob" || mode == "120000" || mode == "160000" {
            continue;
        }
        if oid_ok(oid) && rel_ok(&path) {
            map.insert(path, oid.to_string());
        }
    }
    map
}

fn index_blobs(root: &Path) -> Result<BTreeMap<String, String>, GitFail> {
    let out = run_plumbing(root, "ls-files", &["-s".into(), "-z".into()])?;
    if !out.status.success() {
        return Err(GitFail::Unknown);
    }
    let mut map = BTreeMap::new();
    for rec in out.stdout.split(|b| *b == 0) {
        if rec.is_empty() {
            continue;
        }
        let Some(tab) = rec.iter().position(|b| *b == b'\t') else {
            continue;
        };
        let meta = String::from_utf8_lossy(&rec[..tab]);
        let path = String::from_utf8_lossy(&rec[tab + 1..]).into_owned();
        let mut bits = meta.split_whitespace();
        let mode = bits.next().unwrap_or("");
        let oid = bits.next().unwrap_or("");
        let stage = bits.next().unwrap_or("0");
        if mode == "120000" || mode == "160000" || !rel_ok(&path) {
            continue;
        }
        if stage != "0" {
            map.insert(path, format!("unmerged:{oid}"));
        } else if oid_ok(oid) {
            map.insert(path, oid.to_string());
        }
    }
    Ok(map)
}

/// Paths from `git ls-files`, which already applies `.gitignore` and does
/// not descend into a nested repository. Symlinks are returned separately
/// so a tracked symlink is not reported as deleted, and they are not
/// followed. Directories (a nested repo shows up as one path) are dropped.
fn list_worktree(root: &Path) -> Result<(Vec<String>, Vec<String>), GitFail> {
    let out = run_plumbing(
        root,
        "ls-files",
        &[
            "-z".into(),
            "--cached".into(),
            "--others".into(),
            "--exclude-standard".into(),
        ],
    )?;
    if !out.status.success() {
        return Err(GitFail::Unknown);
    }
    let mut files = Vec::new();
    let mut links = Vec::new();
    for rec in out.stdout.split(|b| *b == 0) {
        if rec.is_empty() {
            continue;
        }
        let path = String::from_utf8_lossy(rec).into_owned();
        if path.contains('\n') || !rel_ok(&path) {
            continue;
        }
        let full = root.join(&path);
        let Ok(meta) = std::fs::symlink_metadata(&full) else {
            continue;
        };
        let ft = meta.file_type();
        if ft.is_symlink() {
            links.push(path);
        } else if ft.is_file() {
            files.push(path);
        }
    }
    Ok((files, links))
}

/// Fold CRLF to LF. Notebook text is compared this way so a checkout
/// that only differs by line endings is not a change on every line.
/// Clean/smudge filters are still not run.
fn normalize_eol(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_string();
    }
    s.replace("\r\n", "\n")
}

fn normalize_eol_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.contains(&0) || !bytes.windows(2).any(|w| w == b"\r\n") {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            out.push(b'\n');
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(out)
}

fn hash_files(root: &Path, files: &[String]) -> Result<Vec<String>, GitFail> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let mut input = Vec::new();
    for f in files {
        input.extend_from_slice(f.as_bytes());
        input.push(b'\n');
    }
    let args = vec!["--no-filters".into(), "--stdin-paths".into()];
    let out = run_plumbing_in(root, "hash-object", &args, Some(&input))?;
    if out.status.success() {
        let lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if lines.len() == files.len() {
            return Ok(lines);
        }
    }
    let mut oids = Vec::with_capacity(files.len());
    for f in files {
        let mut one = f.as_bytes().to_vec();
        one.push(b'\n');
        let single = run_plumbing_in(root, "hash-object", &args, Some(&one))?;
        if single.status.success() {
            oids.push(String::from_utf8_lossy(&single.stdout).trim().to_string());
        } else {
            oids.push(String::new());
        }
    }
    Ok(oids)
}

fn hash_stdin(root: &Path, bytes: &[u8]) -> Result<String, GitFail> {
    let args = vec!["--no-filters".into(), "--stdin".into()];
    let out = run_plumbing_in(root, "hash-object", &args, Some(bytes))?;
    if !out.status.success() {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Blob id of the worktree file with CRLF folded to LF, when that
/// changes the bytes. `None` means the raw id already stands.
fn normalized_file_oid(root: &Path, rel: &str) -> Option<String> {
    let bytes = std::fs::read(root.join(rel)).ok()?;
    let norm = normalize_eol_bytes(&bytes)?;
    let oid = hash_stdin(root, &norm).ok()?;
    oid_ok(&oid).then_some(oid)
}

fn worktree_blobs(
    root: &Path,
    old: &BTreeMap<String, String>,
) -> Result<(BTreeMap<String, String>, bool), GitFail> {
    let (files, links) = list_worktree(root)?;
    // Every path that exists at `old` is kept, and every notebook, so a
    // cap on stray untracked files cannot hide the file being read or
    // turn a real deletion into silence.
    let mut selected = Vec::new();
    let mut extra = Vec::new();
    for path in files {
        if old.contains_key(&path) || path.ends_with(".md") {
            selected.push(path);
        } else {
            extra.push(path);
        }
    }
    let mut truncated = false;
    for path in extra {
        if selected.len() >= WALK_CAP {
            truncated = true;
            break;
        }
        selected.push(path);
    }
    let oids = hash_files(root, &selected)?;
    let mut map = BTreeMap::new();
    for (path, mut oid) in selected.into_iter().zip(oids) {
        if !oid_ok(&oid) {
            continue;
        }
        if old.get(&path) != Some(&oid) {
            if let Some(norm) = normalized_file_oid(root, &path) {
                oid = norm;
            }
        }
        map.insert(path, oid);
    }
    for path in links {
        if let Some(prev) = old.get(&path) {
            map.insert(path, prev.clone());
        }
    }
    Ok((map, truncated))
}

fn diff_maps(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
    emit_deletes: bool,
) -> Vec<NameChange> {
    let mut modified = Vec::new();
    let mut added = Vec::new();
    let mut deleted = Vec::new();
    for (path, oid) in new {
        match old.get(path) {
            Some(prev) if prev == oid => {}
            Some(_) => modified.push(NameChange {
                status: 'M',
                path: path.clone(),
                old_path: None,
            }),
            None => added.push((path.clone(), oid.clone())),
        }
    }
    if emit_deletes {
        for (path, oid) in old {
            if !new.contains_key(path) {
                deleted.push((path.clone(), oid.clone()));
            }
        }
    }
    let mut add_count: HashMap<&str, usize> = HashMap::new();
    let mut del_count: HashMap<&str, usize> = HashMap::new();
    for (_, oid) in &added {
        *add_count.entry(oid).or_default() += 1;
    }
    for (_, oid) in &deleted {
        *del_count.entry(oid).or_default() += 1;
    }
    let mut used_del = vec![false; deleted.len()];
    let mut out = modified;
    for (path, oid) in &added {
        let unique =
            add_count[oid.as_str()] == 1 && del_count.get(oid.as_str()).copied().unwrap_or(0) == 1;
        if unique {
            if let Some((i, (old_path, _))) = deleted
                .iter()
                .enumerate()
                .find(|(i, (_, o))| o == oid && !used_del[*i])
            {
                used_del[i] = true;
                out.push(NameChange {
                    status: 'R',
                    path: path.clone(),
                    old_path: Some(old_path.clone()),
                });
                continue;
            }
        }
        out.push(NameChange {
            status: 'A',
            path: path.clone(),
            old_path: None,
        });
    }
    for (i, (path, _)) in deleted.iter().enumerate() {
        if used_del[i] {
            continue;
        }
        out.push(NameChange {
            status: 'D',
            path: path.clone(),
            old_path: None,
        });
    }
    out
}

/// What the worktree is compared with. `Empty` is an unborn `HEAD`
/// (the branch exists and has no commit yet).
enum RevSide {
    Oid(String),
    Index,
    Empty,
}

fn worktree_changes(root: &Path, against: &RevSide) -> Result<Vec<NameChange>, GitFail> {
    let old = match against {
        RevSide::Oid(oid) => commit_blobs(root, oid)?,
        RevSide::Index => index_blobs(root)?,
        RevSide::Empty => BTreeMap::new(),
    };
    let (new, _truncated) = worktree_blobs(root, &old)?;
    // Deletions stay on. The cap only drops untracked non-notebooks,
    // which are not in `old`, so a missing tracked path is a real delete.
    Ok(diff_maps(&old, &new, true))
}

/// `HEAD` (and the branch it names) when that branch has no commit.
fn unborn_branch(root: &Path) -> Result<Option<String>, GitFail> {
    let out = run_plumbing(root, "rev-parse", &["--git-path".into(), "HEAD".into()])?;
    if !out.status.success() {
        return Ok(None);
    }
    let rel = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if rel.is_empty() || rel.contains('\0') {
        return Ok(None);
    }
    let path = PathBuf::from(&rel);
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let Some(rest) = text.trim().strip_prefix("ref: ") else {
        return Ok(None);
    };
    let Some(branch) = rest.trim().strip_prefix("refs/heads/") else {
        return Ok(None);
    };
    if branch.is_empty() || branch.ends_with('/') {
        return Ok(None);
    }
    Ok(Some(branch.to_string()))
}

fn resolve_side(root: &Path, rev: &str) -> Result<RevSide, GitFail> {
    if rev == "index" {
        return Ok(RevSide::Index);
    }
    match verify(root, rev) {
        Ok(oid) => Ok(RevSide::Oid(oid)),
        Err(GitFail::Missing) => Err(GitFail::Missing),
        Err(GitFail::Dubious) => Err(GitFail::Dubious),
        Err(_) => {
            if let Some(branch) = unborn_branch(root)? {
                if rev == "HEAD" || rev == branch || rev == format!("refs/heads/{branch}") {
                    return Ok(RevSide::Empty);
                }
            }
            Err(GitFail::Unknown)
        }
    }
}

/// A renamed notebook whose bytes also changed is an add plus a delete
/// under exact oid matching. When one deleted notebook shares most of
/// its lines with the file on screen, treat that pair as the rename.
fn attach_rename(
    changes: &mut [NameChange],
    root: &Path,
    rev: &str,
    oid: Option<&str>,
    rel: &str,
    new_text: &str,
) {
    let Some(idx) = changes
        .iter()
        .position(|c| c.path == rel && c.status == 'A')
    else {
        return;
    };
    let deleted: Vec<String> = changes
        .iter()
        .filter(|c| c.status == 'D' && c.path.ends_with(".md"))
        .take(32)
        .map(|c| c.path.clone())
        .collect();
    let new_lines: std::collections::HashSet<&str> =
        new_text.lines().filter(|l| !l.trim().is_empty()).collect();
    if new_lines.is_empty() {
        return;
    }
    let mut best_path: Option<String> = None;
    let mut best = 0.0f64;
    let mut second = 0.0f64;
    for path in deleted {
        let Ok(Some(old)) = show_blob(root, rev, &path, oid) else {
            continue;
        };
        if old.contains('\0') {
            continue;
        }
        let old = normalize_eol(&old);
        let old_lines: std::collections::HashSet<&str> =
            old.lines().filter(|l| !l.trim().is_empty()).collect();
        if old_lines.is_empty() {
            continue;
        }
        let inter = new_lines.intersection(&old_lines).count();
        let denom = new_lines.len().max(old_lines.len());
        let score = inter as f64 / denom as f64;
        if score > best {
            second = best;
            best = score;
            best_path = Some(path);
        } else if score > second {
            second = score;
        }
    }
    if best >= 0.5 && best - second >= 0.15 {
        if let Some(path) = best_path {
            changes[idx].status = 'R';
            changes[idx].old_path = Some(path);
        }
    }
}

fn ref_blobs(
    root: &Path,
    rev: &str,
    oid: Option<&str>,
) -> Result<BTreeMap<String, String>, GitFail> {
    if rev == "index" {
        index_blobs(root)
    } else {
        let owned;
        let oid = if let Some(oid) = oid {
            oid
        } else {
            owned = verify(root, rev)?;
            owned.as_str()
        };
        commit_blobs(root, oid)
    }
}

fn show_blob(
    root: &Path,
    rev: &str,
    rel: &str,
    oid: Option<&str>,
) -> Result<Option<String>, GitFail> {
    if !rel_ok(rel) {
        return Ok(None);
    }
    let spec_rev: String = if rev == "index" {
        "index".into()
    } else if let Some(oid) = oid {
        if !oid_ok(oid) {
            return Err(GitFail::Unknown);
        }
        oid.to_string()
    } else {
        verify(root, rev)?
    };
    // The key is the repo plus the commit oid plus the path. A miss is
    // not stored. A path alone must not answer a later ref, and a missing
    // blob in one repository must not answer another.
    let cache_key = (spec_rev != "index").then(|| blob_cache_key(root, &spec_rev, rel));
    if let Some(key) = &cache_key {
        if let Some(hit) = cache_get(key) {
            return Ok(Some(hit));
        }
    }
    let out = run_plumbing(root, "cat-file", &blob_args(&spec_rev, rel))?;
    if !out.status.success() {
        return Ok(None);
    }
    if out.stdout.contains(&0) {
        return Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()));
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if let Some(key) = cache_key {
        cache_put(key, text.clone());
    }
    Ok(Some(text))
}

fn blob_cache_key(root: &Path, oid: &str, rel: &str) -> String {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    format!("{}:{oid}:{rel}", root.display())
}

#[cfg(test)]
fn cache_keys() -> Vec<String> {
    BLOB_CACHE
        .lock()
        .map(|c| c.iter().map(|(k, _)| k.clone()).collect())
        .unwrap_or_default()
}

fn cache_get(key: &str) -> Option<String> {
    BLOB_CACHE
        .lock()
        .ok()?
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

fn cache_put(key: String, val: String) {
    if let Ok(mut cache) = BLOB_CACHE.lock() {
        if let Some(pos) = cache.iter().position(|(k, _)| k == &key) {
            cache.remove(pos);
        }
        cache.push_front((key, val));
        while cache.len() > 32 {
            cache.pop_back();
        }
    }
}

/// Load `rel` at `rev`. A rename in `changes` (new path == `rel`) falls
/// back to the old path, which is the name `rev` still has.
fn load_side(
    root: &Path,
    rev: &str,
    rel: &str,
    changes: &[NameChange],
    oid: Option<&str>,
) -> Result<Option<String>, GitFail> {
    if let Some(text) = show_blob(root, rev, rel, oid)? {
        return Ok(Some(text));
    }
    if let Some(old) = changes
        .iter()
        .find(|c| c.path == rel)
        .and_then(|c| c.old_path.as_deref())
    {
        return show_blob(root, rev, old, oid);
    }
    Ok(None)
}

struct CommitInfo {
    time: i64,
    subject: String,
    parent: Option<String>,
}

fn read_commit(root: &Path, oid: &str) -> Result<CommitInfo, GitFail> {
    if !oid_ok(oid) {
        return Err(GitFail::Unknown);
    }
    let out = run_plumbing(
        root,
        "cat-file",
        &["commit".into(), "--end-of-options".into(), oid.to_string()],
    )?;
    if !out.status.success() {
        return Err(GitFail::Unknown);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parent = None;
    let mut time = None;
    let mut past_header = false;
    let mut subject = String::new();
    for line in text.split('\n') {
        if !past_header {
            if line.is_empty() {
                past_header = true;
                continue;
            }
            if let Some(rest) = line.strip_prefix("parent ") {
                if parent.is_none() {
                    let p = rest.trim();
                    if oid_ok(p) {
                        parent = Some(p.to_string());
                    }
                }
            } else if let Some(rest) = line.strip_prefix("committer ") {
                let toks: Vec<&str> = rest.split_whitespace().collect();
                if toks.len() >= 2 {
                    time = toks[toks.len() - 2].parse().ok();
                }
            }
        } else if subject.is_empty() {
            subject = line.chars().take(72).collect();
            break;
        }
    }
    Ok(CommitInfo {
        time: time.ok_or(GitFail::Unknown)?,
        subject,
        parent,
    })
}

fn file_dots_json(changes: &[NameChange]) -> String {
    let mut files: Vec<serde_json::Value> = Vec::new();
    for c in changes {
        if c.status == 'D' {
            continue;
        }
        let label = match c.status {
            'M' => "modified",
            'A' => "added",
            'R' => "renamed",
            'C' => "copied",
            'T' => "type changed",
            _ => "changed",
        };
        files.push(serde_json::json!({
            "path": c.path,
            "status": c.status.to_string(),
            "label": label,
        }));
    }
    serde_json::json!({ "files": files })
        .to_string()
        .replace('<', "\\u003c")
}

/// `GET /git/refs` body. Not a repository and a missing `git` are 200
/// with an error field so the picker can explain itself.
pub fn refs_json(start: &Path) -> String {
    refs_json_if(true, start)
}

/// Same as [`refs_json`], but `enabled == false` returns immediately and
/// does not spawn git.
pub fn refs_json_if(enabled: bool, start: &Path) -> String {
    if !enabled {
        return refs_error("git diff is off");
    }
    match collect_refs(start) {
        Ok(body) => body,
        Err(GitFail::Missing) => refs_error("git not found"),
        Err(GitFail::Dubious) => refs_error(OWNERSHIP_MSG),
        Err(_) => refs_error("not a git repository"),
    }
}

fn refs_error(msg: &str) -> String {
    serde_json::json!({
        "error": msg,
        "branches": [],
        "tags": [],
        "commits": [],
    })
    .to_string()
}

#[derive(Clone)]
struct Listed {
    name: String,
    current: bool,
    date: i64,
    tag: bool,
    remote: bool,
}

fn collect_refs(start: &Path) -> Result<String, GitFail> {
    let root = repo_root(start)?;
    let out = run_plumbing(&root, "for-each-ref", &for_each_ref_args())?;
    if !out.status.success() {
        return Err(GitFail::NotRepo);
    }
    let mut rows = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.split('\0');
        let full = parts.next().unwrap_or("");
        let head = parts.next().unwrap_or("");
        let short = parts.next().unwrap_or("");
        let date: i64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let tag = full.starts_with("refs/tags/");
        let remote = full.starts_with("refs/remotes/");
        // `?diff=off` means off, so a branch literally named off is sent
        // as its full refname, which still passes the character check.
        let name = if short == "off" { full } else { short };
        if !ref_ok(name) {
            continue;
        }
        rows.push(Listed {
            name: name.to_string(),
            current: head.starts_with('*'),
            date,
            tag,
            remote,
        });
    }
    let heads: Vec<Listed> = rows
        .iter()
        .filter(|r| !r.tag && !r.remote)
        .cloned()
        .collect();
    let mut current: Vec<Listed> = heads.iter().filter(|h| h.current).cloned().collect();
    let mut rest: Vec<Listed> = heads.into_iter().filter(|h| !h.current).collect();
    rest.sort_by(|a, b| {
        fn rank(name: &str) -> u8 {
            if name == "main" {
                0
            } else {
                1
            }
        }
        rank(&a.name)
            .cmp(&rank(&b.name))
            .then(b.date.cmp(&a.date))
            .then(a.name.cmp(&b.name))
    });
    current.append(&mut rest);
    let mut remotes: Vec<Listed> = rows.iter().filter(|r| r.remote).cloned().collect();
    remotes.sort_by(|a, b| b.date.cmp(&a.date).then(a.name.cmp(&b.name)));
    current.append(&mut remotes);
    let mut tags: Vec<Listed> = rows.into_iter().filter(|r| r.tag).collect();
    tags.sort_by(|a, b| b.date.cmp(&a.date).then(a.name.cmp(&b.name)));

    let mut commits = Vec::new();
    if let Ok(head) = verify(&root, "HEAD") {
        let mut oid = head;
        for _ in 0..12 {
            let info = match read_commit(&root, &oid) {
                Ok(info) => info,
                Err(GitFail::Dubious) => return Err(GitFail::Dubious),
                Err(GitFail::Missing) => return Err(GitFail::Missing),
                Err(_) => break,
            };
            let short: String = oid.chars().take(7).collect();
            if ref_ok(&oid) && ref_ok(&short) {
                let subject = info.subject;
                commits.push(serde_json::json!({
                    "rev": oid,
                    "label": format!("{short} {subject}"),
                    "date": info.time,
                }));
            }
            match info.parent {
                Some(parent) => oid = parent,
                None => break,
            }
        }
    }

    let branches: Vec<_> = current
        .iter()
        .map(|h| serde_json::json!({"name": h.name, "current": h.current, "date": h.date}))
        .collect();
    let tag_json: Vec<_> = tags
        .iter()
        .map(|t| serde_json::json!({"name": t.name, "date": t.date}))
        .collect();
    Ok(serde_json::json!({
        "error": null,
        "branches": branches,
        "tags": tag_json,
        "commits": commits,
    })
    .to_string()
    .replace('<', "\\u003c"))
}

/// Inputs for one page view.
pub struct Present<'a> {
    pub html: &'a str,
    pub source_path: &'a Path,
    pub jail: Option<&'a Path>,
    pub comments_on: bool,
    pub compare: &'a Compare,
    /// `?view=source`: the unified diff of the `.md` instead of the page.
    pub source: bool,
    /// How the notebook's own render resolves links, so a re-rendered
    /// paragraph links the same way.
    pub link: Option<&'a crate::render::LinkMode>,
}

/// What the status chip and `<body>` say about the comparison.
struct View {
    key: &'static str,
    /// Before ref for the menu's base field (custom only).
    base: String,
    /// After ref for the compare field. Empty is the working copy.
    compare: String,
    /// `uncommitted changes`, `this branch vs main`, `main → topic`.
    title: String,
    /// `vs last commit 5f01f1f`, filled in once the sides are known.
    against: String,
    /// The before side in messages: `HEAD`, `main`, `the merge base`.
    before_name: String,
    /// The Source view is showing.
    source: bool,
}

impl View {
    fn new(c: &Compare) -> View {
        let (title, base, compare, before_name) = match c {
            Compare::Uncommitted => (
                "uncommitted changes".to_string(),
                String::new(),
                String::new(),
                "HEAD".to_string(),
            ),
            Compare::Branch => (
                "this branch".to_string(),
                String::new(),
                String::new(),
                "the merge base".to_string(),
            ),
            Compare::Last => (
                "last commit".to_string(),
                String::new(),
                String::new(),
                "HEAD~1".to_string(),
            ),
            Compare::Custom { base, compare } => (
                format!(
                    "{base} → {}",
                    compare.as_deref().unwrap_or("working copy")
                ),
                base.clone(),
                compare.clone().unwrap_or_default(),
                base.clone(),
            ),
            Compare::Unknown(v) => (format!("“{v}”"), String::new(), String::new(), String::new()),
        };
        View {
            key: c.key(),
            base,
            compare,
            title,
            against: String::new(),
            before_name,
            source: false,
        }
    }
}

fn count_text(n: usize) -> String {
    match n {
        0 => "no changes".into(),
        1 => "1 change".into(),
        n => format!("{n} changes"),
    }
}

fn fail_msg(e: &GitFail, other: &str) -> String {
    match e {
        GitFail::Missing => "git not found".into(),
        GitFail::Dubious => OWNERSHIP_MSG.into(),
        _ => other.into(),
    }
}

/// Before and after revisions. `after == None` is the working copy.
struct Sides {
    before: String,
    after: Option<String>,
}

fn sides_for(root: &Path, c: &Compare, view: &mut View) -> Result<Sides, String> {
    match c {
        Compare::Uncommitted => {
            match verify(root, "HEAD") {
                Ok(oid) => view.against = format!("vs last commit {}", short(&oid)),
                Err(e @ (GitFail::Missing | GitFail::Dubious)) => return Err(fail_msg(&e, "")),
                Err(_) => view.against = "no commits yet".into(),
            }
            Ok(Sides {
                before: "HEAD".into(),
                after: None,
            })
        }
        Compare::Last => {
            let head = verify(root, "HEAD").map_err(|e| fail_msg(&e, "no commits yet"))?;
            let info = read_commit(root, &head).map_err(|e| fail_msg(&e, "could not read HEAD"))?;
            let Some(parent) = info.parent else {
                return Err("no earlier commit to compare with".into());
            };
            view.against = format!("{} → {}", short(&parent), short(&head));
            Ok(Sides {
                before: parent,
                after: Some(head),
            })
        }
        Compare::Branch => {
            let default = default_branch(root)
                .map_err(|e| fail_msg(&e, "no main branch"))?
                .ok_or_else(|| "no main or master branch to compare with".to_string())?;
            view.title = format!("this branch vs {default}");
            let current = current_branch(root).map_err(|e| fail_msg(&e, "unknown branch"))?;
            if on_default(current.as_deref(), &default) {
                return Err(format!("already on {default}, nothing to compare"));
            }
            let head = verify(root, "HEAD").map_err(|e| fail_msg(&e, "no commits yet"))?;
            let tip = verify(root, &default).map_err(|e| fail_msg(&e, "unknown ref"))?;
            let base = merge_base(root, &head, &tip)
                .map_err(|e| fail_msg(&e, &format!("no common history with {default}")))?;
            view.against = format!("from {}", short(&base));
            view.before_name = format!("the merge base with {default}");
            Ok(Sides {
                before: base,
                after: None,
            })
        }
        Compare::Custom { base, compare } => {
            if let Some(msg) = ref_problem(base) {
                return Err(msg.into());
            }
            if let Some(c) = compare {
                if let Some(msg) = ref_problem(c) {
                    return Err(msg.into());
                }
            }
            Ok(Sides {
                before: base.clone(),
                after: compare.clone(),
            })
        }
        Compare::Unknown(_) => Err("unknown comparison".into()),
    }
}

pub fn present(p: Present<'_>) -> String {
    let mut view = View::new(p.compare);
    view.source = p.source;
    if let Compare::Unknown(_) = p.compare {
        return decorate(p.html, &view, "unknown comparison", None);
    }
    let start = p.source_path.parent().unwrap_or(p.source_path);
    let root = match repo_root(start) {
        Ok(r) => r,
        Err(e) => return decorate(p.html, &view, &fail_msg(&e, "not a git repository"), None),
    };
    let Ok(rel) = safe_rel(p.source_path, p.jail, &root) else {
        return decorate(p.html, &view, "path escapes notebook directory", None);
    };
    let sides = match sides_for(&root, p.compare, &mut view) {
        Ok(s) => s,
        Err(msg) => return decorate(p.html, &view, &msg, None),
    };
    let before_side = match resolve_side(&root, &sides.before) {
        Ok(side) => side,
        Err(e) => return decorate(p.html, &view, &fail_msg(&e, "unknown ref"), None),
    };
    match &sides.after {
        None => present_working(&p, &view, &root, &rel, &sides.before, &before_side),
        Some(after) => {
            let after_side = match resolve_side(&root, after) {
                Ok(side) => side,
                Err(e) => return decorate(p.html, &view, &fail_msg(&e, "unknown ref"), None),
            };
            present_two_ref(
                &p,
                &view,
                &root,
                &rel,
                (&sides.before, &before_side),
                (after, &after_side),
            )
        }
    }
}

/// `enabled == false` returns the stored HTML and does not spawn git.
pub fn present_if(enabled: bool, p: Present<'_>) -> String {
    if !enabled {
        return p.html.to_string();
    }
    present(p)
}

fn load_named(
    root: &Path,
    rev: &str,
    rel: &str,
    changes: &[NameChange],
    side: &RevSide,
) -> Result<Option<String>, GitFail> {
    match side {
        RevSide::Empty => Ok(None),
        RevSide::Index => load_side(root, "index", rel, changes, None),
        RevSide::Oid(oid) => load_side(root, rev, rel, changes, Some(oid)),
    }
}

enum Loaded {
    Text(String),
    /// No blob at this path on this side. Not cached.
    Missing,
    Binary,
    Fail(&'static str),
}

fn load_text(root: &Path, rev: &str, rel: &str, changes: &[NameChange], side: &RevSide) -> Loaded {
    match load_named(root, rev, rel, changes, side) {
        Ok(Some(s)) if s.contains('\0') => Loaded::Binary,
        Ok(Some(s)) => Loaded::Text(normalize_eol(&s)),
        Ok(None) => Loaded::Missing,
        Err(GitFail::Missing) => Loaded::Fail("git not found"),
        Err(GitFail::Dubious) => Loaded::Fail(OWNERSHIP_MSG),
        Err(_) => Loaded::Fail("could not read that ref"),
    }
}

fn present_working(
    p: &Present<'_>,
    view: &View,
    root: &Path,
    rel: &str,
    before: &str,
    side: &RevSide,
) -> String {
    let named = view.before_name.as_str();
    let mut changes = match worktree_changes(root, side) {
        Ok(c) => c,
        Err(e @ (GitFail::Dubious | GitFail::Missing)) => {
            return decorate(p.html, view, &fail_msg(&e, ""), None);
        }
        Err(_) => Vec::new(),
    };
    let disk = std::fs::read(p.source_path).ok();
    let oid = match side {
        RevSide::Oid(s) => Some(s.as_str()),
        _ => None,
    };
    let new_text = disk.as_ref().map(|b| {
        if b.contains(&0) {
            String::new()
        } else {
            normalize_eol(&String::from_utf8_lossy(b))
        }
    });
    if let Some(text) = &new_text {
        if !text.is_empty() && !matches!(side, RevSide::Empty) {
            attach_rename(&mut changes, root, before, oid, rel, text);
        }
    }
    let dots = file_dots_json(&changes);
    let fail = |msg: &str| decorate(p.html, view, msg, Some(&dots));
    if disk.as_ref().is_some_and(|b| b.contains(&0)) {
        return fail("not a text notebook");
    }
    let Some(new_text) = new_text else {
        return match load_text(root, before, rel, &changes, side) {
            Loaded::Fail(msg) => fail(msg),
            Loaded::Binary => fail("not a text notebook"),
            Loaded::Missing => fail(&format!("not on {named} and not in the working copy")),
            Loaded::Text(old) if p.source => {
                source_page(p, view, rel, &old, "", "deleted from the working copy", &dots)
            }
            Loaded::Text(old) => {
                let notice = format!(
                    "This notebook is not in the working copy. It is still on {named}."
                );
                deleted_page(p.html, &old, &notice, false, p.comments_on, |page| {
                    decorate(&page, view, "deleted from the working copy", Some(&dots))
                })
            }
        };
    };
    let old_text = match load_text(root, before, rel, &changes, side) {
        Loaded::Binary => return fail("not a text notebook"),
        Loaded::Fail(msg) => return fail(msg),
        Loaded::Missing if p.source => {
            let status = format!("new file, not on {named}");
            return source_page(p, view, rel, "", &new_text, &status, &dots);
        }
        Loaded::Missing => {
            let notice = format!("This notebook is not on {named}. Showing the working copy.");
            let page = insert_notice(p.html, &notice);
            return decorate(&page, view, &format!("new file, not on {named}"), Some(&dots));
        }
        Loaded::Text(s) => s,
    };
    if p.source {
        let status = source_status(&old_text, &new_text, p.comments_on);
        return source_page(p, view, rel, &old_text, &new_text, &status, &dots);
    }
    if crate::embed::has_markdown_embeds(&new_text) {
        return fail("embeds are not part of this diff");
    }
    match stamp_working(p.html, &old_text, &new_text, p.comments_on, p.link) {
        Ok((html, n)) => decorate(&html, view, &count_text(n), Some(&dots)),
        Err(_) => match render_aligned(&old_text, &new_text, p.comments_on, false, p.link) {
            // The stored sections don't match the file (an emptied
            // notebook, or a render that folded blocks differently).
            // Rebuild <main> so the added and removed blocks still show.
            Ok((inner, n)) => {
                let page = replace_main(p.html, &inner);
                decorate(&page, view, &count_text(n), Some(&dots))
            }
            Err(msg) if msg == TOO_LARGE => {
                let status = format!("{msg}, showing the source");
                source_page(p, view, rel, &old_text, &new_text, &status, &dots)
            }
            Err(msg) => decorate(p.html, view, msg, Some(&dots)),
        },
    }
}

const TOO_LARGE: &str = "diff was too large";

/// The chip's count on the Source view: the same unit count as the page
/// when that can be computed, else a line count.
fn source_status(old: &str, new_src: &str, comments_on: bool) -> String {
    match align(old, new_src, comments_on) {
        Ok((marks, removed, _)) => count_text(count_changes(&marks, &removed)),
        Err(_) => {
            let a: Vec<&str> = old.lines().collect();
            let b: Vec<&str> = new_src.lines().collect();
            let n = crate::srcdiff::line_diff(&a, &b)
                .iter()
                .filter(|l| !matches!(l, crate::srcdiff::Line::Same(..)))
                .count();
            match n {
                0 => "no changes".into(),
                1 => "1 line changed".into(),
                n => format!("{n} lines changed"),
            }
        }
    }
}

/// `<main>` replaced by the unified `.md` diff.
fn source_page(
    p: &Present<'_>,
    view: &View,
    rel: &str,
    old: &str,
    new_src: &str,
    status: &str,
    dots: &str,
) -> String {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let label = format!("{name} · {} · {status}", view.title);
    let page = replace_main(p.html, &crate::srcdiff::source_view(old, new_src, &label));
    decorate(&page, view, status, Some(dots))
}

fn blobs_for(root: &Path, rev: &str, side: &RevSide) -> Result<BTreeMap<String, String>, GitFail> {
    match side {
        RevSide::Empty => Ok(BTreeMap::new()),
        RevSide::Index => ref_blobs(root, "index", None),
        RevSide::Oid(oid) => ref_blobs(root, rev, Some(oid)),
    }
}

/// Both sides are refs: changes from `before` (base) to `after`
/// (compare), as `git diff base compare` reads. Neither side runs.
fn present_two_ref(
    p: &Present<'_>,
    view: &View,
    root: &Path,
    rel: &str,
    (before, before_side): (&str, &RevSide),
    (after, after_side): (&str, &RevSide),
) -> String {
    let mut changes = match (
        blobs_for(root, before, before_side),
        blobs_for(root, after, after_side),
    ) {
        (Ok(old), Ok(new)) => diff_maps(&old, &new, true),
        (Err(e @ (GitFail::Dubious | GitFail::Missing)), _)
        | (_, Err(e @ (GitFail::Dubious | GitFail::Missing))) => {
            return decorate(p.html, view, &fail_msg(&e, ""), None);
        }
        _ => Vec::new(),
    };
    if let Ok(Some(preview)) = load_named(root, after, rel, &changes, after_side) {
        if !preview.contains('\0') {
            let old_rev = if matches!(before_side, RevSide::Index) {
                "index"
            } else {
                before
            };
            let old_oid = match before_side {
                RevSide::Oid(s) => Some(s.as_str()),
                _ => None,
            };
            attach_rename(
                &mut changes,
                root,
                old_rev,
                old_oid,
                rel,
                &normalize_eol(&preview),
            );
        }
    }
    let dots = file_dots_json(&changes);
    let fail = |msg: &str| decorate(p.html, view, msg, Some(&dots));
    let before_name = view.before_name.as_str();
    let after_name = if view.compare.is_empty() {
        after
    } else {
        view.compare.as_str()
    };

    let new_text = match load_text(root, after, rel, &changes, after_side) {
        Loaded::Fail(msg) => return fail(msg),
        Loaded::Binary => return fail("not a text notebook"),
        other => other,
    };
    let old_text = match load_text(root, before, rel, &changes, before_side) {
        Loaded::Fail(msg) => return fail(msg),
        Loaded::Binary => return fail("not a text notebook"),
        other => other,
    };
    if p.source {
        let text = |l: &Loaded| match l {
            Loaded::Text(t) => Some(t.clone()),
            _ => None,
        };
        let (o, n) = (text(&old_text), text(&new_text));
        if o.is_some() || n.is_some() {
            let status = match (&o, &n) {
                (Some(o), Some(n)) => source_status(o, n, p.comments_on),
                (None, _) => format!("new file, not on {before_name}"),
                (_, None) => format!("not on {after_name}"),
            };
            let (o, n) = (o.unwrap_or_default(), n.unwrap_or_default());
            return source_page(p, view, rel, &o, &n, &status, &dots);
        }
    }
    match (old_text, new_text) {
        (Loaded::Missing, Loaded::Missing) => {
            let page = insert_notice(
                p.html,
                &format!("This notebook is not on {after_name} or {before_name}."),
            );
            decorate(
                &page,
                view,
                &format!("not on {after_name} or {before_name}"),
                Some(&dots),
            )
        }
        (Loaded::Missing, Loaded::Text(text)) => {
            let notice =
                format!("This notebook is not on {before_name}. Showing {after_name}.");
            let inner = format!(
                "{}{}",
                notice_html(&notice),
                render_plain(&text, p.comments_on)
            );
            let page = replace_main(p.html, &inner);
            decorate(
                &page,
                view,
                &format!("new file, not on {before_name} · code was not run"),
                Some(&dots),
            )
        }
        (Loaded::Text(text), Loaded::Missing) => {
            let notice =
                format!("This notebook is not on {after_name}. It is still on {before_name}.");
            deleted_page(p.html, &text, &notice, true, p.comments_on, |page| {
                decorate(
                    &page,
                    view,
                    &format!("not on {after_name} · code was not run"),
                    Some(&dots),
                )
            })
        }
        (Loaded::Text(old_text), Loaded::Text(new_text)) => {
            if crate::embed::has_markdown_embeds(&new_text)
                || crate::embed::has_markdown_embeds(&old_text)
            {
                let page = replace_main(p.html, &render_plain(&new_text, p.comments_on));
                return decorate(
                    &page,
                    view,
                    "embeds are not part of this diff · code was not run",
                    Some(&dots),
                );
            }
            match render_aligned(&old_text, &new_text, p.comments_on, true, p.link) {
                Ok((inner, n)) => {
                    let page = replace_main(p.html, &inner);
                    let status = format!("{} · code was not run", count_text(n));
                    decorate(&page, view, &status, Some(&dots))
                }
                Err(msg) if msg == TOO_LARGE => {
                    let status = format!("{msg}, showing the source");
                    source_page(p, view, rel, &old_text, &new_text, &status, &dots)
                }
                Err(msg) => {
                    let page = replace_main(p.html, &render_plain(&new_text, p.comments_on));
                    let status = format!("{msg} · code was not run");
                    decorate(&page, view, &status, Some(&dots))
                }
            }
        }
        _ => fail("not a text notebook"),
    }
}

/// `GET /git/changes` body: a change count for each preset, so the menu
/// shows `4 changes` before one is picked. Reads blobs and the file on
/// disk; never runs a notebook. `enabled == false` does not spawn git.
pub fn changes_json_if(
    enabled: bool,
    source_path: &Path,
    jail: Option<&Path>,
    comments_on: bool,
) -> String {
    if !enabled {
        return serde_json::json!({ "error": "git diff is off" }).to_string();
    }
    match changes_json(source_path, jail, comments_on) {
        Ok(body) => body,
        Err(msg) => serde_json::json!({ "error": msg }).to_string(),
    }
}

fn changes_json(source_path: &Path, jail: Option<&Path>, comments_on: bool) -> Result<String, String> {
    use serde_json::json;
    let start = source_path.parent().unwrap_or(source_path);
    let root = repo_root(start).map_err(|e| fail_msg(&e, "not a git repository"))?;
    let rel = safe_rel(source_path, jail, &root)
        .map_err(|_| "path escapes notebook directory".to_string())?;
    let disk: Option<String> = std::fs::read(source_path)
        .ok()
        .filter(|b| !b.contains(&0))
        .map(|b| normalize_eol(&String::from_utf8_lossy(&b)));
    let head = match verify(&root, "HEAD") {
        Ok(oid) => Some(oid),
        Err(e @ (GitFail::Missing | GitFail::Dubious)) => return Err(fail_msg(&e, "")),
        Err(_) => None,
    };
    let current = current_branch(&root).map_err(|e| fail_msg(&e, "unknown branch"))?;
    let default = default_branch(&root).map_err(|e| fail_msg(&e, "no default branch"))?;
    let at = |oid: &str| -> Option<String> {
        match load_text(&root, oid, &rel, &[], &RevSide::Oid(oid.to_string())) {
            Loaded::Text(t) => Some(t),
            _ => None,
        }
    };
    let count = |old: Option<String>, new: Option<String>| -> serde_json::Value {
        match (old, new) {
            (Some(o), Some(n)) => {
                if crate::embed::has_markdown_embeds(&o) || crate::embed::has_markdown_embeds(&n) {
                    return json!({ "count": null, "note": "embeds are not part of this diff" });
                }
                match align(&o, &n, comments_on) {
                    Ok((marks, removed, _)) => json!({ "count": count_changes(&marks, &removed) }),
                    Err(msg) => json!({ "count": null, "note": msg }),
                }
            }
            (None, Some(_)) => json!({ "count": null, "note": "new file" }),
            (Some(_), None) => json!({ "count": null, "note": "deleted" }),
            (None, None) => json!({ "count": null, "note": "not in this commit" }),
        }
    };
    let uncommitted = match &head {
        Some(h) => count(at(h), disk.clone()),
        None => count(None, disk.clone()),
    };
    let last = match &head {
        None => json!({ "count": null, "disabled": "No commits yet" }),
        Some(h) => match read_commit(&root, h).ok().and_then(|info| info.parent) {
            None => json!({ "count": null, "disabled": "No earlier commit to compare with" }),
            Some(parent) => count(at(&parent), at(h)),
        },
    };
    let branch = match (&default, &head) {
        (None, _) => json!({ "hidden": true }),
        (Some(d), _) if on_default(current.as_deref(), d) => json!({ "hidden": true }),
        (Some(_), None) => json!({ "hidden": true }),
        (Some(d), Some(h)) => match verify(&root, d).and_then(|tip| merge_base(&root, h, &tip)) {
            Ok(base) => {
                let mut v = count(at(&base), disk.clone());
                v["base"] = json!(short(&base));
                v
            }
            Err(_) => json!({ "count": null, "disabled": format!("No common history with {d}") }),
        },
    };
    Ok(json!({
        "error": null,
        "current": current,
        "default": default,
        "head": head.as_deref().map(short),
        "uncommitted": uncommitted,
        "branch": branch,
        "last": last,
    })
    .to_string()
    .replace('<', "\\u003c"))
}

fn stamp_working(
    html: &str,
    old: &str,
    new_src: &str,
    comments_on: bool,
    link: Option<&crate::render::LinkMode>,
) -> Result<(String, usize), &'static str> {
    let (marks, removed, units) = align(old, new_src, comments_on)?;
    let sections = split_blocks(html);
    let blocks = units.last().map_or(0, |u| u.block + 1);
    if sections.len() != blocks {
        return Err("could not align");
    }
    let n = count_changes(&marks, &removed);
    if n == 0 {
        return Ok((html.to_string(), 0));
    }
    let mut ranges = Vec::with_capacity(sections.len());
    let mut cursor = 0usize;
    for sec in &sections {
        let Some(rel) = html[cursor..].find(sec.html.as_str()) else {
            return Err("could not align");
        };
        let start = cursor + rel;
        let end = start + sec.html.len();
        ranges.push((start, end));
        cursor = end;
    }
    let groups = group_removed(&removed);
    let mut out = String::with_capacity(html.len() + 2048);
    cursor = 0;
    let mut lo = 0usize;
    for (idx, (start, end)) in ranges.iter().enumerate() {
        let mut hi = lo;
        while hi < units.len() && units[hi].block == idx {
            hi += 1;
        }
        out.push_str(&html[cursor..*start]);
        if let Some(g) = groups.get(&lo) {
            out.push_str(&removed_html(g, true));
        }
        out.push_str(&stamp_block(
            &sections[idx].html,
            &units[lo..hi],
            &marks[lo..hi],
            lo,
            &groups,
            link,
        ));
        cursor = *end;
        lo = hi;
    }
    if let Some(g) = groups.get(&units.len()) {
        out.push_str(&removed_html(g, true));
    }
    out.push_str(&html[cursor..]);
    Ok((out, n))
}

/// Stamp one rendered section. Prose marks each top-level element; when
/// the elements do not line up with the source parts (raw HTML, a
/// footnote), the whole section is marked instead.
fn stamp_block(
    section: &str,
    units: &[Unit],
    marks: &[Mark],
    lo: usize,
    groups: &BTreeMap<usize, Vec<&Unit>>,
    link: Option<&crate::render::LinkMode>,
) -> String {
    let inner: BTreeMap<usize, Vec<&Unit>> = groups
        .range(lo + 1..lo + units.len())
        .map(|(k, v)| (k - lo, v.clone()))
        .collect();
    let all_same = marks.iter().all(|m| matches!(m, Mark::Same));
    if all_same && inner.is_empty() {
        return section.to_string();
    }
    if units.len() == 1 && units[0].part.is_none() {
        return stamp_whole(section, &marks[0], &units[0]);
    }
    if units[0].kind == Kind::Prose && units[0].part.is_some() {
        if let Some(s) = stamp_prose(section, units, marks, &inner, link) {
            return s;
        }
    }
    let mut out = String::new();
    for g in inner.values() {
        out.push_str(&removed_html(g, true));
    }
    match marks.iter().position(|m| !matches!(m, Mark::Same)) {
        None => out.push_str(section),
        Some(k) => {
            let added = marks.iter().all(|m| matches!(m, Mark::Added));
            let (class, state) = if added {
                (ADD_CLASS, "added")
            } else {
                (MOD_CLASS, "modified")
            };
            out.push_str(&mark_open_tag(section, class, &dx_attrs(state, &units[k].place)));
        }
    }
    out
}

const ADD_CLASS: &str = "rl-dx rl-dx-add";
const MOD_CLASS: &str = "rl-dx rl-dx-mod";

fn stamp_whole(section: &str, mark: &Mark, unit: &Unit) -> String {
    match mark {
        Mark::Same => section.to_string(),
        Mark::Added => mark_open_tag(section, ADD_CLASS, &dx_attrs("added", &unit.place)),
        Mark::Changed { old, .. } => {
            let s = mark_open_tag(section, MOD_CLASS, &dx_attrs("modified", &unit.place));
            match unit.kind {
                Kind::Code | Kind::Mermaid | Kind::Widget => {
                    let listing = code_listing(&old.text, &unit.text, unit.kind);
                    let s = replace_source_pre(&s, &listing);
                    if unit.kind == Kind::Code {
                        insert_before_close(
                            &s,
                            "<p class=\"rl-dx-note\">Output is from your current file.</p>\n",
                        )
                    } else {
                        s
                    }
                }
                Kind::Prose | Kind::Callout => s,
            }
        }
    }
}

/// Mark the top-level elements of one prose section. `None` when the
/// rendered elements do not line up with the source parts.
fn stamp_prose(
    section: &str,
    units: &[Unit],
    marks: &[Mark],
    inner: &BTreeMap<usize, Vec<&Unit>>,
    link: Option<&crate::render::LinkMode>,
) -> Option<String> {
    const OPEN: &str = "<div class=\"prose\">";
    let start = section.find(OPEN)? + OPEN.len();
    let (elems, _) = top_elements(&section[start..])?;
    if elems.len() != units.len() {
        return None;
    }
    let mut out = String::with_capacity(section.len() + 1024);
    out.push_str(&section[..start]);
    let mut cursor = 0usize;
    for (k, r) in elems.iter().enumerate() {
        out.push_str(&section[start + cursor..start + r.start]);
        if let Some(g) = inner.get(&k) {
            out.push_str(&removed_html(g, false));
        }
        let el = &section[start + r.start..start + r.end];
        out.push_str(&match &marks[k] {
            Mark::Same => el.to_string(),
            Mark::Added => {
                mark_open_tag(&all_added(el), ADD_CLASS, &dx_attrs("added", &units[k].place))
            }
            Mark::Changed { words, .. } => {
                let el = words
                    .as_deref()
                    .and_then(|w| with_words(el, w, link))
                    .unwrap_or_else(|| el.to_string());
                mark_open_tag(&el, MOD_CLASS, &dx_attrs("modified", &units[k].place))
            }
        });
        cursor = r.end;
    }
    out.push_str(&section[start + cursor..]);
    Some(out)
}

const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source",
    "track", "wbr",
];

/// Byte ranges of the top-level elements in `s`, up to the first closing
/// tag at depth zero (the end of the enclosing `<div>`), plus where that
/// tag starts. `None` for top-level text or unbalanced markup.
fn top_elements(s: &str) -> Option<(Vec<std::ops::Range<usize>>, usize)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != b'<' {
            return None;
        }
        if s[i..].starts_with("<!--") {
            i += s[i..].find("-->")? + 3;
            continue;
        }
        if s[i..].starts_with("</") {
            return Some((out, i));
        }
        let start = i;
        let mut depth = 0usize;
        loop {
            let lt = i + s[i..].find('<')?;
            if s[lt..].starts_with("<!--") {
                i = lt + s[lt..].find("-->")? + 3;
                continue;
            }
            let gt = lt + s[lt..].find('>')?;
            let tag = &s[lt + 1..gt];
            if tag.starts_with('/') {
                depth = depth.checked_sub(1)?;
            } else {
                let name: String = tag
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect::<String>()
                    .to_ascii_lowercase();
                if !(tag.ends_with('/') || VOID_TAGS.contains(&name.as_str())) {
                    depth += 1;
                }
            }
            i = gt + 1;
            if depth == 0 {
                break;
            }
        }
        out.push(start..i);
    }
}

/// Add `class` and `attrs` to the first opening tag of `elem`.
fn mark_open_tag(elem: &str, class: &str, attrs: &str) -> String {
    let Some(gt) = elem.find('>') else {
        return elem.to_string();
    };
    let (tag, close) = match elem[..gt].strip_suffix('/') {
        Some(t) => (t, "/"),
        None => (&elem[..gt], ""),
    };
    let tag = if let Some(c) = tag.find(" class=\"") {
        let at = c + " class=\"".len();
        format!("{}{class} {}", &tag[..at], &tag[at..])
    } else {
        format!("{tag} class=\"{class}\"")
    };
    format!("{tag}{attrs}{close}{}", &elem[gt..])
}

fn dx_attrs(state: &str, place: &str) -> String {
    format!(
        " data-rl-dx=\"{state}\" data-rl-where=\"{}\"",
        escape_html(place)
    )
}

fn insert_before_close(section: &str, extra: &str) -> String {
    match section.rfind("</section>") {
        Some(end) => {
            let mut s = section.to_string();
            s.insert_str(end, extra);
            s
        }
        None => format!("{section}{extra}"),
    }
}

/// Swap the cell's source listing for the diff listing, so the source
/// shows once. A section without a source `<pre>` (a widget, a hidden
/// cell) gets the listing in its own disclosure.
fn replace_source_pre(section: &str, listing: &str) -> String {
    const OPEN: &str = "<pre class=\"source\"><code>";
    const CLOSE: &str = "</code></pre>";
    if let Some(start) = section.find(OPEN) {
        if let Some(rel) = section[start..].find(CLOSE) {
            let end = start + rel + CLOSE.len();
            return format!("{}{listing}{}", &section[..start], &section[end..]);
        }
    }
    insert_before_close(
        section,
        &format!(
            "<details class=\"rl-dx-src\" open><summary>Source changes</summary>{listing}</details>\n"
        ),
    )
}

/// One listing that is the diff: unchanged lines, then removed and added
/// lines marked in the gutter. Rustlab lines keep their syntax colors.
/// Within a removed / added line pair the changed tokens are marked too,
/// unless too little of the line survived.
fn code_listing(old: &str, new_src: &str, kind: Kind) -> String {
    use crate::worddiff::{diff, Mode, MIN_SURVIVAL};
    let rustlab = kind == Kind::Code;
    let old = old.trim_end_matches('\n');
    let new_src = new_src.trim_end_matches('\n');
    let a: Vec<&str> = old.split('\n').collect();
    let b: Vec<&str> = new_src.split('\n').collect();
    if a.len() > MAX_LINES || b.len() > MAX_LINES {
        let hl = crate::render::highlight_marked(new_src, rustlab, &[], "ins");
        return format!(
            "<pre class=\"source rl-dx-code\"><code>{hl}</code></pre><p class=\"rl-dx-note\">Line diff omitted (too long).</p>"
        );
    }
    let ops = diff_ops(a.len(), b.len(), |i, j| a[i] == b[j]);
    let line = |class: &str, sign: &str, body: String| {
        format!(
            "<span class=\"{class}\"><span class=\"rl-ln-g\" aria-hidden=\"true\">{sign}</span>{body}</span>"
        )
    };
    let mut html = String::from("<pre class=\"source rl-dx-code\"><code>");
    let (mut i, mut j) = (0usize, 0usize);
    let mut k = 0usize;
    while k < ops.len() {
        if let Op::Eq(_) = ops[k] {
            html.push_str(&line("rl-ln", "", crate::render::highlight_marked(b[j], rustlab, &[], "ins")));
            i += 1;
            j += 1;
            k += 1;
            continue;
        }
        let mut dels = Vec::new();
        let mut ins = Vec::new();
        while k < ops.len() && !matches!(ops[k], Op::Eq(_)) {
            match ops[k] {
                Op::Del(_) => {
                    dels.push(i);
                    i += 1;
                }
                _ => {
                    ins.push(j);
                    j += 1;
                }
            }
            k += 1;
        }
        let mut old_marks = vec![Vec::new(); dels.len()];
        let mut new_marks = vec![Vec::new(); ins.len()];
        for p in 0..dels.len().min(ins.len()) {
            if let Some(d) = diff(a[dels[p]], b[ins[p]], Mode::Code) {
                if d.survival >= MIN_SURVIVAL {
                    old_marks[p] = d.old_marks;
                    new_marks[p] = d.new_marks;
                }
            }
        }
        for (p, &d) in dels.iter().enumerate() {
            let body = crate::render::highlight_marked(a[d], rustlab, &old_marks[p], "del");
            html.push_str(&line("rl-ln rl-ln-del", "−", body));
        }
        for (p, &n) in ins.iter().enumerate() {
            let body = crate::render::highlight_marked(b[n], rustlab, &new_marks[p], "ins");
            html.push_str(&line("rl-ln rl-ln-add", "+", body));
        }
    }
    html.push_str("</code></pre>");
    html
}

/// Removed units by the new-side unit they sit before.
fn group_removed(removed: &[(usize, Unit)]) -> BTreeMap<usize, Vec<&Unit>> {
    let mut out: BTreeMap<usize, Vec<&Unit>> = BTreeMap::new();
    for (at, unit) in removed {
        out.entry(*at).or_default().push(unit);
    }
    out
}

/// A run of removed units, collapsed to one summary line.
fn removed_html(group: &[&Unit], wrap_section: bool) -> String {
    let place = group.first().map(|u| u.place.as_str()).unwrap_or("");
    let mut body = String::new();
    for u in group {
        body.push_str(&match u.kind {
            Kind::Prose | Kind::Callout => {
                format!("<div class=\"prose\">\n{}</div>\n", markdown_to_html(&u.text))
            }
            Kind::Code => format!(
                "<pre class=\"source\"><code>{}</code></pre>\n",
                crate::render::highlight_rustlab(&u.text)
            ),
            Kind::Mermaid | Kind::Widget => format!(
                "<pre class=\"source\"><code>{}</code></pre>\n",
                escape_html(&u.text)
            ),
        });
    }
    let fp: Vec<&str> = group.iter().map(|u| u.fp.as_str()).collect();
    let id = blake_id(&fp.join("\0"));
    let details = format!(
        "<details class=\"rl-dx rl-dx-del rl-dx-removed\" id=\"dx-del-{id:08x}\"{}><summary><b>Removed</b> · {}</summary><div class=\"rl-dx-gone\">\n{body}</div></details>\n",
        dx_attrs("removed", place),
        escape_html(&removed_summary(group)),
    );
    if wrap_section {
        format!("<section class=\"rl-block\" id=\"b-gitdel-{id:08x}\">\n{details}</section>\n")
    } else {
        details
    }
}

/// `“Old notes” heading and 1 paragraph`, `2 code cells`.
fn removed_summary(group: &[&Unit]) -> String {
    let mut bits: Vec<String> = Vec::new();
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    let mut named = false;
    for u in group {
        if u.kind == Kind::Prose && u.sub == "heading" && !named && !u.place.is_empty() {
            bits.push(format!("“{}” heading", u.place));
            named = true;
            continue;
        }
        let noun = match u.kind {
            Kind::Prose => match u.sub {
                "heading" | "paragraph" | "list" | "table" | "quote" | "code block" => u.sub,
                _ => "block",
            },
            Kind::Code => "code cell",
            Kind::Mermaid => "diagram",
            Kind::Widget => "widget",
            Kind::Callout => "callout",
        };
        match counts.iter_mut().find(|(n, _)| *n == noun) {
            Some(c) => c.1 += 1,
            None => counts.push((noun, 1)),
        }
    }
    for (noun, n) in counts {
        bits.push(if n == 1 {
            format!("1 {noun}")
        } else {
            format!("{n} {noun}s")
        });
    }
    match bits.len() {
        0 => String::new(),
        1 => bits.remove(0),
        n => format!("{} and {}", bits[..n - 1].join(", "), bits[n - 1]),
    }
}

fn render_aligned(
    old: &str,
    new_src: &str,
    comments_on: bool,
    two_ref: bool,
    link: Option<&crate::render::LinkMode>,
) -> Result<(String, usize), &'static str> {
    let (marks, removed, units) = align(old, new_src, comments_on)?;
    let n = count_changes(&marks, &removed);
    let groups = group_removed(&removed);
    let mut body = String::new();
    if two_ref {
        body.push_str("<p class=\"rl-dx-note\">Code was not run.</p>\n");
    }
    for (idx, unit) in units.iter().enumerate() {
        if let Some(g) = groups.get(&idx) {
            body.push_str(&removed_html(g, true));
        }
        body.push_str(&fresh_section(unit, &marks[idx], idx, link));
    }
    if let Some(g) = groups.get(&units.len()) {
        body.push_str(&removed_html(g, true));
    }
    Ok((body, n))
}

fn render_plain(src: &str, comments_on: bool) -> String {
    let prepared = prepare(src, comments_on);
    let units = units_of(&prepared);
    let mut body = String::from("<p class=\"rl-dx-note\">Code was not run.</p>\n");
    for (idx, unit) in units.iter().enumerate() {
        body.push_str(&fresh_section(unit, &Mark::Same, idx, None));
    }
    body
}

/// A section built from source, for pages that are not the stored
/// render: two refs, a deleted notebook, or a render that did not line
/// up. Each prose part is its own section.
fn fresh_section(
    unit: &Unit,
    mark: &Mark,
    idx: usize,
    link: Option<&crate::render::LinkMode>,
) -> String {
    let body = match unit.kind {
        Kind::Code | Kind::Mermaid | Kind::Widget => {
            let listing = match mark {
                Mark::Changed { old, .. } => code_listing(&old.text, &unit.text, unit.kind),
                _ if unit.kind == Kind::Code => format!(
                    "<pre class=\"source\"><code>{}</code></pre>",
                    crate::render::highlight_rustlab(&unit.text)
                ),
                _ => format!(
                    "<pre class=\"source\"><code>{}</code></pre>",
                    escape_html(&unit.text)
                ),
            };
            let lang = match unit.kind {
                Kind::Code => "rustlab",
                Kind::Mermaid => "mermaid",
                _ => "widget",
            };
            format!(
                "<div class=\"code-block\"><div class=\"rl-cell\">\
<details class=\"rl-src\" open><summary class=\"rl-lang\">{lang}</summary>{listing}</details>\
</div></div>\n"
            )
        }
        Kind::Prose | Kind::Callout => {
            let html = crate::render::markdown_to_html_linked(&unit.text, link, None);
            let html = match mark {
                Mark::Changed {
                    words: Some(w), ..
                } => with_words(html.trim(), w, link).unwrap_or(html),
                Mark::Added if unit.kind == Kind::Prose => all_added(html.trim()),
                _ => html,
            };
            format!("<div class=\"prose\">\n{html}\n</div>\n")
        }
    };
    let section = format!("<section class=\"rl-block\" id=\"b-git-{idx}\">\n{body}</section>\n");
    match mark {
        Mark::Same => section,
        Mark::Added => mark_open_tag(&section, ADD_CLASS, &dx_attrs("added", &unit.place)),
        Mark::Changed { .. } => {
            mark_open_tag(&section, MOD_CLASS, &dx_attrs("modified", &unit.place))
        }
    }
}

/// Changed and added units, plus one per run of removed units.
fn count_changes(marks: &[Mark], removed: &[(usize, Unit)]) -> usize {
    let runs: std::collections::BTreeSet<usize> = removed.iter().map(|(at, _)| *at).collect();
    marks.iter().filter(|m| !matches!(m, Mark::Same)).count() + runs.len()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Prose,
    Code,
    Mermaid,
    Widget,
    Callout,
}

#[derive(Clone, Debug)]
struct Unit {
    kind: Kind,
    /// Top-level markdown kind for prose (`heading`, `paragraph`, …).
    /// Only units with the same kind and `sub` pair up as modified.
    sub: &'static str,
    fp: String,
    text: String,
    /// The rendered `<section>` this unit sits in.
    block: usize,
    /// Position among that section's top-level prose elements.
    part: Option<usize>,
    /// Where it is, for the change list: `Results ¶2`, `Code cell 3`.
    place: String,
}

#[derive(Debug)]
enum Mark {
    Same,
    Added,
    /// `words`: the new source with word markers (see `worddiff`), when
    /// this pair is marked word by word.
    Changed { old: Box<Unit>, words: Option<String> },
}

/// What a modified prose pair shows inside the element.
enum WordPlan {
    /// Mark removed and added words; the new source with markers.
    Words(String),
    /// Too little survived: show the old one removed and the new added.
    Split,
    /// Keep the element-level mark only.
    Whole,
}

fn word_plan(old: &str, new_src: &str, sub: &str) -> WordPlan {
    use crate::worddiff::{diff, Mode, MIN_SURVIVAL};
    if !matches!(sub, "paragraph" | "heading" | "list" | "quote") {
        return WordPlan::Whole;
    }
    // Images, footnotes, comments and highlights are rewritten by the
    // full render (copied files, margin notes); marks would not survive.
    for s in [old, new_src] {
        if ["![", "<img", "[^", "%%", "=="].iter().any(|p| s.contains(p)) {
            return WordPlan::Whole;
        }
    }
    let Some(d) = diff(old, new_src, Mode::Prose) else {
        return WordPlan::Whole;
    };
    if d.survival < MIN_SURVIVAL {
        return WordPlan::Split;
    }
    // A changed emphasis or structure character would leave both the old
    // and the new syntax in the merged source.
    let structural = |t: &str| {
        matches!(t, "*" | "_" | "~" | "`" | "#" | "|")
            || (sub != "paragraph" && (matches!(t, ">" | "+" | "-") || t.contains('\n')))
    };
    if d.changed.iter().any(|t| structural(t)) {
        return WordPlan::Whole;
    }
    WordPlan::Words(d.merged)
}

/// Word marks for modified prose pairs, and the low-survival ones turned
/// into a removal plus an addition.
fn refine_words(marks: &mut [Mark], removed: &mut Vec<(usize, Unit)>, new_u: &[Unit]) {
    for (idx, mark) in marks.iter_mut().enumerate() {
        let Mark::Changed { old, words } = mark else {
            continue;
        };
        if new_u[idx].kind != Kind::Prose {
            continue;
        }
        match word_plan(&old.text, &new_u[idx].text, new_u[idx].sub) {
            WordPlan::Words(w) => *words = Some(w),
            WordPlan::Whole => {}
            WordPlan::Split => {
                removed.push((idx, (**old).clone()));
                *mark = Mark::Added;
            }
        }
    }
}

/// The element `el` with its content replaced by `words` rendered with
/// `<del>` / `<ins>` marks. `None` when the render does not come back as
/// one element of the same tag, or the marks do not pair up.
fn with_words(el: &str, words: &str, link: Option<&crate::render::LinkMode>) -> Option<String> {
    let html = crate::render::markdown_to_html_linked(words, link, None);
    let wrapped = format!("{html}</div>");
    let (elems, _) = top_elements(&wrapped)?;
    if elems.len() != 1 {
        return None;
    }
    let fresh = crate::worddiff::markers_to_html(&wrapped[elems[0].clone()])?;
    let tag = |h: &str| -> String {
        h.trim_start_matches('<')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect()
    };
    if tag(&fresh) != tag(el) {
        return None;
    }
    let inner_start = fresh.find('>')? + 1;
    let inner_end = fresh.rfind("</")?;
    let el_open = el.find('>')? + 1;
    let el_close = el.rfind("</")?;
    Some(format!(
        "{}{}{}",
        &el[..el_open],
        &fresh[inner_start..inner_end],
        &el[el_close..]
    ))
}

/// An added paragraph or heading reads as all new text.
fn all_added(el: &str) -> String {
    let t: String = el
        .trim_start_matches('<')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    let inline = t == "p" || (t.len() == 2 && t.starts_with('h'));
    match (inline, el.find('>'), el.rfind("</")) {
        (true, Some(o), Some(c)) if o < c => format!(
            "{}<ins class=\"rl-w rl-w-all\">{}</ins>{}",
            &el[..=o],
            &el[o + 1..c],
            &el[c..]
        ),
        _ => el.to_string(),
    }
}

fn prepare(src: &str, comments_on: bool) -> String {
    if comments_on {
        src.to_string()
    } else {
        crate::comments::strip_source(src)
    }
}

fn align(
    old: &str,
    new_src: &str,
    comments_on: bool,
) -> Result<(Vec<Mark>, Vec<(usize, Unit)>, Vec<Unit>), &'static str> {
    if old.contains('\0') || new_src.contains('\0') {
        return Err("not a text notebook");
    }
    if old.len() > MAX_BYTES || new_src.len() > MAX_BYTES {
        return Err(TOO_LARGE);
    }
    let old_u = units_of(&prepare(old, comments_on));
    let new_u = units_of(&prepare(new_src, comments_on));
    if old_u.len() > MAX_UNITS || new_u.len() > MAX_UNITS {
        return Err(TOO_LARGE);
    }
    let ops = diff_ops(old_u.len(), new_u.len(), |i, j| old_u[i].fp == new_u[j].fp);
    let (mut marks, mut removed) = coalesce(&old_u, &new_u, &ops);
    refine_words(&mut marks, &mut removed, &new_u);
    Ok((marks, removed, new_u))
}

/// Document-order labels: `same`, `added`, `changed`, `removed`, one per
/// unit (a prose unit is a heading, paragraph, list, …).
pub fn align_sides(
    old: &str,
    new_src: &str,
    comments_on: bool,
) -> Result<Vec<&'static str>, &'static str> {
    let (marks, removed, units) = align(old, new_src, comments_on)?;
    let mut out = Vec::new();
    for (idx, mark) in marks.iter().enumerate() {
        for (at, _) in &removed {
            if *at == idx {
                out.push("removed");
            }
        }
        out.push(match mark {
            Mark::Same => "same",
            Mark::Added => "added",
            Mark::Changed { .. } => "changed",
        });
    }
    for (at, _) in &removed {
        if *at == units.len() {
            out.push("removed");
        }
    }
    Ok(out)
}

enum Op {
    /// New-side index of a block or line that matched.
    Eq(usize),
    Del(usize),
    Ins(usize),
}

fn diff_ops(n: usize, m: usize, eq: impl Fn(usize, usize) -> bool) -> Vec<Op> {
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in 0..n {
        for j in 0..m {
            dp[i + 1][j + 1] = if eq(i, j) {
                dp[i][j] + 1
            } else {
                dp[i][j + 1].max(dp[i + 1][j])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && eq(i - 1, j - 1) {
            ops.push(Op::Eq(j - 1));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(Op::Ins(j - 1));
            j -= 1;
        } else {
            ops.push(Op::Del(i - 1));
            i -= 1;
        }
    }
    ops.reverse();
    ops
}

fn coalesce(old: &[Unit], new_u: &[Unit], ops: &[Op]) -> (Vec<Mark>, Vec<(usize, Unit)>) {
    let mut marks = Vec::new();
    for _ in new_u {
        marks.push(Mark::Same);
    }
    let mut removed = Vec::new();
    let mut i = 0;
    while i < ops.len() {
        match ops[i] {
            Op::Eq(_) => i += 1,
            Op::Ins(ni) => {
                marks[ni] = Mark::Added;
                i += 1;
            }
            Op::Del(_) => {
                let mut dels = Vec::new();
                while i < ops.len() {
                    if let Op::Del(oi) = ops[i] {
                        dels.push(oi);
                        i += 1;
                    } else {
                        break;
                    }
                }
                let mut ins = Vec::new();
                while i < ops.len() {
                    if let Op::Ins(ni) = ops[i] {
                        ins.push(ni);
                        i += 1;
                    } else {
                        break;
                    }
                }
                let anchor = ins
                    .first()
                    .copied()
                    .unwrap_or_else(|| next_new(ops, i, new_u.len()));
                apply_hunk(old, new_u, &dels, &ins, anchor, &mut marks, &mut removed);
            }
        }
    }
    (marks, removed)
}

fn next_new(ops: &[Op], from: usize, len: usize) -> usize {
    for op in &ops[from..] {
        match op {
            Op::Eq(n) | Op::Ins(n) => return *n,
            Op::Del(_) => {}
        }
    }
    len
}

fn apply_hunk(
    old: &[Unit],
    new_u: &[Unit],
    dels: &[usize],
    ins: &[usize],
    anchor: usize,
    marks: &mut [Mark],
    removed: &mut Vec<(usize, Unit)>,
) {
    let mut del_used = vec![false; dels.len()];
    let mut ins_used = vec![false; ins.len()];
    for (di, &oi) in dels.iter().enumerate() {
        if let Some(ii) = ins
            .iter()
            .enumerate()
            .find(|(ii, &ni)| !ins_used[*ii] && new_u[ni].fp == old[oi].fp)
            .map(|(ii, _)| ii)
        {
            del_used[di] = true;
            ins_used[ii] = true;
            marks[ins[ii]] = Mark::Added;
            removed.push((anchor, old[oi].clone()));
        }
    }
    // A removal that follows a kept or edited unit is shown after it.
    let mut at = anchor;
    let mut di = 0;
    let mut ii = 0;
    loop {
        while di < dels.len() && del_used[di] {
            di += 1;
        }
        while ii < ins.len() && ins_used[ii] {
            ii += 1;
        }
        if di >= dels.len() && ii >= ins.len() {
            break;
        }
        let pair = di < dels.len()
            && ii < ins.len()
            && old[dels[di]].kind == new_u[ins[ii]].kind
            && old[dels[di]].sub == new_u[ins[ii]].sub;
        if pair {
            marks[ins[ii]] = Mark::Changed {
                old: Box::new(old[dels[di]].clone()),
                words: None,
            };
            at = ins[ii] + 1;
            di += 1;
            ii += 1;
        } else if di < dels.len() {
            removed.push((at, old[dels[di]].clone()));
            di += 1;
        } else {
            marks[ins[ii]] = Mark::Added;
            at = ins[ii] + 1;
            ii += 1;
        }
    }
}

fn unit(kind: Kind, sub: &'static str, fp: String, text: String, block: usize) -> Unit {
    Unit {
        kind,
        sub,
        fp,
        text,
        block,
        part: None,
        place: String::new(),
    }
}

/// Comparison units: one per code, mermaid, widget, or callout block, and
/// one per top-level markdown element (heading, paragraph, list, …)
/// inside a prose run.
fn units_of(src: &str) -> Vec<Unit> {
    let blocks = crate::parse::parse_notebook(src);
    let mut out = Vec::new();
    let mut block = 0usize;
    let mut i = 0;
    while i < blocks.len() {
        match &blocks[i] {
            Block::Markdown(s) if s.trim().is_empty() => i += 1,
            Block::ExerciseStart | Block::SolutionStart => i += 1,
            Block::Mermaid { directives, .. } if directives.hidden => i += 1,
            Block::Widget { .. } if panel_at(&blocks, i) => {
                let mut parts = Vec::new();
                while matches!(blocks.get(i), Some(Block::Widget { .. })) {
                    if let Block::Widget { source, .. } = &blocks[i] {
                        parts.push(source.clone());
                    }
                    i += 1;
                }
                if let Some(Block::Code { source, directives }) = blocks.get(i) {
                    parts.push(code_fp(source, directives));
                    let fp = format!("panel\0{}", parts.join("\0"));
                    out.push(unit(Kind::Code, "", fp, source.clone(), block));
                    block += 1;
                    i += 1;
                }
            }
            Block::Markdown(s) => {
                let md = crate::render::transform_wikilinks(s);
                let parts = crate::render::prose_parts(&md);
                if parts.is_empty() {
                    out.push(unit(Kind::Prose, "prose", format!("md\0{s}"), s.clone(), block));
                } else {
                    for (n, (sub, text)) in parts.into_iter().enumerate() {
                        let mut u = unit(Kind::Prose, sub, format!("md\0{sub}\0{text}"), text, block);
                        u.part = Some(n);
                        out.push(u);
                    }
                }
                block += 1;
                i += 1;
            }
            other => {
                if let Some(u) = unit_of(other, block) {
                    out.push(u);
                    block += 1;
                }
                i += 1;
            }
        }
    }
    label_places(&mut out);
    out
}

/// `Results ¶2` for prose (nearest heading above, then the count of
/// non-heading parts since it), `Code cell 3` for code.
fn label_places(units: &mut [Unit]) {
    let mut heading = String::new();
    let mut para = 0usize;
    let mut code = 0usize;
    let mut diagram = 0usize;
    for u in units.iter_mut() {
        u.place = match u.kind {
            Kind::Prose if u.sub == "heading" => {
                heading = heading_title(&u.text);
                para = 0;
                heading.clone()
            }
            Kind::Prose => {
                para += 1;
                if heading.is_empty() {
                    format!("¶{para}")
                } else {
                    format!("{heading} ¶{para}")
                }
            }
            Kind::Code => {
                code += 1;
                format!("Code cell {code}")
            }
            Kind::Mermaid => {
                diagram += 1;
                format!("Diagram {diagram}")
            }
            Kind::Widget => "Widget".into(),
            Kind::Callout if heading.is_empty() => "Callout".into(),
            Kind::Callout => format!("{heading} callout"),
        };
    }
}

/// Heading text without `#`, a trailing `{#id}`, or closing hashes.
fn heading_title(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let t = line.trim_start_matches('#').trim();
    let t = match t.rfind(" {#") {
        Some(i) if t.ends_with('}') => &t[..i],
        _ => t,
    };
    let t = t.trim_end_matches('#').trim();
    let mut out: String = t.chars().take(60).collect();
    if t.chars().count() > 60 {
        out.push('…');
    }
    out
}

fn panel_at(blocks: &[Block], idx: usize) -> bool {
    if !matches!(blocks.get(idx), Some(Block::Widget { .. })) {
        return false;
    }
    let mut i = idx;
    while matches!(blocks.get(i), Some(Block::Widget { .. })) {
        i += 1;
    }
    matches!(blocks.get(i), Some(Block::Code { .. }))
}

fn unit_of(block: &Block, idx: usize) -> Option<Unit> {
    match block {
        Block::Markdown(s) => Some(unit(Kind::Prose, "prose", format!("md\0{s}"), s.clone(), idx)),
        Block::Code { source, directives } => Some(unit(
            Kind::Code,
            "",
            code_fp(source, directives),
            source.clone(),
            idx,
        )),
        Block::Mermaid { source, directives } => Some(unit(
            Kind::Mermaid,
            "",
            mermaid_fp(source, directives),
            source.clone(),
            idx,
        )),
        Block::Widget { source, .. } => Some(unit(
            Kind::Widget,
            "",
            format!("w\0{source}"),
            source.clone(),
            idx,
        )),
        Block::Callout {
            kind,
            title,
            content,
        } => Some(unit(
            Kind::Callout,
            "",
            format!(
                "c\0{}\0{}\0{content}",
                callout_tag(*kind),
                title.as_deref().unwrap_or("")
            ),
            content.clone(),
            idx,
        )),
        Block::ExerciseStart | Block::SolutionStart => None,
    }
}

fn code_fp(source: &str, d: &CodeDirectives) -> String {
    format!(
        "code\0{source}\0{}\0{}\0{}\0{}\0{}",
        d.hidden,
        d.details.as_deref().unwrap_or(""),
        d.grid_cols.map(|n| n.to_string()).unwrap_or_default(),
        d.caption.as_deref().unwrap_or(""),
        d.source_open
            .map(|b| if b { "1" } else { "0" })
            .unwrap_or(""),
    )
}

fn mermaid_fp(source: &str, d: &MermaidDirectives) -> String {
    format!(
        "mm\0{source}\0{}\0{}\0{}",
        d.hidden,
        d.details.as_deref().unwrap_or(""),
        d.caption.as_deref().unwrap_or(""),
    )
}

fn callout_tag(k: CalloutKind) -> &'static str {
    match k {
        CalloutKind::Note => "note",
        CalloutKind::Tip => "tip",
        CalloutKind::Important => "important",
        CalloutKind::Warning => "warning",
        CalloutKind::Caution => "caution",
    }
}

fn blake_id(s: &str) -> u32 {
    // A short stable id. Not a hash crate — a multiply-xor of the bytes.
    let mut h: u32 = 2166136261;
    for b in s.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// Index of the document's `<main>` element.
///
/// The watch client's head script mentions `<main>` in a comment
/// (`inside <main>`). `str::find("<main")` hits that comment first.
/// Splicing there eats `<body>`, the toolbar, and the real notebook,
/// and a newline in the splice ends the comment so the script dies
/// with `Unexpected token '<'`. A one-sided compare then has no picker
/// left to switch refs. Skip `<script>` and `<style>`.
fn find_real_main(html: &str) -> Option<usize> {
    // Byte walk, not `&str` indexing: the banner sits before `<main>` and
    // contains "—" (three bytes). Stepping one byte and then slicing the
    // string panics inside that character.
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"<script") || bytes[i..].starts_with(b"<style") {
            let close: &[u8] = if bytes[i..].starts_with(b"<script") {
                b"</script>"
            } else {
                b"</style>"
            };
            let rest = &bytes[i..];
            let Some(end) = rest.windows(close.len()).position(|w| w == close) else {
                return None;
            };
            i += end + close.len();
            continue;
        }
        if bytes[i..].starts_with(b"<main>")
            || bytes[i..].starts_with(b"<main ")
            || bytes[i..].starts_with(b"<main\n")
        {
            return Some(i);
        }
        let b = bytes[i];
        let step = if b < 0x80 {
            1
        } else if b & 0xE0 == 0xC0 {
            2
        } else if b & 0xF0 == 0xE0 {
            3
        } else if b & 0xF8 == 0xF0 {
            4
        } else {
            1
        };
        i += step;
    }
    None
}

fn replace_main(html: &str, inner: &str) -> String {
    let Some(start) = find_real_main(html) else {
        return format!("{html}{inner}");
    };
    let Some(gt) = html[start..].find('>') else {
        return html.to_string();
    };
    let inner_start = start + gt + 1;
    let Some(end_rel) = html[inner_start..].find("</main>") else {
        return html.to_string();
    };
    let mut out = String::with_capacity(html.len() + inner.len());
    out.push_str(&html[..inner_start]);
    out.push_str(inner);
    out.push_str(&html[inner_start + end_rel..]);
    out
}

/// The chrome placeholder for the status chip (see `server/page.rs`).
const CHIP_SLOT: &str = "<button id=\"rl-diff-banner\" type=\"button\" hidden></button>";

fn decorate(html: &str, view: &View, status: &str, files: Option<&str>) -> String {
    let mut out = patch_body(html, view);
    out = out.replace("id=\"rl-diff\" hidden", "id=\"rl-diff\"");
    out = out.replace(
        "id=\"rl-diff-toggle\" type=\"button\" class=\"rl-quiet\"",
        "id=\"rl-diff-toggle\" type=\"button\" class=\"rl-quiet active\"",
    );
    out = out.replace(
        "id=\"rl-diff-toggle\" type=\"button\" class=\"rl-quiet active\" role=\"switch\" aria-checked=\"false\"",
        "id=\"rl-diff-toggle\" type=\"button\" class=\"rl-quiet active\" role=\"switch\" aria-checked=\"true\"",
    );
    out = out.replace(
        "<span id=\"rl-diff-dot\" class=\"rl-diff-dot\" hidden>",
        "<span id=\"rl-diff-dot\" class=\"rl-diff-dot\">",
    );
    if out.contains("aria-label=\"More, comments off\"") {
        out = out.replace(
            "aria-label=\"More, comments off\" title=\"More, comments off\"",
            "aria-label=\"More, changes on, comments off\" title=\"More, changes on, comments off\"",
        );
    } else {
        out = out.replace(
            "aria-label=\"More\" title=\"More\"",
            "aria-label=\"More, changes on\" title=\"More, changes on\"",
        );
    }
    let chip = chip_html(view, status);
    let mut extra = String::new();
    if out.contains(CHIP_SLOT) {
        out = out.replacen(CHIP_SLOT, &chip, 1);
    } else {
        extra.push_str(&chip);
    }
    if let Some(files) = files {
        extra.push_str(&format!("<div id=\"rl-git-files\" hidden>{files}</div>"));
    }
    if !extra.is_empty() {
        out = insert_after_main(&out, &extra);
    }
    out
}

/// `Showing uncommitted changes · vs last commit 5f01f1f · 4 changes`.
/// A button: clicking it opens the menu where the comparison is chosen.
fn chip_html(view: &View, status: &str) -> String {
    let against = if view.against.is_empty() {
        " <span class=\"rl-k\">·</span>".to_string()
    } else {
        format!(
            " <span class=\"rl-k\">· {} ·</span>",
            escape_html(&view.against)
        )
    };
    format!(
        "<button id=\"rl-diff-banner\" type=\"button\" title=\"Change what is compared\"><span class=\"rl-k\">Showing</span> {}{against} <span class=\"rl-n\">{}</span></button>",
        escape_html(&view.title),
        escape_html(status),
    )
}

fn deleted_page(
    html: &str,
    old: &str,
    notice: &str,
    two_ref: bool,
    comments_on: bool,
    finish: impl FnOnce(String) -> String,
) -> String {
    match render_aligned(old, "", comments_on, two_ref, None) {
        Ok((inner, _)) => {
            let page = replace_main(html, &format!("{}{inner}", notice_html(notice)));
            finish(page)
        }
        Err(msg) => finish(insert_notice(html, msg)),
    }
}

fn notice_html(text: &str) -> String {
    format!("<p class=\"rl-diff-notice\">{}</p>\n", escape_html(text))
}

fn insert_notice(html: &str, text: &str) -> String {
    insert_after_main(html, &notice_html(text))
}

fn insert_after_main(html: &str, extra: &str) -> String {
    let Some(i) = find_real_main(html) else {
        return format!("{extra}{html}");
    };
    let Some(rel) = html[i..].find('>') else {
        return html.to_string();
    };
    let at = i + rel + 1;
    let mut out = String::with_capacity(html.len() + extra.len());
    out.push_str(&html[..at]);
    out.push_str(extra);
    out.push_str(&html[at..]);
    out
}

/// `rl-diff-on` plus the comparison, so the menu can show what is picked.
fn patch_body(html: &str, view: &View) -> String {
    let Some(i) = html.find("<body") else {
        return html.to_string();
    };
    let Some(rel) = html[i..].find('>') else {
        return html.to_string();
    };
    let tag = &html[i..i + rel];
    let attrs = format!(
        " data-rl-changes=\"{}\" data-rl-view=\"{}\" data-rl-base=\"{}\" data-rl-compare=\"{}\"",
        view.key,
        if view.source { "source" } else { "rendered" },
        escape_html(&view.base),
        escape_html(&view.compare)
    );
    let new_tag = if let Some(c) = tag.find("class=\"") {
        let at = c + "class=\"".len();
        format!("{}rl-diff-on {}{}", &tag[..at], &tag[at..], attrs)
    } else {
        format!("{tag} class=\"rl-diff-on\"{attrs}")
    };
    let mut out = String::with_capacity(html.len() + 96);
    out.push_str(&html[..i]);
    out.push_str(&new_tag);
    out.push_str(&html[i + rel..]);
    out
}

pub(crate) fn percent_decode(s: &str) -> String {
    let s = s.replace('+', " ");
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_checker_accepts_common_forms() {
        for ok in [
            "HEAD",
            "HEAD~2",
            "origin/main",
            &"a".repeat(40),
            "index",
            "refs/heads/off",
        ] {
            assert!(ref_ok(ok), "{ok}");
        }
    }

    #[test]
    fn ref_checker_rejects_metacharacters_ranges_and_flags() {
        for bad in [
            "HEAD;id",
            "-rf",
            "HEAD:../../x",
            "main..HEAD",
            "HEAD\n",
            &"a".repeat(300),
            "",
            "has space",
            "a\\b",
        ] {
            assert!(!ref_ok(bad), "{bad:?}");
        }
        assert_eq!(
            ref_problem("main..HEAD"),
            Some("one ref; the page is the working copy.")
        );
    }

    #[test]
    fn fixed_argv_has_end_of_options_and_no_shell() {
        let show = blob_args("HEAD", "notes/a.md");
        assert!(show.iter().any(|a| a == "--end-of-options"));
        assert!(argv_clean(&show), "{show:?}");
        assert_eq!(show.last().map(String::as_str), Some("HEAD:notes/a.md"));
        let index = blob_args("index", "notes/a.md");
        assert_eq!(index.last().map(String::as_str), Some(":notes/a.md"));
        assert!(argv_clean(&verify_args("HEAD~2")));
        assert!(verify_args("HEAD~2")
            .iter()
            .any(|a| a == "--end-of-options"));
        assert!(argv_clean(&for_each_ref_args()));
        assert!(!show.iter().any(|a| a.contains(';') || a.contains('|')));
        let keys: Vec<&str> = config_overrides().iter().map(|(k, _)| *k).collect();
        for required in [
            "core.fsmonitor",
            "core.hooksPath",
            "core.pager",
            "diff.external",
            "protocol.allow",
            "core.sshCommand",
            "credential.helper",
        ] {
            assert!(keys.contains(&required), "{keys:?}");
        }
        assert!(!keys.iter().any(|k| k.contains("safe.directory")));
        for tool in plumbing_tools() {
            assert!(!matches!(
                *tool,
                "diff" | "show" | "log" | "status" | "fetch" | "clone"
            ));
        }
        assert!(ownership_refused(
            b"fatal: detected dubious ownership in repository at /tmp/x\n"
        ));
        assert!(ownership_refused(
            b"add it to the safe.directory list in the global config"
        ));
        assert!(!ownership_refused(b"not a git repository"));
    }

    #[test]
    fn safe_rel_stays_inside_jail_and_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let jail = tmp.path().join("jail");
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        std::fs::create_dir_all(&jail).unwrap();
        let file = repo.join("sub").join("a.md");
        std::fs::write(&file, "x").unwrap();
        assert!(safe_rel(&file, Some(&jail), &repo).is_err());
        let rel = safe_rel(&file, Some(&repo), &repo).unwrap();
        assert_eq!(rel, "sub/a.md");
        assert!(rel_ok(&rel));
        assert!(!rel_ok("../a.md"));
        assert!(!rel_ok("/etc/passwd"));
    }

    #[test]
    fn align_insert_delete_substitute_and_swap() {
        let a = "```rustlab\nx = 1\n```\n";
        let b = "```rustlab\ny = 2\n```\n";
        let c = "```rustlab\nz = 3\n```\n";
        let old = format!("{a}\n{b}");
        let inserted = format!("{a}\n{b}\n{c}");
        assert_eq!(
            align_sides(&old, &inserted, true).unwrap(),
            vec!["same", "same", "added"]
        );
        assert_eq!(align_sides(&old, a, true).unwrap(), vec!["same", "removed"]);
        assert_eq!(align_sides(a, b, true).unwrap(), vec!["changed"]);
        let swap_old = format!("{a}\n{b}");
        let swap_new = format!("{b}\n{a}");
        let sides = align_sides(&swap_old, &swap_new, true).unwrap();
        assert!(sides.contains(&"removed"), "{sides:?}");
        assert!(sides.contains(&"added"), "{sides:?}");
        assert!(!sides.contains(&"changed"), "{sides:?}");
    }

    #[test]
    fn code_directive_change_is_a_substitute() {
        let old = "```rustlab\nx = 1\n```\n";
        let new_src = "<!-- hide -->\n```rustlab\nx = 1\n```\n";
        assert_eq!(align_sides(old, new_src, true).unwrap(), vec!["changed"]);
    }

    #[test]
    fn comment_only_edit_follows_the_checkbox() {
        let old = "Hello\n";
        let new_src = "Hello %%secret%%\n";
        let off = align_sides(old, new_src, false).unwrap();
        let on = align_sides(old, new_src, true).unwrap();
        assert_eq!(off, vec!["same"], "comments off: {off:?}");
        assert_eq!(on, vec!["changed"], "comments on: {on:?}");
    }

    #[test]
    fn stamp_marks_added_changed_and_removed_without_plots_on_removed() {
        let old = "# A\n\n```rustlab\nplot(\"old\")\n```\n";
        let new_src = "# A changed\n";
        let html = "<body><main>\n<section class=\"rl-block\" id=\"b-1\"><div class=\"prose\"><h1>A changed</h1></div></section>\n</main></body>";
        let (out, n) = stamp_working(html, old, new_src, true, None).unwrap();
        assert_eq!(n, 2, "{out}");
        assert!(out.contains("<h1 class=\"rl-dx rl-dx-mod\""), "{out}");
        assert!(out.contains("rl-dx-removed"), "{out}");
        assert!(text_of(&out).contains("plot(\"old\")"), "{out}");
        let del = out.split("rl-dx-removed").nth(1).unwrap_or("");
        assert!(!del.contains("plot-container"), "{out}");
        assert!(del.contains("1 code cell"), "{out}");
    }

    #[test]
    fn changed_code_keeps_both_lines_and_labels_working_copy() {
        let old = "```rustlab\nold line\n```\n";
        let new_src = "```rustlab\nnew line\n```\n";
        let html = "<body><main><section class=\"rl-block\" id=\"b-1\"><div class=\"code-block\"><pre class=\"source\"><code>new line</code></pre><pre class=\"output\">ran</pre></section></main></body>";
        let (out, _) = stamp_working(html, old, new_src, true, None).unwrap();
        assert!(out.contains("rl-dx rl-dx-mod"), "{out}");
        assert!(out.contains("rl-ln-del"), "{out}");
        assert!(out.contains("rl-ln-add"), "{out}");
        assert!(out.contains(">−<"), "{out}");
        assert!(out.contains(">+<"), "{out}");
        let text = text_of(&out);
        assert!(text.contains("old line"), "{out}");
        assert_eq!(text.matches("new line").count(), 1, "source shows once: {out}");
        assert_eq!(out.matches("<pre class=\"source").count(), 1, "{out}");
        assert!(out.contains("Output is from your current file."), "{out}");
        assert!(out.contains(">ran<"), "{out}");
    }

    #[test]
    fn decorate_marks_the_menu_when_diff_is_on() {
        let html = "<button aria-label=\"More\" title=\"More\"></button><span id=\"rl-diff-dot\" class=\"rl-diff-dot\" hidden></span>";
        let view = View::new(&Compare::Uncommitted);
        let out = decorate(html, &view, "1 change", None);
        assert!(
            out.contains("<span id=\"rl-diff-dot\" class=\"rl-diff-dot\">"),
            "{out}"
        );
        assert!(!out.contains("rl-diff-dot\" hidden"), "{out}");
        assert!(out.contains("aria-label=\"More, changes on\""), "{out}");
        assert!(chip(&out).contains("Showing uncommitted changes"), "{out}");
        assert!(chip(&out).contains("1 change"), "{out}");
        let hidden = "<button aria-label=\"More, comments off\" title=\"More, comments off\"></button><span id=\"rl-diff-dot\" class=\"rl-diff-dot\"></span>";
        let both = decorate(hidden, &view, "1 change", None);
        assert!(
            both.contains("aria-label=\"More, changes on, comments off\""),
            "{both}"
        );
    }

    #[test]
    fn size_cap_adds_no_diff_class() {
        let big = "x".repeat(MAX_BYTES + 1);
        let err = align(big.as_str(), "y", true).unwrap_err();
        assert_eq!(err, "diff was too large");
        let html =
            "<body><main><section class=\"rl-block\" id=\"b-1\"><p>y</p></section></main></body>";
        let out = decorate(html, &View::new(&Compare::Uncommitted), "diff was too large", None);
        assert!(!out.contains("rl-dx-add"), "{out}");
        assert!(out.contains("diff was too large"), "{out}");
    }

    #[test]
    fn two_ref_render_says_not_run_and_drops_live_output() {
        let old = "# Old\n\n```rustlab\nx = 1\n```\n";
        let new_src = "# New\n\n```rustlab\nx = 2\n```\n";
        let html = "<body><main><section class=\"rl-block\" id=\"b-1\"><pre class=\"output\">EXECUTED</pre><div class=\"plot-container\"></div></section></main></body>";
        let (inner, _) = render_aligned(old, new_src, true, true, None).unwrap();
        let page = replace_main(html, &inner);
        assert!(page.contains("not run"), "{page}");
        assert!(!page.contains("EXECUTED"), "{page}");
        assert!(!page.contains("plot-container"), "{page}");
        assert!(text_of(&page).contains("x = 2"), "{page}");
    }

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    #[test]
    fn branch_list_puts_current_then_main() {
        if !git_available() {
            eprintln!("SKIP branch list: git not found");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-b", "main"]);
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "user.name", "Test"]);
        std::fs::write(dir.join("n.md"), "# A\n").unwrap();
        git(dir, &["add", "n.md"]);
        git(dir, &["commit", "-m", "init"]);
        git(dir, &["checkout", "-b", "topic"]);
        let body = refs_json(dir);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["error"].is_null(), "{body}");
        let names: Vec<&str> = v["branches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.first().copied(), Some("topic"), "{body}");
        assert!(names.iter().any(|n| *n == "main"), "{body}");
        assert!(names.iter().all(|n| ref_ok(n)), "{body}");
    }

    #[test]
    fn present_two_ref_and_working_copy_against_a_repo() {
        if !git_available() {
            eprintln!("SKIP present fixture: git not found");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-b", "main"]);
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "user.name", "Test"]);
        let path = dir.join("n.md");
        std::fs::write(&path, "# One\n").unwrap();
        git(dir, &["add", "n.md"]);
        git(dir, &["commit", "-m", "one"]);
        git(dir, &["checkout", "-b", "topic"]);
        std::fs::write(&path, "# Two\n").unwrap();
        git(dir, &["add", "n.md"]);
        git(dir, &["commit", "-m", "two"]);
        let html = "<body><div id=\"rl-toolbar\"><button id=\"rl-diff-toggle\" type=\"button\" class=\"rl-quiet\">Diff</button><span id=\"rl-diff\" hidden></span></div><p id=\"rl-diff-banner\" hidden></p><main><section class=\"rl-block\" id=\"b-1\"><div class=\"prose\"><p>Two</p></div><pre class=\"output\">EXECUTED</pre></section></main></body>";
        let page = present(Present {
            html,
            source_path: &path,
            jail: Some(dir),
            comments_on: true,
            compare: &custom("main".into(), Some("topic".into())),
            source: false,
            link: None,
        });
        assert!(page.contains("not run"), "{page}");
        assert!(!page.contains("EXECUTED"), "{page}");
        assert!(!page.contains("plot-container"));
        assert!(page.contains("id=\"rl-diff\""), "{page}");
        assert!(!page.contains("id=\"rl-diff\" hidden"), "{page}");

        let live = present(Present {
            html,
            source_path: &path,
            jail: Some(dir),
            comments_on: true,
            compare: &custom("main".into(), None),
            source: false,
            link: None,
        });
        assert!(live.contains("EXECUTED"), "{live}");
        assert!(live.contains("rl-dx-mod"), "{live}");
        assert!(chip(&live).contains("main → working copy"), "{live}");
    }

    #[test]
    fn git_off_never_spawns_and_hostile_config_does_not_run() {
        if !git_available() {
            eprintln!("SKIP hostile git: git not found");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let marker = dir.join("marker");
        let script = dir.join("hook.sh");
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho ran >> '{}'\n", marker.display()),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&script).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&script, perm).unwrap();

        git(dir, &["init", "-b", "main"]);
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "user.name", "Test"]);
        let path = dir.join("n.md");
        std::fs::write(&path, "# One\n").unwrap();
        git(dir, &["add", "n.md"]);
        git(dir, &["commit", "-m", "init"]);

        let script_s = script.display().to_string();
        let include = dir.join("hostile.ini");
        std::fs::write(
            &include,
            format!("[core]\nfsmonitor = {script_s}\n[diff]\nexternal = {script_s}\n"),
        )
        .unwrap();
        git(dir, &["config", "include.path", include.to_str().unwrap()]);
        git(dir, &["config", "diff.attack.textconv", &script_s]);
        std::fs::write(dir.join(".gitattributes"), "*.md diff=attack\n").unwrap();
        for name in [
            "cat-file",
            "rev-parse",
            "ls-tree",
            "for-each-ref",
            "hash-object",
            "ls-files",
            "merge-base",
        ] {
            git(
                dir,
                &["config", &format!("alias.{name}"), &format!("!{script_s}")],
            );
        }
        std::fs::write(&path, "# Two\n").unwrap();
        let _ = std::fs::remove_file(&marker);

        // Porcelain on purpose: these must run the fixture's programs so
        // the assertion below proves the hooks are real. Output is
        // captured so the marker script's git noise stays out of the log.
        let _ = Command::new("git")
            .arg("-C")
            .arg(dir)
            .arg("status")
            .output();
        let _ = Command::new("git").arg("-C").arg(dir).arg("diff").output();
        let _ = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["cat-file", "-p", "HEAD"])
            .output();
        let tripped = std::fs::read_to_string(&marker).unwrap_or_default();
        assert!(
            !tripped.is_empty(),
            "fixture hooks did not run under plain git"
        );
        std::fs::remove_file(&marker).unwrap();

        let html =
            "<body><main><section class=\"rl-block\" id=\"b-1\"><p>Two</p></section></main></body>";
        let before = git_spawns();
        let off = present_if(
            false,
            Present {
                html,
                source_path: &path,
                jail: Some(dir),
                comments_on: true,
                compare: &Compare::Uncommitted,
                source: false,
                link: None,
            },
        );
        let refs_off = refs_json_if(false, dir);
        assert_eq!(git_spawns(), before, "git spawned while diff is off");
        assert_eq!(off, html);
        assert!(refs_off.contains("git diff is off"), "{refs_off}");
        assert!(!marker.exists(), "disabled path ran a hook");

        let _page = present(Present {
            html,
            source_path: &path,
            jail: Some(dir),
            comments_on: true,
            compare: &Compare::Uncommitted,
            source: false,
            link: None,
        });
        let _counts = changes_json_if(true, &path, Some(dir), true);
        let refs = refs_json(dir);
        assert!(
            !marker.exists(),
            "hardened git ran a repo hook or alias: {refs}"
        );
        assert!(!refs.contains("git diff is off"), "{refs}");
    }

    fn quiet_git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn init_repo(dir: &Path) {
        quiet_git(dir, &["init", "-b", "main"]);
        quiet_git(dir, &["config", "user.email", "t@example.com"]);
        quiet_git(dir, &["config", "user.name", "Test"]);
    }

    fn prose_html(body: &str) -> String {
        format!(
            "<body><main><section class=\"rl-block\" id=\"b-1\"><div class=\"prose\"><p>{body}</p></div></section></main></body>"
        )
    }

    /// The 0.3.12 query: `diff` is the before side, `base` the after side.
    fn show(dir: &Path, path: &Path, diff: &str, base: Option<&str>) -> String {
        present(Present {
            html: &prose_html("x"),
            source_path: path,
            jail: Some(dir),
            comments_on: true,
            compare: &custom(diff.into(), base.map(String::from)),
            source: false,
            link: None,
        })
    }

    /// The status chip's text, tags stripped.
    fn chip(page: &str) -> String {
        let Some(i) = page.find("<button id=\"rl-diff-banner\"") else {
            return String::new();
        };
        let end = page[i..].find("</button>").map_or(page.len(), |e| i + e);
        text_of(&page[i..end])
    }

    /// Visible text of an HTML fragment: tags dropped, the few entities
    /// the renderer writes decoded.
    fn text_of(html: &str) -> String {
        let mut out = String::new();
        let mut in_tag = false;
        for c in html.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => out.push(c),
                _ => {}
            }
        }
        out.replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
    }

    #[test]
    fn ref_checker_accepts_plus_in_a_branch_name() {
        assert!(ref_ok("topic+1"));
        assert!(ref_ok("release/1.2"));
    }

    #[test]
    fn fixture_repos_cover_states_refs_and_file_changes() {
        if !git_available() {
            eprintln!("SKIP git fixtures: git not found");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();

        // Not a repository.
        let stray = repo.join("n.md");
        std::fs::write(&stray, "# Hello\n").unwrap();
        let page = show(&repo, &stray, "HEAD", None);
        assert!(page.contains("not a git repository"), "{page}");

        init_repo(&repo);

        // Unborn branch: the notebook is a new file, not an unknown ref
        // and not a green bar on every block.
        let page = show(&repo, &stray, "HEAD", None);
        assert!(chip(&page).contains("new file, not on HEAD"), "{page}");
        assert!(chip(&page).contains("no commits yet"), "{page}");
        assert!(page.contains("rl-diff-notice"), "{page}");
        assert!(!page.contains("rl-dx-add"), "{page}");
        assert!(!page.contains("unknown ref"), "{page}");

        quiet_git(&repo, &["add", "n.md"]);
        quiet_git(&repo, &["commit", "-m", "one"]);

        // Clean single commit, including a CRLF-only rewrite.
        let page = show(&repo, &stray, "HEAD", None);
        assert!(chip(&page).contains("Showing uncommitted changes · vs last commit"), "{page}");
        assert!(chip(&page).contains("no changes"), "{page}");
        std::fs::write(&stray, "# Hello\r\n").unwrap();
        let page = show(&repo, &stray, "HEAD", None);
        assert!(
            chip(&page).contains("no changes"),
            "CRLF-only edit looked like a change: {page}"
        );
        assert!(!page.contains("rl-ln-del"), "{page}");

        // A real edit on top of CRLF still shows that edit, not a
        // carriage return on every line.
        std::fs::write(&stray, "# Hello\r\n\nworld 🌍\r\n").unwrap();
        let page = show(&repo, &stray, "HEAD", None);
        assert!(chip(&page).contains("1 change"), "{page}");
        // The stub page has one <p> for two source parts, so the whole
        // section is marked instead of one element.
        assert!(page.contains("rl-dx rl-dx-mod"), "{page}");
        assert!(!page.contains("&#13;"), "{page}");

        // Code cells expose the new line itself. CRLF must not make
        // every line a deletion.
        let code = repo.join("c.md");
        std::fs::write(&code, "```rustlab\nx = 1\n```\n").unwrap();
        quiet_git(&repo, &["add", "c.md"]);
        quiet_git(&repo, &["commit", "-m", "code"]);
        std::fs::write(&code, "```rustlab\nx = 1\r\ny = 2\r\n```\r\n").unwrap();
        let code_html = "<body><main><section class=\"rl-block\" id=\"b-1\"><div class=\"code-block\"><pre class=\"source\"><code>x = 1\ny = 2\n</code></pre></section></main></body>";
        let page = present(Present {
            html: code_html,
            source_path: &code,
            jail: Some(&repo),
            comments_on: true,
            compare: &Compare::Uncommitted,
            source: false,
            link: None,
        });
        assert!(page.contains("rl-ln-add"), "{page}");
        assert!(text_of(&page).contains("y = 2"), "{page}");
        assert!(text_of(&page).contains("x = 1"), "{page}");
        assert!(!page.contains("rl-ln-del"), "{page}");
        assert!(!page.contains("&#13;"), "{page}");

        // Very long line.
        let long = format!("# {}\n", "x".repeat(8000));
        std::fs::write(&stray, &long).unwrap();
        let page = show(&repo, &stray, "HEAD", None);
        assert!(chip(&page).contains("change"), "{page}");
        assert!(!page.contains("unknown ref"), "{page}");

        // Empty file: still a diff, not "could not align" alone.
        std::fs::write(&stray, "").unwrap();
        let page = show(&repo, &stray, "HEAD", None);
        assert!(page.contains("rl-dx-removed"), "{page}");
        assert!(chip(&page).contains("change"), "{page}");
        assert!(!page.contains("could not align"), "{page}");

        // Binary.
        std::fs::write(&stray, b"# no\0pe\n").unwrap();
        let page = show(&repo, &stray, "HEAD", None);
        assert!(page.contains("not a text notebook"), "{page}");

        // Restore and commit a second revision for refs.
        std::fs::write(&stray, "# Hello again\n").unwrap();
        quiet_git(&repo, &["add", "n.md"]);
        quiet_git(&repo, &["commit", "-m", "restore"]);
        std::fs::write(&stray, "# Hello\n\nedited\n").unwrap();

        // Staged-only, then both staged and unstaged.
        quiet_git(&repo, &["add", "n.md"]);
        let vs_head = show(&repo, &stray, "HEAD", None);
        let vs_index = show(&repo, &stray, "index", None);
        assert!(chip(&vs_head).contains("change"), "{vs_head}");
        assert!(!chip(&vs_head).contains("no changes"), "{vs_head}");
        assert!(chip(&vs_index).contains("no changes"), "{vs_index}");
        assert!(chip(&vs_index).contains("index → working copy"), "{vs_index}");
        std::fs::write(&stray, "# Hello\n\nedited\n\nunstaged\n").unwrap();
        let vs_index = show(&repo, &stray, "index", None);
        assert!(chip(&vs_index).contains("1 change"), "{vs_index}");
        assert!(vs_index.contains("\"status\":\"M\""), "{vs_index}");

        // Slash branch, plus in the name, lightweight and annotated tags.
        quiet_git(&repo, &["checkout", "-b", "feature/topic+1"]);
        quiet_git(&repo, &["add", "n.md"]);
        quiet_git(&repo, &["commit", "-m", "topic"]);
        quiet_git(&repo, &["tag", "v1"]);
        quiet_git(&repo, &["tag", "-a", "v2", "-m", "annotated"]);
        quiet_git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        let refs = refs_json(&repo);
        assert!(refs.contains("feature/topic+1"), "{refs}");
        assert!(refs.contains("\"v1\""), "{refs}");
        assert!(refs.contains("\"v2\""), "{refs}");
        assert!(refs.contains("origin/main"), "{refs}");
        let head = show(&repo, &stray, "HEAD", None);
        assert!(chip(&head).contains("no changes"), "{head}");
        let sha = {
            let out = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let short: String = sha.chars().take(7).collect();
        let page = show(&repo, &stray, &short, None);
        assert!(!page.contains("unknown ref"), "{page}");
        let page = show(&repo, &stray, "HEAD~1", None);
        assert!(chip(&page).contains("HEAD~1 → working copy"), "{page}");
        let page = show(&repo, &stray, "HEAD^", None);
        assert!(!page.contains("unknown ref"), "{page}");
        let page = show(&repo, &stray, "v2", None);
        assert!(!page.contains("unknown ref"), "{page}");
        let page = show(&repo, &stray, "no-such-ref", None);
        assert!(page.contains("unknown ref"), "{page}");
        let page = show(&repo, &stray, "feature/topic+1", Some("feature/topic+1"));
        assert!(chip(&page).contains("no changes"), "{page}");
        assert!(!page.contains("unknown ref"), "{page}");

        // Two refs read from base to compare; nothing runs.
        let page = show(&repo, &stray, "HEAD~1", Some("HEAD"));
        assert!(chip(&page).contains("HEAD~1 → HEAD"), "{page}");
        assert!(chip(&page).contains("code was not run"), "{page}");

        // Exact rename, then rename with an edit.
        quiet_git(&repo, &["mv", "n.md", "N.md"]);
        let renamed = repo.join("N.md");
        let page = show(&repo, &renamed, "HEAD", None);
        assert!(
            page.contains("\"status\":\"R\"") || page.contains("renamed"),
            "{page}"
        );
        std::fs::write(&renamed, "# Hello\n\nedited\n\nunstaged\n\nplus\n").unwrap();
        let page = show(&repo, &renamed, "HEAD", None);
        assert!(
            page.contains("\"status\":\"R\""),
            "edited rename was not paired: {page}"
        );
        assert!(page.contains("rl-dx rl-dx-"), "{page}");

        // Ignored files and a nested repository must not become additions
        // or exhaust the walk.
        std::fs::write(repo.join(".gitignore"), "junk/\n").unwrap();
        std::fs::create_dir_all(repo.join("junk")).unwrap();
        for i in 0..2100 {
            std::fs::write(repo.join("junk").join(format!("f{i}.txt")), "z").unwrap();
        }
        let nest = repo.join("nest");
        std::fs::create_dir(&nest).unwrap();
        quiet_git(&nest, &["init", "-b", "main"]);
        std::fs::write(nest.join("secret.txt"), "nope").unwrap();
        std::fs::write(repo.join("link.txt"), "target").unwrap();
        std::os::unix::fs::symlink("link.txt", repo.join("sym")).unwrap();
        let page = show(&repo, &renamed, "HEAD", None);
        assert!(!page.contains("junk/"), "{page}");
        assert!(!page.contains("secret.txt"), "{page}");
        assert!(page.contains("\"status\":\"R\""), "{page}");

        // Subfolder of the repo, and the repo root, both resolve.
        let notes = repo.join("notes");
        std::fs::create_dir(&notes).unwrap();
        let sub = notes.join("a.md");
        std::fs::write(&sub, "# Sub\n").unwrap();
        quiet_git(&repo, &["add", "notes/a.md"]);
        quiet_git(&repo, &["commit", "-m", "sub"]);
        std::fs::write(&sub, "# Sub changed\n").unwrap();
        let page = show(&repo, &sub, "HEAD", None);
        assert!(chip(&page).contains("1 change"), "{page}");
        assert!(page.contains("rl-dx-mod"), "{page}");

        // Detached HEAD still compares.
        quiet_git(&repo, &["checkout", "--detach"]);
        let page = show(&repo, &sub, "HEAD", None);
        assert!(!page.contains("unknown ref"), "{page}");
        let refs = refs_json(&repo);
        assert!(refs.contains("commits"), "{refs}");
        assert!(refs.contains("\"error\":null"), "{refs}");

        // Worktree whose .git is a gitdir file.
        let wt = tmp.path().join("wt");
        quiet_git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "wt-branch",
                wt.to_str().unwrap(),
                "HEAD",
            ],
        );
        assert!(
            wt.join(".git").is_file(),
            "worktree .git should be a gitdir file"
        );
        let wt_nb = wt.join("notes").join("a.md");
        std::fs::write(&wt_nb, "# From worktree\n").unwrap();
        let page = show(&wt, &wt_nb, "HEAD", None);
        // No word in common: shown as the old heading removed and the new
        // one added, not a struck-through modify.
        assert!(chip(&page).contains("2 changes"), "{page}");
        assert!(!page.contains("not a git repository"), "{page}");
        let watched = git_watch_paths(&wt);
        assert!(!watched.is_empty(), "worktree git dir not found");

        // Deleted path still has a repo-relative name.
        let gone = notes.join("missing.md");
        let rel = safe_rel(&gone, Some(&repo), &repo).unwrap();
        assert_eq!(rel, "notes/missing.md");

        // Untracked new notebook is one fact, not a green bar per block.
        let fresh = repo.join("new.md");
        std::fs::write(&fresh, "# Brand new\n").unwrap();
        let page = show(&repo, &fresh, "HEAD", None);
        assert!(chip(&page).contains("new file, not on HEAD"), "{page}");
        assert!(page.contains("rl-diff-notice"), "{page}");
        assert!(!page.contains("rl-dx-add"), "{page}");
    }

    /// A watch page whose head script mentions `<main>` in a comment.
    /// `find("<main")` hits that comment. The real element comes later.
    fn watch_html(body: &str) -> String {
        format!(
            "<head><script>\n  // The rendered notebook lives inside <main>; the page chrome\n  function mainEl() {{ return document.querySelector('main'); }}\n</script></head>\n<body class=\"has-files\"><div id=\"rl-toolbar\"><button id=\"rl-diff-toggle\" type=\"button\">Show diff</button><input id=\"rl-diff-ref\"></div><p id=\"rl-diff-banner\" hidden></p><main>\n<section class=\"rl-block\" id=\"b-1\"><div class=\"prose\"><p>{body}</p></div></section>\n</main></body>"
        )
    }

    fn script_was_left_alone(page: &str) {
        let start = page.find("<script>").expect("script");
        let end = page[start..].find("</script>").expect("script end") + start;
        let script = &page[start..end];
        assert!(
            script.contains("document.querySelector('main')"),
            "head script was rewritten: {script}"
        );
        assert!(
            !script.contains("rl-diff-notice") && !script.contains("rl-dx-"),
            "diff html was spliced into the head script: {script}"
        );
        assert!(page.contains("<body"), "body tag was deleted: {page}");
        assert!(
            page.contains("id=\"rl-diff-ref\""),
            "picker was deleted: {page}"
        );
    }

    #[test]
    fn one_sided_notebook_recovers_and_does_not_cache_the_miss() {
        if !git_available() {
            eprintln!("SKIP one-sided: git not found");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        init_repo(&repo);
        std::fs::write(repo.join("readme.md"), "# Readme\n").unwrap();
        quiet_git(&repo, &["add", "readme.md"]);
        quiet_git(&repo, &["commit", "-m", "readme"]);
        quiet_git(&repo, &["branch", "ancient"]);
        let note = repo.join("note.md");
        std::fs::write(
            &note,
            "# Note\n\nShared.\n\n```rustlab\nx = 1\ny = 2\n```\n",
        )
        .unwrap();
        quiet_git(&repo, &["add", "note.md"]);
        quiet_git(&repo, &["commit", "-m", "add note"]);
        quiet_git(&repo, &["checkout", "-b", "dropped"]);
        quiet_git(&repo, &["rm", "note.md"]);
        quiet_git(&repo, &["commit", "-m", "drop note"]);
        quiet_git(&repo, &["checkout", "main"]);
        std::fs::write(
            &note,
            "# Note\n\nShared.\n\n```rustlab\nx = 1\ny = 3\n```\n",
        )
        .unwrap();

        let poison = "POISON-PATH-ONLY";
        cache_put("note.md".into(), poison.into());

        let html = watch_html("working copy");
        // `base` before, `compare` after (working copy when None).
        let present_at = |base: &str, compare: Option<&str>| {
            present(Present {
                html: &html,
                source_path: &note,
                jail: Some(&repo),
                comments_on: true,
                compare: &custom(base.into(), compare.map(String::from)),
                source: false,
                link: None,
            })
        };

        // Missing on the compared side: a notice, not every block green.
        let missing = present_at("ancient", None);
        assert!(chip(&missing).contains("new file, not on ancient"), "{missing}");
        assert!(missing.contains("rl-diff-notice"), "{missing}");
        assert!(missing.contains("not on ancient"), "{missing}");
        assert!(!missing.contains("rl-dx-add"), "{missing}");
        assert!(!missing.contains(poison), "{missing}");
        script_was_left_alone(&missing);
        if let Some(v) = cache_get("note.md") {
            assert_eq!(
                v, poison,
                "a miss must not replace a path-only entry with an empty blob"
            );
        }

        // The next ref, where the file exists, is a normal line diff.
        // A cached miss would still say "new file".
        let recovered = present_at("main", None);
        assert!(
            !recovered.contains("new file"),
            "stayed on the missing-file result: {recovered}"
        );
        assert!(recovered.contains("rl-dx-mod"), "{recovered}");
        assert!(text_of(&recovered).contains("y = 3"), "{recovered}");
        assert!(chip(&recovered).contains("main → working copy · 1 change"), "{recovered}");
        assert!(!recovered.contains(poison), "{recovered}");
        script_was_left_alone(&recovered);
        let root = std::fs::canonicalize(&repo).unwrap();
        let root = root.display().to_string();
        assert!(
            cache_keys().iter().any(|k| k.starts_with(&root)
                && k.ends_with(":note.md")
                && k.matches(':').count() >= 2),
            "blob cache key must include the repo and the oid, not the path alone: {:?}",
            cache_keys()
        );

        // Two refs, shown side deleted the file. The page must keep its
        // chrome (this used to splice into the head script and drop <body>).
        let deleted_side = present_at("main", Some("dropped"));
        assert!(chip(&deleted_side).contains("not on dropped"), "{deleted_side}");
        assert!(deleted_side.contains("rl-dx-removed"), "{deleted_side}");
        assert!(!deleted_side.contains("rl-dx-add"), "{deleted_side}");
        script_was_left_alone(&deleted_side);

        // Newer side has the file, older side does not: a notice, not a
        // green wash, and the head script is still intact.
        let both = present_at("ancient", Some("main"));
        assert!(chip(&both).contains("new file, not on ancient"), "{both}");
        assert!(!both.contains("rl-dx-add"), "{both}");
        script_was_left_alone(&both);
        let again = present_at("HEAD", None);
        assert!(chip(&again).contains("1 change"), "{again}");
        assert!(!again.contains("new file"), "{again}");
        assert!(!again.contains("not on HEAD"), "{again}");

        // Working copy deleted, ref still has it: red / deleted, then
        // putting the file back is a normal diff again.
        std::fs::remove_file(&note).unwrap();
        let gone = present_at("main", None);
        assert!(chip(&gone).contains("deleted from the working copy"), "{gone}");
        assert!(gone.contains("It is still on main."), "{gone}");
        assert!(gone.contains("rl-dx-removed"), "{gone}");
        assert!(gone.contains("rl-diff-notice"), "{gone}");
        script_was_left_alone(&gone);
        std::fs::write(
            &note,
            "# Note\n\nShared.\n\n```rustlab\nx = 1\ny = 3\n```\n",
        )
        .unwrap();
        let back = present_at("main", None);
        assert!(!back.contains("deleted"), "{back}");
        assert!(chip(&back).contains("1 change"), "{back}");
        assert!(!back.contains("rl-dx-removed"), "{back}");

        if let Ok(mut cache) = BLOB_CACHE.lock() {
            cache.retain(|(k, _)| k != "note.md");
        }
    }

    #[test]
    fn presets_map_to_before_and_after_and_old_links_still_resolve() {
        let q = |s: &str| resolve_compare(&parse_query(s), None);
        assert_eq!(q("changes=uncommitted"), Some(Compare::Uncommitted));
        assert_eq!(q("changes=branch"), Some(Compare::Branch));
        assert_eq!(q("changes=last"), Some(Compare::Last));
        assert_eq!(q("changes=off"), None);
        assert_eq!(
            q("changes=custom&base=v0.3&compare=topic"),
            Some(Compare::Custom {
                base: "v0.3".into(),
                compare: Some("topic".into())
            })
        );
        assert_eq!(
            q("changes=custom&base=main"),
            Some(Compare::Custom {
                base: "main".into(),
                compare: None
            })
        );
        // base defaults to HEAD, which against the working copy is the
        // uncommitted preset.
        assert_eq!(q("changes=custom"), Some(Compare::Uncommitted));
        assert_eq!(q("changes=wat"), Some(Compare::Unknown("wat".into())));
        // 0.3.12 links: `diff` is the before side, `base` the after side.
        assert_eq!(q("diff=HEAD"), Some(Compare::Uncommitted));
        assert_eq!(q("diff=off"), None);
        assert_eq!(
            q("diff=main&base=topic"),
            Some(Compare::Custom {
                base: "main".into(),
                compare: Some("topic".into())
            })
        );
        // With `changes` present, `base` is the new before side.
        assert_eq!(
            q("changes=custom&diff=ignored&base=main"),
            Some(Compare::Custom {
                base: "main".into(),
                compare: None
            })
        );
        // `?changes=off` wins over an old key.
        assert_eq!(q("changes=off&diff=main"), None);
        // No keys: the `--diff` default is the before side.
        assert_eq!(resolve_compare(&parse_query(""), None), None);
        assert_eq!(
            resolve_compare(&parse_query(""), Some("main")),
            Some(Compare::Custom {
                base: "main".into(),
                compare: None
            })
        );
        assert_eq!(
            resolve_compare(&parse_query(""), Some("HEAD")),
            Some(Compare::Uncommitted)
        );
        assert_eq!(Compare::Branch.key(), "branch");
    }

    const DEMO_OLD: &str = "# Filters\n\nA lowpass filter.\n\n```rustlab\nh = 31;\nw = \"hann\";\ndisp(h)\n```\n\n## Frequency response\n\nAbout 40 dB of attenuation for 31 taps.\n\n## Old notes\n\nThis section described an earlier design.\n\n## Summary\n\nLonger filters give a sharper transition band.\n";
    const DEMO_NEW: &str = "# Filters\n\nA lowpass filter.\n\n```rustlab\nh = 63;\nw = \"hamming\";\ndisp(h)\n```\n\n## Frequency response\n\nAbout 53 dB of attenuation for 63 taps.\n\n## Summary\n\nDoubling the taps doubles the cost.\n\nLonger filters give a sharper transition band.\n";

    /// The real HTML render of `src`, as the watch server stores it.
    fn rendered(src: &str) -> String {
        let blocks = crate::parse::parse_notebook(src);
        let out = crate::execute::execute_notebook(&blocks);
        crate::render::render_html(
            "Demo",
            &out,
            &std::env::temp_dir().join("rustlab_gitdiff_demo_plots"),
            "plots",
            rustlab_plot::theme_colors("mocha").unwrap(),
            None,
            &crate::render::LinkMode::single_file(),
        )
    }

    #[test]
    fn paragraph_units_mark_only_what_changed_on_a_real_render() {
        let html = rendered(DEMO_NEW);
        let (out, n) = stamp_working(&html, DEMO_OLD, DEMO_NEW, true, None).unwrap();
        assert_eq!(n, 4, "code, one paragraph, one removed run, one added: {out}");
        let marked: Vec<(&str, String)> = out
            .match_indices("data-rl-dx=\"")
            .map(|(i, _)| {
                let rest = &out[i + 12..];
                let state = &rest[..rest.find('"').unwrap()];
                let w = rest.find("data-rl-where=\"").unwrap() + 15;
                let place = &rest[w..w + rest[w..].find('"').unwrap()];
                (state, place.to_string())
            })
            .collect();
        assert_eq!(
            marked,
            vec![
                ("modified", "Code cell 1".to_string()),
                ("modified", "Frequency response ¶1".to_string()),
                ("removed", "Old notes".to_string()),
                ("added", "Summary ¶1".to_string()),
            ],
            "{out}"
        );
        // The modified paragraph is its own <p>, not the whole prose run,
        // and only the changed words are marked inside it.
        assert!(
            out.contains("<p class=\"rl-dx rl-dx-mod\" data-rl-dx=\"modified\" data-rl-where=\"Frequency response ¶1\">About <del class=\"rl-w\">40</del><ins class=\"rl-w\">53</ins> dB of attenuation for <del class=\"rl-w\">31</del><ins class=\"rl-w\">63</ins> taps.</p>"),
            "{out}"
        );
        // The added paragraph reads as all new text.
        assert!(
            out.contains("<ins class=\"rl-w rl-w-all\">Doubling the taps doubles the cost.</ins>"),
            "{out}"
        );
        // Code lines mark the changed tokens and keep their colors.
        assert!(
            out.contains("<ins class=\"rl-w\"><span class=\"syn-num\">63</span></ins>"),
            "{out}"
        );
        assert!(
            out.contains("<ins class=\"rl-w\"><span class=\"syn-str\">&quot;hamming&quot;</span></ins>"),
            "{out}"
        );
        // Summary's heading and old paragraph are not marked.
        let summary = out.find(">Summary</h2>").expect("summary heading");
        let open = out[..summary].rfind("<h2").unwrap();
        assert!(!out[open..summary].contains("rl-dx"), "{out}");
        let longer = out.find("Longer filters").unwrap();
        let p = out[..longer].rfind("<p").unwrap();
        assert_eq!(&out[p..longer], "<p>", "{out}");
        // The removed section is collapsed at its old place, between the
        // edited paragraph and Summary.
        let removed = out.find("<details class=\"rl-dx rl-dx-del rl-dx-removed\"").unwrap();
        assert!(removed > out.find("63</ins> taps.").unwrap() && removed < summary, "{out}");
        assert!(!out[removed..removed + 200].contains(" open"), "{out}");
        assert!(
            out.contains("<b>Removed</b> · “Old notes” heading and 1 paragraph"),
            "{out}"
        );
        assert!(out[removed..].contains("This section described an earlier design."));
        // The code cell shows its source once, as the diff.
        let text = text_of(&out);
        assert_eq!(text.matches("h = 63;").count(), 1, "{out}");
        assert_eq!(text.matches("h = 31;").count(), 1, "{out}");
        assert_eq!(text.matches("disp(h)").count(), 1, "{out}");
        assert!(out.contains("Output is from your current file."), "{out}");
        // Syntax colors stay on the listing.
        let listing = &out[out.find("rl-dx-code").unwrap()..];
        assert!(listing[..listing.find("</pre>").unwrap()].contains("syn-num"), "{out}");
        // Same source, no marks.
        let (same, n0) = stamp_working(&html, DEMO_NEW, DEMO_NEW, true, None).unwrap();
        assert_eq!(n0, 0);
        assert_eq!(same, html);
    }

    #[test]
    fn raw_html_in_a_prose_run_marks_that_run_whole() {
        let old = "Intro.\n\n<details><summary>More</summary>\n\nhidden text\n\n</details>\n\nOutro.\n";
        let new_src = "Intro.\n\n<details><summary>More</summary>\n\nshown text\n\n</details>\n\nOutro.\n";
        let html = rendered(new_src);
        let (out, n) = stamp_working(&html, old, new_src, true, None).unwrap();
        assert_eq!(n, 1, "{out}");
        let at = out.find("data-rl-dx=\"modified\"").expect("marked");
        let tag = &out[out[..at].rfind('<').unwrap()..at];
        assert!(tag.starts_with("<section"), "whole section: {tag}");
        assert!(!out.contains("<p class=\"rl-dx"), "{out}");
    }

    #[test]
    fn a_paragraph_does_not_pair_with_a_heading() {
        let sides = align_sides("Alpha.\n", "## Alpha\n", true).unwrap();
        assert_eq!(sides, vec!["removed", "added"]);
        let sides = align_sides("Alpha.\n\nBeta.\n", "Alpha.\n\nBeta!\n", true).unwrap();
        assert_eq!(sides, vec!["same", "changed"]);
    }

    fn rev(dir: &Path, spec: &str) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", spec])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn branch_preset_uses_the_merge_base_and_counts_every_preset() {
        if !git_available() {
            eprintln!("SKIP branch preset: git not found");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        init_repo(dir);
        let nb = dir.join("nb.md");
        std::fs::write(&nb, "# A\n\nOne.\n").unwrap();
        quiet_git(dir, &["add", "nb.md"]);
        quiet_git(dir, &["commit", "-m", "one"]);
        let fork = rev(dir, "HEAD");
        quiet_git(dir, &["checkout", "-b", "topic"]);
        std::fs::write(&nb, "# A\n\nOne edited.\n").unwrap();
        quiet_git(dir, &["commit", "-am", "topic edit"]);
        // main moves ahead after topic left it.
        quiet_git(dir, &["checkout", "main"]);
        std::fs::write(&nb, "# A\n\nOne.\n\nMain only.\n").unwrap();
        quiet_git(dir, &["commit", "-am", "main ahead"]);
        quiet_git(dir, &["checkout", "topic"]);
        std::fs::write(&nb, "# A\n\nOne edited.\n\nUncommitted.\n").unwrap();

        let page = |c: &Compare| {
            present(Present {
                html: &rendered("# A\n\nOne edited.\n\nUncommitted.\n"),
                source_path: &nb,
                jail: Some(dir),
                comments_on: true,
                compare: c,
                source: false,
                link: None,
            })
        };
        let branch = page(&Compare::Branch);
        let c = chip(&branch);
        assert!(c.contains("this branch vs main"), "{c}");
        assert!(c.contains(&format!("from {}", short(&fork))), "{c}");
        assert!(c.contains("2 changes"), "{c}");
        assert!(!branch.contains("rl-dx-removed"), "main's newer commit is not a removal: {branch}");
        assert!(!text_of(&branch).contains("Main only"), "{branch}");
        assert!(branch.contains("data-rl-changes=\"branch\""), "{branch}");

        let unc = page(&Compare::Uncommitted);
        assert!(chip(&unc).contains("Showing uncommitted changes"), "{unc}");
        assert!(chip(&unc).contains("1 change"), "{unc}");

        let last = page(&Compare::Last);
        assert!(chip(&last).contains("last commit"), "{last}");
        assert!(chip(&last).contains("code was not run"), "{last}");

        let counts: serde_json::Value =
            serde_json::from_str(&changes_json_if(true, &nb, Some(dir), true)).unwrap();
        assert!(counts["error"].is_null(), "{counts}");
        assert_eq!(counts["current"], "topic", "{counts}");
        assert_eq!(counts["default"], "main", "{counts}");
        assert_eq!(counts["uncommitted"]["count"], 1, "{counts}");
        assert_eq!(counts["branch"]["count"], 2, "{counts}");
        assert_eq!(counts["branch"]["base"], short(&fork), "{counts}");
        assert_eq!(counts["last"]["count"], 1, "{counts}");

        // On the default branch the preset is hidden, and asking for it
        // anyway explains why there is nothing to show.
        quiet_git(dir, &["checkout", "--", "nb.md"]);
        quiet_git(dir, &["checkout", "main"]);
        let counts: serde_json::Value =
            serde_json::from_str(&changes_json_if(true, &nb, Some(dir), true)).unwrap();
        assert_eq!(counts["branch"]["hidden"], true, "{counts}");
        let on_main = page(&Compare::Branch);
        assert!(chip(&on_main).contains("already on main"), "{on_main}");
    }

    #[test]
    fn last_commit_needs_a_parent_and_default_branch_falls_back() {
        if !git_available() {
            eprintln!("SKIP last commit: git not found");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        quiet_git(dir, &["init", "-b", "master"]);
        quiet_git(dir, &["config", "user.email", "t@example.com"]);
        quiet_git(dir, &["config", "user.name", "Test"]);
        let nb = dir.join("nb.md");
        std::fs::write(&nb, "# A\n").unwrap();
        quiet_git(dir, &["add", "nb.md"]);
        quiet_git(dir, &["commit", "-m", "root"]);
        let counts: serde_json::Value =
            serde_json::from_str(&changes_json_if(true, &nb, Some(dir), true)).unwrap();
        assert_eq!(
            counts["last"]["disabled"], "No earlier commit to compare with",
            "{counts}"
        );
        assert_eq!(counts["default"], "master", "{counts}");
        let page = present(Present {
            html: &prose_html("A"),
            source_path: &nb,
            jail: Some(dir),
            comments_on: true,
            compare: &Compare::Last,
            source: false,
            link: None,
        });
        assert!(chip(&page).contains("no earlier commit to compare with"), "{page}");

        // Neither main nor master: the branch origin/HEAD points at.
        let other = tmp.path().join("other");
        std::fs::create_dir(&other).unwrap();
        quiet_git(&other, &["init", "-b", "trunk"]);
        quiet_git(&other, &["config", "user.email", "t@example.com"]);
        quiet_git(&other, &["config", "user.name", "Test"]);
        std::fs::write(other.join("nb.md"), "# A\n").unwrap();
        quiet_git(&other, &["add", "nb.md"]);
        quiet_git(&other, &["commit", "-m", "root"]);
        let root = repo_root(&other).unwrap();
        assert_eq!(default_branch(&root).unwrap(), None);
        quiet_git(&other, &["update-ref", "refs/remotes/origin/trunk", "HEAD"]);
        quiet_git(
            &other,
            &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/trunk"],
        );
        assert_eq!(default_branch(&root).unwrap().as_deref(), Some("origin/trunk"));
        assert!(on_default(Some("trunk"), "origin/trunk"));
        assert!(!on_default(Some("topic"), "origin/trunk"));
        assert!(!on_default(None, "main"));
    }

    #[test]
    fn word_marks_keep_math_whole_and_split_rewrites() {
        let stamp = |old: &str, new_src: &str| {
            let html = rendered(new_src);
            stamp_working(&html, old, new_src, true, None).unwrap()
        };
        let (out, n) = stamp("Let $x$ be small.\n", "Let $y$ be small.\n");
        assert_eq!(n, 1, "{out}");
        assert!(
            out.contains("<del class=\"rl-w\">\\(x\\)</del><ins class=\"rl-w\">\\(y\\)</ins>"),
            "each side is one whole math span: {out}"
        );
        // Under half the words survive: removed plus added, not confetti.
        let (out, n) = stamp("Intro.\n\nThe old sentence here.\n", "Intro.\n\nSomething else entirely now.\n");
        assert_eq!(n, 2, "{out}");
        assert!(out.contains("data-rl-dx=\"removed\""), "{out}");
        assert!(out.contains("<ins class=\"rl-w rl-w-all\">Something else entirely now.</ins>"), "{out}");
        assert!(!out.contains("<del class=\"rl-w\">"), "{out}");
        // A changed emphasis marker keeps the element mark only.
        let (out, n) = stamp("Some *bold* words here.\n", "Some **bold** words here.\n");
        assert_eq!(n, 1, "{out}");
        assert!(out.contains("rl-dx rl-dx-mod"), "{out}");
        assert!(!out.contains("class=\"rl-w\""), "{out}");
        // A link keeps rendering as a link inside a marked paragraph.
        let (out, _) = stamp(
            "See [the guide](https://example.com) for 3 steps.\n",
            "See [the guide](https://example.com) for 4 steps.\n",
        );
        assert!(out.contains("<a href=\"https://example.com\">the guide</a>"), "{out}");
        assert!(out.contains("<ins class=\"rl-w\">4</ins>"), "{out}");
        // A list item edit is marked word by word inside the list.
        let (out, _) = stamp("- one apple\n- two pears\n", "- one apple\n- three pears\n");
        assert!(out.contains("<li><del class=\"rl-w\">two</del><ins class=\"rl-w\">three</ins> pears</li>"), "{out}");
    }

    #[test]
    fn source_view_replaces_main_with_the_md_diff() {
        if !git_available() {
            eprintln!("SKIP source view: git not found");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        init_repo(dir);
        let nb = dir.join("nb.md");
        std::fs::write(&nb, DEMO_OLD).unwrap();
        quiet_git(dir, &["add", "nb.md"]);
        quiet_git(dir, &["commit", "-m", "one"]);
        std::fs::write(&nb, DEMO_NEW).unwrap();
        let html = rendered(DEMO_NEW);
        let page = |c: &Compare, source: bool| {
            present(Present {
                html: &html,
                source_path: &nb,
                jail: Some(dir),
                comments_on: true,
                compare: c,
                source,
                link: None,
            })
        };
        let src = page(&Compare::Uncommitted, true);
        assert!(src.contains("data-rl-view=\"source\""), "{src}");
        assert!(src.contains("class=\"rl-dx-srcview\""), "{src}");
        assert!(src.contains("nb.md · uncommitted changes · 4 changes"), "{src}");
        // The edits are close together, so their context merges into one
        // hunk, named after the heading above the first change.
        assert!(src.contains("@@ Filters @@"), "code hunk names its heading: {src}");
        assert_eq!(src.matches("class=\"rl-dx-hunk").count(), 1, "{src}");
        assert!(src.contains("<del class=\"rl-w\">40</del>"), "{src}");
        assert!(chip(&src).contains("4 changes"), "{src}");
        // The rendered notebook is gone from <main>; the source lines are there.
        assert!(!src.contains("Output is from your current file."), "{src}");
        assert!(text_of(&src).contains("## Old notes"), "{src}");
        let rendered_view = page(&Compare::Uncommitted, false);
        assert!(rendered_view.contains("data-rl-view=\"rendered\""));
        assert!(!rendered_view.contains("rl-dx-srcview"));
        // A notebook too big for the rendered diff falls back to the source.
        let big_old: String = (0..1600).map(|i| format!("Line {i}.\n\n")).collect();
        let big_new = big_old.replace("Line 7.", "Line seven.");
        std::fs::write(&nb, &big_old).unwrap();
        quiet_git(dir, &["commit", "-qam", "big"]);
        std::fs::write(&nb, &big_new).unwrap();
        let big = present(Present {
            html: &prose_html("x"),
            source_path: &nb,
            jail: Some(dir),
            comments_on: true,
            compare: &Compare::Uncommitted,
            source: false,
            link: None,
        });
        assert!(big.contains("rl-dx-srcview"), "{}", chip(&big));
        assert!(chip(&big).contains("diff was too large, showing the source"), "{}", chip(&big));
        assert!(big.contains("<ins class=\"rl-w\">seven</ins>"), "{}", chip(&big));
    }
}
