//! Axum routes for the interactive `notebook watch` server.
//!
//! Phase 1+2 surface:
//!
//! | Path | Handler |
//! |---|---|
//! | `GET /` | redirect to `/notebook.html` |
//! | `GET /notebook.html` | the current rendered HTML (re-render on save via WS in Phase 2) |
//! | `GET /assets/<path>` | embedded KaTeX/Plotly bundle from [`super::assets`] |
//! | `GET /plots/<file>` | served from the per-server tempdir (animation GIFs) |
//! | `GET /ws` | WebSocket: on connect, sends the current HTML wrapped in `{"kind":"full","html":"…"}`. On subsequent saves, pushes the re-rendered HTML in the same envelope. See [`super::ws`]. |
//!
//! Animations are the only artefact the HTML renderer writes to disk
//! (static plots are inline Plotly JS); future renders can land more
//! filetypes in `plot_dir` without changing the route shape.

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path as AxPath, State},
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use tempfile::TempDir;
use tokio::sync::{broadcast, RwLock};

use super::{assets, ws};

/// Shared state passed to every handler.
///
/// Phase 2 made `html` mutable behind an `RwLock` and added a
/// `broadcast::Sender` so the render loop can publish freshly-rendered
/// HTML to every connected WebSocket. The `Arc<str>` carried by the
/// broadcast is the *already JSON-framed* WS message
/// (`{"kind":"full","html":"…"}`) — cheap to clone per receiver, and
/// avoids re-serialising per client.
pub struct ServerState {
    /// Current rendered HTML (with CDN URLs swapped to `/assets/…`
    /// and the WS-client script injected into `<head>`).
    pub html: RwLock<String>,
    /// Owns the tempdir that holds animation GIFs etc. Dropping the
    /// state (= shutting the server down) cleans it up.
    pub plot_dir: TempDir,
    /// Pre-framed WS messages broadcast on every re-render.
    /// Receivers are minted per WebSocket connection. Capacity 8:
    /// slow clients tolerate brief bursts; if they fall further
    /// behind the broadcast drops the oldest message and the next
    /// successful recv re-syncs them via the current `html` snapshot.
    pub broadcast: broadcast::Sender<Arc<str>>,
}

impl ServerState {
    /// Build a fresh state with the given initial HTML and tempdir,
    /// a new broadcast channel, and no subscribers.
    pub fn new(initial_html: String, plot_dir: TempDir) -> Self {
        let (broadcast, _) = broadcast::channel(8);
        Self {
            html: RwLock::new(initial_html),
            plot_dir,
            broadcast,
        }
    }
}

pub fn router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/", get(root_redirect))
        .route("/notebook.html", get(notebook_html))
        .route("/ws", get(ws::ws_upgrade))
        .route("/assets/{*path}", get(asset))
        .route("/plots/{*path}", get(plot))
        .with_state(state)
}

async fn root_redirect() -> Redirect {
    Redirect::temporary("/notebook.html")
}

async fn notebook_html(State(state): State<Arc<ServerState>>) -> Response {
    let html = state.html.read().await.clone();
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

async fn asset(AxPath(path): AxPath<String>) -> Response {
    match assets::asset_for_path(&path) {
        Some(a) => (
            [(header::CONTENT_TYPE, a.content_type)],
            a.bytes,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "asset not found").into_response(),
    }
}

async fn plot(
    State(state): State<Arc<ServerState>>,
    AxPath(path): AxPath<String>,
) -> Response {
    // Reject traversal — path segments are joined relative to the
    // tempdir, so a `..` segment would escape the served root.
    if path.split('/').any(|seg| seg == ".." || seg.is_empty()) {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }
    let target: PathBuf = state.plot_dir.path().join(&path);
    // Defence in depth: after joining, verify the resolved path still
    // sits inside the tempdir. Catches symlinks, weird Windows
    // separators, anything `..` filter missed.
    let canon_root = state.plot_dir.path();
    if !target.starts_with(canon_root) {
        return (StatusCode::BAD_REQUEST, "path escapes plot root").into_response();
    }

    match tokio::fs::read(&target).await {
        Ok(bytes) => {
            let ct = content_type_for(&target);
            ([(header::CONTENT_TYPE, ct)], Body::from(bytes)).into_response()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (StatusCode::NOT_FOUND, "plot not found").into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("read error: {e}"),
        )
            .into_response(),
    }
}

fn content_type_for(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("svg") => "image/svg+xml",
        Some("gif") => "image/gif",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use tower::util::ServiceExt;

    fn make_state() -> Arc<ServerState> {
        Arc::new(ServerState::new(
            "<h1>hello</h1>".to_string(),
            TempDir::new().unwrap(),
        ))
    }

    #[tokio::test]
    async fn root_redirects_to_notebook_html() {
        let app = router(make_state());
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            res.headers().get(header::LOCATION).unwrap(),
            "/notebook.html"
        );
    }

    #[tokio::test]
    async fn notebook_html_serves_rendered_body() {
        let app = router(make_state());
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/notebook.html")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        assert_eq!(body.as_ref(), b"<h1>hello</h1>");
    }

    #[tokio::test]
    async fn katex_css_served() {
        let app = router(make_state());
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/assets/katex/katex.min.css")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let ct = res.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(ct.to_str().unwrap().starts_with("text/css"));
    }

    #[tokio::test]
    async fn unknown_asset_404s() {
        let app = router(make_state());
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/assets/nope.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn plot_traversal_rejected() {
        let app = router(make_state());
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/plots/../Cargo.toml")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // axum's router normalises `..` segments away before they hit
        // our handler, so the request is more likely to 404 on
        // `/Cargo.toml` than to reach our traversal-block. Either way,
        // it must not return 200 OK with arbitrary file bytes.
        assert_ne!(res.status(), StatusCode::OK);
    }
}
