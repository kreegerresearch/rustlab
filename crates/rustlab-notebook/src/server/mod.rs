//! Interactive `notebook watch` server — Phase 1 skeleton.
//!
//! Replaces the read-only `cmd_check` fallback that bare
//! `notebook watch <file>` runs today (see `watch.rs::cmd_watch`)
//! with a local web server that renders the notebook to an HTML page,
//! serves embedded KaTeX + Plotly assets locally, and (in Phase 2)
//! will push re-renders to the browser on save.
//!
//! Design decisions live in `dev/plans/notebook_interactive_server.md`
//! § "Locked-in design decisions"; the dep-stack rationale lives in the
//! companion trade-off doc.
//!
//! Phase 1 scope: one-shot render at startup; no re-render on save
//! yet. Source `.md` is never modified.

pub mod assets;
pub mod diff;
pub mod http;
pub mod render_loop;
pub mod ws;

use anyhow::{Context, Result};
use rustlab_plot::ThemeColors;
use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

/// Default port. Auto-increments up to [`MAX_PORT_ATTEMPTS`] when not
/// explicitly set by the user (per locked-in #12). Reference: the
/// answer is 42.
pub const DEFAULT_PORT: u16 = 8042;

/// Maximum bind attempts when auto-incrementing from the default port.
pub const MAX_PORT_ATTEMPTS: u16 = 10;

/// Caller-facing knobs for [`start`].
#[derive(Debug, Clone, Default)]
pub struct ServerOpts {
    /// Port to bind. `None` means use [`DEFAULT_PORT`] with
    /// auto-increment up to [`MAX_PORT_ATTEMPTS`]; `Some(N)` is
    /// explicit and fails loud on `EADDRINUSE`.
    pub port: Option<u16>,
    /// When true, never auto-open the browser even on a TTY.
    pub no_browser: bool,
    /// When true, mount the `/save/{slug}` write-back route and serve the
    /// in-browser editor. This is the one interactive path that modifies
    /// source `.md` files (parallels the "only `--obsidian` modifies"
    /// rule), so it is strictly opt-in.
    pub editable: bool,
}

/// Start the interactive server against `input` and block until
/// Ctrl-C (or unrecoverable error). The initial render runs once on
/// the calling thread before the tokio runtime spins up; the
/// runtime serves the page, accepts WebSocket connections, and
/// (Phase 2) drives the render coordinator that re-renders on save.
pub fn start(input: &Path, theme: &'static ThemeColors, opts: ServerOpts) -> Result<()> {
    let canonical_input = std::fs::canonicalize(input)
        .with_context(|| format!("resolving {}", input.display()))?;
    let is_dir = canonical_input.is_dir();

    // ── 1+2. Discover + render every notebook, build server state ──
    let state = build_state(&canonical_input, is_dir, theme, opts.editable)?;

    // ── 3. Bind ───────────────────────────────────────────────────
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building current-thread tokio runtime")?;

    rt.block_on(async move {
        let (listener, addr) = bind_with_policy(opts.port).await?;
        let url = format!("http://{addr}");

        // ── 4. Log + open browser ─────────────────────────────────
        log_bind(&url, opts.port);
        if is_dir {
            eprintln!(
                "[watch] serving {} notebook{} from {}",
                state.order.len(),
                if state.order.len() == 1 { "" } else { "s" },
                canonical_input.display(),
            );
        }
        if opts.editable {
            eprintln!("[watch] --editable: in-browser edits write back to source .md");
        }

        if !opts.no_browser && should_auto_open_browser() {
            if let Err(e) = open_browser(&url) {
                eprintln!("[watch] could not open browser automatically: {e}");
                eprintln!("[watch] open {url} manually");
            }
        }

        // ── 5. Spawn fs watcher + render coordinator ──────────────
        // `_watcher` is held to keep the notify watcher alive; the
        // task handle is dropped on shutdown. The watcher covers the
        // directory (dir mode) or the single file's parent (file mode).
        let (_watcher, _coord) =
            render_loop::spawn(&canonical_input, is_dir, theme, state.clone())
                .context("spawning render coordinator")?;

        // ── 6. Serve until Ctrl-C ─────────────────────────────────
        let app = http::router(state.clone());
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .context("axum::serve failed")?;
        eprintln!("[watch] shutting down");
        Ok::<_, anyhow::Error>(())
    })
}

