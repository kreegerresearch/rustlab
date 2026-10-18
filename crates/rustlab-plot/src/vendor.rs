//! Vendored Plotly bundle shared by `savefig` / `saveanim` HTML and the
//! notebook renderer.
//!
//! The bytes live once on disk under `rustlab-notebook`'s vendor tree.
//! Inlining the ~4.4 MB bundle into every HTML file is the wrong trade:
//! a directory of notebooks would repeat it on each page (the example
//! collection is ~40 notebooks, ~200 MB of duplicated JavaScript). A
//! sibling `assets/plotly.min.js`, referenced with a relative URL, is
//! written once per output directory and opens from `file://` with no
//! network. `notebook watch` serves the same bytes at `/assets/` and
//! does not use this writer.

use std::path::Path;

use crate::error::PlotError;

/// Plotly 2.35.0 minified bundle (MIT). One copy, compiled into every
/// binary that links `rustlab-plot`.
pub const PLOTLY_JS: &[u8] =
    include_bytes!("../../rustlab-notebook/assets/vendor/plotly/plotly.min.js");

/// Relative URL written into standalone HTML (`savefig`, `saveanim`).
pub const PLOTLY_REL_SRC: &str = "assets/plotly.min.js";

/// Write `assets/plotly.min.js` next to `html_path` when it is missing
/// or a different length. Repeated saves in the same directory skip the
/// rewrite.
pub fn ensure_plotly_asset(html_path: &str) -> Result<(), PlotError> {
    let html = Path::new(html_path);
    let parent = match html.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let dest = parent.join("assets").join("plotly.min.js");
    if let Ok(meta) = std::fs::metadata(&dest) {
        if meta.len() == PLOTLY_JS.len() as u64 {
            return Ok(());
        }
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| PlotError::FileOutput(format!("creating {}: {e}", dir.display())))?;
    }
    std::fs::write(&dest, PLOTLY_JS)
        .map_err(|e| PlotError::FileOutput(format!("writing {}: {e}", dest.display())))
}

/// True when a `<script>` or `<link>` tag loads an `http:`, `https:`, or
/// protocol-relative (`//host/...`) URL.
///
/// Ordinary `<a href="https://...">` prose links are not library loads
/// and are ignored. The check is what the offline-HTML tests pin.
pub fn html_loads_remote_script_or_style(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let rest = &lower[i..];
            let is_script = rest.starts_with("<script");
            let is_link = rest.starts_with("<link");
            if is_script || is_link {
                if let Some(end) = rest.find('>') {
                    let tag = &rest[..end];
                    if tag_attr_is_remote(tag, "src") || tag_attr_is_remote(tag, "href") {
                        return true;
                    }
                    i += end + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    false
}

fn tag_attr_is_remote(tag: &str, attr: &str) -> bool {
    let mut rest = tag;
    while let Some(pos) = rest.find(attr) {
        let before_ok = pos == 0
            || rest.as_bytes()[pos - 1].is_ascii_whitespace()
            || rest.as_bytes()[pos - 1] == b'<';
        let after_name = &rest[pos + attr.len()..];
        let trimmed = after_name.trim_start();
        if before_ok && trimmed.starts_with('=') {
            let after_eq = trimmed[1..].trim_start();
            let value = if let Some(stripped) = after_eq.strip_prefix('"') {
                stripped.split('"').next().unwrap_or("")
            } else if let Some(stripped) = after_eq.strip_prefix('\'') {
                stripped.split('\'').next().unwrap_or("")
            } else {
                after_eq.split_whitespace().next().unwrap_or("")
            };
            if is_remote_url(value.trim()) {
                return true;
            }
        }
        rest = &rest[pos + attr.len()..];
    }
    false
}

fn is_remote_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://") || value.starts_with("//")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_script_and_link_are_flagged_anchors_are_not() {
        assert!(html_loads_remote_script_or_style(
            r#"<script src="https://cdn.plot.ly/plotly-2.35.0.min.js"></script>"#
        ));
        assert!(html_loads_remote_script_or_style(
            r#"<link rel="stylesheet" href="http://cdn.jsdelivr.net/katex.min.css">"#
        ));
        assert!(html_loads_remote_script_or_style(
            r#"<script src="//cdn.plot.ly/plotly.min.js"></script>"#
        ));
        assert!(html_loads_remote_script_or_style(
            r#"<SCRIPT SRC='HTTPS://CDN.PLOT.LY/plotly.min.js'></SCRIPT>"#
        ));
        assert!(!html_loads_remote_script_or_style(
            r#"<a href="https://example.com">docs</a><script src="assets/plotly.min.js"></script><link href="../assets/katex/katex.min.css">"#
        ));
        assert!(!html_loads_remote_script_or_style(
            r#"<script src="/assets/plotly.min.js"></script>"#
        ));
    }

    #[test]
    fn plotly_bundle_is_the_vendored_file() {
        assert!(
            PLOTLY_JS.len() > 1_000_000,
            "plotly bundle suspiciously small: {} bytes",
            PLOTLY_JS.len()
        );
        assert!(PLOTLY_JS.windows(6).any(|w| w == b"Plotly"));
    }
}
