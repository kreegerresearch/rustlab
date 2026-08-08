//! Phase 2 end-to-end test: bind a real server on an ephemeral port,
//! open a WebSocket client, modify the watched .md, assert the
//! re-rendered HTML arrives over WS wrapped in
//! `{"kind":"full","html":"…"}`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rustlab_notebook::server::{
    http::{router, Notebook, ServerState},
    render_loop,
};
use rustlab_plot::Theme;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Build a single-notebook `ServerState` keyed by `slug` — the shape
/// `server::start` produces for `watch <file>`.
fn single_state(
    slug: &str,
    source_path: &Path,
    html: String,
    plot_dir: TempDir,
) -> Arc<ServerState> {
    single_state_with(slug, source_path, html, plot_dir, false)
}

/// Like [`single_state`] but with an explicit `--editable` flag (the
/// cell-save tests need a server that mounts the write paths).
fn single_state_with(
    slug: &str,
    source_path: &Path,
    html: String,
    plot_dir: TempDir,
    editable: bool,
) -> Arc<ServerState> {
    let nb = Arc::new(Notebook::new(
        slug.to_string(),
        source_path.to_path_buf(),
        slug.to_string(),
        html,
    ));
    let mut notebooks = HashMap::new();
    notebooks.insert(slug.to_string(), nb);
    Arc::new(ServerState {
        notebooks,
        order: vec![slug.to_string()],
        plot_dir,
        editable,
        single: true,
        theme: Theme::Dark.colors(),
        index_title: slug.to_string(),
        link_slugs: HashMap::new(),
        render_tx: std::sync::OnceLock::new(),
    })
}

const INITIAL: &str = "# Live Reload Smoke\n\nbefore-edit body.\n";
const EDITED: &str = "# Live Reload Smoke\n\nafter-edit body with marker LIVE_RELOAD_OK.\n";

// Phase 3 fixture: prose / code / prose / code / prose. Code
// fences are the cheapest block-boundary trigger the parser
// recognises (paragraphs alone collapse into a single Markdown
// chunk). We edit only the middle prose; the two trivial code
// blocks straddle it so the renderer emits five separate blocks.
const PHASE3_INITIAL: &str = r#"# Diff Smoke

paragraph alpha — stable.

```rustlab
1
```

paragraph beta — will change.

```rustlab
2
```

paragraph gamma — stable.
"#;
const PHASE3_EDITED: &str = r#"# Diff Smoke

paragraph alpha — stable.

```rustlab
1
```

paragraph beta — CHANGED for phase3.

```rustlab
2
```

paragraph gamma — stable.
"#;