/// Discover notebooks under `canonical_input` (one file, or every `.md`
/// under a directory), render each once with a unique slug, and assemble
/// the [`http::ServerState`]. Extracted from [`start`] so directory-mode
/// wiring can be unit-tested without binding a socket.
fn build_state(
    canonical_input: &Path,
    is_dir: bool,
    theme: &'static ThemeColors,
    editable: bool,
) -> Result<Arc<http::ServerState>> {
    let sources: Vec<PathBuf> = if is_dir {
        let files = crate::list_md_files_recursive(canonical_input);
        if files.is_empty() {
            anyhow::bail!("no .md notebooks found in {}", canonical_input.display());
        }
        files
    } else {
        vec![canonical_input.to_path_buf()]
    };

    let plot_tempdir = TempDir::new().context("creating tempdir for served plot artefacts")?;
    let mut notebooks: HashMap<String, Arc<http::Notebook>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut used: HashSet<String> = HashSet::new();

    for path in &sources {
        let slug = unique_slug(path, &mut used);
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let title = crate::extract_title(&source, &path.to_path_buf());
        let html = render_for_server(path, theme, plot_tempdir.path(), &slug)
            .with_context(|| format!("rendering {} for server", path.display()))?;
        let nb = Arc::new(http::Notebook::new(slug.clone(), path.clone(), title, html));
        notebooks.insert(slug.clone(), nb);
        order.push(slug);
    }

    let index_title = if is_dir {
        canonical_input
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Notebooks".to_string())
    } else {
        notebooks
            .get(&order[0])
            .map(|nb| nb.title.clone())
            .unwrap_or_default()
    };

    Ok(Arc::new(http::ServerState {
        notebooks,
        order,
        plot_dir: plot_tempdir,
        editable,
        single: !is_dir,
        theme,
        index_title,
    }))
}

/// Derive a unique, URL-safe slug for `path`, deduping collisions with a
/// `-N` suffix. Mutates `used` to record the chosen slug.
fn unique_slug(path: &Path, used: &mut HashSet<String>) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let base = http::slugify(&stem);
    if used.insert(base.clone()) {
        return base;
    }
    let mut n = 2;
    loop {
        let cand = format!("{base}-{n}");
        if used.insert(cand.clone()) {
            return cand;
        }
        n += 1;
    }
}

/// Re-implementation of the render pipeline in `lib::cmd_render` minus
/// the disk-output step: read input → strip artefacts → expand embeds
/// → parse → execute → render to HTML → swap CDN URLs to local
/// `/assets/` → inject the WS-client live-reload script into
/// `<head>`. Animations land in `plot_dir`; the server serves them
/// from there at `/plots/<filename>`.
///
/// Public to the server module so [`render_loop`] can re-invoke it
/// on save without duplicating the pipeline.
///
/// `plot_root` is the shared tempdir; plot artefacts for this notebook
/// land in `plot_root/<slug>/` and are served at `/plots/<slug>/…`, so
/// directory mode keeps each notebook's plots separate.
pub(super) fn render_for_server(
    input: &Path,
    theme: &ThemeColors,
    plot_root: &Path,
    slug: &str,
) -> Result<String> {
    use crate::{embed, execute, parse, render};

    let source = std::fs::read_to_string(input)
        .with_context(|| format!("reading {}", input.display()))?;
    let source = crate::strip_render_artifacts(&source);

    let host_dir = input
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let title = crate::extract_title(&source, &input.to_path_buf());
    let expanded = embed::expand_embeds(&source, &host_dir, &host_dir);
    let blocks = parse::parse_notebook(&expanded);
    let rendered = execute::execute_notebook(&blocks);

    let plot_dir = plot_root.join(slug);
    let plot_href = format!("/plots/{slug}");
    let html = render::render_html(&title, &rendered, &plot_dir, &plot_href, theme, None);
    let html = assets::rewrite_cdn_urls(&html);
    let html = ws::inject_ws_client(&html);
    Ok(html)
}

