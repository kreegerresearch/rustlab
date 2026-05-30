//! Render coordinator + fs watcher.
//!
//! ## Pipeline
//!
//! ```text
//! notify watcher (std thread)
//!     │   (raw events on the watched file / directory tree)
//!     ▼
//! filter & map path → slug (same std thread)
//!     │   (tokio::sync::mpsc — carries the slug that changed)
//!     ▼
//! coordinator task (tokio)
//!     │  debounce 250ms → for each changed slug: spawn_blocking(render)
//!     ▼
//! notebook.html.write() + notebook.broadcast.send(json envelope)
//!     │
//!     ▼  /n/<slug>/ws subscribers forward the JSON to every connected page.
//! ```
//!
//! Phase 5 generalised this from one notebook to many: a single
//! directory watcher feeds a single coordinator that maps each changed
//! path back to its [`Notebook`] (by source path) and re-renders just
//! that one. Single-file mode is the one-entry case.
//!
//! ## Cancellation policy
//!
//! Today this ships *let-it-finish*: a save during a slow render does
//! not preempt the in-flight execution. Only one render runs at a time
//! per coordinator; once it finishes, any pending slugs trigger fresh
//! renders. (Phase 5d replaces this with true preemption via a
//! rustlab-script cancellation token.)

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{event::EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rustlab_plot::ThemeColors;
use tokio::sync::mpsc;

use super::diff::{self, Broadcast};
use super::http::{Notebook, ServerState};
use super::ws;

/// Debounce window for filesystem events. Matches the existing
/// `watch.rs` default (`watch::DEFAULT_DEBOUNCE_MS`) so a single
/// editor save collapses to one render pass.
const DEBOUNCE: Duration = Duration::from_millis(250);

/// Spawn the render coordinator. `watch_root` is the directory (dir
/// mode) or the single notebook file (file mode); `is_dir` selects
/// recursive directory watching vs watching the file's parent.
///
/// Returns the live `notify` watcher (caller must keep it alive —
/// dropping it stops fs events) and a `JoinHandle` for the coordinator
/// task (caller drops it when the runtime shuts down).
pub fn spawn(
    watch_root: &Path,
    is_dir: bool,
    theme: &'static ThemeColors,
    state: Arc<ServerState>,
) -> Result<(RecommendedWatcher, tokio::task::JoinHandle<()>)> {
    // Map every notebook's source path → slug so the bridge can route a
    // changed file to the right notebook. Key by canonical path when we
    // can resolve it, falling back to the stored path.
    let mut by_path: HashMap<PathBuf, String> = HashMap::new();
    for (slug, nb) in &state.notebooks {
        let key = std::fs::canonicalize(&nb.source_path).unwrap_or_else(|_| nb.source_path.clone());
        by_path.insert(key, slug.clone());
    }

    // What to watch, and how. Dir mode: the whole tree (catches new
    // files and nested notebooks). File mode: the parent dir
    // non-recursively, so atomic-rename editor saves still fire.
    let (watch_target, mode) = if is_dir {
        (watch_root.to_path_buf(), RecursiveMode::Recursive)
    } else {
        let parent = watch_root
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        (parent, RecursiveMode::NonRecursive)
    };

    let (raw_tx, raw_rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
        let _ = raw_tx.send(res);
    })
    .context("creating notify watcher")?;
    watcher
        .watch(&watch_target, mode)
        .with_context(|| format!("watching {}", watch_target.display()))?;

    // Bridge: std mpsc (notify thread) → tokio mpsc (coordinator task).
    // Forwards the slug of whichever watched notebook changed.
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        while let Ok(res) = raw_rx.recv() {
            let event = match res {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("[watch] notify error: {e}");
                    continue;
                }
            };
            if !is_relevant_event(&event) {
                continue;
            }
            for path in &event.paths {
                if let Some(slug) = match_slug(&by_path, path) {
                    if tx.send(slug).is_err() {
                        return; // coordinator gone
                    }
                }
            }
        }
    });

    let handle = tokio::spawn(coordinator(theme, state, rx));
    Ok((watcher, handle))
}