#[tokio::test(flavor = "current_thread")]
async fn ws_receives_full_envelope_on_file_save() {
    let theme: &'static _ = Theme::Dark.colors();

    // ── 1. Fixture notebook on disk ───────────────────────────────
    let nb_dir = TempDir::new().unwrap();
    let nb_path = nb_dir.path().join("smoke.md");
    std::fs::write(&nb_path, INITIAL).unwrap();
    let nb_path = std::fs::canonicalize(&nb_path).unwrap();

    // ── 2. Initial render + state ─────────────────────────────────
    let plot_dir = TempDir::new().unwrap();
    let initial_html = {
        let path = nb_path.clone();
        let plot = plot_dir.path().to_path_buf();
        // Render via the public surface that the server uses
        // internally — we can't call render_for_server (private to
        // the module), so we exercise the public API: build HTML
        // with `render::render_html` and post-process. This mirrors
        // what server::start does on startup.
        let source = std::fs::read_to_string(&path).unwrap();
        let source = rustlab_notebook::strip_render_artifacts(&source);
        let title = rustlab_notebook::extract_title(&source, &path);
        let expanded = rustlab_notebook::embed::expand_embeds(
            &source,
            path.parent().unwrap(),
            path.parent().unwrap(),
        );
        let blocks = rustlab_notebook::parse::parse_notebook(&expanded);
        let rendered = rustlab_notebook::execute::execute_notebook(&blocks);
        let html =
            rustlab_notebook::render::render_html(&title, &rendered, &plot, "/plots", theme, None, &rustlab_notebook::render::LinkMode::single_file());
        let html = rustlab_notebook::server::assets::rewrite_cdn_urls(&html);
        rustlab_notebook::server::ws::inject_ws_client(&html)
    };
    let state = single_state("smoke", &nb_path, initial_html, plot_dir);

    // ── 3. Bind ephemeral port ────────────────────────────────────
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // ── 4. Spawn the fs watcher + render coordinator ──────────────
    let (_watcher, _coord_handle) =
        render_loop::spawn(&nb_path, false, theme, state.clone()).unwrap();

    // ── 5. Spawn axum server on this runtime ──────────────────────
    let app = router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    // Give the WS upgrade handler a moment to be ready.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // ── 6. Open WebSocket client ──────────────────────────────────
    let ws_url = format!("ws://{}/n/smoke/ws", addr);
    let (mut ws, _resp) = connect_async(&ws_url)
        .await
        .expect("ws connect failed");

    // ── 7. Trigger a re-render by editing the .md ─────────────────
    // Give the watcher a beat to start listening (notify spins up
    // its filesystem subscription asynchronously).
    tokio::time::sleep(Duration::from_millis(150)).await;
    std::fs::write(&nb_path, EDITED).unwrap();

    // ── 8. Expect a `{"kind":"full",...}` message ─────────────────
    let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
        .await
        .expect("ws message did not arrive in time")
        .expect("ws stream closed")
        .expect("ws read error");

    let payload = match msg {
        Message::Text(s) => s.to_string(),
        other => panic!("expected text frame, got {other:?}"),
    };
    let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(parsed["kind"], "full", "wrong message kind: {parsed}");
    let html = parsed["html"].as_str().expect("html field missing");
    assert!(
        html.contains("LIVE_RELOAD_OK"),
        "re-rendered HTML missing edit marker. First 256 bytes:\n{}",
        &html[..html.len().min(256)],
    );
    assert!(
        html.contains("/assets/katex/katex.min.css"),
        "expected local KaTeX asset reference"
    );

    // Tear down. Drop ws + watcher first so axum exits when its
    // listener closes; the test spawn is best-effort cancellation.
    drop(ws);
    server.abort();
}

/// Phase 3: editing one of several blocks produces a
/// `{"kind":"partial","blocks":[…]}` envelope carrying only the
/// changed block at its source-order position.
#[tokio::test(flavor = "current_thread")]
async fn ws_receives_partial_envelope_when_one_of_many_blocks_changes() {
    let theme: &'static _ = Theme::Dark.colors();

    // Fixture.
    let nb_dir = TempDir::new().unwrap();
    let nb_path = nb_dir.path().join("phase3.md");
    std::fs::write(&nb_path, PHASE3_INITIAL).unwrap();
    let nb_path = std::fs::canonicalize(&nb_path).unwrap();

    // Initial render (mirrors server::start's pipeline via public API).
    let plot_dir = TempDir::new().unwrap();
    let initial_html = {
        let source = std::fs::read_to_string(&nb_path).unwrap();
        let source = rustlab_notebook::strip_render_artifacts(&source);
        let title = rustlab_notebook::extract_title(&source, &nb_path);
        let expanded = rustlab_notebook::embed::expand_embeds(
            &source,
            nb_path.parent().unwrap(),
            nb_path.parent().unwrap(),
        );
        let blocks = rustlab_notebook::parse::parse_notebook(&expanded);
        let rendered = rustlab_notebook::execute::execute_notebook(&blocks);
        let html = rustlab_notebook::render::render_html(
            &title,
            &rendered,
            plot_dir.path(),
            "/plots",
            theme,
            None,
            &rustlab_notebook::render::LinkMode::single_file(),
        );
        let html = rustlab_notebook::server::assets::rewrite_cdn_urls(&html);
        rustlab_notebook::server::ws::inject_ws_client(&html)
    };
    let state = single_state("phase3", &nb_path, initial_html, plot_dir);

    // Bind, spawn coordinator, serve.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (_watcher, _coord) = render_loop::spawn(&nb_path, false, theme, state.clone()).unwrap();
    let app = router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    tokio::time::sleep(Duration::from_millis(50)).await;
    let ws_url = format!("ws://{}/n/phase3/ws", addr);
    let (mut ws, _) = connect_async(&ws_url).await.expect("ws connect failed");

    // Edit only the middle prose block.
    tokio::time::sleep(Duration::from_millis(150)).await;
    std::fs::write(&nb_path, PHASE3_EDITED).unwrap();

    // Expect a partial envelope.
    let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
        .await
        .expect("ws message did not arrive in time")
        .expect("ws stream closed")
        .expect("ws read error");
    let payload = match msg {
        Message::Text(s) => s.to_string(),
        other => panic!("expected text frame, got {other:?}"),
    };
    let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(
        parsed["kind"], "partial",
        "expected partial envelope; got: {parsed}"
    );
    let blocks = parsed["blocks"].as_array().expect("blocks array missing");
    assert_eq!(
        blocks.len(),
        1,
        "expected exactly one changed block; got {} in {parsed}",
        blocks.len(),
    );
    let only = &blocks[0];
    let html = only["html"].as_str().expect("html field missing");
    assert!(
        html.contains("CHANGED for phase3"),
        "changed block missing new content. First 256 bytes:\n{}",
        &html[..html.len().min(256)],
    );
    // Position: depends on whether the renderer split the source
    // into 3, 4, or more blocks (heading + paragraphs + hrs). The
    // tight invariant is "one block changed", not the exact index,
    // so we just sanity-check it's within the block list.
    let position = only["position"]
        .as_u64()
        .expect("position field missing or not a number");
    assert!(position < 10, "improbable position: {position}");

    drop(ws);
    server.abort();
}

