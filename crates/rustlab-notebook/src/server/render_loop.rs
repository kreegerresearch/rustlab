//! Phase-2 render coordinator + fs watcher.
//!
//! ## Pipeline
//!
//! ```text
//! notify watcher (std thread)
//!     │   (raw events on the input .md / its parent dir)
//!     ▼
//! filter & forward (same std thread)
//!     │   (tokio::sync::mpsc — bounded channel)
//!     ▼
//! coordinator task (tokio)
//!     │  debounce 250ms → spawn_blocking(render_for_server)
//!     ▼
//! state.html.write() + state.broadcast.send(json envelope)
//!     │
//!     ▼  /ws subscribers forward the JSON message to every connected page.
//! ```
//!
//! ## Cancellation policy
//!
//! Phase 2 ships *let-it-finish*: a save during a slow render does
//! not preempt the in-flight execution (rustlab-script doesn't poll
//! for cancellation tokens — that's a Phase 5 follow-up). What
//! Phase 2 *does* guarantee: only one render runs at a time, and
//! once the current render finishes, the coordinator immediately
//! consumes any pending event and starts a fresh render. So a
//! prose edit during a slow code-block render waits exactly one
//! debounce cycle past the slow render completing — not multiple
//! cycles, and not forever.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{event::EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rustlab_plot::ThemeColors;
use tokio::sync::mpsc;

use super::diff::{self, Block, Broadcast};
use super::http::ServerState;
use super::ws;

/// Debounce window for filesystem events. Matches the existing
/// `watch.rs` default (`watch::DEFAULT_DEBOUNCE_MS`) so a single
/// editor save collapses to one render pass.
const DEBOUNCE: Duration = Duration::from_millis(250);

/// Spawn the render coordinator. Returns the live `notify` watcher
/// (caller must keep it alive — dropping it stops the fs events) and
/// a `JoinHandle` for the coordinator task (caller drops it when
/// the runtime shuts down).
pub fn spawn(
    input: PathBuf,
    theme: &'static ThemeColors,
    state: Arc<ServerState>,
) -> Result<(RecommendedWatcher, tokio::task::JoinHandle<()>)> {
    let canonical_input = std::fs::canonicalize(&input)
        .with_context(|| format!("canonicalizing {}", input.display()))?;
    let target_name = canonical_input
        .file_name()
        .map(|s| s.to_os_string())
        .ok_or_else(|| anyhow::anyhow!("input has no file name: {}", input.display()))?;

    // Watch the parent dir non-recursively so atomic-rename editor
    // saves (vim default, vscode's "safe write") still trigger us
    // after the inode swap. We filter by file name in the bridge
    // below so unrelated siblings don't cause spurious renders.
    let parent = canonical_input
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let (raw_tx, raw_rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
        let _ = raw_tx.send(res);
    })
    .context("creating notify watcher")?;
    watcher
        .watch(&parent, RecursiveMode::NonRecursive)
        .with_context(|| format!("watching {}", parent.display()))?;

    // Bridge: std mpsc (notify thread) → tokio mpsc (coordinator task).
    let (tx, rx) = mpsc::unbounded_channel::<()>();
    let target_name_for_thread = target_name.clone();
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
            if !event
                .paths
                .iter()
                .any(|p| p.file_name() == Some(&target_name_for_thread))
            {
                continue;
            }
            if tx.send(()).is_err() {
                break; // coordinator gone, watcher will drop next
            }
        }
    });

    // Spawn coordinator task.
    let handle = tokio::spawn(coordinator(canonical_input, theme, state, rx));

    Ok((watcher, handle))
}

/// Decide whether a notify event is a content-changing save worth
/// re-rendering on. We drop pure-access and metadata-only events
/// because editors and the OS produce a steady drizzle of them; only
/// content writes (Modify / Create / Remove + the catch-all `Any`
/// that some backends emit) actually warrant a re-render.
fn is_relevant_event(event: &notify::Event) -> bool {
    matches!(
        event.kind,
        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) | EventKind::Any
    )
}

