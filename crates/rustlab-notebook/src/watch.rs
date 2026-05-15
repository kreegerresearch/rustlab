//! Filesystem watcher: re-render notebooks the moment they're saved.
//!
//! Long-running counterpart to [`crate::cmd_render_dir`]. The user runs
//! it once in a terminal and any edit to a `.md` source under the
//! watched directory triggers a debounced re-render of just the
//! affected notebooks. Pairs naturally with `--obsidian` so an
//! Obsidian Reading view stays current as you author in Editing view.
//!
//! ## Behaviour
//!
//! - Filesystem events from [`notify`] are debounced at the configured
//!   window (default 250 ms) so the burst of events most editors emit
//!   per save collapses to a single render pass.
//! - On startup the watcher renders every notebook once so the output
//!   directory is in sync. After that, only notebooks the watcher
//!   knows depend on the changed source are re-rendered.
//! - The dependency graph is built incrementally from the file-embeds
//!   expander's source cache: after rendering each notebook the
//!   watcher records which files it loaded. Edit `_setup.md` and only
//!   the lessons that embed it re-render.
//! - Stale plot SVGs in the per-notebook plot directory are deleted
//!   after each render so `_attachments/` doesn't accumulate dead
//!   files when blocks are removed from a notebook.
//! - Parse / execution errors in one notebook log to stderr and
//!   render inline (existing renderer behaviour); the watcher keeps
//!   running.

use crate::{cmd_render, paths_equal, Format};
use notify::{event::EventKind, Event, RecommendedWatcher, RecursiveMode, Watcher};
use rustlab_plot::theme::ThemeColors;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Default debounce window for filesystem events.
pub const DEFAULT_DEBOUNCE_MS: u64 = 250;

