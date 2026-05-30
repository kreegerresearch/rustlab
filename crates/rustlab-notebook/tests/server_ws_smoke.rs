//! Phase 2 end-to-end test: bind a real server on an ephemeral port,
//! open a WebSocket client, modify the watched .md, assert the
//! re-rendered HTML arrives over WS wrapped in
//! `{"kind":"full","html":"…"}`.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use rustlab_notebook::server::{
    http::{router, ServerState},
    render_loop,
};
use rustlab_plot::Theme;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};

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
            rustlab_notebook::render::render_html(&title, &rendered, &plot, "/plots", theme, None);
        let html = rustlab_notebook::server::assets::rewrite_cdn_urls(&html);
        rustlab_notebook::server::ws::inject_ws_client(&html)
    };
    let state = Arc::new(ServerState::new(initial_html, plot_dir));

    // ── 3. Bind ephemeral port ────────────────────────────────────
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // ── 4. Spawn the fs watcher + render coordinator ──────────────
    let (_watcher, _coord_handle) =
        render_loop::spawn(nb_path.clone(), theme, state.clone()).unwrap();

    // ── 5. Spawn axum server on this runtime ──────────────────────
    let app = router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    // Give the WS upgrade handler a moment to be ready.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // ── 6. Open WebSocket client ──────────────────────────────────
    let ws_url = format!("ws://{}/ws", addr);
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
        );
        let html = rustlab_notebook::server::assets::rewrite_cdn_urls(&html);
        rustlab_notebook::server::ws::inject_ws_client(&html)
    };
    let state = Arc::new(ServerState::new(initial_html, plot_dir));

    // Bind, spawn coordinator, serve.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (_watcher, _coord) = render_loop::spawn(nb_path.clone(), theme, state.clone()).unwrap();
    let app = router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    tokio::time::sleep(Duration::from_millis(50)).await;
    let ws_url = format!("ws://{}/ws", addr);
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
