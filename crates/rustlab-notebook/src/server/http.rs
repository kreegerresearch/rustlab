//! Axum routes for the interactive `notebook watch` server.
//!
//! Phase 5 generalised the server from a single notebook to a *set* of
//! notebooks keyed by URL-safe slug, so `watch <dir>` can serve a whole
//! directory behind a generated index page. Single-file `watch <file>`
//! is just a one-entry set (DRY — same routing, same render loop).
//!
//! | Path | Handler |
//! |---|---|
//! | `GET /` | single: redirect to the one notebook; directory: the index listing |
//! | `GET /n/{slug}` | that notebook's current rendered HTML |
//! | `GET /n/{slug}/ws` | WebSocket: re-render push for that notebook (see [`super::ws`]) |
//! | `GET /raw/{slug}` | raw `.md` source from disk (drives the split-view source pane) |
//! | `POST /save/{slug}` | write edited source back to disk — **only mounted when `--editable`** |
//! | `GET /assets/{path}` | embedded KaTeX/Plotly/CodeMirror bundle from [`super::assets`] |
//! | `GET /plots/{path}` | served from the per-server tempdir (`<slug>/<file>`) |
//!
//! Animations are the only artefact the HTML renderer writes to disk
//! (static plots are inline Plotly JS); each notebook gets its own
//! `plot_dir/<slug>/` subdir so directory mode keeps them separate.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::{Path as AxPath, State},
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use rustlab_plot::ThemeColors;
use tempfile::TempDir;
use tokio::sync::{broadcast, RwLock};

use super::diff::{self, Block};
use super::{assets, ws};

/// Per-notebook live state. In single-file mode the [`ServerState`]
/// holds exactly one of these; in directory mode, one per discovered
/// `.md`.
pub struct Notebook {
    /// URL-safe identifier (the `{slug}` in `/n/{slug}`). Unique within
    /// a server instance.
    pub slug: String,
    /// Absolute path to the source `.md`. The `--editable` save handler
    /// writes here; the raw handler reads here.
    pub source_path: PathBuf,
    /// Display title (frontmatter `title:` › first H1 › file stem).
    pub title: String,
    /// Current rendered HTML (CDN URLs swapped to `/assets/…`, WS-client
    /// + page chrome injected).
    pub html: RwLock<String>,
    /// Previous render's per-block snapshot, used by the coordinator to
    /// compute partial diffs. Each notebook tracks its own baseline so
    /// directory mode can re-render one notebook without disturbing the
    /// others. Only the (synchronous) coordinator touches this.
    pub prev_blocks: Mutex<Vec<Block>>,
    /// Pre-framed WS messages broadcast on every re-render of *this*
    /// notebook. Receivers are minted per WebSocket connection.
    pub broadcast: broadcast::Sender<Arc<str>>,
}

impl Notebook {
    /// Build a notebook entry from its initial render. Seeds the
    /// diff baseline from `initial_html` and opens a fresh broadcast
    /// channel (capacity 8, matching the prior single-notebook server).
    pub fn new(slug: String, source_path: PathBuf, title: String, initial_html: String) -> Self {
        let prev_blocks = diff::split_blocks(&initial_html);
        let (broadcast, _) = broadcast::channel(8);
        Self {
            slug,
            source_path,
            title,
            html: RwLock::new(initial_html),
            prev_blocks: Mutex::new(prev_blocks),
            broadcast,
        }
    }
}