/// Watch a directory of notebooks and re-render on change. Blocks
/// indefinitely; returns only on Ctrl-C or unrecoverable error.
///
/// `out_dir` defaults to `dir` when `None` — same convention as
/// [`crate::cmd_render_dir`]. The format must currently be
/// [`Format::Markdown`] (HTML/PDF/LaTeX in watch mode aren't yet
/// supported and would produce surprising stale-iframe behaviour).
pub fn cmd_watch(
    dir: PathBuf,
    output: Option<PathBuf>,
    format: Format,
    theme: &'static ThemeColors,
    debounce_ms: u64,
) {
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    let out_dir = output
        .map(|o| std::path::absolute(&o).unwrap_or(o))
        .unwrap_or_else(|| dir.clone());

    if !matches!(format, Format::Markdown { .. }) {
        eprintln!(
            "error: notebook watch supports --format markdown only (HTML/PDF/LaTeX coming later)"
        );
        std::process::exit(1);
    }

    println!(
        "[watch] watching {} → {} (debounce: {} ms)",
        dir.display(),
        out_dir.display(),
        debounce_ms
    );

    // Two-dir mode: auto-clean the source tree before the initial render.
    // The source dir is supposed to hold authoring-only content; if any
    // file has rustlab-generated artifacts (from a prior in-place run, a
    // mis-placed copy, etc.) strip them now so the source stays pristine
    // and re-renders are deterministic. Single-dir mode is in-place and
    // the artifacts there are load-bearing — don't touch.
    if !paths_equal(&dir, &out_dir) {
        let cleaned = crate::cmd_clean(dir.clone(), None, false);
        if cleaned > 0 {
            println!("[watch] auto-cleaned {cleaned} source file(s) before initial render");
        }
    }

    // Seed the dependency graph by rendering every notebook once.
    let mut graph = DependencyGraph::default();
    // Track the bytes of every file we just wrote, by canonicalised
    // output path. When an fs event arrives for one of these paths and
    // the file's current bytes still match what we wrote, the event is
    // our own write echoing back through the notify channel — drop it.
    // This is what kills runaway loops on *non-deterministic* notebooks
    // (`randn`, time, network) where the rendered bytes legitimately
    // differ each pass, so `write_output`'s hash-equal skip can't trip.
    let mut self_writes: HashMap<PathBuf, Vec<u8>> = HashMap::new();
    let initial = list_notebooks(&dir);
    for src in &initial {
        if let Some(out_path) =
            render_one_with_tracking(src, &dir, &out_dir, &format, theme, &mut graph)
        {
            if let (Ok(bytes), Some(canon)) =
                (std::fs::read(&out_path), canonicalize_lossy(&out_path))
            {
                self_writes.insert(canon, bytes);
            }
        }
    }
    println!("[watch] initial render complete ({} notebooks)", initial.len());

    // Set up the fs watcher.
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher: RecommendedWatcher = match notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("error: cannot create fs watcher: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = watcher.watch(&dir, RecursiveMode::Recursive) {
        eprintln!("error: cannot watch {}: {e}", dir.display());
        std::process::exit(1);
    }

    let debounce = Duration::from_millis(debounce_ms);
    let mut pending: HashSet<PathBuf> = HashSet::new();
    let mut last_event: Option<Instant> = None;

    loop {
        // Block for the next event, or timeout if we have pending work.
        let recv_timeout = if pending.is_empty() {
            Duration::from_secs(3600)
        } else {
            debounce
        };
        match rx.recv_timeout(recv_timeout) {
            Ok(Ok(event)) => {
                if !is_relevant(&event) {
                    continue;
                }
                for path in &event.paths {
                    if let Some(canon) = canonicalize_lossy(path) {
                        // Only react to .md files inside the watched tree,
                        // and skip editor tempfiles (sed: `.!PID!name.md`,
                        // vim: `.name.md.swp`, emacs: `.#name.md`, plus
                        // any `name~` backup). Without this filter the
                        // watcher tries to render transient files that
                        // were renamed away before the event delivered.
                        if !canon.starts_with(&dir)
                            || !canon.extension().map_or(false, |e| e == "md")
                            || is_editor_tempfile(&canon)
                        {
                            continue;
                        }
                        // Self-write suppression: if the file's current
                        // bytes match what we just wrote, this event is
                        // our own write echoing back. Drop it. A genuine
                        // user edit changes the bytes and falls through.
                        if is_self_write_echo(&canon, &self_writes) {
                            continue;
                        }
                        pending.insert(canon);
                    }
                }
                last_event = Some(Instant::now());
            }
            Ok(Err(e)) => {
                eprintln!("[watch] fs error: {e}");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Fall through to debounce check below.
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("[watch] watcher channel closed; exiting");
                return;
            }
        }

        // If the debounce window has elapsed since the last event,
        // flush pending work.
        if let Some(when) = last_event {
            if when.elapsed() >= debounce && !pending.is_empty() {
                let to_render = compute_render_set(&pending, &dir, &graph);
                pending.clear();
                last_event = None;
                if to_render.is_empty() {
                    continue;
                }
                let n = to_render.len();
                let started = Instant::now();
                for src in &to_render {
                    if let Some(out_path) = render_one_with_tracking(
                        src, &dir, &out_dir, &format, theme, &mut graph,
                    ) {
                        if let (Ok(bytes), Some(canon)) =
                            (std::fs::read(&out_path), canonicalize_lossy(&out_path))
                        {
                            self_writes.insert(canon, bytes);
                        }
                    }
                }
                println!(
                    "[watch] re-rendered {} notebook{} in {} ms",
                    n,
                    if n == 1 { "" } else { "s" },
                    started.elapsed().as_millis()
                );
            }
        }
    }
}

/// Tracks which embedded sources each notebook loaded last time it
/// rendered. When a source file changes, every notebook that depends
/// on it (transitively) needs a re-render.
#[derive(Default, Debug)]
pub(crate) struct DependencyGraph {
    /// Map: notebook source → set of files it embeds (canonical paths).
    deps: HashMap<PathBuf, HashSet<PathBuf>>,
}