/// Resolve an event path back to a notebook slug. Tries an exact
/// canonical-path match first, then falls back to a unique file-name
/// match (handles atomic-rename saves where the new inode hasn't been
/// canonicalised into our map).
fn match_slug(by_path: &HashMap<PathBuf, String>, path: &Path) -> Option<String> {
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(slug) = by_path.get(&canon) {
        return Some(slug.clone());
    }
    // Fall back to matching by file name if exactly one notebook has it.
    let name = path.file_name()?;
    let mut hit: Option<&String> = None;
    for (key, slug) in by_path {
        if key.file_name() == Some(name) {
            if hit.is_some() {
                return None; // ambiguous — don't guess
            }
            hit = Some(slug);
        }
    }
    hit.cloned()
}

/// Decide whether a notify event is a content-changing save worth
/// re-rendering on. Pure-access / metadata-only events are dropped.
fn is_relevant_event(event: &notify::Event) -> bool {
    matches!(
        event.kind,
        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) | EventKind::Any
    )
}

/// Coordinator task: receives debounced "slug X changed" pings and
/// produces one re-render per changed notebook per debounce cycle.
/// Loops until the channel closes (server shutdown).
async fn coordinator(
    theme: &'static ThemeColors,
    state: Arc<ServerState>,
    mut rx: mpsc::UnboundedReceiver<String>,
) {
    loop {
        // Wait for at least one event, capturing the first slug.
        let first = match rx.recv().await {
            Some(s) => s,
            None => return,
        };
        let mut pending: HashSet<String> = HashSet::new();
        pending.insert(first);

        // Debounce: keep draining slugs until quiet for `DEBOUNCE`.
        loop {
            tokio::select! {
                evt = rx.recv() => {
                    match evt {
                        Some(s) => { pending.insert(s); }
                        None => return,
                    }
                }
                _ = tokio::time::sleep(DEBOUNCE) => break,
            }
        }

        for slug in pending {
            let Some(nb) = state.notebook(&slug).cloned() else {
                continue;
            };
            rerender_notebook(theme, &state, &nb).await;
        }
    }
}