// Item 5: inserting blocks (a structural / count change) on a flat
// notebook yields a `{"kind":"reconcile",…}` envelope where unchanged
// blocks are reused (no `html`) and only the new blocks carry `html`.
const RECONCILE_INITIAL: &str = r#"# Reconcile

alpha prose — stable.

```rustlab
1
```

gamma prose — stable.
"#;
// Inserts a new prose block + code block between the code and gamma.
const RECONCILE_EDITED: &str = r#"# Reconcile

alpha prose — stable.

```rustlab
1
```

beta INSERTED prose.

```rustlab
2
```

gamma prose — stable.
"#;

#[tokio::test(flavor = "current_thread")]
async fn ws_receives_reconcile_envelope_when_blocks_are_inserted() {
    let theme: &'static _ = Theme::Dark.colors();

    let nb_dir = TempDir::new().unwrap();
    let nb_path = nb_dir.path().join("recon.md");
    std::fs::write(&nb_path, RECONCILE_INITIAL).unwrap();
    let nb_path = std::fs::canonicalize(&nb_path).unwrap();

    let plot_dir = TempDir::new().unwrap();
    let initial_html = {
        let source = std::fs::read_to_string(&nb_path).unwrap();
        let source = rustlab_notebook::strip_render_artifacts(&source);
        let title = rustlab_notebook::extract_title(&source, &nb_path);
        let expanded = rustlab_notebook::embed::expand_embeds(
            &source,
            nb_path.parent().unwrap(),
            nb_path.parent().unwrap(),
        );
        let blocks = rustlab_notebook::parse::parse_notebook(&expanded);
        let rendered = rustlab_notebook::execute::execute_notebook(&blocks);
        let html = rustlab_notebook::render::render_html(
            &title,
            &rendered,
            plot_dir.path(),
            "/plots",
            theme,
            None,
            &rustlab_notebook::render::LinkMode::single_file(),
        );
        let html = rustlab_notebook::server::assets::rewrite_cdn_urls(&html);
        rustlab_notebook::server::ws::inject_ws_client(&html)
    };
    let state = single_state("recon", &nb_path, initial_html, plot_dir);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (_watcher, _coord) = render_loop::spawn(&nb_path, false, theme, state.clone()).unwrap();
    let app = router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    tokio::time::sleep(Duration::from_millis(50)).await;
    let ws_url = format!("ws://{}/n/recon/ws", addr);
    let (mut ws, _) = connect_async(&ws_url).await.expect("ws connect failed");

    tokio::time::sleep(Duration::from_millis(150)).await;
    std::fs::write(&nb_path, RECONCILE_EDITED).unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
        .await
        .expect("ws message did not arrive in time")
        .expect("ws stream closed")
        .expect("ws read error");
    let payload = match msg {
        Message::Text(s) => s.to_string(),
        other => panic!("expected text frame, got {other:?}"),
    };
    let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(parsed["kind"], "reconcile", "expected reconcile; got: {parsed}");

    let blocks = parsed["blocks"].as_array().expect("blocks array missing");
    // Some blocks reused (no html), some fresh (carry html); at least one
    // fresh block must contain the inserted prose.
    let reused = blocks.iter().filter(|b| b.get("html").is_none()).count();
    let fresh: Vec<_> = blocks.iter().filter(|b| b.get("html").is_some()).collect();
    assert!(reused >= 1, "expected some reused blocks; got {parsed}");
    assert!(!fresh.is_empty(), "expected some fresh blocks; got {parsed}");
    assert!(
        fresh
            .iter()
            .any(|b| b["html"].as_str().unwrap_or("").contains("INSERTED")),
        "no fresh block carried the inserted content: {parsed}"
    );
    // Every entry carries a stable id.
    assert!(blocks.iter().all(|b| b["id"].as_str().is_some_and(|s| s.starts_with("b-"))));

    drop(ws);
    server.abort();
}