impl DependencyGraph {
    /// Notebook sources whose render depends on `changed`. Always
    /// includes `changed` itself if it is a notebook in the graph.
    pub(crate) fn dependents_of(&self, changed: &Path) -> HashSet<PathBuf> {
        let mut out = HashSet::new();
        if self.deps.contains_key(changed) {
            out.insert(changed.to_path_buf());
        }
        for (notebook, embedded) in &self.deps {
            if embedded.contains(changed) {
                out.insert(notebook.clone());
            }
        }
        out
    }

    pub(crate) fn record(&mut self, notebook: PathBuf, embedded: HashSet<PathBuf>) {
        self.deps.insert(notebook, embedded);
    }
}

/// Render one notebook and update the dependency graph. Embedded-source
/// tracking is approximate: we re-walk the source for `![[name]]`
/// references and try to resolve each. False positives (e.g. an
/// unresolved ref) become orphan entries that simply never trigger a
/// re-render.
fn render_one_with_tracking(
    src: &Path,
    root_dir: &Path,
    out_dir: &Path,
    format: &Format,
    theme: &'static ThemeColors,
    graph: &mut DependencyGraph,
) -> Option<PathBuf> {
    // Race protection: the file may have vanished between the fs event
    // and now (atomic rename, deletion, etc). `cmd_render` calls
    // `process::exit(1)` on read failure, which would kill the long-
    // running watcher. Just skip — if the file reappears we'll see a
    // fresh event.
    if !src.exists() {
        return None;
    }

    // Compute the output file path mirroring `cmd_render_dir`.
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".to_string());
    let out_path = out_dir.join(format!("{stem}.{}", format.extension()));

    // GC stale plot files: clear the per-notebook plot subdir before
    // rendering so removed code blocks don't leave orphan SVGs behind.
    // Safe — the renderer recreates the dir on the way in.
    if let Some(plot_dir) = crate::plot_dir_for_format(&out_path, format) {
        let _ = std::fs::remove_dir_all(&plot_dir);
    }

    // Skip index.md — handled separately by cmd_render_dir, but in
    // watch mode the simplest behaviour is to render it as a notebook
    // too. Acceptable trade-off for the watch loop's simplicity.
    cmd_render(
        src.to_path_buf(),
        Some(out_path.clone()),
        format.clone(),
        theme,
    );

    // Re-scan source for embeds and record dependencies.
    let embedded = match std::fs::read_to_string(src) {
        Ok(s) => collect_embed_targets(&s, src, root_dir),
        Err(_) => HashSet::new(),
    };
    graph.record(src.to_path_buf(), embedded);

    Some(out_path)
}

/// Walk every `![[target]]` reference reachable from `source` and
/// return the **transitive** set of canonical file paths that
/// contribute to the rendered output. Mirrors what
/// [`crate::embed::expand_embeds`] inlines, so a change to any node in
/// the chain correctly invalidates every notebook that ultimately
/// embeds it.
///
/// Recursion is restricted to markdown targets — non-markdown
/// references (`![[diagram.svg]]`) are passed through by the renderer
/// as wikilinks and are not file-content dependencies of the rendered
/// markdown. Cycles and missing targets are tolerated: the visited
/// set bounds the walk, and unresolved refs are skipped.
fn collect_embed_targets(source: &str, src: &Path, root_dir: &Path) -> HashSet<PathBuf> {
    let mut found: HashSet<PathBuf> = HashSet::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    // Each entry is (source text, host directory for ref resolution).
    let mut stack: Vec<(String, PathBuf)> =
        vec![(source.to_string(), src.parent().unwrap_or(root_dir).to_path_buf())];

    while let Some((text, host_dir)) = stack.pop() {
        for line in text.lines() {
            for (_, _, eref) in crate::embed::find_embed_refs_in_line(line) {
                if !crate::embed::is_markdown_target(&eref.target) {
                    continue;
                }
                let Ok(p) = crate::embed::resolve_target(&eref.target, &host_dir, root_dir)
                else {
                    continue;
                };
                let Some(canon) = canonicalize_lossy(&p) else {
                    continue;
                };
                if !visited.insert(canon.clone()) {
                    continue;
                }
                found.insert(canon.clone());
                if let Ok(child_src) = std::fs::read_to_string(&canon) {
                    let child_host = canon.parent().unwrap_or(root_dir).to_path_buf();
                    stack.push((child_src, child_host));
                }
            }
        }
    }
    found
}

