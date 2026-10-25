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
//! | `GET /n/{*path}` | directory: `/n/<relative path without .md>`; single-file: `/n/<slug>`. An old `/n/<stem>` redirects only when that stem names one notebook. A path ending in `/ws` with `Upgrade: websocket` is the live-reload socket (see [`super::ws`]); matchit will not register that route beside this wildcard |
//! | `GET /raw/{*slug}` | raw `.md` source from disk (drives the split-view source pane). `slug` may contain `/` |
//! | `POST /save/{*slug}` | write edited source back to disk — **only mounted when `--editable`** |
//! | `GET /assets/{path}` | embedded KaTeX/Plotly/CodeMirror bundle from [`super::assets`] |
//! | `GET /plots/{path}` | served from the per-server tempdir (`<slug>/<file>`) |
//!
//! Animations are the only artefact the HTML renderer writes to disk
//! (static plots are inline Plotly JS); each notebook gets its own
//! `plot_dir/<slug>/` subdir so directory mode keeps them separate.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use crate::cache::NotebookCache;
use crate::widget::WidgetDecl;
use rustlab_script::WidgetValue;

use axum::{
    body::Body,
    extract::{FromRequestParts, Path as AxPath, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use rustlab_plot::ThemeColors;
use tempfile::TempDir;
use tokio::sync::{broadcast, RwLock};

use super::auth;
use super::diff::{self, Block};
use super::{assets, ws};

/// Per-notebook live state. In single-file mode the [`ServerState`]
/// holds exactly one of these; in directory mode, one per discovered
/// `.md`.
pub struct Notebook {
    /// Notebook identity, unique within a server instance. Directory
    /// mode: the collection-relative path with `.md` stripped
    /// (`ch2/intro`). Single-file mode: the URL-safe file stem. A route
    /// parameter, never a filesystem path.
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
    /// Monotonic render generation. Each scheduled render bumps this; a
    /// finishing render only publishes if its generation is still the
    /// latest, so a preempted (stale) render can't clobber a newer one.
    pub render_gen: std::sync::atomic::AtomicU64,
    /// Cancel flag of the render currently in flight for this notebook
    /// (if any). A newer save sets it to preempt the slow render. See
    /// `render_loop::schedule_render`.
    pub cancel: Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>,
    /// Live widget values for this notebook, keyed by widget name. Updated
    /// by the WS `widget_update` handler (slider drags) and read as render
    /// overrides by `render_loop::schedule_render`. Server-side, ephemeral,
    /// per-server-process (locked-in: not persisted to `.md`, shared across
    /// tabs). Empty means every widget renders at its declared default.
    pub widget_values: Mutex<BTreeMap<String, WidgetValue>>,
    /// Widget declarations from the most recent render, used by the WS
    /// handler to validate incoming `widget_update` values (range / choice)
    /// before storing them. Refreshed on every render so a `.md` edit that
    /// changes a widget is reflected in validation.
    pub widget_decls: Mutex<Vec<WidgetDecl>>,
    /// In-memory widget-aware prefix cache for this notebook's renders
    /// (server-session-scoped). Lets a slider drag or a source edit re-run
    /// only from the first affected block onward (scoped re-render). Held
    /// for the duration of a render by `render_loop::schedule_render`.
    pub render_cache: Mutex<NotebookCache>,
    /// Serialises every write to `source_path` — the whole-doc
    /// `POST /save/{slug}` and the WS cell editor's read-splice-write —
    /// so two concurrent saves can't tear the file. Async (tokio) because
    /// both holders await file IO while holding it; never nested with any
    /// other lock on this struct.
    pub save_lock: tokio::sync::Mutex<()>,
    /// Last annotate write in this process. One level. The client asks
    /// for it with `{"op":"undo"}`; the previous bytes are not accepted
    /// from the request. Cleared after a successful undo.
    undo: std::sync::Mutex<Option<UndoSnap>>,
}

struct UndoSnap {
    before: String,
    after_hash: String,
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
            render_gen: std::sync::atomic::AtomicU64::new(0),
            cancel: Mutex::new(None),
            widget_values: Mutex::new(BTreeMap::new()),
            widget_decls: Mutex::new(Vec::new()),
            render_cache: Mutex::new(NotebookCache::default()),
            save_lock: tokio::sync::Mutex::new(()),
            undo: std::sync::Mutex::new(None),
        }
    }
}

/// Shared state passed to every handler.
pub struct ServerState {
    /// Notebooks keyed by slug.
    pub notebooks: HashMap<String, Arc<Notebook>>,
    /// Slugs in listing order — frontmatter `order:` ascending, ties by
    /// collection-relative path, unordered entries last (the same rule as
    /// the static build; see `compare_notebook_order`). Drives the index.
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
    /// Cross-notebook link resolution: normalized collection-root-relative
    /// path (`"ch1/notes.md"`) → identity. In directory mode the identity
    /// is that path with `.md` stripped (`ch1/notes`), so two files named
    /// `intro.md` do not share a key. Single-file mode holds just that
    /// file, keyed by its bare filename, and the identity is `slugify`'d.
    pub link_slugs: HashMap<String, String>,
    /// Rendered body of the collection root's `index.md`, shown on `/` —
    /// matching the static build, which hoists it into `index.html`.
    /// Empty when the collection has none. RwLock: an `index.md` edit
    /// during watch refreshes it (the index page holds no WS socket, so
    /// the next page load simply reads the new body).
    pub index_body: tokio::sync::RwLock<String>,
    /// Canonical path of the root `index.md`, when present. The watcher
    /// routes its change events to an index-body refresh instead of a
    /// notebook render (it has no slug — it is not a notebook). Its
    /// *title* is read once at startup; a title edit needs a restart.
    pub index_md_path: Option<PathBuf>,
    /// Render-request channel into the coordinator. Set once by
    /// `render_loop::spawn` after it creates the coordinator channel; the
    /// WS `widget_update` / `run_block` handlers send requests here
    /// (reusing the same debounce + preemption path as a file save).
    pub render_tx: OnceLock<tokio::sync::mpsc::UnboundedSender<super::render_loop::RenderRequest>>,
    /// CSP `script-src` nonce stamped at render time on the scripts the
    /// renderer and page chrome emit (see `server::auth` module docs).
    pub csp_nonce: String,
    /// Bound TCP port (set after listen). Used for Origin allowlisting.
    pub bind_port: std::sync::atomic::AtomicU16,
    /// Path-jail root for notebook file I/O: the collection root in
    /// directory mode, `--jail-root` when given, `None` (= each notebook's
    /// own directory) for a single-file serve.
    pub jail_root: Option<PathBuf>,
    /// `POST /annotate/{slug}` and the right-click UI. `--editable` implies this.
    pub annotate: bool,
    /// Initial Comments checkbox on watch pages.
    pub comments_on: bool,
    /// `watch --git-diff` or `[notebook] git_diff`. When false the server
    /// never spawns git, including for `?diff=` and `GET /git/refs`.
    pub git_enabled: bool,
}

impl ServerState {
    /// Look up a notebook by its identity.
    ///
    /// `slug` is a map key (`ch2/intro` in directory mode, a URL-safe stem
    /// for a single file). Callers must reject `..` before calling this.
    /// The value is never joined onto the collection root.
    pub fn notebook(&self, slug: &str) -> Option<&Arc<Notebook>> {
        self.notebooks.get(slug)
    }

    /// Invert `link_slugs` (relative `.md` path → identity) to identity →
    /// path. Identities are unique, so this cannot drop a notebook that
    /// shares a file name with another folder.
    pub(crate) fn rel_by_slug(&self) -> HashMap<&str, &str> {
        let map: HashMap<&str, &str> = self
            .link_slugs
            .iter()
            .map(|(rel, slug)| (slug.as_str(), rel.as_str()))
            .collect();
        debug_assert_eq!(
            map.len(),
            self.link_slugs.len(),
            "two notebooks share an identity; a collection path was dropped"
        );
        map
    }

    /// The single notebook, when in single-file mode.
    pub fn sole(&self) -> Option<&Arc<Notebook>> {
        if self.single {
            self.order.first().and_then(|s| self.notebooks.get(s))
        } else {
            None
        }
    }

    /// The [`crate::render::LinkMode`] for re-rendering the notebook at
    /// `slug` — same resolution `build_state` used at startup, so a live
    /// re-render cannot emit different hrefs than the first render.
    pub fn link_mode_for(&self, slug: &str) -> crate::render::LinkMode {
        let current_rel_dir = self
            .link_slugs
            .iter()
            .find(|(_, s)| s.as_str() == slug)
            .and_then(|(rel, _)| rel.rsplit_once('/'))
            .map(|(dir, _)| dir.to_string())
            .unwrap_or_default();
        crate::render::LinkMode::Server {
            slugs: self.link_slugs.clone(),
            current_rel_dir,
            // Single-file `/` redirects to the notebook itself — mapping
            // index.md there would send the link in a circle.
            index_at_root: !self.single,
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
        // One route: matchit rejects `/n/{*path}` beside `/n/{slug}/ws`.
        // `notebook_page` upgrades `{slug}/ws` when the client asked for
        // a WebSocket and otherwise serves the page (including a notebook
        // whose public path ends in `/ws`).
        .route("/n/{*path}", get(notebook_page))
        .route("/raw/{*slug}", get(raw_source))
        .route("/assets/{*path}", get(asset))
        .route("/plots/{*path}", get(plot))
        .route("/git/refs", get(git_refs))
        .route("/git/changes", get(git_changes));

    // The write-back route exists only under `--editable`. The wildcard
    // is required because a directory identity contains `/`.
    if state.editable {
        r = r.route("/save/{*slug}", post(save_source));
    }
    // Annotate is implied by --editable and also mounted for --annotate alone.
    if state.annotate || state.editable {
        r = r.route("/annotate/{*slug}", post(annotate_source));
    }

    r.with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, require_loopback_host))
}

