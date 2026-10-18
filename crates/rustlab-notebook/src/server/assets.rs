//! Embedded third-party assets — KaTeX + Plotly bundles served at
//! `/assets/…` by the interactive `notebook watch` server, and copied
//! next to batch HTML (`render -f html`) so those pages open from
//! `file://`.
//!
//! The bytes are vendored under
//! `crates/rustlab-notebook/assets/vendor/{katex,plotly}/` and pulled in
//! at compile time via `include_bytes!` (Plotly itself is the
//! `rustlab_plot::PLOTLY_JS` static, so the notebook binary does not
//! embed the bundle twice). See locked-in #15/#16 of
//! `dev/plans/notebook_interactive_server.md` for the offline-capability
//! rationale and licensing notes.
//!
//! Only woff2 fonts are shipped — every browser released since ~2018
//! supports the format, so the woff/ttf alternates in the upstream
//! KaTeX tarball would be dead weight in the binary.
//!
//! Lookup: [`asset_for_path`] maps a URL path (the portion after
//! `/assets/`) to `(bytes, content_type)`. Returns `None` for unknown
//! paths so the router can respond with 404.

/// One served asset: borrowed byte slice + MIME type.
pub struct Asset {
    pub bytes: &'static [u8],
    pub content_type: &'static str,
}

macro_rules! asset {
    ($bytes:expr, $ct:expr) => {
        Some(Asset {
            bytes: $bytes,
            content_type: $ct,
        })
    };
}

const KATEX_CSS: &str = "text/css; charset=utf-8";
const KATEX_JS: &str = "application/javascript; charset=utf-8";
const FONT_WOFF2: &str = "font/woff2";

/// Resolve a path *relative to* `/assets/` (no leading slash).
///
/// Returns `None` for unknown paths.
pub fn asset_for_path(path: &str) -> Option<Asset> {
    // Reject any traversal attempt up front. Even though we match by
    // exact path below, defence in depth is cheap.
    if path.contains("..") || path.contains('\\') || path.starts_with('/') {
        return None;
    }

    match path {
        // ── KaTeX ─────────────────────────────────────────────────────
        "katex/katex.min.css" => asset!(
            include_bytes!("../../assets/vendor/katex/katex.min.css"),
            KATEX_CSS
        ),
        "katex/katex.min.js" => asset!(
            include_bytes!("../../assets/vendor/katex/katex.min.js"),
            KATEX_JS
        ),
        "katex/contrib/auto-render.min.js" => asset!(
            include_bytes!("../../assets/vendor/katex/contrib/auto-render.min.js"),
            KATEX_JS
        ),

        // ── KaTeX fonts (woff2 only) ─────────────────────────────────
        "katex/fonts/KaTeX_AMS-Regular.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_AMS-Regular.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Caligraphic-Bold.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Caligraphic-Bold.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Caligraphic-Regular.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Caligraphic-Regular.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Fraktur-Bold.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Fraktur-Bold.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Fraktur-Regular.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Fraktur-Regular.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Main-Bold.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Main-Bold.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Main-BoldItalic.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Main-BoldItalic.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Main-Italic.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Main-Italic.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Main-Regular.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Main-Regular.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Math-BoldItalic.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Math-BoldItalic.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Math-Italic.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Math-Italic.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_SansSerif-Bold.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_SansSerif-Bold.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_SansSerif-Italic.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_SansSerif-Italic.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_SansSerif-Regular.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_SansSerif-Regular.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Script-Regular.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Script-Regular.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Size1-Regular.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Size1-Regular.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Size2-Regular.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Size2-Regular.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Size3-Regular.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Size3-Regular.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Size4-Regular.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Size4-Regular.woff2"),
            FONT_WOFF2
        ),
        "katex/fonts/KaTeX_Typewriter-Regular.woff2" => asset!(
            include_bytes!("../../assets/vendor/katex/fonts/KaTeX_Typewriter-Regular.woff2"),
            FONT_WOFF2
        ),

        // ── Plotly ────────────────────────────────────────────────────
        // Same bytes `savefig` / `saveanim` write beside their HTML.
        "plotly.min.js" => asset!(rustlab_plot::PLOTLY_JS, KATEX_JS),

        // ── CodeMirror 5 (in-browser editor; served only under
        //    `--editable`, but embedded unconditionally — harmless dead
        //    weight on the non-editable path) ──────────────────────────
        "codemirror/codemirror.min.js" => asset!(
            include_bytes!("../../assets/vendor/codemirror/codemirror.min.js"),
            KATEX_JS
        ),
        "codemirror/codemirror.min.css" => asset!(
            include_bytes!("../../assets/vendor/codemirror/codemirror.min.css"),
            KATEX_CSS
        ),
        "codemirror/mode/markdown/markdown.min.js" => asset!(
            include_bytes!("../../assets/vendor/codemirror/mode/markdown/markdown.min.js"),
            KATEX_JS
        ),

        _ => None,
    }
}