fn list_notebooks(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map_or(false, |ext| ext == "md"))
            .filter(|p| p.file_name().map_or(true, |n| n != "README.md"))
            .collect(),
        Err(_) => Vec::new(),
    };
    out.sort();
    out
}

fn canonicalize_lossy(p: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(p).ok().or_else(|| Some(p.to_path_buf()))
}

/// Return true when the file at `path` matches the bytes we last wrote
/// to it, i.e. an fs event for it is our own write echoing back through
/// notify. Used by the watch loop to break runaway loops when a
/// notebook's rendered output is genuinely non-deterministic (`randn`,
/// timestamps, etc.) — `write_output`'s hash-equal skip can't trip in
/// that case because the bytes differ each pass, so we suppress at the
/// event-receive end instead.
fn is_self_write_echo(path: &Path, self_writes: &HashMap<PathBuf, Vec<u8>>) -> bool {
    let Some(prev) = self_writes.get(path) else {
        return false;
    };
    match std::fs::read(path) {
        Ok(current) => current.as_slice() == prev.as_slice(),
        Err(_) => false,
    }
}

/// Editor tempfile heuristic. Catches the common atomic-write patterns:
/// sed -i (`.!PID!name.md`), vim (`.name.md.swp`/`.swo`), emacs
/// (`.#name.md`), plus generic `name~` backups. Anything matching is
/// skipped by the watcher so a `cmd_render` race on the rename target
/// can't kill the watcher process.
fn is_editor_tempfile(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.starts_with('.') || name.ends_with('~')
}

/// Filter notify events. Accept everything except pure access events;
/// the per-platform backends emit different `Modify(...)` shapes
/// (FSEvents tends toward `Any`, inotify toward `Data(_)`), so we
/// trust the path filter downstream rather than over-narrowing here.
fn is_relevant(event: &Event) -> bool {
    !matches!(event.kind, EventKind::Access(_))
}

