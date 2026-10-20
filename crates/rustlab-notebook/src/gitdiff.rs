//! Git diff for interactive `notebook watch`.
//!
//! Off unless `watch --git-diff` or `[notebook] git_diff` is set. When off,
//! nothing here spawns git. When on, every invocation is a dashed plumbing
//! binary (`git-rev-parse`, `git-cat-file`, `git-ls-tree`, `git-for-each-ref`,
//! `git-hash-object`, `git-ls-files`) with config and environment overrides.
//! Name-status is computed in-process from blob ids. There is no shell and
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
const MAX_BLOCKS: usize = 400;
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchQuery {
    pub diff: Option<String>,
    pub base: Option<String>,
    pub cm: Option<bool>,
}

pub fn parse_query(q: &str) -> WatchQuery {
    let mut diff = None;
    let mut base = None;
    let mut cm = None;
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut it = pair.splitn(2, '=');
        let key = it.next().unwrap_or("");
        let val = percent_decode(it.next().unwrap_or(""));
        match key {
            "diff" => diff = Some(val),
            "base" => base = Some(val),
            "cm" => cm = Some(matches!(val.as_str(), "1" | "on" | "true")),
            _ => {}
        }
    }
    WatchQuery { diff, base, cm }
}

/// `?diff=off` forces off. A missing `diff` uses the CLI default.
pub fn resolve_diff(q: &WatchQuery, cli: Option<&str>) -> Option<String> {
    match q.diff.as_deref() {
        Some("off") => None,
        Some(s) => Some(s.to_string()),
        None => cli.map(str::to_string),
    }
}