/// Bind 127.0.0.1 on either the explicit user port (fail loud) or
/// the default with auto-increment up to 10 attempts (per
/// locked-in #12).
async fn bind_with_policy(
    port: Option<u16>,
) -> Result<(tokio::net::TcpListener, SocketAddr)> {
    let loopback = Ipv4Addr::LOCALHOST;

    // Explicit --port: try once, fail loud with a hint.
    if let Some(p) = port {
        let addr = SocketAddr::from((loopback, p));
        let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| {
            format!(
                "port {p} already in use\n\
                 hint: another rustlab-notebook server may be running, or\n\
                       choose a different port with --port <N>",
            )
        })?;
        return Ok((listener, addr));
    }

    // Default port: try DEFAULT_PORT..DEFAULT_PORT+MAX_PORT_ATTEMPTS.
    let mut last_err: Option<std::io::Error> = None;
    for offset in 0..MAX_PORT_ATTEMPTS {
        let p = DEFAULT_PORT + offset;
        let addr = SocketAddr::from((loopback, p));
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                if offset > 0 {
                    eprintln!(
                        "[watch] port {DEFAULT_PORT} busy through {prev}, bound on {p}",
                        prev = DEFAULT_PORT + offset - 1
                    );
                }
                return Ok((listener, addr));
            }
            Err(e) => {
                if offset + 1 < MAX_PORT_ATTEMPTS {
                    eprintln!("[watch] port {p} busy, trying {next}…", next = p + 1);
                }
                last_err = Some(e);
            }
        }
    }
    let last = last_err.expect("loop must have run at least once");
    anyhow::bail!(
        "could not bind any port in {start}..{end} ({err})\n\
         hint: free up the range or pass --port <N>",
        start = DEFAULT_PORT,
        end = DEFAULT_PORT + MAX_PORT_ATTEMPTS,
        err = last,
    )
}

fn log_bind(url: &str, explicit_port: Option<u16>) {
    if explicit_port.is_some() {
        eprintln!("[watch] listening on {url}");
    } else {
        eprintln!("[watch] listening on {url}  (Ctrl-C to stop)");
    }
}

/// Phase-1 policy (per locked-in #8): auto-open only when stderr is a
/// TTY and `CI` is unset. Tests, CI, and pipe redirects never auto-open.
fn should_auto_open_browser() -> bool {
    if std::env::var_os("CI").is_some() {
        return false;
    }
    std::io::stderr().is_terminal()
}