/// Source for the widget round-trip test: one slider plus a code block
/// that multiplies its value, so a `widget_update` produces observably
/// different output.
const WIDGET_NB: &str = "\
# Widget WS

```rustlab-widget
name = \"gain\"
type = \"slider\"
min = 0
max = 10
step = 1
default = 2
```

```rustlab
disp(widget(\"gain\") * 100)
```
";

/// Interactive widgets (Phase 1): a client-sent
/// `{"kind":"widget_update",…}` updates the live value and the server
/// pushes back a re-render reflecting it — in both the control and the
/// dependent code output. Unlike the file-save tests this drives the
/// coordinator through the render-request channel, so it needs only a
/// bound socket (no filesystem-watch event).
#[tokio::test(flavor = "current_thread")]
async fn ws_widget_update_triggers_rerender_with_new_value() {
    let theme: &'static _ = Theme::Dark.colors();

    // Fixture.
    let nb_dir = TempDir::new().unwrap();
    let nb_path = nb_dir.path().join("widget.md");
    std::fs::write(&nb_path, WIDGET_NB).unwrap();
    let nb_path = std::fs::canonicalize(&nb_path).unwrap();

    // Initial render at defaults (gain = 2 → 200), mirroring server::start.
    let plot_dir = TempDir::new().unwrap();
    let initial_html = {
        let source = std::fs::read_to_string(&nb_path).unwrap();
        let source = rustlab_notebook::strip_render_artifacts(&source);
        let title = rustlab_notebook::extract_title(&source, &nb_path);
        let blocks = rustlab_notebook::parse::parse_notebook(&source);
        let rendered = rustlab_notebook::execute::execute_notebook(&blocks);
        let html = rustlab_notebook::render::render_html(
            &title,
            &rendered,
            plot_dir.path(),
            "/plots",
            theme,
            None,
            &rustlab_notebook::render::LinkMode::single_file(),
        );
        let html = rustlab_notebook::server::assets::rewrite_cdn_urls(&html);
        rustlab_notebook::server::ws::inject_ws_client(&html)
    };
    assert!(initial_html.contains("200"), "fixture: default output should be 200");
    let state = single_state("widget", &nb_path, initial_html, plot_dir);
    // Seed the widget declarations the WS handler validates against
    // (server::start does this from render_for_server; the test mirrors it).
    {
        let blocks = rustlab_notebook::parse::parse_notebook(WIDGET_NB);
        let decls: Vec<_> = blocks
            .into_iter()
            .filter_map(|b| match b {
                rustlab_notebook::parse::Block::Widget { decl, .. } => Some(decl),
                _ => None,
            })
            .collect();
        *state.notebook("widget").unwrap().widget_decls.lock().unwrap() = decls;
    }

    // Bind, spawn coordinator (publishes the render-request channel), serve.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (_watcher, _coord) = render_loop::spawn(&nb_path, false, theme, state.clone()).unwrap();
    let app = router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    tokio::time::sleep(Duration::from_millis(50)).await;
    let ws_url = format!("ws://{}/n/widget/ws", addr);
    let (mut ws, _) = connect_async(&ws_url).await.expect("ws connect failed");

    // Drag the slider to 7 → expect 7 * 100 = 700 on the re-render.
    ws.send(Message::Text(
        r#"{"kind":"widget_update","name":"gain","value":7}"#.into(),
    ))
    .await
    .expect("ws send failed");

    let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
        .await
        .expect("widget re-render did not arrive in time")
        .expect("ws stream closed")
        .expect("ws read error");
    let payload = match msg {
        Message::Text(s) => s.to_string(),
        other => panic!("expected text frame, got {other:?}"),
    };
    let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
    // Phase 1: a widget change re-runs the whole notebook → full envelope.
    assert_eq!(parsed["kind"], "full", "expected full envelope; got {parsed}");
    let html = parsed["html"].as_str().expect("html field missing");
    assert!(html.contains("700"), "output didn't follow the slider:\n{html}");
    assert!(
        html.contains("value=\"7\""),
        "slider control not re-rendered at the new value"
    );

    drop(ws);
    server.abort();
}


