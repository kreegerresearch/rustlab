//! Phase 1 smoke test for the interactive `notebook watch` server.
//!
//! Constructs the same `ServerState` `crate::server::start` would
//! (via the public render pipeline + the rest of the server module)
//! and drives the router via `tower::ServiceExt::oneshot` to verify
//! the four routes return the expected content. Does not bind a real
//! socket — keeps the test hermetic and fast.

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use rustlab_notebook::server::assets::{asset_for_path, rewrite_cdn_urls};
use rustlab_notebook::server::http::{Notebook, ServerState};
use rustlab_plot::Theme;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tower::util::ServiceExt;

const SMOKE_NOTEBOOK: &str = r#"# Server Smoke

Some prose with inline math $E = mc^2$.

```rustlab
1 + 1
```
"#;

/// Re-runs the same render `server::start` does but exposes the
/// pieces we need to drive the router directly. The internal
/// `render_for_server` is private to the server module; we replicate
/// it here using the public render API.
fn build_state() -> (Arc<rustlab_notebook::server::http::ServerState>, TempDir, TempDir) {
    let src_dir = TempDir::new().unwrap();
    let plot_dir = TempDir::new().unwrap();
    let notebook = src_dir.path().join("smoke.md");
    std::fs::write(&notebook, SMOKE_NOTEBOOK).unwrap();

    let theme = Theme::Dark.colors();
    let source = std::fs::read_to_string(&notebook).unwrap();
    let source = rustlab_notebook::strip_render_artifacts(&source);
    let title = rustlab_notebook::extract_title(&source, &notebook);
    let expanded = rustlab_notebook::embed::expand_embeds(
        &source,
        src_dir.path(),
        src_dir.path(),
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
    let html = rewrite_cdn_urls(&html);

    // Single-notebook state keyed by slug "smoke".
    let owned_plot_dir = TempDir::new().unwrap();
    let nb = Arc::new(Notebook::new(
        "smoke".to_string(),
        notebook.clone(),
        title,
        html,
    ));
    let mut notebooks = HashMap::new();
    notebooks.insert("smoke".to_string(), nb);
    let state = Arc::new(ServerState {
        notebooks,
        order: vec!["smoke".to_string()],
        plot_dir: owned_plot_dir,
        editable: false,
        single: true,
        theme,
        index_title: "smoke".to_string(),
    });
    (state, src_dir, plot_dir)
}

#[tokio::test]
async fn notebook_html_renders_smoke_fixture() {
    let (state, _src, _plot) = build_state();
    let app = rustlab_notebook::server::http::router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/n/smoke")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1_024 * 1_024).await.unwrap();
    let html = std::str::from_utf8(&body).expect("html is utf-8");

    // 1. Prose content survived rendering.
    assert!(html.contains("Some prose"), "prose missing from rendered HTML");

    // 2. KaTeX is present (math span survived).
    assert!(html.contains("E = mc^2"), "math expression missing");

    // 3. Code block rendered as a code-block container (syntax
    //    highlighting fragments the source across `<span>`s, so we
    //    look for the wrapping class instead of the raw source).
    assert!(
        html.contains("class=\"code-block\""),
        "code block container missing from rendered HTML"
    );

    // 4. The page references local /assets/ — NOT the CDN.
    assert!(
        html.contains("/assets/katex/katex.min.css"),
        "expected local KaTeX CSS reference, got: {}",
        &html[..html.len().min(1024)]
    );
    assert!(
        html.contains("/assets/plotly.min.js"),
        "expected local Plotly reference"
    );
    assert!(
        !html.contains("cdn.jsdelivr.net"),
        "stray CDN URL: cdn.jsdelivr.net"
    );
    assert!(
        !html.contains("cdn.plot.ly"),
        "stray CDN URL: cdn.plot.ly"
    );
}

#[tokio::test]
async fn root_redirects_to_sole_notebook() {
    let (state, _src, _plot) = build_state();
    let app = rustlab_notebook::server::http::router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(res.headers().get(header::LOCATION).unwrap(), "/n/smoke");
}

#[tokio::test]
async fn katex_css_served_from_embedded_assets() {
    let (state, _src, _plot) = build_state();
    let app = rustlab_notebook::server::http::router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/assets/katex/katex.min.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert!(res
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/css"));

    let body = to_bytes(res.into_body(), 4 * 1_024 * 1_024).await.unwrap();
    // KaTeX CSS starts with /*! KaTeX banner.
    let head = std::str::from_utf8(&body[..body.len().min(64)]).unwrap_or("");
    assert!(
        head.contains("KaTeX") || head.starts_with("/*"),
        "unexpected KaTeX CSS head: {head:?}"
    );
}

#[tokio::test]
async fn plotly_bundle_served_from_embedded_assets() {
    let (state, _src, _plot) = build_state();
    let app = rustlab_notebook::server::http::router(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/assets/plotly.min.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 16 * 1_024 * 1_024).await.unwrap();
    assert!(
        body.len() > 1_000_000,
        "plotly bundle smaller than expected: {}",
        body.len()
    );
}

#[test]
fn assets_module_resolves_a_known_font() {
    // Quick sanity check the embedded fonts actually compiled in.
    let asset = asset_for_path("katex/fonts/KaTeX_Main-Regular.woff2")
        .expect("KaTeX_Main-Regular missing");
    assert_eq!(&asset.bytes[..4], b"wOF2", "not a woff2 file");
}