/// Character check shared by `?diff=`, `?base=`, and the branch list.
/// `index` is the staged blob. A range (`..`) is rejected here.
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
/// notebook that sits outside the jail or the repository.
pub fn safe_rel(notebook: &Path, jail: Option<&Path>, repo: &Path) -> Result<String, ()> {
    let nb = std::fs::canonicalize(notebook).map_err(|_| ())?;
    if let Some(jail) = jail {
        let jail = std::fs::canonicalize(jail).map_err(|_| ())?;
        if !nb.starts_with(&jail) {
            return Err(());
        }
    }
    let repo = std::fs::canonicalize(repo).map_err(|_| ())?;
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

fn walk_files(root: &Path) -> (Vec<String>, bool) {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in rd.flatten() {
            let Ok(ft) = ent.file_type() else {
                continue;
            };
            if ft.is_symlink() {
                continue;
            }
            if ent.file_name() == *".git" {
                continue;
            }
            let path = ent.path();
            if ft.is_dir() {
                stack.push(path);
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let s = rel.to_string_lossy().replace('\\', "/");
            if s.contains('\n') || !rel_ok(&s) {
                continue;
            }
            if files.len() >= WALK_CAP {
                return (files, true);
            }
            files.push(s);
        }
    }
    (files, false)
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

fn worktree_blobs(root: &Path) -> Result<(BTreeMap<String, String>, bool), GitFail> {
    let (files, truncated) = walk_files(root);
    let oids = hash_files(root, &files)?;
    let mut map = BTreeMap::new();
    for (path, oid) in files.into_iter().zip(oids) {
        if oid_ok(&oid) {
            map.insert(path, oid);
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

fn worktree_changes(root: &Path, against: Option<&str>) -> Result<Vec<NameChange>, GitFail> {
    let old = match against {
        Some(oid) => commit_blobs(root, oid)?,
        None => index_blobs(root)?,
    };
    let (new, truncated) = worktree_blobs(root)?;
    Ok(diff_maps(&old, &new, !truncated))
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
    let cache_key = (spec_rev != "index").then(|| format!("{spec_rev}:{rel}"));
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

fn commit_time(root: &Path, rev: &str) -> Result<i64, GitFail> {
    let owned;
    let oid = if oid_ok(rev) {
        rev
    } else {
        owned = verify(root, rev)?;
        owned.as_str()
    };
    Ok(read_commit(root, oid)?.time)
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
        });
    }
    let heads: Vec<Listed> = rows.iter().filter(|r| !r.tag).cloned().collect();
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
        .map(|h| serde_json::json!({"name": h.name, "current": h.current}))
        .collect();
    let tag_json: Vec<_> = tags
        .iter()
        .map(|t| serde_json::json!({"name": t.name}))
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

/// Inputs for one page view. `diff` is the "vs" side. `base` set means
/// both sides are refs and neither notebook is executed.
pub struct Present<'a> {
    pub html: &'a str,
    pub source_path: &'a Path,
    pub jail: Option<&'a Path>,
    pub comments_on: bool,
    pub diff: &'a str,
    pub base: Option<&'a str>,
}

pub fn present(p: Present<'_>) -> String {
    let base = p.base.map(str::trim).filter(|s| !s.is_empty());
    if let Some(msg) = ref_problem(p.diff) {
        return decorate(p.html, p.diff, base, msg, None);
    }
    if let Some(b) = base.as_deref() {
        if let Some(msg) = ref_problem(b) {
            return decorate(p.html, p.diff, base, msg, None);
        }
    }
    let start = p.source_path.parent().unwrap_or(p.source_path);
    let root = match repo_root(start) {
        Ok(r) => r,
        Err(GitFail::Missing) => return decorate(p.html, p.diff, base, "git not found", None),
        Err(GitFail::Dubious) => return decorate(p.html, p.diff, base, OWNERSHIP_MSG, None),
        Err(_) => return decorate(p.html, p.diff, base, "not a git repository", None),
    };
    if safe_rel(p.source_path, p.jail, &root).is_err() {
        return decorate(
            p.html,
            p.diff,
            base,
            "path escapes notebook directory",
            None,
        );
    }
    let rel = safe_rel(p.source_path, p.jail, &root).expect("checked");

    let diff_oid = if p.diff == "index" {
        None
    } else {
        match verify(&root, p.diff) {
            Ok(oid) => Some(oid),
            Err(GitFail::Missing) => {
                return decorate(p.html, p.diff, base, "git not found", None);
            }
            Err(GitFail::Dubious) => {
                return decorate(p.html, p.diff, base, OWNERSHIP_MSG, None);
            }
            Err(_) => return decorate(p.html, p.diff, base, "unknown ref", None),
        }
    };
    let base_oid = if let Some(b) = base.as_deref() {
        if b == "index" {
            None
        } else {
            match verify(&root, b) {
                Ok(oid) => Some(oid),
                Err(GitFail::Missing) => {
                    return decorate(p.html, p.diff, base, "git not found", None);
                }
                Err(GitFail::Dubious) => {
                    return decorate(p.html, p.diff, base, OWNERSHIP_MSG, None);
                }
                Err(_) => return decorate(p.html, p.diff, base, "unknown ref", None),
            }
        }
    } else {
        None
    };

    if let Some(b) = base.as_deref() {
        return present_two_ref(p, &root, &rel, b, diff_oid.as_deref(), base_oid.as_deref());
    }
    present_working(p, &root, &rel, diff_oid.as_deref())
}

/// `enabled == false` returns the stored HTML and does not spawn git.
pub fn present_if(enabled: bool, p: Present<'_>) -> String {
    if !enabled {
        return p.html.to_string();
    }
    present(p)
}

fn present_working(p: Present<'_>, root: &Path, rel: &str, oid: Option<&str>) -> String {
    let against = if p.diff == "index" { None } else { oid };
    let changes = match worktree_changes(root, against) {
        Ok(c) => c,
        Err(GitFail::Dubious) => {
            return decorate(p.html, p.diff, None, OWNERSHIP_MSG, None);
        }
        Err(GitFail::Missing) => return decorate(p.html, p.diff, None, "git not found", None),
        Err(_) => Vec::new(),
    };
    let dots = file_dots_json(&changes);
    let disk = match std::fs::read(p.source_path) {
        Ok(b) => b,
        Err(_) => {
            return decorate(p.html, p.diff, None, "could not read notebook", Some(&dots));
        }
    };
    if disk.contains(&0) {
        return decorate(p.html, p.diff, None, "not a text notebook", Some(&dots));
    }
    let new_text = String::from_utf8_lossy(&disk).into_owned();
    if crate::embed::has_markdown_embeds(&new_text) {
        return decorate(
            p.html,
            p.diff,
            None,
            "embeds are not part of this diff",
            Some(&dots),
        );
    }
    let old_text = match load_side(root, p.diff, rel, &changes, oid) {
        Ok(Some(s)) if s.contains('\0') => {
            return decorate(p.html, p.diff, None, "not a text notebook", Some(&dots));
        }
        Ok(Some(s)) => s,
        Ok(None) => String::new(),
        Err(GitFail::Missing) => {
            return decorate(p.html, p.diff, None, "git not found", Some(&dots))
        }
        Err(GitFail::Dubious) => return decorate(p.html, p.diff, None, OWNERSHIP_MSG, Some(&dots)),
        Err(_) => String::new(),
    };
    match stamp_working(p.html, &old_text, &new_text, p.comments_on) {
        Ok((html, n)) => {
            let banner = if n == 0 {
                format!("no changes vs {}", p.diff)
            } else {
                format!("{n} changes vs {}", p.diff)
            };
            decorate(&html, p.diff, None, &banner, Some(&dots))
        }
        Err(msg) => decorate(p.html, p.diff, None, msg, Some(&dots)),
    }
}

fn present_two_ref(
    p: Present<'_>,
    root: &Path,
    rel: &str,
    base: &str,
    diff_oid: Option<&str>,
    base_oid: Option<&str>,
) -> String {
    // `git diff A B` lists what changes from A to B. The page shows the
    // newer commit. A tie, or either side `index`, keeps the `diff` param.
    let show_diff = match (p.diff == "index", base == "index") {
        (true, _) | (_, true) => true,
        _ => match (commit_time(root, p.diff), commit_time(root, base)) {
            (Err(GitFail::Dubious), _) | (_, Err(GitFail::Dubious)) => {
                return decorate(p.html, p.diff, Some(base), OWNERSHIP_MSG, None);
            }
            (Err(GitFail::Missing), _) | (_, Err(GitFail::Missing)) => {
                return decorate(p.html, p.diff, Some(base), "git not found", None);
            }
            (Ok(d), Ok(b)) if b > d => false,
            _ => true,
        },
    };
    let (new_ref, new_oid, old_ref, old_oid) = if show_diff {
        (p.diff, diff_oid, base, base_oid)
    } else {
        (base, base_oid, p.diff, diff_oid)
    };
    let changes = match (
        ref_blobs(root, old_ref, old_oid),
        ref_blobs(root, new_ref, new_oid),
    ) {
        (Ok(old), Ok(new)) => diff_maps(&old, &new, true),
        (Err(GitFail::Dubious), _) | (_, Err(GitFail::Dubious)) => {
            return decorate(p.html, p.diff, Some(base), OWNERSHIP_MSG, None);
        }
        (Err(GitFail::Missing), _) | (_, Err(GitFail::Missing)) => {
            return decorate(p.html, p.diff, Some(base), "git not found", None);
        }
        _ => Vec::new(),
    };
    let dots = file_dots_json(&changes);
    let newer = match (p.diff == "index" || base == "index", show_diff) {
        (true, _) => None,
        (false, true) => match (commit_time(root, p.diff), commit_time(root, base)) {
            (Ok(d), Ok(b)) if d > b => Some(()),
            (Err(GitFail::Dubious), _) | (_, Err(GitFail::Dubious)) => {
                return decorate(p.html, p.diff, Some(base), OWNERSHIP_MSG, Some(&dots));
            }
            _ => None,
        },
        (false, false) => Some(()),
    };
    let newer_word = if newer.is_some() { " (newer)" } else { "" };
    let fail = |msg: &str| decorate(p.html, p.diff, Some(base), msg, Some(&dots));

    let new_text = match load_side(root, new_ref, rel, &changes, new_oid) {
        Ok(Some(s)) => s,
        Ok(None) => String::new(),
        Err(GitFail::Missing) => return fail("git not found"),
        Err(GitFail::Dubious) => return fail(OWNERSHIP_MSG),
        Err(_) => String::new(),
    };
    let old_text = match load_side(root, old_ref, rel, &changes, old_oid) {
        Ok(Some(s)) => s,
        Ok(None) => String::new(),
        Err(GitFail::Missing) => return fail("git not found"),
        Err(GitFail::Dubious) => return fail(OWNERSHIP_MSG),
        Err(_) => String::new(),
    };
    if new_text.contains('\0') || old_text.contains('\0') {
        return fail("not a text notebook");
    }
    let banner = format!("showing {new_ref}{newer_word} vs {old_ref} — code was not run");
    if crate::embed::has_markdown_embeds(&new_text) || crate::embed::has_markdown_embeds(&old_text)
    {
        let page = replace_main(p.html, &render_plain(&new_text, p.comments_on));
        return decorate(
            &page,
            p.diff,
            Some(base),
            "embeds are not part of this diff — code was not run",
            Some(&dots),
        );
    }
    match render_aligned(&old_text, &new_text, p.comments_on, true) {
        Ok((inner, _)) => {
            let page = replace_main(p.html, &inner);
            decorate(&page, p.diff, Some(base), &banner, Some(&dots))
        }
        Err(msg) => {
            let page = replace_main(p.html, &render_plain(&new_text, p.comments_on));
            let banner = format!("{msg} — code was not run");
            decorate(&page, p.diff, Some(base), &banner, Some(&dots))
        }
    }
}

fn stamp_working(
    html: &str,
    old: &str,
    new_src: &str,
    comments_on: bool,
) -> Result<(String, usize), &'static str> {
    let (marks, removed, units) = align(old, new_src, comments_on)?;
    let sections = split_blocks(html);
    if sections.len() != units.len() {
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
    let mut out = String::with_capacity(html.len() + 1024);
    cursor = 0;
    for (idx, (start, end)) in ranges.iter().enumerate() {
        out.push_str(&html[cursor..*start]);
        for (at, unit) in &removed {
            if *at == idx {
                out.push_str(&removed_section(unit));
            }
        }
        out.push_str(&stamp_section(
            &sections[idx].html,
            &marks[idx],
            &units[idx],
            false,
        ));
        cursor = *end;
    }
    for (at, unit) in &removed {
        if *at == units.len() {
            out.push_str(&removed_section(unit));
        }
    }
    out.push_str(&html[cursor..]);
    Ok((out, n))
}

fn render_aligned(
    old: &str,
    new_src: &str,
    comments_on: bool,
    two_ref: bool,
) -> Result<(String, usize), &'static str> {
    let (marks, removed, units) = align(old, new_src, comments_on)?;
    let n = count_changes(&marks, &removed);
    let mut body = String::new();
    if two_ref {
        body.push_str("<p class=\"rl-diff-note\">code was not run</p>\n");
    }
    for (idx, unit) in units.iter().enumerate() {
        for (at, gone) in &removed {
            if *at == idx {
                body.push_str(&removed_section(gone));
            }
        }
        body.push_str(&fresh_section(unit, &marks[idx], idx, two_ref));
    }
    for (at, gone) in &removed {
        if *at == units.len() {
            body.push_str(&removed_section(gone));
        }
    }
    Ok((body, n))
}

fn render_plain(src: &str, comments_on: bool) -> String {
    let prepared = prepare(src, comments_on);
    let units = units_of(&prepared);
    let mut body = String::from("<p class=\"rl-diff-note\">code was not run</p>\n");
    for (idx, unit) in units.iter().enumerate() {
        body.push_str(&fresh_section(unit, &Mark::Same, idx, true));
    }
    body
}

fn count_changes(marks: &[Mark], removed: &[(usize, Unit)]) -> usize {
    marks.iter().filter(|m| !matches!(m, Mark::Same)).count() + removed.len()
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
    fp: String,
    text: String,
}

#[derive(Debug)]
enum Mark {
    Same,
    Added,
    Changed { old_text: String },
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
        return Err("diff was too large");
    }
    let old_u = units_of(&prepare(old, comments_on));
    let new_u = units_of(&prepare(new_src, comments_on));
    if old_u.len() > MAX_BLOCKS || new_u.len() > MAX_BLOCKS {
        return Err("diff was too large");
    }
    let ops = diff_ops(old_u.len(), new_u.len(), |i, j| old_u[i].fp == new_u[j].fp);
    let (marks, removed) = coalesce(&old_u, &new_u, &ops);
    Ok((marks, removed, new_u))
}

/// Document-order labels: `same`, `added`, `changed`, `removed`.
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
            removed.push((anchor, clone_unit(&old[oi])));
        }
    }
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
        if di < dels.len() && ii < ins.len() && old[dels[di]].kind == new_u[ins[ii]].kind {
            marks[ins[ii]] = Mark::Changed {
                old_text: old[dels[di]].text.clone(),
            };
            di += 1;
            ii += 1;
        } else if di < dels.len() {
            removed.push((anchor, clone_unit(&old[dels[di]])));
            di += 1;
        } else {
            marks[ins[ii]] = Mark::Added;
            ii += 1;
        }
    }
}