/// Coordinator task: receives debounced "something changed" pings
/// and produces one re-render per debounce cycle. Loops forever
/// until the channel closes (server shutdown).
///
/// Phase 3 added the block-diff decision: after each render the
/// coordinator splits the new document into blocks
/// ([`diff::split_blocks`]), compares against the previous
/// snapshot, and either sends a `kind="partial"` envelope (only
/// changed blocks), a `kind="full"` envelope (large change or
/// blocks removed — see [`diff::classify`]), or nothing at all
/// (renderer was deterministic and the source didn't change).
async fn coordinator(
    input: PathBuf,
    theme: &'static ThemeColors,
    state: Arc<ServerState>,
    mut rx: mpsc::UnboundedReceiver<()>,
) {
    // Seed the prev-block snapshot from the initial render that
    // `server::start` produced before we were spawned.
    let mut prev_blocks: Vec<Block> = {
        let html = state.html.read().await;
        diff::split_blocks(&html)
    };

    loop {
        // Wait for at least one event.
        if rx.recv().await.is_none() {
            return;
        }
        // Debounce: drain further events until the channel is quiet
        // for `DEBOUNCE`. Editors produce a burst per save; this
        // collapses the burst into one render pass.
        loop {
            tokio::select! {
                evt = rx.recv() => {
                    if evt.is_none() { return; }
                }
                _ = tokio::time::sleep(DEBOUNCE) => break,
            }
        }

        // Render off the runtime thread — execution is CPU-bound.
        // The render reads the plot tempdir from state directly.
        let render_input = input.clone();
        let render_state = state.clone();
        let render_result = tokio::task::spawn_blocking(move || {
            let plot_dir = render_state.plot_dir.path().to_path_buf();
            super::render_for_server(&render_input, theme, &plot_dir)
        })
        .await;

        let new_html = match render_result {
            Ok(Ok(html)) => html,
            Ok(Err(e)) => {
                eprintln!("[watch] render error: {e:#}");
                continue;
            }
            Err(e) => {
                eprintln!("[watch] render task panicked: {e}");
                continue;
            }
        };

        // Diff against the previous render *before* publishing the
        // new HTML, so the comparison is against last-render's
        // blocks (not the freshly-overwritten state).
        let new_blocks = diff::split_blocks(&new_html);
        let decision = diff::classify(&prev_blocks, &new_blocks);

        // Publish state regardless of broadcast kind so a fresh
        // page-load over GET /notebook.html always gets the latest.
        {
            let mut guard = state.html.write().await;
            *guard = new_html.clone();
        }

        match decision {
            Broadcast::None => {
                eprintln!("[watch] re-rendered {} (no change)", input.display());
            }
            Broadcast::Partial(changed) => {
                let env: Arc<str> = Arc::from(diff::partial_envelope(&changed));
                let _ = state.broadcast.send(env);
                eprintln!(
                    "[watch] re-rendered {} (partial: {} block{})",
                    input.display(),
                    changed.len(),
                    if changed.len() == 1 { "" } else { "s" },
                );
            }
            Broadcast::Full => {
                let env: Arc<str> = Arc::from(ws::full_envelope(&new_html));
                let _ = state.broadcast.send(env);
                eprintln!("[watch] re-rendered {} (full)", input.display());
            }
        }

        prev_blocks = new_blocks;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Metadata-only modifies still count as Modify so we accept
        // them — better to re-render once spuriously than miss a save.
        let mtime_only = notify::Event::new(EventKind::Modify(notify::event::ModifyKind::Metadata(
            MetadataKind::WriteTime,
        )));
        assert!(is_relevant_event(&mtime_only));
    }

    #[tokio::test]
    async fn coordinator_renders_on_event_and_broadcasts() {
        use rustlab_plot::Theme;
        use tempfile::TempDir;

        let theme: &'static _ = Theme::Dark.colors();

        let dir = TempDir::new().unwrap();
        let nb = dir.path().join("nb.md");
        std::fs::write(&nb, "# Initial\n\nbody A.\n").unwrap();

        let html0 = super::super::render_for_server(&nb, theme, dir.path()).unwrap();
        let state = Arc::new(ServerState::new(html0, TempDir::new().unwrap()));

        let mut sub = state.broadcast.subscribe();
        let (tx, rx) = mpsc::unbounded_channel::<()>();

        let coord_state = state.clone();
        let coord = tokio::spawn(coordinator(nb.clone(), theme, coord_state, rx));

        // Edit the file and ping.
        std::fs::write(&nb, "# Initial\n\nbody B with marker XYZ.\n").unwrap();
        tx.send(()).unwrap();

        // Wait for the broadcast (debounce window + render).
        let msg = tokio::time::timeout(Duration::from_secs(5), sub.recv())
            .await
            .expect("broadcast did not arrive in time")
            .expect("broadcast channel closed");
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(parsed["kind"], "full");
        let html = parsed["html"].as_str().unwrap();
        assert!(
            html.contains("XYZ"),
            "re-rendered HTML missing the marker: {}",
            &html[..html.len().min(256)]
        );

        // Also check the state was updated.
        assert!(state.html.read().await.contains("XYZ"));

        drop(tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), coord).await;
    }
}