/// Shell out to the platform's URL opener. Errors propagate so the
/// caller can log a hint instead of failing the server.
///
/// On Linux/WSL there is no single canonical opener, so we try a list
/// in order and use the first that is present on PATH and exits 0:
///
/// - **`wslview`** (from `wslu`) — only attempted under WSL, where it
///   launches the *Windows* host browser. `xdg-open` is usually absent
///   or useless there, so trying it first is what makes WSL work.
/// - **`xdg-open`** — the standard freedesktop opener.
/// - **`gio open`** / **`sensible-browser`** — fallbacks for setups
///   that ship one but not the other.
///
/// A "binary not found" spawn error is treated as "try the next
/// candidate", not a failure. If every candidate is missing or errors,
/// the overall call fails and the caller prints the manual-open hint.
fn open_browser(url: &str) -> Result<()> {
    let candidates: Vec<(&str, Vec<&str>)> = if cfg!(target_os = "macos") {
        vec![("open", vec![url])]
    } else if cfg!(target_os = "windows") {
        // `cmd /c start "" <url>` — the empty "" is start's required
        // window-title arg, otherwise start treats <url> as the title.
        vec![("cmd", vec!["/c", "start", "", url])]
    } else {
        let mut c: Vec<(&str, Vec<&str>)> = Vec::new();
        if is_wsl() {
            c.push(("wslview", vec![url]));
        }
        c.push(("xdg-open", vec![url]));
        c.push(("gio", vec!["open", url]));
        c.push(("sensible-browser", vec![url]));
        c
    };

    let mut last_err: Option<anyhow::Error> = None;
    for (cmd, args) in &candidates {
        match try_open(cmd, args) {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no URL opener available")))
}

/// Spawn one opener candidate. Returns `Err` if the binary is missing
/// (so the caller falls through to the next candidate) or exits non-zero.
fn try_open(cmd: &str, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("spawning `{cmd}`"))?;
    if !status.success() {
        anyhow::bail!("`{cmd}` exited with {status}");
    }
    Ok(())
}

/// Best-effort WSL detection. WSL2 sets `WSL_DISTRO_NAME` and
/// `WSL_INTEROP`; either is sufficient. Cheap env check — no file IO.
fn is_wsl() -> bool {
    std::env::var_os("WSL_DISTRO_NAME").is_some() || std::env::var_os("WSL_INTEROP").is_some()
}

/// Resolve on Ctrl-C. Used as axum's graceful-shutdown trigger.
async fn shutdown_signal() {
    if let Err(e) = tokio::signal::ctrl_c().await {
        eprintln!("[watch] failed to install Ctrl-C handler: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use rustlab_plot::Theme;
    use tower::util::ServiceExt;

    #[test]
    fn default_opts_are_sensible() {
        let o = ServerOpts::default();
        assert!(o.port.is_none());
        assert!(!o.no_browser);
        assert!(!o.editable);
    }

    #[test]
    fn unique_slug_dedupes_collisions() {
        let mut used = HashSet::new();
        // Two different dirs, same stem → distinct slugs.
        let a = unique_slug(Path::new("/x/intro.md"), &mut used);
        let b = unique_slug(Path::new("/y/intro.md"), &mut used);
        assert_eq!(a, "intro");
        assert_eq!(b, "intro-2");
    }

    #[tokio::test]
    async fn directory_mode_builds_index_and_serves_each_notebook() {
        let theme: &'static _ = Theme::Dark.colors();
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("alpha.md"), "# Alpha\n\nfirst.\n").unwrap();
        std::fs::write(dir.path().join("beta.md"), "# Beta\n\nsecond.\n").unwrap();
        let canon = std::fs::canonicalize(dir.path()).unwrap();

        let state = build_state(&canon, true, theme, false).unwrap();
        assert!(!state.single, "directory mode is not single");
        assert_eq!(state.order.len(), 2);
        assert!(state.notebook("alpha").is_some());
        assert!(state.notebook("beta").is_some());

        let app = http::router(state);

        // `/` is the index listing both notebooks (no redirect).
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("Alpha"), "index missing Alpha: {html:.400}");
        assert!(html.contains("Beta"), "index missing Beta");
        assert!(html.contains("n/alpha") && html.contains("n/beta"), "index links missing");

        // Each notebook page renders.
        for slug in ["alpha", "beta"] {
            let res = app
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .uri(format!("/n/{slug}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), axum::http::StatusCode::OK, "{slug} page");
        }
    }

    #[tokio::test]
    async fn directory_mode_editing_one_notebook_broadcasts_only_to_it() {
        let theme: &'static _ = Theme::Dark.colors();
        let dir = TempDir::new().unwrap();
        let alpha = dir.path().join("alpha.md");
        let beta = dir.path().join("beta.md");
        std::fs::write(&alpha, "# Alpha\n\nfirst.\n").unwrap();
        std::fs::write(&beta, "# Beta\n\nsecond.\n").unwrap();
        let canon = std::fs::canonicalize(dir.path()).unwrap();

        let state = build_state(&canon, true, theme, false).unwrap();
        let nb_alpha = state.notebook("alpha").unwrap().clone();
        let nb_beta = state.notebook("beta").unwrap().clone();
        let mut sub_alpha = nb_alpha.broadcast.subscribe();
        let mut sub_beta = nb_beta.broadcast.subscribe();

        let (_watcher, _coord) = render_loop::spawn(&canon, true, theme, state.clone()).unwrap();

        // Edit only alpha.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        std::fs::write(&alpha, "# Alpha\n\nfirst EDITED_ALPHA.\n").unwrap();

        // Alpha's channel gets a message…
        let got = tokio::time::timeout(std::time::Duration::from_secs(10), sub_alpha.recv())
            .await
            .expect("alpha broadcast timed out")
            .expect("alpha channel closed");
        assert!(got.contains("EDITED_ALPHA"), "alpha did not carry the edit");

        // …and beta's channel stays silent (nothing within a short window).
        let beta_quiet =
            tokio::time::timeout(std::time::Duration::from_millis(400), sub_beta.recv()).await;
        assert!(beta_quiet.is_err(), "beta should not have broadcast");
    }

    #[tokio::test]
    async fn explicit_port_fails_loud_on_collision() {
        // Hold the port, then ask for it explicitly — must error.
        let held = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let p = held.local_addr().unwrap().port();
        let err = bind_with_policy(Some(p)).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains(&format!("port {p}")), "expected port mention in: {msg}");
    }

    #[tokio::test]
    async fn default_port_auto_increments() {
        // Hold DEFAULT_PORT so the default policy is forced to bump.
        // If the port is already in use by something else on this
        // machine (rare in CI), the test is still valid — the policy
        // either succeeds on a later attempt or errors after 10.
        let _held = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, DEFAULT_PORT))
            .await
            .ok();
        let (listener, addr) = bind_with_policy(None).await.expect("auto-bump failed");
        if _held.is_some() {
            assert_ne!(addr.port(), DEFAULT_PORT, "should have bumped past held port");
        }
        drop(listener);
    }
}