/// DNS-rebinding defense: every request (pages, assets, raw source,
/// save, WebSocket upgrade) must present a loopback `Host` for the
/// port this server actually bound. The bound port is `ServerState::bind_port`
/// — callers set it to the real listen port; there is no default-port
/// fallback.
async fn require_loopback_host(
    State(state): State<Arc<ServerState>>,
    request: Request,
    next: Next,
) -> Response {
    let port = state.bind_port.load(std::sync::atomic::Ordering::Relaxed);
    if !auth::host_is_loopback(request.headers(), port) {
        let body = auth::host_rejection_body(request.headers(), port);
        return (StatusCode::FORBIDDEN, body).into_response();
    }
    next.run(request).await
}

async fn root(State(state): State<Arc<ServerState>>) -> Response {
    if let Some(nb) = state.sole() {
        let loc = format!("/n/{}", nb.slug);
        return Redirect::temporary(&loc).into_response();
    }
    let rel_of = state.rel_by_slug();
    let entries: Vec<(String, String)> = state
        .order
        .iter()
        .filter_map(|slug| {
            let nb = state.notebooks.get(slug)?;
            let href = if state.single {
                format!("n/{}", nb.slug)
            } else {
                let rel = rel_of.get(slug.as_str()).copied().unwrap_or("");
                format!("n/{}", crate::render::watch_public_rel(rel))
            };
            Some((nb.title.clone(), href))
        })
        .collect();
    let body = state.index_body.read().await;
    let browser = super::collection_browser(&state, None);
    let html = crate::generate_index_html(
        &state.index_title,
        &entries,
        state.theme,
        &body,
        browser.as_ref(),
    );
    // The WS client is the only script on the index page; it gets the
    // nonce here. The index body itself was sanitised at render time.
    let html = ws::inject_ws_client_nonced(&html, Some(&state.csp_nonce));
    html_response(&state, html)
}

/// Phase 1–4 served the lone notebook at `/notebook.html`. Keep that
/// URL working by redirecting to the canonical root.
async fn legacy_redirect() -> Redirect {
    Redirect::temporary("/")
}

/// What `GET /n/{path}` should do. The path is never joined to the
/// filesystem: a hit is a key already in `link_slugs`, or (for an old
/// unambiguous stem URL) a redirect to that notebook's path.
enum PageHit {
    Serve(String),
    Redirect(String),
    /// 404 HTML listing every notebook that shares the requested stem.
    Ambiguous(String),
    Bad,
    Missing,
}

/// Resolve a captured `/n/` path.
///
/// Directory mode: `{decoded}.md` in `link_slugs` wins, so `/n/intro`
/// is the root `intro.md` even when `a/intro.md` also exists, and
/// `/n/ch2/filters` is that file. A single-segment path that misses
/// every relative path is an old `/n/<stem>` bookmark: it redirects
/// only when exactly one notebook has that stem (the file name, or the
/// old lowercased `slugify` form). Two matches return a disambiguation
/// page. `..`, `.`, empty segments, `\`, and NUL are rejected.
/// Single-file mode serves `/n/<slug>` and does not interpret the path
/// as a collection path.
fn resolve_watch_page(state: &ServerState, path: &str) -> PageHit {
    if path.contains('\\') || path.contains('\0') {
        return PageHit::Bad;
    }
    let mut decoded_segs = Vec::new();
    for seg in path.split('/') {
        if seg.is_empty() {
            return PageHit::Bad;
        }
        let decoded = crate::render::percent_decode(seg);
        if decoded.is_empty()
            || decoded == "."
            || decoded == ".."
            || decoded.contains('/')
            || decoded.contains('\\')
            || decoded.contains('\0')
        {
            return PageHit::Bad;
        }
        decoded_segs.push(decoded);
    }
    let decoded = decoded_segs.join("/");

    if state.single {
        if decoded.contains('/') {
            return PageHit::Missing;
        }
        return match state.notebook(&decoded) {
            Some(nb) => PageHit::Serve(nb.slug.clone()),
            None => PageHit::Missing,
        };
    }

    let key = format!("{decoded}.md");
    if let Some(slug) = state.link_slugs.get(&key) {
        return PageHit::Serve(slug.clone());
    }
    // Legacy `/n/<stem>`. An exact path already won, so a root
    // `intro.md` is served above and is not part of this search.
    // `/n/<stem>-N` is not an alias: it 404s unless a file is named that.
    if !decoded.contains('/') {
        let mut hits: Vec<(String, String)> = Vec::new();
        for (rel, slug) in &state.link_slugs {
            if stem_matches(rel, &decoded) {
                hits.push((slug.clone(), rel.clone()));
            }
        }
        hits.sort();
        hits.dedup();
        match hits.len() {
            0 => {}
            1 => {
                let public = crate::render::watch_public_rel(&hits[0].1);
                if public != decoded {
                    return PageHit::Redirect(format!("/n/{public}"));
                }
                return PageHit::Serve(hits[0].0.clone());
            }
            _ => return PageHit::Ambiguous(disambiguation_page(state, &decoded, &hits)),
        }
    }
    PageHit::Missing
}

/// File-name stem equals `decoded`, or its old URL-safe form does
/// (`My Notes.md` still answers `/n/my-notes` when it is the only match).
fn stem_matches(rel_md: &str, decoded: &str) -> bool {
    let file = rel_md.rsplit('/').next().unwrap_or(rel_md);
    let stem = file.strip_suffix(".md").unwrap_or(file);
    stem == decoded || slugify(stem) == decoded
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 404 body for an old stem URL that names more than one notebook.
/// Titles and paths are escaped. There is no script.
fn disambiguation_page(state: &ServerState, stem: &str, hits: &[(String, String)]) -> String {
    let mut rows = hits.to_vec();
    rows.sort_by(|a, b| a.1.cmp(&b.1));
    let mut items = String::new();
    for (slug, rel) in &rows {
        let href = format!("/n/{}", crate::render::watch_public_rel(rel));
        let title = state
            .notebook(slug)
            .map(|nb| nb.title.as_str())
            .unwrap_or(rel.as_str());
        items.push_str(&format!(
            "<li><a href=\"{href}\">{}</a> ({})</li>\n",
            escape_html(title),
            escape_html(rel),
        ));
    }
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>Ambiguous notebook</title></head>\n\
<body>\n<h1>More than one notebook matches {stem}</h1>\n\
<p>This short name is not unique. Open one of these notebooks:</p>\n<ul>\n{items}</ul>\n</body></html>\n",
        stem = escape_html(stem),
    )
}

/// `true` when `slug` must not be used as a map key: empty, `.`, `..`,
/// `\`, or NUL. The caller has already percent-decoded it. This does
/// not touch the filesystem.
pub(crate) fn slug_segments_unsafe(slug: &str) -> bool {
    if slug.is_empty() || slug.contains('\\') || slug.contains('\0') {
        return true;
    }
    slug.split('/')
        .any(|seg| seg.is_empty() || seg == "." || seg == "..")
}

/// Read-only branch, tag, and recent-commit list for the compare picker.
/// POST is not mounted; axum answers it with 405.
async fn git_refs(State(state): State<Arc<ServerState>>) -> Response {
    let start = state.jail_root.clone().or_else(|| {
        state
            .order
            .first()
            .and_then(|slug| state.notebooks.get(slug))
            .and_then(|nb| nb.source_path.parent().map(|p| p.to_path_buf()))
    });
    let body = if !state.git_enabled {
        crate::gitdiff::refs_json_if(false, std::path::Path::new("."))
    } else {
        match &start {
            Some(dir) => crate::gitdiff::refs_json(dir),
            None => crate::gitdiff::refs_json(std::path::Path::new(".")),
        }
    };
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response()
}

/// Change counts for the menu's presets: `GET /git/changes?slug=<id>`.
/// Read-only like `/git/refs`, and "git diff is off" without spawning git
/// when git is off. `cm=0` counts with comments hidden, as the page does.
async fn git_changes(State(state): State<Arc<ServerState>>, req: Request) -> Response {
    let query = req.uri().query().unwrap_or("").to_string();
    let mut slug = None;
    let mut cm = None;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        match (it.next(), it.next()) {
            (Some("slug"), Some(v)) => slug = Some(crate::gitdiff::percent_decode(v)),
            (Some("cm"), Some(v)) => cm = Some(matches!(v, "1" | "on" | "true")),
            _ => {}
        }
    }
    let nb = slug.as_deref().and_then(|s| state.notebook(s));
    let body = match nb {
        _ if !state.git_enabled => crate::gitdiff::changes_json_if(
            false,
            std::path::Path::new("."),
            None,
            false,
        ),
        None => serde_json::json!({ "error": "notebook not found" }).to_string(),
        Some(nb) => {
            let comments_on = cm.unwrap_or(state.comments_on);
            let path = nb.source_path.clone();
            let jail = state.jail_root.clone();
            tokio::task::spawn_blocking(move || {
                crate::gitdiff::changes_json_if(true, &path, jail.as_deref(), comments_on)
            })
            .await
            .unwrap_or_else(|_| serde_json::json!({ "error": "count failed" }).to_string())
        }
    };
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response()
}

