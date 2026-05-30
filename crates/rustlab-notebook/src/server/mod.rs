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
pub mod http;
pub mod render_loop;
pub mod ws;

use anyhow::{Context, Result};
use rustlab_plot::ThemeColors;
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
#[derive(Debug, Clone)]
pub struct ServerOpts {
    /// Port to bind. `None` means use [`DEFAULT_PORT`] with
    /// auto-increment up to [`MAX_PORT_ATTEMPTS`]; `Some(N)` is
    /// explicit and fails loud on `EADDRINUSE`.
    pub port: Option<u16>,
    /// When true, never auto-open the browser even on a TTY.
    pub no_browser: bool,
}

impl Default for ServerOpts {
    fn default() -> Self {
        Self {
            port: None,
            no_browser: false,
        }
    }
}

/// Start the interactive server against `input` and block until
/// Ctrl-C (or unrecoverable error). The initial render runs once on
/// the calling thread before the tokio runtime spins up; the
/// runtime serves the page, accepts WebSocket connections, and
/// (Phase 2) drives the render coordinator that re-renders on save.
pub fn start(input: &Path, theme: &'static ThemeColors, opts: ServerOpts) -> Result<()> {
    let canonical_input = std::fs::canonicalize(input)
        .with_context(|| format!("resolving {}", input.display()))?;

    // ── 1. Render once ────────────────────────────────────────────
    let plot_tempdir =
        TempDir::new().context("creating tempdir for served plot artefacts")?;
    let html = render_for_server(&canonical_input, theme, plot_tempdir.path())
        .with_context(|| format!("rendering {} for server", canonical_input.display()))?;

    // ── 2. Build server state ─────────────────────────────────────
    let state = Arc::new(http::ServerState::new(html, plot_tempdir));

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

        if !opts.no_browser && should_auto_open_browser() {
            if let Err(e) = open_browser(&url) {
                eprintln!("[watch] could not open browser automatically: {e}");
                eprintln!("[watch] open {url} manually");
            }
        }

        // ── 5. Spawn fs watcher + render coordinator ──────────────
        // `_watcher` is held to keep the notify watcher alive; the
        // task handle is dropped on shutdown.
        let (_watcher, _coord) =
            render_loop::spawn(canonical_input.clone(), theme, state.clone())
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

/// Re-implementation of the render pipeline in `lib::cmd_render` minus
/// the disk-output step: read input → strip artefacts → expand embeds
/// → parse → execute → render to HTML → swap CDN URLs to local
/// `/assets/` → inject the WS-client live-reload script into
/// `<head>`. Animations land in `plot_dir`; the server serves them
/// from there at `/plots/<filename>`.
///
/// Public to the server module so [`render_loop`] can re-invoke it
/// on save without duplicating the pipeline.
pub(super) fn render_for_server(
    input: &Path,
    theme: &ThemeColors,
    plot_dir: &Path,
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

    let html = render::render_html(&title, &rendered, plot_dir, "/plots", theme, None);
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
fn open_browser(url: &str) -> Result<()> {
    let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(target_os = "windows") {
        // `cmd /c start "" <url>` — the empty "" is start's required
        // window-title arg, otherwise start treats <url> as the title.
        ("cmd", vec!["/c", "start", "", url])
    } else {
        ("xdg-open", vec![url])
    };
    let status = std::process::Command::new(cmd)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("spawning `{cmd}`"))?;
    if !status.success() {
        anyhow::bail!("`{cmd}` exited with {status}");
    }
    Ok(())
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

    #[test]
    fn default_opts_are_sensible() {
        let o = ServerOpts::default();
        assert!(o.port.is_none());
        assert!(!o.no_browser);
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
