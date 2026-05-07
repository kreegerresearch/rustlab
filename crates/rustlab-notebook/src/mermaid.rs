//! Server-side Mermaid diagram rendering via `mermaid-rs-renderer`.
//!
//! Pure-Rust SVG rendering — no browser, no Node, no shell-out. Used by
//! both the HTML renderer (inline `<svg>`) and the LaTeX renderer
//! (`\includesvg{...}`). Output is cached by source hash under
//! `<plot_dir>/.cache/<blake3-hex>.svg` so unchanged diagrams aren't
//! re-rendered.
//!
//! All entry points catch panics from the underlying crate and convert
//! them to `MermaidRenderError` — Mermaid blocks must never panic the
//! notebook render. Failed renders fall back to verbatim source at the
//! call site.

use std::fs;
use std::io;
use std::panic;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum MermaidRenderError {
    /// The renderer returned an error or panicked.
    Render(String),
    /// I/O while reading or writing cache / output files.
    Io(io::Error),
}

impl std::fmt::Display for MermaidRenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MermaidRenderError::Render(msg) => write!(f, "render: {msg}"),
            MermaidRenderError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl From<io::Error> for MermaidRenderError {
    fn from(e: io::Error) -> Self {
        MermaidRenderError::Io(e)
    }
}

/// Render a Mermaid diagram to SVG. Returns the SVG file path under
/// `plot_dir`. Caches by BLAKE3 hash of the source.
///
/// On success: writes `<plot_dir>/diagram-<idx>.svg` and returns its path.
/// The cached SVG lives at `<plot_dir>/.cache/<hash>.svg`.
pub fn render_to_svg_file(
    source: &str,
    plot_dir: &Path,
    diagram_idx: usize,
) -> Result<PathBuf, MermaidRenderError> {
    let cache_path = ensure_rendered(source, plot_dir)?;
    let target = plot_dir.join(format!("diagram-{diagram_idx}.svg"));
    // Always copy fresh — diagram_idx may have moved between renders even
    // if source hasn't, and copy is cheap relative to render.
    fs::copy(&cache_path, &target)?;
    Ok(target)
}

/// Render a Mermaid diagram to SVG and return the SVG content as a String.
/// Suitable for inline embedding in HTML. Caches by BLAKE3 hash of source.
pub fn render_to_svg_string(
    source: &str,
    plot_dir: &Path,
) -> Result<String, MermaidRenderError> {
    let cache_path = ensure_rendered(source, plot_dir)?;
    Ok(fs::read_to_string(&cache_path)?)
}

/// Ensure the SVG for `source` exists in the cache; render if needed.
/// Returns the cache file path.
fn ensure_rendered(source: &str, plot_dir: &Path) -> Result<PathBuf, MermaidRenderError> {
    let cache_dir = plot_dir.join(".cache");
    fs::create_dir_all(&cache_dir)?;

    let hash = blake3::hash(source.as_bytes()).to_hex();
    let cache_path = cache_dir.join(format!("{hash}.svg"));

    if let Ok(meta) = fs::metadata(&cache_path) {
        if meta.len() > 0 {
            return Ok(cache_path);
        }
    }

    // Cache miss — render. Catch panics from the upstream crate (0.2.x).
    let svg = render_one(source)?;

    // Atomic write: tmp file then rename.
    let tmp_path = cache_dir.join(format!("{hash}.svg.tmp"));
    fs::write(&tmp_path, &svg)?;
    fs::rename(&tmp_path, &cache_path)?;
    Ok(cache_path)
}

/// One-shot render. `catch_unwind`-protected so a crate panic becomes
/// a clean error rather than tearing down the notebook render.
fn render_one(source: &str) -> Result<String, MermaidRenderError> {
    let src = source.to_string();
    let result = panic::catch_unwind(panic::AssertUnwindSafe(move || {
        mermaid_rs_renderer::render(&src)
    }));
    match result {
        Ok(Ok(svg)) => Ok(svg),
        Ok(Err(e)) => Err(MermaidRenderError::Render(format!("{e}"))),
        Err(_) => Err(MermaidRenderError::Render(
            "renderer panicked".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "rustlab_mermaid_{}_{}_{}",
            std::process::id(),
            tag,
            blake3::hash(tag.as_bytes()).to_hex().as_str()
        ));
        let _ = fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn renders_simple_flowchart() {
        let dir = tmp_dir("simple");
        let svg = render_to_svg_string("flowchart LR\n  A --> B\n", &dir).unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"), "expected SVG, got {:?}", &svg[..svg.len().min(80)]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_hit_returns_same_content() {
        let dir = tmp_dir("hit");
        let src = "flowchart LR\n  X --> Y\n";
        let a = render_to_svg_string(src, &dir).unwrap();
        let b = render_to_svg_string(src, &dir).unwrap();
        assert_eq!(a, b);
        // Confirm exactly one cache entry exists for this source.
        let cache = dir.join(".cache");
        let count = fs::read_dir(&cache).unwrap().count();
        assert_eq!(count, 1, "expected exactly one cache file, got {count}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_miss_on_source_change() {
        let dir = tmp_dir("miss");
        let _ = render_to_svg_string("flowchart LR\n  A --> B\n", &dir).unwrap();
        let _ = render_to_svg_string("flowchart LR\n  A --> C\n", &dir).unwrap();
        let cache = dir.join(".cache");
        let count = fs::read_dir(&cache).unwrap().count();
        assert_eq!(count, 2, "expected two distinct cache files, got {count}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn renderer_error_returns_err_no_panic() {
        let dir = tmp_dir("err");
        // Empty source / unsupported diagram type. Either returns Err or
        // produces a degenerate SVG — both acceptable. The test only
        // requires no panic.
        let _ = render_to_svg_string("@@@ not a diagram @@@", &dir);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn svg_output_has_no_script_tags() {
        let dir = tmp_dir("noscript");
        let svg = render_to_svg_string("flowchart LR\n  A --> B\n", &dir).unwrap();
        assert!(!svg.contains("<script"), "SVG must not contain <script>");
        assert!(!svg.contains("onclick="), "SVG must not contain onclick=");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_to_svg_file_writes_diagram_n() {
        let dir = tmp_dir("file");
        let path = render_to_svg_file("flowchart LR\n  A --> B\n", &dir, 3).unwrap();
        assert_eq!(path, dir.join("diagram-3.svg"));
        assert!(path.exists());
        assert!(fs::metadata(&path).unwrap().len() > 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