/// Re-render a single notebook, publish its new HTML, and broadcast a
/// partial/full/none envelope to that notebook's WS subscribers.
async fn rerender_notebook(theme: &'static ThemeColors, state: &Arc<ServerState>, nb: &Arc<Notebook>) {
    let input = nb.source_path.clone();
    let slug = nb.slug.clone();
    let plot_root = state.plot_dir.path().to_path_buf();
    let editable = state.editable;

    let render_result = tokio::task::spawn_blocking(move || {
        super::render_for_server(&input, theme, &plot_root, &slug, editable)
    })
    .await;

    let new_html = match render_result {
        Ok(Ok(html)) => html,
        Ok(Err(e)) => {
            eprintln!("[watch] render error ({}): {e:#}", nb.slug);
            return;
        }
        Err(e) => {
            eprintln!("[watch] render task panicked ({}): {e}", nb.slug);
            return;
        }
    };

    // Diff against this notebook's previous block list before publishing.
    let new_blocks = diff::split_blocks(&new_html);
    let decision = {
        let prev = nb.prev_blocks.lock().unwrap();
        diff::classify(&prev, &new_blocks)
    };

    // Publish state regardless of broadcast kind so a fresh page load
    // always gets the latest, and update the diff baseline.
    {
        let mut guard = nb.html.write().await;
        *guard = new_html.clone();
    }
    {
        let mut prev = nb.prev_blocks.lock().unwrap();
        *prev = new_blocks;
    }

    match decision {
        Broadcast::None => {
            eprintln!("[watch] re-rendered {} (no change)", nb.slug);
        }
        Broadcast::Partial(changed) => {
            let env: Arc<str> = Arc::from(diff::partial_envelope(&changed));
            let _ = nb.broadcast.send(env);
            eprintln!(
                "[watch] re-rendered {} (partial: {} block{})",
                nb.slug,
                changed.len(),
                if changed.len() == 1 { "" } else { "s" },
            );
        }
        Broadcast::Full => {
            let env: Arc<str> = Arc::from(ws::full_envelope(&new_html));
            let _ = nb.broadcast.send(env);
            eprintln!("[watch] re-rendered {} (full)", nb.slug);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustlab_plot::Theme;
    use std::collections::HashMap as Map;
    use tempfile::TempDir;

    #[test]
    fn is_relevant_event_drops_access_events() {
        use notify::event::{AccessKind, MetadataKind};
        let access = notify::Event::new(EventKind::Access(AccessKind::Read));
        assert!(!is_relevant_event(&access));

        let modify = notify::Event::new(EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )));
        assert!(is_relevant_event(&modify));

        let create = notify::Event::new(EventKind::Create(notify::event::CreateKind::File));
        assert!(is_relevant_event(&create));

        let mtime_only = notify::Event::new(EventKind::Modify(notify::event::ModifyKind::Metadata(
            MetadataKind::WriteTime,
        )));
        assert!(is_relevant_event(&mtime_only));
    }

    #[test]
    fn match_slug_by_filename_fallback() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("note.md");
        std::fs::write(&p, "x").unwrap();
        let mut by_path = HashMap::new();
        by_path.insert(std::fs::canonicalize(&p).unwrap(), "note".to_string());
        // A non-canonical spelling of the same file still resolves.
        let alt = dir.path().join("./note.md");
        assert_eq!(match_slug(&by_path, &alt).as_deref(), Some("note"));
    }

    /// Build a single-notebook state for the coordinator test.
    fn single_state(nb_path: &Path, html: String) -> (Arc<ServerState>, Arc<Notebook>) {
        let nb = Arc::new(Notebook::new(
            "nb".to_string(),
            nb_path.to_path_buf(),
            "nb".to_string(),
            html,
        ));
        let mut notebooks: Map<String, Arc<Notebook>> = Map::new();
        notebooks.insert("nb".to_string(), nb.clone());
        let state = Arc::new(ServerState {
            notebooks,
            order: vec!["nb".to_string()],
            plot_dir: TempDir::new().unwrap(),
            editable: false,
            single: true,
            theme: Theme::Dark.colors(),
            index_title: "nb".to_string(),
        });
        (state, nb)
    }

    #[tokio::test]
    async fn coordinator_renders_on_event_and_broadcasts() {
        let theme: &'static _ = Theme::Dark.colors();

        let dir = TempDir::new().unwrap();
        let nb_path = dir.path().join("nb.md");
        std::fs::write(&nb_path, "# Initial\n\nbody A.\n").unwrap();

        let html0 =
            super::super::render_for_server(&nb_path, theme, dir.path(), "nb", false).unwrap();
        let (state, nb) = single_state(&nb_path, html0);

        let mut sub = nb.broadcast.subscribe();
        let (tx, rx) = mpsc::unbounded_channel::<String>();
        let coord = tokio::spawn(coordinator(theme, state.clone(), rx));

        // Edit the file and ping the coordinator with the slug.
        std::fs::write(&nb_path, "# Initial\n\nbody B with marker XYZ.\n").unwrap();
        tx.send("nb".to_string()).unwrap();

        let msg = tokio::time::timeout(Duration::from_secs(5), sub.recv())
            .await
            .expect("broadcast did not arrive in time")
            .expect("broadcast channel closed");
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();
        // Single prose edit on a 1-block doc → kind may be full or partial;
        // either way the new marker must be present.
        let text = parsed.to_string();
        assert!(text.contains("XYZ"), "re-render missing marker: {text:.256}");

        assert!(state.notebook("nb").unwrap().html.read().await.contains("XYZ"));

        drop(tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), coord).await;
    }
}