/// Shared state passed to every handler.
pub struct ServerState {
    /// Notebooks keyed by slug.
    pub notebooks: HashMap<String, Arc<Notebook>>,
    /// Slugs in listing order (sorted by source path), for the index.
    pub order: Vec<String>,
    /// Owns the tempdir that holds plot artefacts (`<slug>/<file>`).
    /// Dropping the state (= server shutdown) cleans it up.
    pub plot_dir: TempDir,
    /// When false (the default), the `/save/{slug}` write-back route is
    /// not mounted at all. Set by `--editable` (locked-in: the in-browser
    /// editor is the one interactive path that modifies source).
    pub editable: bool,
    /// True for `watch <file>` (one notebook, `/` redirects to it);
    /// false for `watch <dir>` (`/` shows the index listing).
    pub single: bool,
    /// Theme for the generated index page.
    pub theme: &'static ThemeColors,
    /// Index-page heading (directory name, or the lone notebook's title).
    pub index_title: String,
}

impl ServerState {
    /// Look up a notebook by slug.
    pub fn notebook(&self, slug: &str) -> Option<&Arc<Notebook>> {
        self.notebooks.get(slug)
    }

    /// The single notebook, when in single-file mode.
    pub fn sole(&self) -> Option<&Arc<Notebook>> {
        if self.single {
            self.order.first().and_then(|s| self.notebooks.get(s))
        } else {
            None
        }
    }
}

