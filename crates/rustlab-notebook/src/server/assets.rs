//! Embedded third-party assets — KaTeX + Plotly bundles served at
//! `/assets/…` by the interactive `notebook watch` server.
//!
//! The bytes are vendored under
//! `crates/rustlab-notebook/assets/vendor/{katex,plotly}/` and pulled in
//! at compile time via `include_bytes!`. See locked-in #15/#16 of
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
        Some(Asset { bytes: $bytes, content_type: $ct })
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
        "plotly.min.js" => asset!(
            include_bytes!("../../assets/vendor/plotly/plotly.min.js"),
            KATEX_JS
        ),

        _ => None,
    }
}

/// Rewrite the CDN URLs the existing renderer hardcodes (see
/// `render.rs` ~line 308) so the same HTML loads our embedded assets
/// instead. Used by the server when it post-processes the rendered
/// page before sending it to the browser.
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
        assert!(asset.bytes.len() > 1_000_000, "plotly bundle suspiciously small");
    }

    #[test]
    fn font_resolves() {
        let asset = asset_for_path("katex/fonts/KaTeX_Main-Regular.woff2")
            .expect("main font missing");
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