fn clone_unit(u: &Unit) -> Unit {
    Unit {
        kind: u.kind,
        fp: u.fp.clone(),
        text: u.text.clone(),
    }
}

fn units_of(src: &str) -> Vec<Unit> {
    let blocks = crate::parse::parse_notebook(src);
    let mut out = Vec::new();
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
                    let text = source.clone();
                    out.push(Unit {
                        kind: Kind::Code,
                        fp: format!("panel\0{}", parts.join("\0")),
                        text,
                    });
                    i += 1;
                }
            }
            other => {
                if let Some(unit) = unit_of(other) {
                    out.push(unit);
                }
                i += 1;
            }
        }
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

fn unit_of(block: &Block) -> Option<Unit> {
    match block {
        Block::Markdown(s) => Some(Unit {
            kind: Kind::Prose,
            fp: format!("md\0{s}"),
            text: s.clone(),
        }),
        Block::Code { source, directives } => Some(Unit {
            kind: Kind::Code,
            fp: code_fp(source, directives),
            text: source.clone(),
        }),
        Block::Mermaid { source, directives } => Some(Unit {
            kind: Kind::Mermaid,
            fp: mermaid_fp(source, directives),
            text: source.clone(),
        }),
        Block::Widget { source, .. } => Some(Unit {
            kind: Kind::Widget,
            fp: format!("w\0{source}"),
            text: source.clone(),
        }),
        Block::Callout {
            kind,
            title,
            content,
        } => Some(Unit {
            kind: Kind::Callout,
            fp: format!(
                "c\0{}\0{}\0{content}",
                callout_tag(*kind),
                title.as_deref().unwrap_or("")
            ),
            text: content.clone(),
        }),
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

fn stamp_section(section: &str, mark: &Mark, unit: &Unit, _two: bool) -> String {
    match mark {
        Mark::Same => section.to_string(),
        Mark::Added => {
            let s = with_class(section, "rl-diff rl-diff-add");
            insert_after_open(&s, "<span class=\"rl-diff-tag\">added</span>")
        }
        Mark::Changed { old_text } => {
            let mut s = with_class(section, "rl-diff rl-diff-chg");
            s = insert_after_open(&s, "<span class=\"rl-diff-tag\">changed</span>");
            if matches!(unit.kind, Kind::Code | Kind::Mermaid) {
                let lines = line_diff_html(old_text, &unit.text);
                let note = "<p class=\"rl-diff-note\">output is the working copy</p>";
                s = inject_after_source(&s, &lines, note);
            }
            s
        }
    }
}

fn fresh_section(unit: &Unit, mark: &Mark, idx: usize, two_ref: bool) -> String {
    let (class, tag, old) = match mark {
        Mark::Same => ("", "", None),
        Mark::Added => (" rl-diff rl-diff-add", "added", None),
        Mark::Changed { old_text } => (" rl-diff rl-diff-chg", "changed", Some(old_text.as_str())),
    };
    let tag_html = if tag.is_empty() {
        String::new()
    } else {
        format!("<span class=\"rl-diff-tag\">{tag}</span>")
    };
    let note = if two_ref && matches!(unit.kind, Kind::Code | Kind::Mermaid) {
        "<p class=\"rl-diff-note\">code was not run</p>\n"
    } else if !two_ref && old.is_some() && matches!(unit.kind, Kind::Code | Kind::Mermaid) {
        "<p class=\"rl-diff-note\">output is the working copy</p>\n"
    } else {
        ""
    };
    let body = match unit.kind {
        Kind::Code | Kind::Mermaid => {
            let lines = old
                .map(|o| line_diff_html(o, &unit.text))
                .unwrap_or_default();
            let lang = if unit.kind == Kind::Code {
                "rustlab"
            } else {
                "mermaid"
            };
            format!(
                "<div class=\"code-block\"><div class=\"rl-cell\">\
<details class=\"rl-src\" open><summary class=\"rl-lang\">{lang}</summary>\
<pre class=\"source\"><code>{}</code></pre>{lines}</details>\
{note}</div></div>\n",
                escape_html(&unit.text)
            )
        }
        Kind::Prose | Kind::Callout => {
            format!(
                "<div class=\"prose\">\n{}\n</div>\n{note}",
                markdown_to_html(&unit.text)
            )
        }
        Kind::Widget => format!(
            "<pre class=\"source\"><code>{}</code></pre>\n{note}",
            escape_html(&unit.text)
        ),
    };
    format!("<section class=\"rl-block{class}\" id=\"b-git-{idx}\">\n{tag_html}{body}</section>\n")
}

fn removed_section(unit: &Unit) -> String {
    let body = match unit.kind {
        Kind::Code | Kind::Mermaid | Kind::Widget => {
            format!(
                "<pre class=\"source\"><code>{}</code></pre>\n<p class=\"rl-diff-note\">removed</p>\n",
                escape_html(&unit.text)
            )
        }
        Kind::Prose | Kind::Callout => {
            format!(
                "<div class=\"prose\">\n{}\n</div>\n<p class=\"rl-diff-note\">removed</p>\n",
                markdown_to_html(&unit.text)
            )
        }
    };
    format!(
        "<section class=\"rl-block rl-diff rl-diff-del\" id=\"b-gitdel-{}\">\n{body}</section>\n",
        blake_id(&unit.fp)
    )
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

fn with_class(section: &str, class: &str) -> String {
    section.replacen(
        "<section class=\"rl-block\"",
        &format!("<section class=\"rl-block {class}\""),
        1,
    )
}

fn insert_after_open(section: &str, extra: &str) -> String {
    match section.find('>') {
        Some(i) => {
            let mut s = String::with_capacity(section.len() + extra.len());
            s.push_str(&section[..=i]);
            s.push_str(extra);
            s.push_str(&section[i + 1..]);
            s
        }
        None => section.to_string(),
    }
}

fn inject_after_source(section: &str, lines: &str, note: &str) -> String {
    let mut s = section.to_string();
    if let Some(src) = s.find("class=\"source\"") {
        if let Some(rel) = s[src..].find("</pre>") {
            let at = src + rel + "</pre>".len();
            s.insert_str(at, lines);
        }
    }
    if let Some(end) = s.rfind("</section>") {
        s.insert_str(end, note);
    }
    s
}

fn line_diff_html(old: &str, new_src: &str) -> String {
    let a: Vec<&str> = old.split('\n').collect();
    let b: Vec<&str> = new_src.split('\n').collect();
    if a.len() > MAX_LINES || b.len() > MAX_LINES {
        return "<p class=\"rl-diff-note\">line diff omitted</p>".into();
    }
    let ops = diff_ops(a.len(), b.len(), |i, j| a[i] == b[j]);
    let mut html = String::from("<div class=\"rl-diff-lines\">");
    let mut i = 0usize;
    let mut j = 0usize;
    for op in &ops {
        match op {
            Op::Eq(_) => {
                html.push_str(&line_row("rl-ln-ctx", a.get(i).copied().unwrap_or("")));
                i += 1;
                j += 1;
            }
            Op::Del(_) => {
                html.push_str(&line_row("rl-ln-del", a.get(i).copied().unwrap_or("")));
                i += 1;
            }
            Op::Ins(_) => {
                html.push_str(&line_row("rl-ln-add", b.get(j).copied().unwrap_or("")));
                j += 1;
            }
        }
    }
    html.push_str("</div>");
    html
}

fn line_row(class: &str, text: &str) -> String {
    let mark = if class.ends_with("add") {
        "+"
    } else if class.ends_with("del") {
        "-"
    } else {
        ""
    };
    format!(
        "<div class=\"rl-ln {class}\"><span class=\"rl-ln-gutter\" aria-hidden=\"true\">{mark}</span><code>{}</code></div>",
        escape_html(text)
    )
}

fn replace_main(html: &str, inner: &str) -> String {
    let Some(start) = html.find("<main") else {
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

fn decorate(
    html: &str,
    diff: &str,
    base: Option<&str>,
    banner: &str,
    files: Option<&str>,
) -> String {
    let base = base.unwrap_or("");
    let mut out = patch_body(html, diff, base);
    out = out.replace("id=\"rl-diff\" hidden", "id=\"rl-diff\"");
    out = out.replace(
        "id=\"rl-diff-toggle\" type=\"button\" class=\"rl-quiet\"",
        "id=\"rl-diff-toggle\" type=\"button\" class=\"rl-quiet active\"",
    );
    out = out.replace(
        "id=\"rl-diff-toggle\" type=\"button\" class=\"rl-quiet active\" role=\"switch\" aria-checked=\"false\"",
        "id=\"rl-diff-toggle\" type=\"button\" class=\"rl-quiet active\" role=\"switch\" aria-checked=\"true\"",
    );
    out = out.replace("id=\"rl-diff-dot\" hidden", "id=\"rl-diff-dot\"");
    out = out.replace(
        "aria-label=\"More\" title=\"More\"",
        "aria-label=\"More, diff on\" title=\"More, diff on\"",
    );
    let banner_html = format!("<p id=\"rl-diff-banner\">{}</p>", escape_html(banner));
    if out.contains("<p id=\"rl-diff-banner\" hidden></p>") {
        out = out.replace("<p id=\"rl-diff-banner\" hidden></p>", &banner_html);
    } else if out.contains("<p id=\"rl-diff-banner\" hidden>") {
        // already filled; leave the chrome copy and also put one in main
    }
    let mut extra = String::new();
    if !out.contains("id=\"rl-diff-banner\"")
        || out.contains("<p id=\"rl-diff-banner\" hidden></p>")
    {
        extra.push_str(&banner_html);
    }
    if let Some(files) = files {
        extra.push_str(&format!("<div id=\"rl-git-files\" hidden>{}</div>", files));
    }
    if !extra.is_empty() {
        out = insert_after_main(&out, &extra);
    }
    // The chrome placeholder was replaced above. If it was missing, the
    // banner inserted into main is the one the page shows.
    if out.contains("<p id=\"rl-diff-banner\" hidden></p>") {
        out = out.replace("<p id=\"rl-diff-banner\" hidden></p>", &banner_html);
    }
    out
}

fn insert_after_main(html: &str, extra: &str) -> String {
    let Some(i) = html.find("<main") else {
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

fn patch_body(html: &str, diff: &str, base: &str) -> String {
    let Some(i) = html.find("<body") else {
        return html.to_string();
    };
    let Some(rel) = html[i..].find('>') else {
        return html.to_string();
    };
    let tag = &html[i..i + rel];
    let attrs = format!(
        " data-rl-diff=\"{}\" data-rl-base=\"{}\"",
        escape_html(diff),
        escape_html(base)
    );
    let new_tag = if let Some(c) = tag.find("class=\"") {
        let at = c + "class=\"".len();
        format!("{}rl-diff-on {}{}", &tag[..at], &tag[at..], attrs)
    } else {
        format!("{tag} class=\"rl-diff-on\"{attrs}")
    };
    let mut out = String::with_capacity(html.len() + 64);
    out.push_str(&html[..i]);
    out.push_str(&new_tag);
    out.push_str(&html[i + rel..]);
    out
}

fn percent_decode(s: &str) -> String {
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
        let html = "<body><main>\n<section class=\"rl-block\" id=\"b-1\"><div class=\"prose\"><p>A changed</p></div></section>\n</main></body>";
        let (out, n) = stamp_working(html, old, new_src, true).unwrap();
        assert!(n >= 1, "{out}");
        assert!(
            out.contains("rl-diff-chg") || out.contains("rl-diff-del"),
            "{out}"
        );
        assert!(
            out.contains("plot(&quot;old&quot;)") || out.contains("plot(\"old\")"),
            "{out}"
        );
        let del = out.split("rl-diff-del").nth(1).unwrap_or("");
        assert!(!del.contains("plot-container"), "{out}");
    }

    #[test]
    fn changed_code_keeps_both_lines_and_labels_working_copy() {
        let old = "```rustlab\nold line\n```\n";
        let new_src = "```rustlab\nnew line\n```\n";
        let html = "<body><main><section class=\"rl-block\" id=\"b-1\"><div class=\"code-block\"><pre class=\"source\"><code>new line</code></pre><pre class=\"output\">ran</pre></section></main></body>";
        let (out, _) = stamp_working(html, old, new_src, true).unwrap();
        assert!(out.contains("rl-diff-chg"), "{out}");
        assert!(out.contains("rl-ln-del"), "{out}");
        assert!(out.contains("rl-ln-add"), "{out}");
        assert!(out.contains(">-<"), "{out}");
        assert!(out.contains(">+<"), "{out}");
        assert!(out.contains("old line"), "{out}");
        assert!(out.contains("new line"), "{out}");
        assert!(out.contains("output is the working copy"), "{out}");
        assert!(out.contains(">ran<") || out.contains("ran"), "{out}");
    }

    #[test]
    fn size_cap_adds_no_diff_class() {
        let big = "x".repeat(MAX_BYTES + 1);
        let err = align(big.as_str(), "y", true).unwrap_err();
        assert_eq!(err, "diff was too large");
        let html =
            "<body><main><section class=\"rl-block\" id=\"b-1\"><p>y</p></section></main></body>";
        let out = decorate(html, "HEAD", None, "diff was too large", None);
        assert!(!out.contains("rl-diff-add"), "{out}");
        assert!(out.contains("diff was too large"), "{out}");
    }

    #[test]
    fn two_ref_render_says_not_run_and_drops_live_output() {
        let old = "# Old\n\n```rustlab\nx = 1\n```\n";
        let new_src = "# New\n\n```rustlab\nx = 2\n```\n";
        let html = "<body><main><section class=\"rl-block\" id=\"b-1\"><pre class=\"output\">EXECUTED</pre><div class=\"plot-container\"></div></section></main></body>";
        let (inner, _) = render_aligned(old, new_src, true, true).unwrap();
        let page = replace_main(html, &inner);
        assert!(page.contains("not run"), "{page}");
        assert!(!page.contains("EXECUTED"), "{page}");
        assert!(!page.contains("plot-container"), "{page}");
        assert!(page.contains("x = 2"), "{page}");
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
            diff: "main",
            base: Some("topic"),
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
            diff: "main",
            base: None,
        });
        assert!(live.contains("EXECUTED"), "{live}");
        assert!(
            live.contains("rl-diff-chg") || live.contains("changes vs main"),
            "{live}"
        );
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
                diff: "HEAD",
                base: None,
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
            diff: "HEAD",
            base: None,
        });
        let refs = refs_json(dir);
        assert!(
            !marker.exists(),
            "hardened git ran a repo hook or alias: {refs}"
        );
        assert!(!refs.contains("git diff is off"), "{refs}");
    }
}