/// Given a set of changed files (absolute paths) and the current
/// dependency graph, return the set of notebook source paths that
/// must be re-rendered. A change to a notebook source pulls in only
/// itself; a change to a file embedded by N notebooks pulls in all N.
///
/// Newly-created `.md` files inside the watched tree are always
/// included even if the dependency graph hasn't seen them yet — the
/// renderer will record them on the way through. Without this, the
/// first save of a new notebook would be silently dropped because
/// `dependents_of()` only knows about files rendered before.
pub(crate) fn compute_render_set(
    changed: &HashSet<PathBuf>,
    root_dir: &Path,
    graph: &DependencyGraph,
) -> Vec<PathBuf> {
    let mut to_render: HashSet<PathBuf> = HashSet::new();
    for c in changed {
        let is_md_in_root = c.starts_with(root_dir)
            && c.extension().map_or(false, |e| e == "md");
        let is_tracked_embed = graph.deps.values().any(|d| d.contains(c));
        if !is_md_in_root && !is_tracked_embed {
            continue;
        }
        if is_md_in_root && c.file_name().map_or(true, |n| n != "README.md") {
            // Render any `.md` under the watched tree, whether or not the
            // graph already tracks it. Matches `list_notebooks` filtering
            // at startup (README.md is excluded the same way).
            to_render.insert(c.clone());
        }
        for dep in graph.dependents_of(c) {
            to_render.insert(dep);
        }
    }
    let mut out: Vec<PathBuf> = to_render.into_iter().collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn dependency_graph_dependents_of_self() {
        let mut g = DependencyGraph::default();
        let n = PathBuf::from("/n/lesson.md");
        g.record(n.clone(), HashSet::new());
        let d = g.dependents_of(&n);
        assert!(d.contains(&n));
    }

    #[test]
    fn dependency_graph_invalidates_dependents() {
        let mut g = DependencyGraph::default();
        let setup = PathBuf::from("/n/_setup.md");
        let lessons: Vec<PathBuf> = (1..=3)
            .map(|i| PathBuf::from(format!("/n/lesson_{i}.md")))
            .collect();
        for l in &lessons {
            let mut deps = HashSet::new();
            deps.insert(setup.clone());
            g.record(l.clone(), deps);
        }
        let dependents = g.dependents_of(&setup);
        for l in &lessons {
            assert!(dependents.contains(l), "expected {l:?} in dependents");
        }
    }

    #[test]
    fn dependency_graph_unrelated_change_returns_empty() {
        let mut g = DependencyGraph::default();
        g.record(PathBuf::from("/n/a.md"), HashSet::new());
        let d = g.dependents_of(&PathBuf::from("/n/random.txt"));
        assert!(d.is_empty());
    }

    #[test]
    fn collect_embed_targets_picks_up_simple_embed() {
        let dir = TempDir::new().unwrap();
        let setup = dir.path().join("setup.md");
        fs::write(&setup, "Fs = 48000\n").unwrap();
        let host = dir.path().join("host.md");
        fs::write(&host, "intro\n\n![[setup]]\n\nafter\n").unwrap();
        let found = collect_embed_targets(
            &fs::read_to_string(&host).unwrap(),
            &host,
            dir.path(),
        );
        let canon_setup = fs::canonicalize(&setup).unwrap();
        assert!(found.contains(&canon_setup), "missing setup in {:?}", found);
    }

    // A → B → C: the transitive walker must record both B and C as
    // dependencies of A, so editing C invalidates A.
    #[test]
    fn collect_embed_targets_walks_transitively() {
        let dir = TempDir::new().unwrap();
        let leaf = dir.path().join("leaf.md");
        fs::write(&leaf, "leaf body\n").unwrap();
        let mid = dir.path().join("mid.md");
        fs::write(&mid, "before\n\n![[leaf]]\n\nafter\n").unwrap();
        let host = dir.path().join("host.md");
        fs::write(&host, "intro\n\n![[mid]]\n").unwrap();
        let found = collect_embed_targets(
            &fs::read_to_string(&host).unwrap(),
            &host,
            dir.path(),
        );
        let canon_mid = fs::canonicalize(&mid).unwrap();
        let canon_leaf = fs::canonicalize(&leaf).unwrap();
        assert!(found.contains(&canon_mid), "missing mid in {:?}", found);
        assert!(found.contains(&canon_leaf), "missing leaf in {:?}", found);
    }

    // Cycle A → B → A must not loop. Both A and B end up in the visited
    // set; the walker terminates and returns the discovered files.
    #[test]
    fn collect_embed_targets_handles_cycle() {
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        fs::write(&a, "a body\n\n![[b]]\n").unwrap();
        fs::write(&b, "b body\n\n![[a]]\n").unwrap();
        let found = collect_embed_targets(
            &fs::read_to_string(&a).unwrap(),
            &a,
            dir.path(),
        );
        let canon_b = fs::canonicalize(&b).unwrap();
        assert!(found.contains(&canon_b), "missing b in {:?}", found);
        // a re-references itself; either including or excluding the host
        // is fine — the contract is "transitive deps", which excludes
        // the host. Termination is what we're really asserting here.
    }

    // Section/block-id refs collapse to a file-level dependency.
    #[test]
    fn collect_embed_targets_section_ref_yields_file() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("doc.md");
        fs::write(&target, "# Foo\n\nx\n\n# Bar\n\ny\n").unwrap();
        let host = dir.path().join("host.md");
        fs::write(&host, "see ![[doc#Bar]]\n").unwrap();
        let found = collect_embed_targets(
            &fs::read_to_string(&host).unwrap(),
            &host,
            dir.path(),
        );
        let canon_target = fs::canonicalize(&target).unwrap();
        assert!(
            found.contains(&canon_target),
            "section ref should record the file: {:?}",
            found
        );
    }

    // Non-markdown targets (image embeds) are not file-content
    // dependencies of the rendered markdown — skip them.
    #[test]
    fn collect_embed_targets_skips_non_markdown() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("diagram.svg");
        fs::write(&img, "<svg/>").unwrap();
        let host = dir.path().join("host.md");
        fs::write(&host, "see ![[diagram.svg]]\n").unwrap();
        let found = collect_embed_targets(
            &fs::read_to_string(&host).unwrap(),
            &host,
            dir.path(),
        );
        assert!(
            found.is_empty(),
            "image embeds must not appear in the dep graph: {:?}",
            found
        );
    }

    // End-to-end through the dependency graph: a transitive change to
    // the leaf invalidates the top-level notebook.
    #[test]
    fn dependency_graph_invalidates_transitive_chain() {
        let mut g = DependencyGraph::default();
        let a = PathBuf::from("/n/a.md");
        let b = PathBuf::from("/n/b.md");
        let c = PathBuf::from("/n/c.md");
        // After the transitive walker runs, A's deps include both B and C.
        let mut a_deps = HashSet::new();
        a_deps.insert(b.clone());
        a_deps.insert(c.clone());
        g.record(a.clone(), a_deps);
        let mut b_deps = HashSet::new();
        b_deps.insert(c.clone());
        g.record(b.clone(), b_deps);
        g.record(c.clone(), HashSet::new());

        let dependents = g.dependents_of(&c);
        assert!(dependents.contains(&a), "a should be invalidated by c");
        assert!(dependents.contains(&b), "b should be invalidated by c");
        assert!(dependents.contains(&c), "c itself should be in the set");
    }

    #[test]
    fn compute_render_set_pulls_dependents_only() {
        let mut g = DependencyGraph::default();
        let root = PathBuf::from("/n");
        let setup = root.join("_setup.md");
        let l1 = root.join("l1.md");
        let l2 = root.join("l2.md");
        let l3 = root.join("l3.md");
        let mut deps = HashSet::new();
        deps.insert(setup.clone());
        g.record(l1.clone(), deps.clone());
        g.record(l2.clone(), deps.clone());
        g.record(l3.clone(), HashSet::new());

        let mut changed = HashSet::new();
        changed.insert(setup.clone());
        let render = compute_render_set(&changed, &root, &g);
        assert!(render.contains(&l1));
        assert!(render.contains(&l2));
        assert!(!render.contains(&l3));
    }

    #[test]
    fn plot_dir_for_format_resolves_obsidian_attachments() {
        let format = crate::Format::Markdown {
            obsidian: Some(crate::ObsidianOpts::default()),
        };
        let dir = crate::plot_dir_for_format(
            &PathBuf::from("/out/lesson.md"),
            &format,
        );
        let dir = dir.expect("markdown obsidian should yield a plot dir");
        assert!(dir.ends_with("_attachments/lesson"), "got {:?}", dir);
    }

    #[test]
    fn plot_dir_for_format_returns_none_for_html() {
        let format = crate::Format::Html;
        assert!(crate::plot_dir_for_format(&PathBuf::from("/out/x.html"), &format).is_none());
    }

    #[test]
    fn plot_gc_removes_orphan_dir_contents() {
        // Drop a stale plot file into the dir; pre-render gc deletes it.
        let dir = TempDir::new().unwrap();
        let stale = dir.path().join("plot-7.svg");
        fs::write(&stale, "stale").unwrap();
        assert!(stale.exists());
        let _ = fs::remove_dir_all(dir.path());
        assert!(!stale.exists(), "gc should have removed stale plot");
    }

    // Regression: new `.md` files created after the watcher starts must
    // be rendered on first save, even though the dependency graph has
    // never seen them. Previously `compute_render_set` only consulted
    // `dependents_of()`, which returned an empty set for untracked
    // files, so every "create a new note in Obsidian" event was silently
    // dropped.
    #[test]
    fn is_self_write_echo_true_when_file_matches_recorded_bytes() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, b"hello\n").unwrap();
        let mut writes = HashMap::new();
        writes.insert(path.clone(), b"hello\n".to_vec());

        assert!(
            is_self_write_echo(&path, &writes),
            "matching bytes must register as a self-write echo",
        );
    }

    #[test]
    fn is_self_write_echo_false_when_file_diverges_from_recorded_bytes() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, b"hello\n").unwrap();
        let mut writes = HashMap::new();
        writes.insert(path.clone(), b"old content\n".to_vec());

        assert!(
            !is_self_write_echo(&path, &writes),
            "diverging bytes mean the user (or something else) wrote — must not be suppressed",
        );
    }

    #[test]
    fn is_self_write_echo_false_when_path_never_written_by_us() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, b"hello\n").unwrap();
        let writes: HashMap<PathBuf, Vec<u8>> = HashMap::new();

        assert!(
            !is_self_write_echo(&path, &writes),
            "files we never wrote can never be self-write echoes",
        );
    }

    #[test]
    fn is_editor_tempfile_catches_common_patterns() {
        assert!(is_editor_tempfile(Path::new("/n/.!1234!note.md"))); // sed -i
        assert!(is_editor_tempfile(Path::new("/n/.note.md.swp")));   // vim
        assert!(is_editor_tempfile(Path::new("/n/.note.md.swo")));
        assert!(is_editor_tempfile(Path::new("/n/.#note.md")));      // emacs
        assert!(is_editor_tempfile(Path::new("/n/note.md~")));       // generic backup
        assert!(!is_editor_tempfile(Path::new("/n/note.md")));       // real notebook
        assert!(!is_editor_tempfile(Path::new("/n/sub/note.md")));   // nested
    }

    #[test]
    fn compute_render_set_includes_new_md_file_not_yet_in_graph() {
        let g = DependencyGraph::default();
        let root = PathBuf::from("/n");
        let new_note = root.join("brand_new.md");

        let mut changed = HashSet::new();
        changed.insert(new_note.clone());
        let render = compute_render_set(&changed, &root, &g);
        assert_eq!(
            render,
            vec![new_note],
            "newly-created md file inside root must render even when absent from graph",
        );
    }

    #[test]
    fn compute_render_set_skips_readme_md_to_match_startup() {
        let g = DependencyGraph::default();
        let root = PathBuf::from("/n");
        let readme = root.join("README.md");

        let mut changed = HashSet::new();
        changed.insert(readme);
        let render = compute_render_set(&changed, &root, &g);
        assert!(render.is_empty(), "README.md must not be rendered (mirrors list_notebooks)");
    }

    #[test]
    fn compute_render_set_for_notebook_change_returns_only_self() {
        let mut g = DependencyGraph::default();
        let root = PathBuf::from("/n");
        let l1 = root.join("l1.md");
        let l2 = root.join("l2.md");
        g.record(l1.clone(), HashSet::new());
        g.record(l2.clone(), HashSet::new());

        let mut changed = HashSet::new();
        changed.insert(l1.clone());
        let render = compute_render_set(&changed, &root, &g);
        assert_eq!(render, vec![l1]);
    }
}