/// Turn a file stem into a URL-safe slug: lowercase, non-alphanumerics
/// collapsed to single dashes, trimmed. Empty input → `"notebook"`.
/// Callers dedupe collisions with a numeric suffix.
pub fn slugify(stem: &str) -> String {
    let mut s = String::with_capacity(stem.len());
    let mut last_dash = false;
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            s.push('-');
            last_dash = true;
        }
    }
    let trimmed = s.trim_matches('-');
    if trimmed.is_empty() {
        "notebook".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn router(state: Arc<ServerState>) -> Router {
    let mut r = Router::new()
        .route("/", get(root))
        .route("/notebook.html", get(legacy_redirect)) // back-compat with Phase 1–4 URLs
        .route("/n/{slug}", get(notebook_page))
        .route("/n/{slug}/ws", get(ws::ws_upgrade))
        .route("/raw/{slug}", get(raw_source))
        .route("/assets/{*path}", get(asset))
        .route("/plots/{*path}", get(plot));

    // The write-back route exists only under `--editable`.
    if state.editable {
        r = r.route("/save/{slug}", post(save_source));
    }

    r.with_state(state)
}

async fn root(State(state): State<Arc<ServerState>>) -> Response {
    if let Some(nb) = state.sole() {
        return Redirect::temporary(&format!("/n/{}", nb.slug)).into_response();
    }
    // Directory mode: generated index listing.
    let entries: Vec<(String, String)> = state
        .order
        .iter()
        .filter_map(|slug| state.notebooks.get(slug))
        .map(|nb| (nb.title.clone(), format!("n/{}", nb.slug)))
        .collect();
    let html = crate::generate_index_html(&state.index_title, &entries, state.theme, "");
    // Inject the WS client so a future "index refresh on add/remove"
    // has a socket to push over; harmless today (no slug → no connect).
    let html = ws::inject_ws_client(&html);
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

/// Phase 1–4 served the lone notebook at `/notebook.html`. Keep that
/// URL working by redirecting to the canonical root.
async fn legacy_redirect() -> Redirect {
    Redirect::temporary("/")
}

async fn notebook_page(
    State(state): State<Arc<ServerState>>,
    AxPath(slug): AxPath<String>,
) -> Response {
    match state.notebook(&slug) {
        Some(nb) => {
            let html = nb.html.read().await.clone();
            (
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                html,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "notebook not found").into_response(),
    }
}

async fn raw_source(
    State(state): State<Arc<ServerState>>,
    AxPath(slug): AxPath<String>,
) -> Response {
    let Some(nb) = state.notebook(&slug) else {
        return (StatusCode::NOT_FOUND, "notebook not found").into_response();
    };
    match tokio::fs::read(&nb.source_path).await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
            Body::from(bytes),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("read error: {e}"),
        )
            .into_response(),
    }
}

/// `POST /save/{slug}` — write the request body back to the notebook's
/// source `.md`. Only mounted under `--editable`. The fs watcher then
/// picks up the change and pushes a re-render to the page.
async fn save_source(
    State(state): State<Arc<ServerState>>,
    AxPath(slug): AxPath<String>,
    body: String,
) -> Response {
    let Some(nb) = state.notebook(&slug) else {
        return (StatusCode::NOT_FOUND, "notebook not found").into_response();
    };
    match tokio::fs::write(&nb.source_path, body.as_bytes()).await {
        Ok(()) => (StatusCode::OK, "saved").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("write error: {e}"),
        )
            .into_response(),
    }
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
    // separators, anything the `..` filter missed.
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
    use rustlab_plot::Theme;
    use tower::util::ServiceExt;

    /// Build a single-notebook state with the given initial HTML.
    fn single_state(html: &str) -> Arc<ServerState> {
        let plot_dir = TempDir::new().unwrap();
        let src = plot_dir.path().join("nb.md");
        std::fs::write(&src, "# nb\n").unwrap();
        let nb = Arc::new(Notebook::new(
            "nb".to_string(),
            src,
            "nb".to_string(),
            html.to_string(),
        ));
        let mut notebooks = HashMap::new();
        notebooks.insert("nb".to_string(), nb);
        Arc::new(ServerState {
            notebooks,
            order: vec!["nb".to_string()],
            plot_dir,
            editable: false,
            single: true,
            theme: Theme::Dark.colors(),
            index_title: "nb".to_string(),
        })
    }

    #[test]
    fn slugify_basics() {
        assert_eq!(slugify("Contour Plots"), "contour-plots");
        assert_eq!(slugify("a__b--c"), "a-b-c");
        assert_eq!(slugify("--trim--"), "trim");
        assert_eq!(slugify("***"), "notebook");
        assert_eq!(slugify("Already-Slug"), "already-slug");
    }

    #[tokio::test]
    async fn root_redirects_to_notebook_in_single_mode() {
        let app = router(single_state("<h1>hello</h1>"));
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
        assert_eq!(res.headers().get(header::LOCATION).unwrap(), "/n/nb");
    }

    #[tokio::test]
    async fn notebook_page_serves_rendered_body() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/n/nb")
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
    async fn unknown_notebook_404s() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/n/nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn raw_source_returns_markdown() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/raw/nb")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let ct = res.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(ct.to_str().unwrap().starts_with("text/markdown"));
        let body = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        assert_eq!(body.as_ref(), b"# nb\n");
    }

    #[tokio::test]
    async fn save_route_absent_when_not_editable() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/save/nb")
                    .body(Body::from("# edited\n"))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Route not mounted → axum returns 404 (not 200).
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn save_route_writes_file_when_editable() {
        let state = single_state("<h1>hello</h1>");
        // Rebuild as editable (single_state defaults editable=false).
        let src = state.notebook("nb").unwrap().source_path.clone();
        let plot_dir = TempDir::new().unwrap();
        let nb = Arc::new(Notebook::new(
            "nb".to_string(),
            src.clone(),
            "nb".to_string(),
            "<h1>hello</h1>".to_string(),
        ));
        let mut notebooks = HashMap::new();
        notebooks.insert("nb".to_string(), nb);
        let editable_state = Arc::new(ServerState {
            notebooks,
            order: vec!["nb".to_string()],
            plot_dir,
            editable: true,
            single: true,
            theme: Theme::Dark.colors(),
            index_title: "nb".to_string(),
        });
        let app = router(editable_state);
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/save/nb")
                    .body(Body::from("# edited\n"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let on_disk = std::fs::read_to_string(&src).unwrap();
        assert_eq!(on_disk, "# edited\n");
    }

    #[tokio::test]
    async fn katex_css_served() {
        let app = router(single_state("<h1>hello</h1>"));
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
    async fn plot_traversal_rejected() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/plots/../Cargo.toml")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(res.status(), StatusCode::OK);
    }
}