async fn notebook_page(
    State(state): State<Arc<ServerState>>,
    AxPath(path): AxPath<String>,
    req: Request,
) -> Response {
    let query = req.uri().query().unwrap_or("").to_string();
    if let Some(slug) = ws_slug(&path) {
        let upgrade = req
            .headers()
            .get(header::UPGRADE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|s| s.eq_ignore_ascii_case("websocket"));
        if upgrade {
            let (mut parts, _body) = req.into_parts();
            let headers = parts.headers.clone();
            match axum::extract::ws::WebSocketUpgrade::from_request_parts(&mut parts, &state).await
            {
                Ok(ws) => return ws::upgrade_socket(state, slug, headers, ws).await,
                Err(err) => return err.into_response(),
            }
        }
    }
    serve_notebook_path(state, path, &query).await
}

/// Live-reload socket: the notebook identity plus a trailing `/ws`.
///
/// `ch2/intro/ws` is the socket for `ch2/intro`. `ch2/intro/ws/ws` is
/// the socket for a notebook whose path is `ch2/intro/ws`. A path that
/// does not end in `/ws` is a page. A GET without `Upgrade: websocket`
/// falls through to the page lookup, so `ch2/intro/ws.md` stays reachable.
fn ws_slug(path: &str) -> Option<String> {
    let slug = path.strip_suffix("/ws")?;
    if slug.is_empty() || slug.ends_with('/') {
        None
    } else {
        Some(slug.to_string())
    }
}

/// Serve or redirect the notebook at a captured `/n/` path. Also used
/// when `/n/{slug}/ws` is fetched without a WebSocket upgrade, so a
/// public page whose path ends in `/ws` is not swallowed by the socket
/// route.
pub(super) async fn serve_notebook_path(
    state: Arc<ServerState>,
    path: String,
    query: &str,
) -> Response {
    match resolve_watch_page(&state, &path) {
        PageHit::Serve(slug) => match state.notebook(&slug) {
            Some(nb) => {
                // Nonces were stamped when this HTML was rendered.
                // A diff query stamps a copy; the stored HTML stays plain.
                let html = nb.html.read().await.clone();
                let parsed = crate::gitdiff::parse_query(query);
                let cli = crate::gitdiff::diff_default();
                let html = if state.git_enabled {
                    if let Some(compare) = crate::gitdiff::resolve_compare(&parsed, cli.as_deref())
                    {
                        let comments_on = parsed.cm.unwrap_or(state.comments_on);
                        crate::gitdiff::present(crate::gitdiff::Present {
                            html: &html,
                            source_path: &nb.source_path,
                            jail: state.jail_root.as_deref(),
                            comments_on,
                            compare: &compare,
                            source: parsed.source_view(),
                            link: Some(&state.link_mode_for(&slug)),
                        })
                    } else {
                        html
                    }
                } else {
                    html
                };
                html_response(&state, html)
            }
            None => (StatusCode::NOT_FOUND, "notebook not found").into_response(),
        },
        PageHit::Redirect(loc) => Redirect::temporary(&loc).into_response(),
        PageHit::Ambiguous(html) => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        )
            .into_response(),
        PageHit::Bad => (StatusCode::BAD_REQUEST, "bad path").into_response(),
        PageHit::Missing => (StatusCode::NOT_FOUND, "notebook not found").into_response(),
    }
}

fn html_response(state: &ServerState, html: String) -> Response {
    let csp = auth::csp_header(&state.csp_nonce);
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
            (
                header::HeaderName::from_static("content-security-policy"),
                csp,
            ),
        ],
        html,
    )
        .into_response()
}

/// Headers for a notebook file (`/plots`, `/raw`). The page CSP is not
/// used here: these responses are not the HTML document.
fn user_file_response(content_type: &'static str, body: Body) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::HeaderName::from_static("content-security-policy"),
                auth::USER_FILE_CSP,
            ),
            (
                header::HeaderName::from_static("x-content-type-options"),
                "nosniff",
            ),
        ],
        body,
    )
        .into_response()
}

/// `POST /save`, cell save, and `POST /annotate` (including undo) may
/// write only this notebook's `.md`, and only when that file resolves
/// inside the jail. A symlink whose target leaves the jail, or whose
/// target is not a `.md` file, is refused.
///
/// The returned path is the canonical target. Callers read and replace
/// that path, not the path the request named, so a symlink swap after
/// the check cannot retarget the write.
pub(crate) fn notebook_md_target(
    path: &std::path::Path,
    jail_root: Option<&std::path::Path>,
) -> Result<PathBuf, &'static str> {
    if !extension_is_md(path) {
        return Err("save only writes the notebook .md file");
    }
    let root = match jail_root {
        Some(root) => root.to_path_buf(),
        None => path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_path_buf())
            .ok_or("save only writes the notebook .md file")?,
    };
    let resolved = rustlab_script::path_jail::check_under_root(path, &root)
        .map_err(|_| "path escapes notebook directory")?;
    if !extension_is_md(&resolved) {
        return Err("save only writes the notebook .md file");
    }
    Ok(resolved)
}

/// Write `data` to a temp file in `path`'s directory, then `rename` it
/// over `path`. `rename` replaces the directory entry and does not follow
/// a symlink that appears at `path` after the jail check. The temp file
/// is removed if the rename fails.
pub(crate) async fn atomic_replace(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("notebook.md");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!(".{file_name}.{}.{nonce}.tmp", std::process::id()));
    if let Err(e) = tokio::fs::write(&tmp, data).await {
        return Err(e);
    }
    match tokio::fs::rename(&tmp, path).await {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            Err(e)
        }
    }
}

fn extension_is_md(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

async fn save_source(
    State(state): State<Arc<ServerState>>,
    AxPath(slug): AxPath<String>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let port = state.bind_port.load(std::sync::atomic::Ordering::Relaxed);
    // Origin is required (missing or non-loopback → 403). Host was
    // already checked by `require_loopback_host`.
    if auth::authorize_mutate(&headers, port).is_err() {
        let body = auth::origin_rejection_body(&headers, port);
        return (StatusCode::FORBIDDEN, body).into_response();
    }
    if slug_segments_unsafe(&slug) {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }

    let Some(nb) = state.notebook(&slug) else {
        return (StatusCode::NOT_FOUND, "notebook not found").into_response();
    };
    let _guard = nb.save_lock.lock().await;
    let target = match notebook_md_target(&nb.source_path, state.jail_root.as_deref()) {
        Ok(p) => p,
        Err(msg) => return (StatusCode::FORBIDDEN, msg).into_response(),
    };
    let previous = match tokio::fs::read_to_string(&target).await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("read error: {e}"),
            )
                .into_response();
        }
    };
    // SVG prose images are a real feature (diagrams copied and shown in
    // `<img>`). Rejecting every save that mentions one would break
    // `--editable` for notebooks that already have them. A *new*
    // reference is how a same-origin script would plant `![x](eval.svg)`
    // for the next render to copy and serve, so that step is refused.
    if crate::prose_media::introduces_svg_prose_image(&previous, &body) {
        return (
            StatusCode::BAD_REQUEST,
            crate::prose_media::SVG_PROSE_REJECTION,
        )
            .into_response();
    }
    match atomic_replace(&target, body.as_bytes()).await {
        Ok(()) => (StatusCode::OK, "saved").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("write error: {e}"),
        )
            .into_response(),
    }
}