/// Source for the option round-trip test: one option widget plus a code
/// block that echoes its string value.
const OPTION_NB: &str = "\
# Option WS

```rustlab-widget
name = \"window\"
type = \"option\"
choices = [\"hamming\", \"hann\", \"blackman\"]
default = \"hamming\"
```

```rustlab
disp(widget(\"window\"))
```
";

/// Phase 2: a string-valued `widget_update` for an `option` widget round
/// trips — the server validates the choice, re-renders, and the new value
/// shows in both the radio selection and the code output.
#[tokio::test(flavor = "current_thread")]
async fn ws_option_update_selects_choice_and_drives_output() {
    let theme: &'static _ = Theme::Dark.colors();

    let nb_dir = TempDir::new().unwrap();
    let nb_path = nb_dir.path().join("opt.md");
    std::fs::write(&nb_path, OPTION_NB).unwrap();
    let nb_path = std::fs::canonicalize(&nb_path).unwrap();

    let plot_dir = TempDir::new().unwrap();
    let initial_html = {
        let source = std::fs::read_to_string(&nb_path).unwrap();
        let source = rustlab_notebook::strip_render_artifacts(&source);
        let title = rustlab_notebook::extract_title(&source, &nb_path);
        let blocks = rustlab_notebook::parse::parse_notebook(&source);
        let rendered = rustlab_notebook::execute::execute_notebook(&blocks);
        let html = rustlab_notebook::render::render_html(
            &title, &rendered, plot_dir.path(), "/plots", theme, None,
            &rustlab_notebook::render::LinkMode::single_file(),
        );
        let html = rustlab_notebook::server::assets::rewrite_cdn_urls(&html);
        rustlab_notebook::server::ws::inject_ws_client(&html)
    };
    let state = single_state("opt", &nb_path, initial_html, plot_dir);
    // Seed the widget declarations the WS handler validates against
    // (server::start does this from render_for_server; the test mirrors it).
    {
        let blocks = rustlab_notebook::parse::parse_notebook(OPTION_NB);
        let decls: Vec<_> = blocks
            .into_iter()
            .filter_map(|b| match b {
                rustlab_notebook::parse::Block::Widget { decl, .. } => Some(decl),
                _ => None,
            })
            .collect();
        *state.notebook("opt").unwrap().widget_decls.lock().unwrap() = decls;
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (_watcher, _coord) = render_loop::spawn(&nb_path, false, theme, state.clone()).unwrap();
    let app = router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    tokio::time::sleep(Duration::from_millis(50)).await;
    let ws_url = format!("ws://{}/n/opt/ws", addr);
    let (mut ws, _) = connect_async(&ws_url).await.expect("ws connect failed");

    ws.send(Message::Text(
        r#"{"kind":"widget_update","name":"window","value":"blackman"}"#.into(),
    ))
    .await
    .expect("ws send failed");

    let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
        .await
        .expect("widget re-render did not arrive in time")
        .expect("ws stream closed")
        .expect("ws read error");
    let payload = match msg {
        Message::Text(s) => s.to_string(),
        other => panic!("expected text frame, got {other:?}"),
    };
    let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(parsed["kind"], "full", "expected full envelope; got {parsed}");
    let html = parsed["html"].as_str().expect("html field missing");
    assert!(html.contains("value=\"blackman\" checked"), "choice not selected:\n{html}");
    assert!(html.contains("blackman"), "output didn't follow the choice");

    drop(ws);
    server.abort();
}

// ─── Cell execution: ▶ Run forces re-execution of an unchanged block ───

/// The code block reads a sidecar CSV. Editing the CSV never touches the
/// watched `.md`, so the only way its new value can reach the page is a
/// browser-triggered forced re-execution — which is exactly what
/// `{"kind":"run_block"}` must produce even though the block's *source*
/// hash is unchanged (a plain render would be a full cache hit).
const RUN_BLOCK_SRC: &str = "# Run Smoke\n\n```rustlab\nv = load(\"probe.csv\");\nprint(v)\n```\n";

/// Receive text frames until the terminal `cell_status done`, returning
/// every parsed frame (including the `done`).
async fn frames_until_done<S>(ws: &mut S) -> Vec<serde_json::Value>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let mut out = Vec::new();
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("ws frame did not arrive in time")
            .expect("ws stream closed")
            .expect("ws read error");
        let Message::Text(s) = msg else { continue };
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        let done = v["kind"] == "cell_status" && v["state"] == "done";
        out.push(v);
        if done {
            return out;
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn ws_run_block_forces_reexecution_of_unchanged_block() {
    let theme: &'static _ = Theme::Dark.colors();

    let nb_dir = TempDir::new().unwrap();
    let nb_path = nb_dir.path().join("run.md");
    std::fs::write(&nb_path, RUN_BLOCK_SRC).unwrap();
    std::fs::write(nb_dir.path().join("probe.csv"), "31415\n").unwrap();
    let nb_path = std::fs::canonicalize(&nb_path).unwrap();

    // Initial render via the public pipeline. It runs without the
    // server's chdir, so `load("probe.csv")` may error here — irrelevant:
    // it only seeds prev_blocks; the assertions ride the server renders.
    let plot_dir = TempDir::new().unwrap();
    let initial_html = {
        let source = std::fs::read_to_string(&nb_path).unwrap();
        let source = rustlab_notebook::strip_render_artifacts(&source);
        let title = rustlab_notebook::extract_title(&source, &nb_path);
        let expanded = rustlab_notebook::embed::expand_embeds(
            &source,
            nb_path.parent().unwrap(),
            nb_path.parent().unwrap(),
        );
        let blocks = rustlab_notebook::parse::parse_notebook(&expanded);
        let rendered = rustlab_notebook::execute::execute_notebook(&blocks);
        let html = rustlab_notebook::render::render_html(
            &title,
            &rendered,
            plot_dir.path(),
            "/plots",
            theme,
            None,
            &rustlab_notebook::render::LinkMode::single_file(),
        );
        let html = rustlab_notebook::server::assets::rewrite_cdn_urls(&html);
        rustlab_notebook::server::ws::inject_ws_client(&html)
    };
    let state = single_state("run", &nb_path, initial_html, plot_dir);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (_watcher, _coord) = render_loop::spawn(&nb_path, false, theme, state.clone()).unwrap();
    let app = router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    tokio::time::sleep(Duration::from_millis(50)).await;
    let ws_url = format!("ws://{}/n/run/ws", addr);
    let (mut ws, _) = connect_async(&ws_url).await.expect("ws connect failed");

    // ── Round 1: warm the render cache through the server path ──────
    ws.send(Message::Text(r#"{"kind":"run_block","idx":0}"#.into()))
        .await
        .expect("ws send failed");
    let frames = frames_until_done(&mut ws).await;
    assert_eq!(
        (frames[0]["kind"].as_str(), frames[0]["state"].as_str(), frames[0]["idx"].as_u64()),
        (Some("cell_status"), Some("running"), Some(0)),
        "first frame after Run must be the running status: {:?}",
        frames[0]
    );
    let round1 = serde_json::Value::Array(frames).to_string();
    assert!(
        round1.contains("31415"),
        "round 1 render must show the CSV value:\n{round1:.512}"
    );

    // ── Round 2: change the sidecar (not the .md), Run again ─────────
    // Without the force, this render would be a full cache hit
    // (source hash unchanged) and no content frame could carry the new
    // value — the assertion below is the end-to-end force proof.
    std::fs::write(nb_dir.path().join("probe.csv"), "27182\n").unwrap();
    ws.send(Message::Text(r#"{"kind":"run_block","idx":0}"#.into()))
        .await
        .expect("ws send failed");
    let frames = frames_until_done(&mut ws).await;
    let round2 = serde_json::Value::Array(frames).to_string();
    assert!(
        round2.contains("27182"),
        "forced re-run must re-execute the unchanged block and pick up the new CSV value:\n{round2:.512}"
    );

    drop(ws);
    server.abort();
}

// ─── Cell save: Shift+Enter writes the block through to the .md ────────

const SAVE_BLOCK_SRC: &str = "# Save Smoke\n\n```rustlab\nc = 10;\nprint(c)\n```\n\ntail prose.\n";

#[tokio::test(flavor = "current_thread")]
async fn ws_save_run_block_writes_file_and_rerenders() {
    let theme: &'static _ = Theme::Dark.colors();

    let nb_dir = TempDir::new().unwrap();
    let nb_path = nb_dir.path().join("save.md");
    std::fs::write(&nb_path, SAVE_BLOCK_SRC).unwrap();
    let nb_path = std::fs::canonicalize(&nb_path).unwrap();

    let plot_dir = TempDir::new().unwrap();
    let state = single_state_with("save", &nb_path, "<main></main>".into(), plot_dir, true);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (_watcher, _coord) = render_loop::spawn(&nb_path, false, theme, state.clone()).unwrap();
    let app = router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    tokio::time::sleep(Duration::from_millis(50)).await;
    let ws_url = format!("ws://{}/n/save/ws", addr);
    let (mut ws, _) = connect_async(&ws_url).await.expect("ws connect failed");

    ws.send(Message::Text(
        r#"{"kind":"save_run_block","idx":0,"source":"c = 77;\nprint(c)","prev_source":"c = 10;\nprint(c)"}"#.into(),
    ))
    .await
    .expect("ws send failed");

    let frames = frames_until_done(&mut ws).await;
    assert_eq!(
        (frames[0]["kind"].as_str(), frames[0]["ok"].as_bool(), frames[0]["idx"].as_u64()),
        (Some("cell_saved"), Some(true), Some(0)),
        "first frame must be the per-socket save verdict: {:?}",
        frames[0]
    );

    // Write-through: the .md on disk carries the new body, siblings intact.
    let on_disk = std::fs::read_to_string(&nb_path).unwrap();
    assert!(on_disk.contains("c = 77;"), "disk not updated:\n{on_disk}");
    assert!(!on_disk.contains("c = 10;"));
    assert!(on_disk.contains("tail prose."), "prose untouched");

    // The forced render pushed the new output.
    let all = serde_json::Value::Array(frames).to_string();
    assert!(all.contains("77"), "render output missing new value:\n{all:.512}");

    // Watcher echo of our own write must not produce a second *content*
    // broadcast: the echo render is a full cache hit → Broadcast::None.
    // (A stray extra `cell_status done` from a late-arriving fs event is
    // fine — status frames are idempotent.)
    loop {
        match tokio::time::timeout(Duration::from_millis(1500), ws.next()).await {
            Err(_) => break, // quiet — no echo content
            Ok(Some(Ok(Message::Text(s)))) => {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap();
                assert_eq!(
                    v["kind"], "cell_status",
                    "watcher echo must not rebroadcast content: {v}"
                );
            }
            Ok(other) => panic!("unexpected ws frame during quiet window: {other:?}"),
        }
    }

    drop(ws);
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn ws_save_run_block_rejected_without_editable() {
    let theme: &'static _ = Theme::Dark.colors();

    let nb_dir = TempDir::new().unwrap();
    let nb_path = nb_dir.path().join("ro.md");
    std::fs::write(&nb_path, SAVE_BLOCK_SRC).unwrap();
    let nb_path = std::fs::canonicalize(&nb_path).unwrap();

    let plot_dir = TempDir::new().unwrap();
    // editable: false — the save must be refused server-side even if a
    // client hand-crafts the frame.
    let state = single_state("ro", &nb_path, "<main></main>".into(), plot_dir);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (_watcher, _coord) = render_loop::spawn(&nb_path, false, theme, state.clone()).unwrap();
    let app = router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    tokio::time::sleep(Duration::from_millis(50)).await;
    let ws_url = format!("ws://{}/n/ro/ws", addr);
    let (mut ws, _) = connect_async(&ws_url).await.expect("ws connect failed");

    ws.send(Message::Text(
        r#"{"kind":"save_run_block","idx":0,"source":"c = 77;\nprint(c)","prev_source":"c = 10;\nprint(c)"}"#.into(),
    ))
    .await
    .expect("ws send failed");

    let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
        .await
        .expect("verdict did not arrive")
        .expect("ws stream closed")
        .expect("ws read error");
    let Message::Text(s) = msg else { panic!("expected text frame") };
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v["kind"], "cell_saved");
    assert_eq!(v["ok"], false);
    assert!(
        v["error"].as_str().unwrap().contains("--editable"),
        "error should name the flag: {v}"
    );
    assert_eq!(
        std::fs::read_to_string(&nb_path).unwrap(),
        SAVE_BLOCK_SRC,
        "read-only server must never write"
    );

    drop(ws);
    server.abort();
}