/// Paths copied beside batch HTML. CodeMirror is watch-only (the
/// in-browser editor) and is not part of a static page.
const BATCH_ASSET_PATHS: &[&str] = &[
    "plotly.min.js",
    "katex/katex.min.css",
    "katex/katex.min.js",
    "katex/contrib/auto-render.min.js",
    "katex/fonts/KaTeX_AMS-Regular.woff2",
    "katex/fonts/KaTeX_Caligraphic-Bold.woff2",
    "katex/fonts/KaTeX_Caligraphic-Regular.woff2",
    "katex/fonts/KaTeX_Fraktur-Bold.woff2",
    "katex/fonts/KaTeX_Fraktur-Regular.woff2",
    "katex/fonts/KaTeX_Main-Bold.woff2",
    "katex/fonts/KaTeX_Main-BoldItalic.woff2",
    "katex/fonts/KaTeX_Main-Italic.woff2",
    "katex/fonts/KaTeX_Main-Regular.woff2",
    "katex/fonts/KaTeX_Math-BoldItalic.woff2",
    "katex/fonts/KaTeX_Math-Italic.woff2",
    "katex/fonts/KaTeX_SansSerif-Bold.woff2",
    "katex/fonts/KaTeX_SansSerif-Italic.woff2",
    "katex/fonts/KaTeX_SansSerif-Regular.woff2",
    "katex/fonts/KaTeX_Script-Regular.woff2",
    "katex/fonts/KaTeX_Size1-Regular.woff2",
    "katex/fonts/KaTeX_Size2-Regular.woff2",
    "katex/fonts/KaTeX_Size3-Regular.woff2",
    "katex/fonts/KaTeX_Size4-Regular.woff2",
    "katex/fonts/KaTeX_Typewriter-Regular.woff2",
];

/// Copy Plotly, KaTeX, and the woff2 fonts into `dir` (the `assets/`
/// directory next to batch HTML). Skips a file whose length already
/// matches, so a directory render does not rewrite ~5 MB per page.
///
/// Inlining was rejected: Plotly alone is ~4.4 MB, and a directory of
/// notebooks would repeat it on every page. KaTeX's CSS references
/// `url(fonts/*.woff2)` relative to the stylesheet, so copying the CSS
/// and the fonts side by side keeps the font URLs valid without
/// rewriting them. woff/ttf fallbacks in the CSS are not shipped (same
/// as the watch server); browsers that understand woff2 use the first
/// `src`.
pub fn write_batch_assets(dir: &std::path::Path) -> std::io::Result<()> {
    for rel in BATCH_ASSET_PATHS {
        let asset = asset_for_path(rel).expect("batch asset path is registered");
        let dest = dir.join(rel);
        if let Ok(meta) = std::fs::metadata(&dest) {
            if meta.len() == asset.bytes.len() as u64 {
                continue;
            }
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, asset.bytes)?;
    }
    Ok(())
}

/// The four library URLs the HTML template emits. `notebook watch`
/// serves them at the site root. Batch HTML rewrites them to `prefix`
/// (`assets/` beside a single file, `../assets/` for a nested page in
/// a directory render) so `file://` finds the copied tree.
const ABSOLUTE_ASSET_URLS: &[(&str, &str)] = &[
    ("/assets/plotly.min.js", "plotly.min.js"),
    ("/assets/katex/katex.min.css", "katex/katex.min.css"),
    (
        "/assets/katex/contrib/auto-render.min.js",
        "katex/contrib/auto-render.min.js",
    ),
    ("/assets/katex/katex.min.js", "katex/katex.min.js"),
];

/// Rewrite `/assets/…` library URLs to `{prefix}…`. `prefix` is a
/// relative directory ending in `/`, such as `assets/` or `../assets/`.
/// Longer paths are replaced first so `katex.min.js` cannot eat a
/// longer sibling URL.
pub fn relativize_batch_assets(html: &str, prefix: &str) -> String {
    let mut out = html.to_string();
    for (abs, rel) in ABSOLUTE_ASSET_URLS {
        out = out.replace(abs, &format!("{prefix}{rel}"));
    }
    out
}