async fn annotate_source(
    State(state): State<Arc<ServerState>>,
    AxPath(slug): AxPath<String>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let port = state.bind_port.load(std::sync::atomic::Ordering::Relaxed);
    if auth::authorize_mutate(&headers, port).is_err() {
        let msg = auth::origin_rejection_body(&headers, port);
        return (StatusCode::FORBIDDEN, msg).into_response();
    }
    if slug_segments_unsafe(&slug) {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }
    if body.len() > 64 * 1024 {
        return (StatusCode::BAD_REQUEST, "comment is too long").into_response();
    }
    let Some(nb) = state.notebook(&slug) else {
        return (StatusCode::NOT_FOUND, "notebook not found").into_response();
    };
    let _guard = nb.save_lock.lock().await;
    // Same target rule as `POST /save`, including when `jail_root` is
    // unset (the notebook's own directory is the jail). Read and replace
    // the canonical path from that check.
    let canon = match notebook_md_target(&nb.source_path, state.jail_root.as_deref()) {
        Ok(p) => p,
        Err(msg) => return (StatusCode::FORBIDDEN, msg).into_response(),
    };
    let file = match tokio::fs::read_to_string(&canon).await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("read error: {e}"),
            )
                .into_response();
        }
    };
    let want = crate::comments::sha256_hex(file.as_bytes());
    let Some(tag) = headers.get(header::IF_MATCH).and_then(|v| v.to_str().ok()) else {
        return (StatusCode::BAD_REQUEST, "If-Match required").into_response();
    };
    let tag = tag.trim().trim_matches('"').to_ascii_lowercase();
    if tag != want {
        return (StatusCode::CONFLICT, "source changed").into_response();
    }
    let v: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "bad json").into_response(),
    };
    let op = json_str(&v, "op");
    let target = json_str(&v, "target");
    let text = json_str(&v, "text");
    let comment = json_str(&v, "comment");
    let author = json_str(&v, "author");
    let id = json_str(&v, "id");
    let expect = json_str(&v, "expect");
    let today = crate::comments::utc_today();
    if op == "undo" {
        let before = {
            let slot = nb.undo.lock().unwrap_or_else(|e| e.into_inner());
            match slot.as_ref() {
                Some(snap) if snap.after_hash == want => snap.before.clone(),
                Some(_) => {
                    return (StatusCode::CONFLICT, "source changed").into_response();
                }
                None => {
                    return (StatusCode::BAD_REQUEST, "nothing to undo").into_response();
                }
            }
        };
        // Undo restores bytes the browser does not send. Those bytes can
        // put back an SVG prose image the current file no longer has
        // (deleting the only reference, then undoing). Same introduce-only
        // rule as a forward annotate.
        if crate::prose_media::introduces_svg_prose_image(&file, &before) {
            return (
                StatusCode::BAD_REQUEST,
                crate::prose_media::SVG_PROSE_REJECTION,
            )
                .into_response();
        }
        if before.len() > crate::comments::MAX_ANNOTATE_BYTES {
            return (StatusCode::BAD_REQUEST, "notebook is too large to annotate").into_response();
        }
        let hash = crate::comments::sha256_hex(before.as_bytes());
        return match atomic_replace(&canon, before.as_bytes()).await {
            Ok(()) => {
                *nb.undo.lock().unwrap_or_else(|e| e.into_inner()) = None;
                annotate_ok(&hash)
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("write error: {e}"),
            )
                .into_response(),
        };
    }
    let req = crate::comments::AnnotateRequest {
        op: &op,
        target: &target,
        start: v.get("start").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
        end: v.get("end").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
        text: &text,
        comment: &comment,
        author: &author,
        id: &id,
        expect: &expect,
        today: &today,
    };
    match crate::comments::apply_annotate(&file, &req) {
        Ok(next) => {
            // `==` / `%%` are not markdown, so a highlight or comment can
            // plant `![x](eval.svg)` (or a raw `<img src="….svg">`) that
            // the next render copies. Refuse only a dest the file does
            // not already reference. A mention inside a code fence or
            // inline code is not a prose image. Do not record undo: the
            // file is unchanged.
            if crate::prose_media::introduces_svg_prose_image(&file, &next) {
                return (
                    StatusCode::BAD_REQUEST,
                    crate::prose_media::SVG_PROSE_REJECTION,
                )
                    .into_response();
            }
            let hash = crate::comments::sha256_hex(next.as_bytes());
            match atomic_replace(&canon, next.as_bytes()).await {
                Ok(()) => {
                    *nb.undo.lock().unwrap_or_else(|e| e.into_inner()) = Some(UndoSnap {
                        before: file,
                        after_hash: hash.clone(),
                    });
                    annotate_ok(&hash)
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("write error: {e}"),
                )
                    .into_response(),
            }
        }
        Err(crate::comments::AnnotateError::BadRequest(msg)) => {
            (StatusCode::BAD_REQUEST, msg).into_response()
        }
        Err(crate::comments::AnnotateError::Conflict(msg)) => {
            (StatusCode::CONFLICT, msg).into_response()
        }
    }
}

fn annotate_ok(sha: &str) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        format!(r#"{{"sha256":"{sha}"}}"#),
    )
        .into_response()
}

fn json_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

async fn raw_source(
    State(state): State<Arc<ServerState>>,
    AxPath(slug): AxPath<String>,
) -> Response {
    if slug_segments_unsafe(&slug) {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }
    let Some(nb) = state.notebook(&slug) else {
        return (StatusCode::NOT_FOUND, "notebook not found").into_response();
    };
    match tokio::fs::read(&nb.source_path).await {
        Ok(bytes) => user_file_response("text/markdown; charset=utf-8", Body::from(bytes)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("read error: {e}"),
        )
            .into_response(),
    }
}

async fn asset(AxPath(path): AxPath<String>) -> Response {
    match assets::asset_for_path(&path) {
        // Vendored scripts and styles. `nosniff` only: a document CSP
        // here is not the page policy, and these files are not notebook
        // bytes. The page CSP still decides how they load.
        Some(a) => (
            [
                (header::CONTENT_TYPE, a.content_type),
                (
                    header::HeaderName::from_static("x-content-type-options"),
                    "nosniff",
                ),
            ],
            a.bytes,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "asset not found").into_response(),
    }
}

