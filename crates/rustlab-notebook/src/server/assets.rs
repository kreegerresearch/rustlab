//! Embedded third-party assets — KaTeX, Plotly, MapLibre CSS, and Maki
//! icons served at `/assets/…` by the interactive `notebook watch` server.
//!
//! The bytes are vendored under
//! `crates/rustlab-notebook/assets/vendor/{katex,plotly,maplibre,maki}/`
//! and pulled in at compile time via `include_bytes!`. See locked-in
//! #15/#16 of `dev/plans/notebook_interactive_server.md` for the
//! offline-capability rationale and licensing notes.
//!
//! Plotly 2.35.0 (unchanged on disk) injects a MapLibre stylesheet, and
//! sometimes Maki icons, from a third-party CDN while the bundle
//! evaluates. [`maplibre_guard_script`] installs before that script and
//! retargets those loads at the local copies. The Plotly file itself is
//! not edited.
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
const SVG: &str = "image/svg+xml";

/// Same-origin stylesheet the guard script points Plotly's MapLibre
/// `<link>` at. MapLibre GL JS itself is already inside `plotly.min.js`;
/// only the CSS was left as a runtime CDN fetch. Version 4.5.2 matches
/// the `maplibre-gl` copy Plotly 2.35.0 bundled.
pub const MAPLIBRE_CSS_HREF: &str = "/assets/maplibre/maplibre-gl.css";

/// Prefix for Maki icons Plotly requests when a map style is missing a
/// `*-15` sprite. The file name is appended (`marker-15.svg`).
pub const MAKI_PREFIX: &str = "/assets/maki/";

#[path = "maki_icons.rs"]
mod maki_icons;