/// Rewrite CDN URLs to `/assets/…`. The renderer now emits the local
/// paths directly; this remains so a page that still contains the old
/// CDN strings (and the unit tests that feed those strings in) loads
/// the vendored copies. `notebook watch` calls it before serving.
pub fn rewrite_cdn_urls(html: &str) -> String {
    html.replace(
        "https://cdn.plot.ly/plotly-2.35.0.min.js",
        "/assets/plotly.min.js",
    )
    .replace(
        "https://cdn.jsdelivr.net/npm/katex@0.16.21/dist/katex.min.css",
        "/assets/katex/katex.min.css",
    )
    .replace(
        "https://cdn.jsdelivr.net/npm/katex@0.16.21/dist/katex.min.js",
        "/assets/katex/katex.min.js",
    )
    .replace(
        "https://cdn.jsdelivr.net/npm/katex@0.16.21/dist/contrib/auto-render.min.js",
        "/assets/katex/contrib/auto-render.min.js",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn katex_css_resolves() {
        let asset = asset_for_path("katex/katex.min.css").expect("katex css missing");
        assert!(asset.bytes.starts_with(b"/*") || asset.bytes.starts_with(b"@font-face"));
        assert_eq!(asset.content_type, "text/css; charset=utf-8");
    }

    #[test]
    fn plotly_js_resolves() {
        let asset = asset_for_path("plotly.min.js").expect("plotly js missing");
        // Plotly's minified bundle starts with a license banner comment.
        assert!(
            asset.bytes.len() > 1_000_000,
            "plotly bundle suspiciously small"
        );
    }

    #[test]
    fn font_resolves() {
        let asset =
            asset_for_path("katex/fonts/KaTeX_Main-Regular.woff2").expect("main font missing");
        // woff2 magic: 0x77 0x4F 0x46 0x32 ("wOF2")
        assert_eq!(&asset.bytes[..4], b"wOF2");
        assert_eq!(asset.content_type, "font/woff2");
    }

    #[test]
    fn unknown_path_returns_none() {
        assert!(asset_for_path("nope.js").is_none());
        assert!(asset_for_path("katex/missing.js").is_none());
    }

    #[test]
    fn traversal_blocked() {
        assert!(asset_for_path("../Cargo.toml").is_none());
        assert!(asset_for_path("katex/../../etc/passwd").is_none());
        assert!(asset_for_path("/etc/passwd").is_none());
    }

    #[test]
    fn rewrite_swaps_cdn_to_local() {
        let html = r#"<script src="https://cdn.plot.ly/plotly-2.35.0.min.js"></script>"#;
        let out = rewrite_cdn_urls(html);
        assert!(out.contains("/assets/plotly.min.js"));
        assert!(!out.contains("cdn.plot.ly"));
    }

    #[test]
    fn relativize_points_nested_pages_at_a_shared_assets_dir() {
        let html = r#"<script src="/assets/plotly.min.js"></script>
<link rel="stylesheet" href="/assets/katex/katex.min.css">
<script defer src="/assets/katex/katex.min.js"></script>
<script defer src="/assets/katex/contrib/auto-render.min.js"></script>"#;
        let out = relativize_batch_assets(html, "../assets/");
        assert!(out.contains("../assets/plotly.min.js"));
        assert!(out.contains("../assets/katex/katex.min.css"));
        assert!(out.contains("../assets/katex/katex.min.js"));
        assert!(out.contains("../assets/katex/contrib/auto-render.min.js"));
        assert!(!out.contains("\"/assets/"));
        assert!(!html_has_cdn(&out));
    }

    #[test]
    fn write_batch_assets_copies_plotly_katex_and_a_font() {
        let dir = std::env::temp_dir().join(format!("rustlab-batch-assets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_batch_assets(&dir).unwrap();
        let plotly = std::fs::metadata(dir.join("plotly.min.js")).unwrap().len();
        assert!(plotly > 1_000_000, "plotly copy too small: {plotly}");
        assert!(dir.join("katex/katex.min.css").is_file());
        assert!(dir.join("katex/fonts/KaTeX_Main-Regular.woff2").is_file());
        // Second call skips the rewrite when the length matches.
        write_batch_assets(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn html_has_cdn(html: &str) -> bool {
        rustlab_plot::html_loads_remote_script_or_style(html)
    }

    #[test]
    fn rewrite_swaps_all_katex_urls() {
        let html = r#"<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16.21/dist/katex.min.css">
            <script src="https://cdn.jsdelivr.net/npm/katex@0.16.21/dist/katex.min.js"></script>
            <script src="https://cdn.jsdelivr.net/npm/katex@0.16.21/dist/contrib/auto-render.min.js"></script>"#;
        let out = rewrite_cdn_urls(html);
        assert!(!out.contains("cdn.jsdelivr.net"));
        assert!(out.contains("/assets/katex/katex.min.css"));
        assert!(out.contains("/assets/katex/katex.min.js"));
        assert!(out.contains("/assets/katex/contrib/auto-render.min.js"));
    }
}