async fn plot(State(state): State<Arc<ServerState>>, AxPath(path): AxPath<String>) -> Response {
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
            user_file_response(ct, Body::from(bytes))
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
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("svg") => "image/svg+xml",
        Some("gif") => "image/gif",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
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
            link_slugs: HashMap::new(),
            single: true,
            theme: Theme::Dark.colors(),
            index_title: "nb".to_string(),
            index_body: tokio::sync::RwLock::new(String::new()),
            index_md_path: None,
            render_tx: std::sync::OnceLock::new(),
            csp_nonce: "testnonce".to_string(),
            bind_port: std::sync::atomic::AtomicU16::new(8042),
            jail_root: None,
            annotate: false,
            comments_on: true,
            git_enabled: false,
        })
    }

    /// Loopback Host for the port [`single_state`] records.
    fn host(builder: axum::http::request::Builder) -> axum::http::request::Builder {
        builder.header(header::HOST, "127.0.0.1:8042")
    }

    #[test]
    fn slugify_basics() {
        assert_eq!(slugify("Contour Plots"), "contour-plots");
        assert_eq!(slugify("a__b--c"), "a-b-c");
        assert_eq!(slugify("--trim--"), "trim");
        assert_eq!(slugify("***"), "notebook");
        assert_eq!(slugify("Already-Slug"), "already-slug");
    }

    #[test]
    fn ws_slug_keeps_directory_separators() {
        assert_eq!(ws_slug("a/intro/ws").as_deref(), Some("a/intro"));
        assert_eq!(ws_slug("intro/ws").as_deref(), Some("intro"));
        assert_eq!(ws_slug("a/intro/ws/ws").as_deref(), Some("a/intro/ws"));
        assert!(ws_slug("a/intro").is_none());
        assert!(ws_slug("ws").is_none());
        assert!(slug_segments_unsafe("a/../intro"));
        assert!(slug_segments_unsafe("a/./intro"));
        assert!(!slug_segments_unsafe("a/intro"));
    }

    #[tokio::test]
    async fn root_redirects_to_notebook_in_single_mode() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                host(axum::http::Request::builder().uri("/"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);
        let loc = res
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(loc, "/n/nb", "loc={loc}");
        assert!(!loc.contains("token="), "loc={loc}");
    }

    #[tokio::test]
    async fn notebook_page_serves_rendered_body() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                host(axum::http::Request::builder().uri("/n/nb"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        let s = String::from_utf8_lossy(&body);
        assert!(s.contains("<h1>hello</h1>"));
        assert!(!s.contains("__RL_TOKEN"));
        assert!(!s.contains("rl-token"));
    }

    #[tokio::test]
    async fn unknown_notebook_404s() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                host(axum::http::Request::builder().uri("/n/nope"))
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
                host(axum::http::Request::builder().uri("/raw/nb"))
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

    fn annotate_state(
        source: &str,
        annotate: bool,
        editable: bool,
        jail_root: Option<std::path::PathBuf>,
    ) -> (Arc<ServerState>, std::path::PathBuf) {
        let plot_dir = TempDir::new().unwrap();
        let src = plot_dir.path().join("nb.md");
        std::fs::write(&src, source).unwrap();
        let nb = Arc::new(Notebook::new(
            "nb".to_string(),
            src.clone(),
            "nb".to_string(),
            "<h1>hello</h1>".to_string(),
        ));
        let mut notebooks = HashMap::new();
        notebooks.insert("nb".to_string(), nb);
        let state = Arc::new(ServerState {
            notebooks,
            order: vec!["nb".to_string()],
            plot_dir,
            editable,
            link_slugs: HashMap::new(),
            single: true,
            theme: Theme::Dark.colors(),
            index_title: "nb".to_string(),
            index_body: tokio::sync::RwLock::new(String::new()),
            index_md_path: None,
            render_tx: std::sync::OnceLock::new(),
            csp_nonce: "testnonce".to_string(),
            bind_port: std::sync::atomic::AtomicU16::new(8042),
            jail_root,
            annotate,
            comments_on: true,
            git_enabled: false,
        });
        (state, src)
    }

    fn annotate_body() -> String {
        r#"{"op":"insert","target":"prose","start":8,"end":16,"text":"constant","comment":"why"}"#
            .to_string()
    }

    #[tokio::test]
    async fn annotate_route_absent_without_flag() {
        let (state, _) = annotate_state("# hello constant world\n", false, false, None);
        let app = router(state);
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/annotate/nb"),
                )
                .header("Origin", "http://127.0.0.1:8042")
                .body(Body::from(annotate_body()))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn annotate_route_mounted_for_annotate_and_for_editable() {
        for (annotate, editable) in [(true, false), (false, true)] {
            let (state, _) = annotate_state("# hello constant world\n", annotate, editable, None);
            let app = router(state);
            let res = app
                .oneshot(
                    host(
                        axum::http::Request::builder()
                            .method("POST")
                            .uri("/annotate/nb"),
                    )
                    .header("Origin", "http://127.0.0.1:8042")
                    .body(Body::from(annotate_body()))
                    .unwrap(),
                )
                .await
                .unwrap();
            assert_ne!(
                res.status(),
                StatusCode::NOT_FOUND,
                "annotate={annotate} editable={editable}"
            );
            assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn annotate_rejects_bad_host_and_origin() {
        let (state, src) = annotate_state("# hello constant world\n", true, false, None);
        let before = std::fs::read_to_string(&src).unwrap();
        let app = router(Arc::clone(&state));
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/annotate/nb")
                    .header(header::HOST, "evil.example:8042")
                    .header("Origin", "http://127.0.0.1:8042")
                    .body(Body::from(annotate_body()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        let app = router(Arc::clone(&state));
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/annotate/nb"),
                )
                .header("Origin", "http://evil.example")
                .body(Body::from(annotate_body()))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(std::fs::read_to_string(&src).unwrap(), before);
    }

    #[tokio::test]
    async fn annotate_rejects_a_path_outside_the_jail() {
        let jail = TempDir::new().unwrap();
        let (state, src) = annotate_state(
            "# hello constant world\n",
            true,
            false,
            Some(jail.path().to_path_buf()),
        );
        let before = std::fs::read_to_string(&src).unwrap();
        let app = router(Arc::clone(&state));
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/annotate/nb"),
                )
                .header("Origin", "http://127.0.0.1:8042")
                .header(header::IF_MATCH, "\"abc\"")
                .body(Body::from(annotate_body()))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(std::fs::read_to_string(&src).unwrap(), before);
    }

    #[tokio::test]
    async fn annotate_if_match_conflict_does_not_write() {
        let source = "# hello constant world\n";
        let (state, src) = annotate_state(source, true, false, None);
        let digest = crate::comments::sha256_hex(source.as_bytes());
        let app = router(Arc::clone(&state));
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/annotate/nb"),
                )
                .header("Origin", "http://127.0.0.1:8042")
                .header(
                    header::IF_MATCH,
                    "\"0000000000000000000000000000000000000000000000000000000000000000\"",
                )
                .body(Body::from(annotate_body()))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
        assert_eq!(std::fs::read_to_string(&src).unwrap(), source);

        let app = router(Arc::clone(&state));
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/annotate/nb"),
                )
                .header("Origin", "http://127.0.0.1:8042")
                .header(header::IF_MATCH, format!("\"{digest}\""))
                .body(Body::from(annotate_body()))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let written = std::fs::read_to_string(&src).unwrap();
        assert_eq!(
            body["sha256"].as_str().unwrap(),
            crate::comments::sha256_hex(written.as_bytes())
        );
        assert!(written.contains("==constant=="), "{written}");
        assert!(written.contains("why"), "{written}");
        assert_ne!(written, source);
    }

    #[tokio::test]
    async fn annotate_inside_a_fence_is_rejected() {
        let source = "# T\n\n```rustlab\na == b\ns = \"x %% y\"\n```\n\n~~~python\nprint(1)\n~~~\n";
        let (state, src) = annotate_state(source, true, false, None);
        let digest = crate::comments::sha256_hex(source.as_bytes());
        let start = source.find("a == b").unwrap();
        let body = format!(
            r#"{{"op":"insert","target":"prose","start":{start},"end":{},"text":"a == b","comment":"no"}}"#,
            start + "a == b".len()
        );
        let app = router(Arc::clone(&state));
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/annotate/nb"),
                )
                .header("Origin", "http://127.0.0.1:8042")
                .header(header::IF_MATCH, format!("\"{digest}\""))
                .body(Body::from(body))
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            res.status().is_client_error(),
            "fence target must be 4xx, got {}",
            res.status()
        );
        assert_eq!(std::fs::read_to_string(&src).unwrap(), source);

        let info = source.find("rustlab").unwrap();
        let body = format!(
            r#"{{"op":"insert","target":"prose","start":{info},"end":{},"text":"rustlab","comment":""}}"#,
            info + "rustlab".len()
        );
        let app = router(Arc::clone(&state));
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/annotate/nb"),
                )
                .header("Origin", "http://127.0.0.1:8042")
                .header(header::IF_MATCH, format!("\"{digest}\""))
                .body(Body::from(body))
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(res.status().is_client_error(), "{}", res.status());
        assert_eq!(std::fs::read_to_string(&src).unwrap(), source);
        drop(state);
    }

    #[tokio::test]
    async fn annotate_undo_restores_the_previous_bytes() {
        let source = "# hello constant world\n";
        let (state, src) = annotate_state(source, true, false, None);
        let digest = crate::comments::sha256_hex(source.as_bytes());
        let app = router(Arc::clone(&state));
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/annotate/nb"),
                )
                .header("Origin", "http://127.0.0.1:8042")
                .header(header::IF_MATCH, format!("\"{digest}\""))
                .body(Body::from(annotate_body()))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let next = body["sha256"].as_str().unwrap().to_string();
        assert_ne!(std::fs::read_to_string(&src).unwrap(), source);

        let app = router(Arc::clone(&state));
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/annotate/nb"),
                )
                .header("Origin", "http://127.0.0.1:8042")
                .header(header::IF_MATCH, format!("\"{next}\""))
                .body(Body::from(r#"{"op":"undo"}"#))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(std::fs::read_to_string(&src).unwrap(), source);
        drop(state);
    }

    async fn post_annotate(
        state: &Arc<ServerState>,
        if_match: &str,
        body: &str,
    ) -> axum::response::Response {
        router(Arc::clone(state))
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/annotate/nb"),
                )
                .header("Origin", "http://127.0.0.1:8042")
                .header(header::IF_MATCH, format!("\"{if_match}\""))
                .body(Body::from(body.to_string()))
                .unwrap(),
            )
            .await
            .unwrap()
    }

    fn prose_insert(start: usize, end: usize, text: &str, comment: &str) -> String {
        serde_json::json!({
            "op": "insert",
            "target": "prose",
            "start": start,
            "end": end,
            "text": text,
            "comment": comment,
        })
        .to_string()
    }

    #[tokio::test]
    async fn annotate_rejects_a_new_svg_prose_image_and_does_not_write() {
        let source = "# hello constant world\n";
        let digest = crate::comments::sha256_hex(source.as_bytes());
        let start = source.find("constant").unwrap();
        let end = start + "constant".len();
        // `==` / `%%` are not markdown, so the image in the comment (or a
        // highlight that pulls image syntax out of an HTML block) is a new
        // prose reference. Inline code is not.
        let attacks = [
            prose_insert(start, end, "constant", "![x](eval.svg)"),
            prose_insert(start, end, "constant", "![[eval.svg]]"),
            prose_insert(start, end, "constant", "<img src=\"eval.svg\" alt=\"x\">"),
            prose_insert(start, end, "constant", "![x][fig]\n\n[fig]: eval.svg"),
        ];
        for body in &attacks {
            let (state, src) = annotate_state(source, true, false, None);
            let res = post_annotate(&state, &digest, body).await;
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{body}");
            let text = to_bytes(res.into_body(), 8 * 1024).await.unwrap();
            let text = String::from_utf8_lossy(&text);
            assert!(
                text.contains(crate::prose_media::SVG_PROSE_REJECTION),
                "{text}"
            );
            assert_eq!(std::fs::read_to_string(&src).unwrap(), source);
        }

        // A one-line HTML block hides `![x](eval.svg)`. Wrapping the whole
        // line in `==` ends the block, so the result is a new prose image
        // (`==<div>![x](eval.svg)==`).
        let hidden = "<div>![x](eval.svg)\n";
        let digest = crate::comments::sha256_hex(hidden.as_bytes());
        let line = "<div>![x](eval.svg)";
        let (state, src) = annotate_state(hidden, true, false, None);
        let res = post_annotate(&state, &digest, &prose_insert(0, line.len(), line, "")).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let text = to_bytes(res.into_body(), 8 * 1024).await.unwrap();
        assert!(String::from_utf8_lossy(&text).contains(crate::prose_media::SVG_PROSE_REJECTION));
        assert_eq!(std::fs::read_to_string(&src).unwrap(), hidden);

        let fenced = "```rustlab\nx = 1\n```\n";
        let digest = crate::comments::sha256_hex(fenced.as_bytes());
        let at = fenced.find("```rustlab").unwrap();
        let (state, src) = annotate_state(fenced, true, false, None);
        let body = serde_json::json!({
            "op": "insert",
            "target": "cell",
            "start": at,
            "end": at,
            "text": "",
            "comment": "![x](eval.svg)",
        })
        .to_string();
        let res = post_annotate(&state, &digest, &body).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(std::fs::read_to_string(&src).unwrap(), fenced);
    }

    #[tokio::test]
    async fn annotate_allows_an_existing_svg_and_code_mentions() {
        let source = "# see ![x](eval.svg) please\n";
        let digest = crate::comments::sha256_hex(source.as_bytes());
        let needle = "![x](eval.svg)";
        let start = source.find(needle).unwrap();
        let (state, src) = annotate_state(source, true, false, None);
        let res = post_annotate(
            &state,
            &digest,
            &prose_insert(start, start + needle.len(), needle, ""),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let written = std::fs::read_to_string(&src).unwrap();
        assert!(
            written.contains("==![x](eval.svg)=="),
            "highlight of an existing figure stays: {written}"
        );

        let source = "# hello constant world\n";
        let digest = crate::comments::sha256_hex(source.as_bytes());
        let start = source.find("constant").unwrap();
        let end = start + "constant".len();
        let (state, src) = annotate_state(source, true, false, None);
        let res = post_annotate(
            &state,
            &digest,
            &prose_insert(start, end, "constant", "see `![x](eval.svg)` please"),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let written = std::fs::read_to_string(&src).unwrap();
        assert!(written.contains("`![x](eval.svg)`"), "{written}");
        assert!(!crate::prose_media::introduces_svg_prose_image(
            source, &written
        ));

        let source = "![kept](fig.svg)\n\n# hello constant world\n";
        let digest = crate::comments::sha256_hex(source.as_bytes());
        let start = source.find("constant").unwrap();
        let end = start + "constant".len();
        let (state, src) = annotate_state(source, true, false, None);
        let res = post_annotate(
            &state,
            &digest,
            &prose_insert(start, end, "constant", "again ![kept](fig.svg)"),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_ne!(std::fs::read_to_string(&src).unwrap(), source);

        let source = "# hello constant world\n";
        let digest = crate::comments::sha256_hex(source.as_bytes());
        let start = source.find("constant").unwrap();
        let end = start + "constant".len();
        let (state, src) = annotate_state(source, true, false, None);
        let res = post_annotate(
            &state,
            &digest,
            &prose_insert(start, end, "constant", "![photo](dot.png)"),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(std::fs::read_to_string(&src).unwrap().contains("dot.png"));
    }

    #[tokio::test]
    async fn annotate_undo_refuses_to_reintroduce_an_svg_prose_image() {
        let source = "# hello\n\n%%#c1: ![x](eval.svg)%%\n";
        let digest = crate::comments::sha256_hex(source.as_bytes());
        let expect = "%%#c1: ![x](eval.svg)%%";
        let start = source.find(expect).unwrap();
        let (state, src) = annotate_state(source, true, false, None);
        let body = serde_json::json!({
            "op": "delete",
            "target": "prose",
            "start": start,
            "end": start + expect.len(),
            "expect": expect,
        })
        .to_string();
        let res = post_annotate(&state, &digest, &body).await;
        assert_eq!(
            res.status(),
            StatusCode::OK,
            "removing the only SVG ref is allowed"
        );
        let bytes = to_bytes(res.into_body(), 1024).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let next = body_json["sha256"].as_str().unwrap().to_string();
        let removed = std::fs::read_to_string(&src).unwrap();
        assert!(!removed.contains("eval.svg"), "{removed}");

        let res = post_annotate(&state, &next, r#"{"op":"undo"}"#).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let text = to_bytes(res.into_body(), 8 * 1024).await.unwrap();
        assert!(String::from_utf8_lossy(&text).contains(crate::prose_media::SVG_PROSE_REJECTION));
        assert_eq!(std::fs::read_to_string(&src).unwrap(), removed);
    }

    fn annotate_nb(
        path: std::path::PathBuf,
        jail_root: Option<std::path::PathBuf>,
    ) -> Arc<ServerState> {
        let nb = Arc::new(Notebook::new(
            "nb".to_string(),
            path,
            "nb".to_string(),
            String::new(),
        ));
        let mut notebooks = HashMap::new();
        notebooks.insert("nb".to_string(), nb);
        Arc::new(ServerState {
            notebooks,
            order: vec!["nb".to_string()],
            plot_dir: TempDir::new().unwrap(),
            editable: false,
            single: true,
            theme: Theme::Dark.colors(),
            index_title: "nb".to_string(),
            link_slugs: HashMap::new(),
            index_body: tokio::sync::RwLock::new(String::new()),
            index_md_path: None,
            render_tx: std::sync::OnceLock::new(),
            csp_nonce: "testnonce".to_string(),
            bind_port: std::sync::atomic::AtomicU16::new(8042),
            jail_root,
            annotate: true,
            comments_on: true,
            git_enabled: false,
        })
    }

    #[tokio::test]
    async fn annotate_refuses_a_non_md_target_even_without_a_jail_root() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("eval.svg");
        let bytes = b"<svg></svg>";
        std::fs::write(&src, bytes).unwrap();
        let state = annotate_nb(src.clone(), None);
        let res = post_annotate(&state, "abc", &annotate_body()).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let text = to_bytes(res.into_body(), 1024).await.unwrap();
        assert!(String::from_utf8_lossy(&text).contains("save only writes the notebook .md file"));
        assert_eq!(std::fs::read(&src).unwrap(), bytes);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn annotate_refuses_a_symlink_that_leaves_the_jail_or_is_not_markdown() {
        let jail = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let secret = outside.path().join("secret.md");
        std::fs::write(&secret, "secret\n").unwrap();
        let link = jail.path().join("nb.md");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        // `jail_root` unset: the notebook directory is still the jail.
        let state = annotate_nb(link.clone(), None);
        let res = post_annotate(&state, "abc", &annotate_body()).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), "secret\n");

        let state = annotate_nb(link, Some(jail.path().to_path_buf()));
        let res = post_annotate(&state, "abc", &annotate_body()).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), "secret\n");

        let svg = jail.path().join("eval.svg");
        std::fs::write(&svg, b"<svg></svg>").unwrap();
        let alias = jail.path().join("alias.md");
        std::os::unix::fs::symlink(&svg, &alias).unwrap();
        let state = annotate_nb(alias, None);
        let res = post_annotate(&state, "abc", &annotate_body()).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let text = to_bytes(res.into_body(), 1024).await.unwrap();
        assert!(String::from_utf8_lossy(&text).contains("save only writes the notebook .md file"));
        assert_eq!(std::fs::read(&svg).unwrap(), b"<svg></svg>");
    }

    #[tokio::test]
    async fn save_route_absent_when_not_editable() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/save/nb"),
                )
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
            link_slugs: HashMap::new(),
            index_body: tokio::sync::RwLock::new(String::new()),
            index_md_path: None,
            render_tx: std::sync::OnceLock::new(),
            csp_nonce: "testnonce".to_string(),
            bind_port: std::sync::atomic::AtomicU16::new(8042),
            jail_root: None,
            annotate: false,
            comments_on: true,
            git_enabled: false,
        });
        let app = router(editable_state.clone());
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/save/nb"),
                )
                .header("Origin", "http://127.0.0.1:8042")
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
    async fn save_route_rejects_missing_origin() {
        let state = single_state("<h1>hello</h1>");
        let src = state.notebook("nb").unwrap().source_path.clone();
        let plot_dir = TempDir::new().unwrap();
        let nb = Arc::new(Notebook::new(
            "nb".to_string(),
            src,
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
            link_slugs: HashMap::new(),
            index_body: tokio::sync::RwLock::new(String::new()),
            index_md_path: None,
            render_tx: std::sync::OnceLock::new(),
            csp_nonce: "testnonce".to_string(),
            bind_port: std::sync::atomic::AtomicU16::new(8042),
            jail_root: None,
            annotate: false,
            comments_on: true,
            git_enabled: false,
        });
        let app = router(editable_state);
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/save/nb"),
                )
                .body(Body::from("# edited\n"))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn save_route_rejects_bad_origin() {
        let state = single_state("<h1>hello</h1>");
        let src = state.notebook("nb").unwrap().source_path.clone();
        let plot_dir = TempDir::new().unwrap();
        let nb = Arc::new(Notebook::new(
            "nb".to_string(),
            src,
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
            link_slugs: HashMap::new(),
            index_body: tokio::sync::RwLock::new(String::new()),
            index_md_path: None,
            render_tx: std::sync::OnceLock::new(),
            csp_nonce: "testnonce".to_string(),
            bind_port: std::sync::atomic::AtomicU16::new(8042),
            jail_root: None,
            annotate: false,
            comments_on: true,
            git_enabled: false,
        });
        let app = router(editable_state);
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/save/nb"),
                )
                .header("Origin", "http://evil.example")
                .body(Body::from("# edited\n"))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn notebook_page_sets_csp_header() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                host(axum::http::Request::builder().uri("/n/nb"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let csp = res
            .headers()
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(csp.contains("default-src 'self'"));
        assert!(csp.contains("nonce-testnonce"));
        assert!(!csp.contains("[::1]"));
        let body = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        let s = String::from_utf8_lossy(&body);
        assert!(s.contains("hello"));
        assert!(!s.contains("__RL_TOKEN"));
    }

    #[tokio::test]
    async fn get_rejects_bad_host() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/n/nb")
                    .header(header::HOST, "evil.example:8042")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    /// The CSP nonce must never be added to a `<script>` that arrived
    /// through content: serving is not allowed to post-process nonces
    /// in. Only the WS client the server injects on `/` carries one.
    #[tokio::test]
    async fn index_page_does_not_bless_content_scripts_with_nonce() {
        let plot_dir = TempDir::new().unwrap();
        let src = plot_dir.path().join("nb.md");
        std::fs::write(&src, "# nb\n").unwrap();
        let nb = Arc::new(Notebook::new(
            "nb".to_string(),
            src,
            "nb".to_string(),
            "<h1>hello</h1>".to_string(),
        ));
        let mut notebooks = HashMap::new();
        notebooks.insert("nb".to_string(), nb);
        let state = Arc::new(ServerState {
            notebooks,
            order: vec!["nb".to_string()],
            plot_dir,
            editable: false,
            link_slugs: HashMap::new(),
            single: false,
            theme: Theme::Dark.colors(),
            index_title: "Collection".to_string(),
            // Simulates a body that somehow still carries live markup —
            // the sanitiser normally prevents this at render time.
            index_body: tokio::sync::RwLock::new(
                "<p>x</p><script>window.__evil=1</script>".to_string(),
            ),
            index_md_path: None,
            render_tx: std::sync::OnceLock::new(),
            csp_nonce: "testnonce".to_string(),
            bind_port: std::sync::atomic::AtomicU16::new(8042),
            jail_root: None,
            annotate: false,
            comments_on: true,
            git_enabled: false,
        });
        let app = router(state);
        let res = app
            .oneshot(
                host(axum::http::Request::builder().uri("/"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), 256 * 1024).await.unwrap();
        let s = String::from_utf8_lossy(&body);
        assert!(
            s.contains("<script>window.__evil=1</script>"),
            "content script must be served exactly as stored, unnonced: {s:.300}"
        );
        assert!(
            s.contains("<script nonce=\"testnonce\">"),
            "server-injected WS client must carry the nonce"
        );
        assert_eq!(
            s.matches("nonce=\"testnonce\"").count(),
            1,
            "exactly one nonced script on the index page"
        );
    }

    #[tokio::test]
    async fn notebook_page_is_served_verbatim_with_csp() {
        // Rendered HTML is stored with nonces already stamped; serving
        // adds the header and nothing else.
        let app = router(single_state(
            "<script nonce=\"testnonce\">1</script><script>2</script>",
        ));
        let res = app
            .oneshot(
                host(axum::http::Request::builder().uri("/n/nb"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get("content-security-policy").is_some());
        let body = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        assert_eq!(
            body.as_ref(),
            b"<script nonce=\"testnonce\">1</script><script>2</script>"
        );
    }

    #[tokio::test]
    async fn get_rejects_missing_host() {
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
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn katex_css_served() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                host(axum::http::Request::builder().uri("/assets/katex/katex.min.css"))
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
                host(axum::http::Request::builder().uri("/plots/../Cargo.toml"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(res.status(), StatusCode::OK);
    }

    fn header_value(res: &axum::response::Response, name: &str) -> String {
        res.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    }

    fn assert_user_file_headers(res: &axum::response::Response, content_type_prefix: &str) {
        let ct = header_value(res, "content-type");
        assert!(
            ct.starts_with(content_type_prefix),
            "content-type {ct} does not start with {content_type_prefix}"
        );
        assert_eq!(
            header_value(res, "content-security-policy"),
            auth::USER_FILE_CSP
        );
        assert_eq!(header_value(res, "x-content-type-options"), "nosniff");
        assert!(header_value(res, "content-disposition").is_empty());
    }

    /// The PoC: `eval.svg` is a prose image the watch server copies into
    /// the plot tempdir and serves as `image/svg+xml`. Before the sandbox
    /// header, opening that URL (top-level navigation, or `<object>` /
    /// `<embed>` / `<iframe>`) ran the SVG script on the watch origin, so
    /// `fetch('/save/…')` sent a loopback `Origin` and bypassed the page
    /// CSP nonce. `<img src>` does not run SVG script; the regression is
    /// the response headers, which force an opaque origin and disable
    /// script even when the bytes still contain `<script>`.
    #[tokio::test]
    async fn svg_and_other_plot_files_are_sandboxed() {
        let state = single_state("<h1>hello</h1>");
        let root = state.plot_dir.path();
        let svg = root.join("nb/eval.svg");
        std::fs::create_dir_all(svg.parent().unwrap()).unwrap();
        std::fs::write(
            &svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><script>fetch("/save/nb",{method:"POST",headers:{"Content-Type":"text/markdown"},body:"pwned"})</script></svg>"#,
        )
        .unwrap();
        std::fs::write(root.join("nb/dot.png"), b"\x89PNG").unwrap();
        std::fs::write(root.join("nb/anim.gif"), b"GIF89a").unwrap();
        std::fs::write(root.join("nb/blob.bin"), b"not-an-image").unwrap();

        let cases = [
            ("/plots/nb/eval.svg", "image/svg+xml"),
            ("/plots/nb/dot.png", "image/png"),
            ("/plots/nb/anim.gif", "image/gif"),
            ("/plots/nb/blob.bin", "application/octet-stream"),
        ];
        for (uri, ct) in cases {
            let app = router(state.clone());
            let res = app
                .oneshot(
                    host(axum::http::Request::builder().uri(uri))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{uri}");
            assert_user_file_headers(&res, ct);
            if uri.ends_with(".svg") {
                let body = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
                let s = String::from_utf8_lossy(&body);
                assert!(s.contains("<script>"), "bytes are unchanged: {s}");
                assert!(s.contains("/save/nb"), "{s}");
            }
        }
    }

    #[tokio::test]
    async fn raw_markdown_is_sandboxed_like_other_notebook_files() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                host(axum::http::Request::builder().uri("/raw/nb"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_user_file_headers(&res, "text/markdown");
    }

    #[tokio::test]
    async fn html_page_keeps_its_nonce_csp() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                host(axum::http::Request::builder().uri("/n/nb"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let csp = header_value(&res, "content-security-policy");
        assert!(csp.contains("default-src 'self'"), "{csp}");
        assert!(csp.contains("nonce-testnonce"), "{csp}");
        assert!(!csp.contains("sandbox"), "{csp}");
        assert_ne!(csp, auth::USER_FILE_CSP);
    }

    #[tokio::test]
    async fn vendored_assets_are_nosniff_and_not_sandboxed() {
        for (uri, ct) in [
            ("/assets/katex/katex.min.css", "text/css"),
            ("/assets/katex/katex.min.js", "application/javascript"),
            ("/assets/katex/fonts/KaTeX_Main-Regular.woff2", "font/woff2"),
        ] {
            let app = router(single_state("<h1>hello</h1>"));
            let res = app
                .oneshot(
                    host(axum::http::Request::builder().uri(uri))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{uri}");
            let got = header_value(&res, "content-type");
            assert!(got.starts_with(ct), "{uri} content-type {got}");
            assert_eq!(header_value(&res, "x-content-type-options"), "nosniff");
            assert!(
                header_value(&res, "content-security-policy").is_empty(),
                "{uri} must not carry the document sandbox policy"
            );
        }
    }

    fn editable_state(initial_md: &str) -> (Arc<ServerState>, PathBuf) {
        let plot_dir = TempDir::new().unwrap();
        let src = plot_dir.path().join("nb.md");
        std::fs::write(&src, initial_md).unwrap();
        let nb = Arc::new(Notebook::new(
            "nb".to_string(),
            src.clone(),
            "nb".to_string(),
            "<h1>hello</h1>".to_string(),
        ));
        let mut notebooks = HashMap::new();
        notebooks.insert("nb".to_string(), nb);
        let state = Arc::new(ServerState {
            notebooks,
            order: vec!["nb".to_string()],
            plot_dir,
            editable: true,
            single: true,
            theme: Theme::Dark.colors(),
            index_title: "nb".to_string(),
            link_slugs: HashMap::new(),
            index_body: tokio::sync::RwLock::new(String::new()),
            index_md_path: None,
            render_tx: std::sync::OnceLock::new(),
            csp_nonce: "testnonce".to_string(),
            bind_port: std::sync::atomic::AtomicU16::new(8042),
            jail_root: None,
            annotate: false,
            comments_on: true,
            git_enabled: false,
        });
        (state, src)
    }

    async fn post_save(state: &Arc<ServerState>, body: &str) -> axum::response::Response {
        // The caller keeps `state` so the plot tempdir (and the notebook
        // file inside it) outlives the request.
        router(Arc::clone(state))
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/save/nb"),
                )
                .header("Origin", "http://127.0.0.1:8042")
                .body(Body::from(body.to_string()))
                .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn save_rejects_a_new_svg_prose_image_and_does_not_write() {
        let original = "# notes\n\n![kept](fig.svg)\n";
        let attacks = [
            "![x](eval.svg)\n",
            "![[eval.svg]]\n",
            "<img src=\"eval.svg\" alt=\"x\">\n",
            "![x][fig]\n\n[fig]: eval.svg\n",
            "![kept](fig.svg)\n\n![new](other.svg)\n",
        ];
        for body in attacks {
            let (state, src) = editable_state(original);
            let res = post_save(&state, body).await;
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{body}");
            let text = to_bytes(res.into_body(), 8 * 1024).await.unwrap();
            let text = String::from_utf8_lossy(&text);
            assert!(
                text.contains(crate::prose_media::SVG_PROSE_REJECTION),
                "{text}"
            );
            assert_eq!(std::fs::read_to_string(&src).unwrap(), original);
        }
    }

    #[tokio::test]
    async fn save_allows_edits_that_keep_an_existing_svg_figure() {
        let original = "# notes\n\n![diagram](fig.svg)\n";
        let (state, src) = editable_state(original);
        let next = "# notes\n\nMore prose.\n\n![diagram](fig.svg)\n\n![photo](dot.png)\n";
        let res = post_save(&state, next).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(std::fs::read_to_string(&src).unwrap(), next);
    }

    #[tokio::test]
    async fn save_allows_an_svg_mention_inside_a_code_fence() {
        let (state, src) = editable_state("# notes\n");
        let next = "# notes\n\n```\n![x](eval.svg)\n```\n";
        let res = post_save(&state, next).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(std::fs::read_to_string(&src).unwrap(), next);
    }

    #[tokio::test]
    async fn save_refuses_a_non_md_target() {
        let plot_dir = TempDir::new().unwrap();
        let src = plot_dir.path().join("eval.svg");
        let bytes = b"<svg></svg>";
        std::fs::write(&src, bytes).unwrap();
        let nb = Arc::new(Notebook::new(
            "nb".to_string(),
            src.clone(),
            "nb".to_string(),
            String::new(),
        ));
        let mut notebooks = HashMap::new();
        notebooks.insert("nb".to_string(), nb);
        let state = Arc::new(ServerState {
            notebooks,
            order: vec!["nb".to_string()],
            plot_dir,
            editable: true,
            single: true,
            theme: Theme::Dark.colors(),
            index_title: "nb".to_string(),
            link_slugs: HashMap::new(),
            index_body: tokio::sync::RwLock::new(String::new()),
            index_md_path: None,
            render_tx: std::sync::OnceLock::new(),
            csp_nonce: "testnonce".to_string(),
            bind_port: std::sync::atomic::AtomicU16::new(8042),
            jail_root: None,
            annotate: false,
            comments_on: true,
            git_enabled: false,
        });
        let res = post_save(&state, "# not an svg\n").await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(std::fs::read(&src).unwrap(), bytes);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn save_refuses_a_symlink_that_leaves_the_jail_or_is_not_markdown() {
        let jail = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let secret = outside.path().join("secret.md");
        std::fs::write(&secret, "secret\n").unwrap();
        let link = jail.path().join("nb.md");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        let nb = Arc::new(Notebook::new(
            "nb".to_string(),
            link.clone(),
            "nb".to_string(),
            String::new(),
        ));
        let mut notebooks = HashMap::new();
        notebooks.insert("nb".to_string(), nb);
        let state = Arc::new(ServerState {
            notebooks,
            order: vec!["nb".to_string()],
            plot_dir: TempDir::new().unwrap(),
            editable: true,
            single: true,
            theme: Theme::Dark.colors(),
            index_title: "nb".to_string(),
            link_slugs: HashMap::new(),
            index_body: tokio::sync::RwLock::new(String::new()),
            index_md_path: None,
            render_tx: std::sync::OnceLock::new(),
            csp_nonce: "testnonce".to_string(),
            bind_port: std::sync::atomic::AtomicU16::new(8042),
            jail_root: Some(jail.path().to_path_buf()),
            annotate: false,
            comments_on: true,
            git_enabled: false,
        });
        let res = post_save(&state, "# pwn\n").await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), "secret\n");

        let svg = jail.path().join("eval.svg");
        std::fs::write(&svg, "<svg></svg>").unwrap();
        let alias = jail.path().join("alias.md");
        std::os::unix::fs::symlink(&svg, &alias).unwrap();
        assert!(
            notebook_md_target(&alias, Some(jail.path())).is_err(),
            "a .md symlink to an svg must not be a write target"
        );
        assert_eq!(std::fs::read(&svg).unwrap(), b"<svg></svg>");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn atomic_replace_writes_the_canonical_file() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.md");
        std::fs::write(&real, "old\n").unwrap();
        let link = dir.path().join("link.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let canon = notebook_md_target(&link, Some(dir.path())).unwrap();
        assert_eq!(canon, std::fs::canonicalize(&real).unwrap());
        atomic_replace(&canon, b"new\n").await.unwrap();
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new\n");
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        let temps: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(temps.is_empty(), "{temps:?}");
    }

    #[tokio::test]
    async fn post_git_refs_is_not_a_route() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/git/refs"),
                )
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn git_changes_is_get_only_and_off_without_git() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .clone()
            .oneshot(
                host(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/git/changes?slug=nb"),
                )
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
        let before = crate::gitdiff::git_spawns();
        let res = app
            .oneshot(
                host(axum::http::Request::builder().uri("/git/changes?slug=nb"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 1 << 16).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("git diff is off"), "{body}");
        assert_eq!(crate::gitdiff::git_spawns(), before, "git ran while off");
    }

    #[tokio::test]
    async fn post_diff_is_not_a_route() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                host(axum::http::Request::builder().method("POST").uri("/diff"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn unknown_slug_does_not_invent_a_diff_page() {
        let app = router(single_state("<h1>hello</h1>"));
        let res = app
            .oneshot(
                host(axum::http::Request::builder().uri("/n/nope?diff=HEAD&base=main"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn git_refs_lists_current_branch_first() {
        if !std::process::Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            eprintln!("SKIP git refs route: git not found");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let st = std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .status()
                .unwrap();
            assert!(st.success(), "{args:?}");
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "Test"]);
        let src = dir.path().join("nb.md");
        std::fs::write(&src, "# nb\n").unwrap();
        git(&["add", "nb.md"]);
        git(&["commit", "-m", "init"]);
        git(&["checkout", "-b", "topic"]);
        let plot_dir = TempDir::new().unwrap();
        let nb = Arc::new(Notebook::new(
            "nb".into(),
            src,
            "nb".into(),
            "<h1>hello</h1>".into(),
        ));
        let mut notebooks = HashMap::new();
        notebooks.insert("nb".into(), nb);
        let state = Arc::new(ServerState {
            notebooks,
            order: vec!["nb".into()],
            plot_dir,
            editable: false,
            link_slugs: HashMap::new(),
            single: true,
            theme: Theme::Dark.colors(),
            index_title: "nb".into(),
            index_body: tokio::sync::RwLock::new(String::new()),
            index_md_path: None,
            render_tx: std::sync::OnceLock::new(),
            csp_nonce: "testnonce".into(),
            bind_port: std::sync::atomic::AtomicU16::new(8042),
            jail_root: Some(dir.path().to_path_buf()),
            annotate: false,
            comments_on: true,
            git_enabled: true,
        });
        let app = router(state);
        let res = app
            .oneshot(
                host(axum::http::Request::builder().uri("/git/refs"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["error"].is_null(), "{body}");
        let names: Vec<&str> = v["branches"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b["name"].as_str())
            .collect();
        assert_eq!(names.first().copied(), Some("topic"), "{body}");
        assert!(names.contains(&"main"), "{body}");
    }

    #[tokio::test]
    async fn two_ref_page_does_not_keep_executed_output() {
        if !std::process::Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            eprintln!("SKIP two-ref page: git not found");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let st = std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .status()
                .unwrap();
            assert!(st.success(), "{args:?}");
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "Test"]);
        let src = dir.path().join("nb.md");
        std::fs::write(&src, "# One\n").unwrap();
        git(&["add", "nb.md"]);
        git(&["commit", "-m", "one"]);
        git(&["checkout", "-b", "topic"]);
        std::fs::write(&src, "# Two\n\n```rustlab\nx = 1\n```\n").unwrap();
        git(&["add", "nb.md"]);
        git(&["commit", "-m", "two"]);
        let html = "<body><main><section class=\"rl-block\" id=\"b-1\"><pre class=\"output\">EXECUTED</pre><div class=\"plot-container\"></div></section></main></body>";
        let plot_dir = TempDir::new().unwrap();
        let nb = Arc::new(Notebook::new("nb".into(), src, "nb".into(), html.into()));
        let mut notebooks = HashMap::new();
        notebooks.insert("nb".into(), nb);
        let state = Arc::new(ServerState {
            notebooks,
            order: vec!["nb".into()],
            plot_dir,
            editable: false,
            link_slugs: HashMap::new(),
            single: true,
            theme: Theme::Dark.colors(),
            index_title: "nb".into(),
            index_body: tokio::sync::RwLock::new(String::new()),
            index_md_path: None,
            render_tx: std::sync::OnceLock::new(),
            csp_nonce: "testnonce".into(),
            bind_port: std::sync::atomic::AtomicU16::new(8042),
            jail_root: Some(dir.path().to_path_buf()),
            annotate: false,
            comments_on: true,
            git_enabled: true,
        });
        let app = router(state);
        let res = app
            .oneshot(
                host(axum::http::Request::builder().uri("/n/nb?diff=main&base=topic"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), 256 * 1024).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(body.contains("not run"), "{body}");
        assert!(!body.contains("EXECUTED"), "{body}");
        assert!(!body.contains("plot-container"), "{body}");
    }
}