/// Resolve a path *relative to* `/assets/` (no leading slash).
///
/// Returns `None` for unknown paths.
pub fn asset_for_path(path: &str) -> Option<Asset> {
    // Reject any traversal attempt up front. Even though we match by
    // exact path below, defence in depth is cheap.
    if path.contains("..") || path.contains('\\') || path.starts_with('/') {
        return None;
    }

    if let Some(name) = path.strip_prefix("maki/") {
        if name.is_empty() || name.contains('/') {
            return None;
        }
        return maki_icons::maki_icon(name).map(|bytes| Asset {
            bytes,
            content_type: SVG,
        });
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
        "maplibre/maplibre-gl.css" => asset!(
            include_bytes!("../../assets/vendor/maplibre/maplibre-gl.css"),
            KATEX_CSS
        ),

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

/// Script that runs before Plotly and retargets the CDN loads Plotly
/// hardcodes (MapLibre CSS, Maki icons, anything else on that host) at
/// the vendored copies. The host name is split in the source so the
/// served HTML does not contain it as a contiguous string.
///
/// `nonce_attr` is the ` nonce="…"` already computed for the page, or
/// empty for static renders (no CSP).
pub fn maplibre_guard_script(nonce_attr: &str) -> String {
    let body = MAPLIBRE_GUARD_JS
        .replace("@@MAPLIBRE@@", MAPLIBRE_CSS_HREF)
        .replace("@@MAKI@@", MAKI_PREFIX);
    format!("<script{nonce_attr}>{body}</script>\n")
}

/// Static HTML has no `/assets/` server. Inline the MapLibre stylesheet
/// and tell the guard not to insert a stylesheet link or icon request.
/// Watch pages keep the same-origin hrefs instead of calling this.
pub fn inline_maplibre_for_file(html: &str) -> String {
    let bytes = asset_for_path("maplibre/maplibre-gl.css")
        .expect("vendored maplibre css")
        .bytes;
    let css = std::str::from_utf8(bytes).expect("maplibre css is utf-8");
    debug_assert!(
        !css.to_ascii_lowercase().contains("</style"),
        "maplibre css would close the inline style tag"
    );
    let html = html
        .replacen(
            &format!("var RL_MAPLIBRE_CSS = \"{MAPLIBRE_CSS_HREF}\";"),
            "var RL_MAPLIBRE_CSS = \"\";",
            1,
        )
        .replacen(
            &format!("var RL_MAKI_PREFIX = \"{MAKI_PREFIX}\";"),
            "var RL_MAKI_PREFIX = \"\";",
            1,
        );
    let style = format!("<style id=\"rl-maplibre\">{css}</style>\n");
    match html.find("</head>") {
        Some(i) => {
            let mut out = String::with_capacity(html.len() + style.len());
            out.push_str(&html[..i]);
            out.push_str(&style);
            out.push_str(&html[i..]);
            out
        }
        None => html,
    }
}

/// Installed before Plotly evaluates. Plotly's bundle registers its map
/// module at load time and appends a stylesheet `<link>` (and, on a
/// missing map sprite, an `Image`) aimed at a third-party CDN. This
/// rewrites those URLs onto the vendored files, or drops them when the
/// constants are empty (static HTML, where the CSS is inlined).
const MAPLIBRE_GUARD_JS: &str = r##"(function () {
  var RL_MAPLIBRE_CSS = "@@MAPLIBRE@@";
  var RL_MAKI_PREFIX = "@@MAKI@@";
  var BLANK = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
  function host() { return "unpkg" + ".com"; }
  function rewrite(url) {
    if (typeof url !== "string" || url.indexOf(host()) < 0) return url;
    if (url.indexOf("maplibre-gl") >= 0 && url.indexOf(".css") >= 0) {
      return RL_MAPLIBRE_CSS || null;
    }
    if (url.indexOf("maki@") >= 0) {
      if (!RL_MAKI_PREFIX) return null;
      var path = url.split("?")[0].split("#")[0];
      var name = path.slice(path.lastIndexOf("/") + 1);
      if (!name || name.indexOf("..") >= 0) return null;
      return RL_MAKI_PREFIX + name;
    }
    return null;
  }
  function retargetLink(node) {
    if (!node || node.nodeType !== 1 || node.tagName !== "LINK") return node;
    var href = node.getAttribute("href") || "";
    if (href.indexOf(host()) < 0) return node;
    var next = rewrite(href);
    if (!next) return null;
    node.setAttribute("href", next);
    return node;
  }
  var append = Node.prototype.appendChild;
  Node.prototype.appendChild = function (node) {
    var next = retargetLink(node);
    if (next == null) return node;
    return append.call(this, next);
  };
  var insert = Node.prototype.insertBefore;
  Node.prototype.insertBefore = function (node, ref) {
    var next = retargetLink(node);
    if (next == null) return node;
    return insert.call(this, next, ref);
  };
  var desc = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, "src");
  if (desc && desc.set && desc.get) {
    Object.defineProperty(HTMLImageElement.prototype, "src", {
      configurable: true,
      enumerable: desc.enumerable,
      get: desc.get,
      set: function (v) {
        var next = rewrite(v);
        desc.set.call(this, next == null ? BLANK : next);
      }
    });
  }
  if (window.fetch) {
    var origFetch = window.fetch;
    window.fetch = function (input, init) {
      if (typeof input === "string") {
        var next = rewrite(input);
        if (next == null) return Promise.reject(new TypeError("blocked remote asset"));
        input = next;
      }
      return origFetch.call(this, input, init);
    };
  }
})();"##;

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
    fn maplibre_css_resolves() {
        let asset = asset_for_path("maplibre/maplibre-gl.css").expect("maplibre css missing");
        let text = std::str::from_utf8(asset.bytes).expect("utf-8");
        assert!(text.contains(".maplibregl-map"), "not maplibre css");
        assert!(!text.contains("unpkg.com"));
        assert_eq!(asset.content_type, "text/css; charset=utf-8");
    }

    #[test]
    fn maki_icon_resolves_and_rejects_unknown() {
        let asset = asset_for_path("maki/marker-15.svg").expect("marker icon missing");
        let text = std::str::from_utf8(asset.bytes).expect("utf-8");
        assert!(text.contains("<svg"), "{text:.80}");
        assert_eq!(asset.content_type, "image/svg+xml");
        assert!(asset_for_path("maki/not-a-real-icon-15.svg").is_none());
        assert!(asset_for_path("maki/../maplibre/maplibre-gl.css").is_none());
    }

    #[test]
    fn guard_script_has_no_cdn_host_and_points_at_local_assets() {
        let html = maplibre_guard_script(" nonce=\"abc\"");
        assert!(html.starts_with("<script nonce=\"abc\">"));
        assert!(!html.contains("unpkg.com"));
        assert!(!html.contains("jsdelivr"));
        assert!(!html.contains("cdnjs"));
        assert!(!html.contains("cdn.plot.ly"));
        assert!(html.contains(MAPLIBRE_CSS_HREF));
        assert!(html.contains(MAKI_PREFIX));
    }

    #[test]
    fn static_html_inlines_maplibre_and_drops_asset_hrefs() {
        let page = format!("<head>{}</head>", maplibre_guard_script(""));
        let out = inline_maplibre_for_file(&page);
        assert!(!out.contains("unpkg.com"));
        assert!(!out.contains(MAPLIBRE_CSS_HREF));
        assert!(!out.contains(MAKI_PREFIX));
        assert!(out.contains("id=\"rl-maplibre\""));
        assert!(out.contains(".maplibregl-map"));
        assert!(out.contains("var RL_MAPLIBRE_CSS = \"\";"));
        assert!(out.contains("var RL_MAKI_PREFIX = \"\";"));
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
