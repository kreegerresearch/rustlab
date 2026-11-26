//! Server-side Mermaid diagram rendering via `mermaid-rs-renderer`.
//!
//! Pure-Rust SVG rendering — no browser, no Node, no shell-out. Used by
//! both the HTML renderer (inline `<svg>`) and the LaTeX renderer
//! (`\includegraphics{...}`, after a fixed-argv Inkscape SVG→PDF pass). Output is cached by
//! source and theme hash under `<plot_dir>/.cache/<blake3-hex>.svg` so unchanged diagrams
//! aren't re-rendered.
//!
//! Colours come from the notebook theme ([`mermaid_theme`]): nodes take
//! `bg_secondary` / `border_subtle`, labels `text`, edges `text_dim`. The
//! canvas rectangle is transparent, so a diagram sits on whatever page
//! colour surrounds it. LaTeX passes Latte, since PDF always prints light.
//!
//! All entry points catch panics from the underlying crate and convert
//! them to `MermaidRenderError` — Mermaid blocks must never panic the
//! notebook render. Failed renders fall back to verbatim source at the
//! call site.

use std::fs;
use std::io;
use std::panic;
use std::path::{Path, PathBuf};

use mermaid_rs_renderer::{RenderOptions, Theme};
use rustlab_plot::theme::ThemeColors;

/// Part of every cache key. Bump it when the SVG post-processing changes,
/// so an old cache entry is not reused.
const CACHE_VERSION: &[u8] = b"rustlab-mermaid-themed-1\0";

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

/// Render a Mermaid diagram to SVG in `colors`. Returns the SVG file path
/// under `plot_dir`. Caches by BLAKE3 hash of the source and theme.
///
/// On success: writes `<plot_dir>/diagram-<idx>.svg` and returns its path.
/// The cached SVG lives at `<plot_dir>/.cache/<hash>.svg`.
pub fn render_to_svg_file(
    source: &str,
    plot_dir: &Path,
    diagram_idx: usize,
    colors: &ThemeColors,
) -> Result<PathBuf, MermaidRenderError> {
    let cache_path = ensure_rendered(source, plot_dir, colors)?;
    let target = plot_dir.join(format!("diagram-{diagram_idx}.svg"));
    // Always copy fresh — diagram_idx may have moved between renders even
    // if source hasn't, and copy is cheap relative to render.
    fs::copy(&cache_path, &target)?;
    Ok(target)
}

/// Render a Mermaid diagram to SVG and return the SVG content as a String.
/// Suitable for inline embedding in HTML. Caches by BLAKE3 hash of source and theme.
pub fn render_to_svg_string(
    source: &str,
    plot_dir: &Path,
    colors: &ThemeColors,
) -> Result<String, MermaidRenderError> {
    let cache_path = ensure_rendered(source, plot_dir, colors)?;
    Ok(fs::read_to_string(&cache_path)?)
}

/// Ensure the SVG for `source` exists in the cache; render if needed.
/// Returns the cache file path.
fn ensure_rendered(
    source: &str,
    plot_dir: &Path,
    colors: &ThemeColors,
) -> Result<PathBuf, MermaidRenderError> {
    let cache_dir = plot_dir.join(".cache");
    fs::create_dir_all(&cache_dir)?;

    let theme = mermaid_theme(colors);
    let mut hasher = blake3::Hasher::new();
    hasher.update(CACHE_VERSION);
    hasher.update(source.as_bytes());
    hasher.update(b"\0");
    hasher.update(format!("{theme:?}").as_bytes());
    let hash = hasher.finalize().to_hex();
    let cache_path = cache_dir.join(format!("{hash}.svg"));

    if let Ok(meta) = fs::metadata(&cache_path) {
        if meta.len() > 0 {
            return Ok(cache_path);
        }
    }

    // Cache miss — render. Catch panics from the upstream crate (0.2.x).
    let svg = clear_canvas(&render_one(source, theme)?, &colors.bg);
    let svg = recolor_literals(&svg, &fixed_palette(colors));

    // Atomic write: tmp file then rename.
    let tmp_path = cache_dir.join(format!("{hash}.svg.tmp"));
    fs::write(&tmp_path, &svg)?;
    fs::rename(&tmp_path, &cache_path)?;
    Ok(cache_path)
}

/// One-shot render. `catch_unwind`-protected so a crate panic becomes
/// a clean error rather than tearing down the notebook render.
fn render_one(source: &str, theme: Theme) -> Result<String, MermaidRenderError> {
    let src = source.to_string();
    let result = panic::catch_unwind(panic::AssertUnwindSafe(move || {
        let options = RenderOptions {
            theme,
            ..RenderOptions::default()
        };
        mermaid_rs_renderer::render_with_options(&src, options)
    }));
    match result {
        Ok(Ok(svg)) => Ok(svg),
        Ok(Err(e)) => Err(MermaidRenderError::Render(format!("{e}"))),
        Err(_) => Err(MermaidRenderError::Render("renderer panicked".to_string())),
    }
}

/// The renderer's colours from notebook theme roles. Fonts, sizes, and the
/// git-graph palette stay the renderer's own.
pub fn mermaid_theme(c: &ThemeColors) -> Theme {
    let s = |v: &str| v.to_string();
    let series = [
        &c.accent_primary,
        &c.accent_secondary,
        &c.accent_tertiary,
        &c.syn_function,
        &c.syn_string,
        &c.syn_number,
        &c.syn_keyword,
        &c.error_text,
        &c.cm_note_border,
        &c.dx_add_bar,
        &c.dx_mod_bar,
        &c.syn_operator,
    ];
    Theme {
        primary_color: s(&c.bg_secondary),
        primary_text_color: s(&c.text),
        primary_border_color: s(&c.border_subtle),
        line_color: s(&c.text_dim),
        secondary_color: s(&c.inline_code_bg),
        tertiary_color: s(&c.bg),
        // `background` is also the knockout colour (state end rings, pie
        // slice gaps), so it stays the page colour; `clear_canvas` then
        // makes only the canvas rectangle transparent.
        background: s(&c.bg),
        edge_label_background: s(&c.bg),
        cluster_background: s(&c.bg),
        cluster_border: s(&c.border),
        sequence_actor_fill: s(&c.bg_secondary),
        sequence_actor_border: s(&c.border_subtle),
        sequence_actor_line: s(&c.text_dim),
        sequence_note_fill: s(&c.inline_code_bg),
        sequence_note_border: s(&c.accent_secondary),
        sequence_activation_fill: s(&c.border),
        sequence_activation_border: s(&c.border_subtle),
        text_color: s(&c.text),
        pie_colors: series.map(|v| s(v)),
        pie_title_text_color: s(&c.text),
        pie_section_text_color: s(&c.bg),
        pie_legend_text_color: s(&c.text),
        pie_stroke_color: s(&c.bg),
        pie_outer_stroke_color: s(&c.border_subtle),
        ..Theme::modern()
    }
}

/// Colours the renderer writes as literals instead of reading from
/// [`Theme`], with their theme replacements. Pale fills (timeline events,
/// quadrant backgrounds, mindmap nodes) become an accent at a fifth over
/// `bg`, so labels in `text` keep 4.5:1; light grid lines become
/// `border_subtle`, the radar axis `text_dim`, quadrant labels `text`. Saturated series colours
/// (gantt, sankey, xychart) and white text on dark badges are left alone.
fn fixed_palette(c: &ThemeColors) -> Vec<(&'static str, String)> {
    let tint = |accent: &str, amount: f64| {
        rustlab_plot::theme::blend_srgb(accent, &c.bg, amount)
            .map(|hex| format!("#{hex}"))
            .unwrap_or_else(|| c.bg_secondary.to_string())
    };
    vec![
        ("#ECECFF", tint(&c.accent_primary, 0.2)),
        ("#FFE6CC", tint(&c.accent_secondary, 0.2)),
        ("#D5E8D4", tint(&c.dx_add_bar, 0.2)),
        ("#F8CECC", tint(&c.error_text, 0.2)),
        ("#FFF2CC", tint(&c.syn_number, 0.2)),
        ("#E1D5E7", tint(&c.accent_tertiary, 0.2)),
        ("#f1f1ff", tint(&c.accent_primary, 0.15)),
        ("#f6f6ff", tint(&c.accent_primary, 0.1)),
        ("#fbfbff", tint(&c.accent_primary, 0.05)),
        ("#E2E8F0", c.border_subtle.to_string()),
        ("#c7c7f1", c.border_subtle.to_string()),
        ("#DEDEDE", c.border_subtle.to_string()),
        ("#333333", c.text_dim.to_string()),
        ("#131300", c.text.to_string()),
    ]
}

/// Replace each fixed literal `="#…"` attribute value in one pass, so a
/// replacement is never itself replaced.
fn recolor_literals(svg: &str, palette: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(i) = rest.find("=\"#") {
        out.push_str(&rest[..i + 2]);
        rest = &rest[i + 2..];
        let end = rest[1..].find('"').map_or(rest.len(), |e| e + 1);
        let value = &rest[..end];
        match palette.iter().find(|(lit, _)| *lit == value) {
            Some((_, new)) => out.push_str(new),
            None => out.push_str(value),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Make the renderer's canvas rectangles transparent: `<rect>` tags whose
/// only attributes are `x`, `y`, `width`, `height` and `fill = bg`. Every
/// other shape keeps `bg` (edge-label backings, knockouts).
fn clear_canvas(svg: &str, bg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(i) = rest.find("<rect ") {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..=end];
        out.push_str(&canvas_to_none(tag, bg).unwrap_or_else(|| tag.to_string()));
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn canvas_to_none(tag: &str, bg: &str) -> Option<String> {
    let body = tag.strip_prefix("<rect")?.strip_suffix("/>")?;
    let mut names = Vec::new();
    let mut fill = None;
    let mut attrs = body.trim();
    while !attrs.is_empty() {
        let eq = attrs.find("=\"")?;
        let name = attrs[..eq].trim();
        let value_end = eq + 2 + attrs[eq + 2..].find('"')?;
        if name == "fill" {
            fill = Some(&attrs[eq + 2..value_end]);
        }
        names.push(name);
        attrs = attrs[value_end + 1..].trim_start();
    }
    names.sort_unstable();
    let fill = fill?;
    let canvas = names == ["fill", "height", "width", "x", "y"] && fill.eq_ignore_ascii_case(bg);
    canvas.then(|| tag.replacen(&format!("fill=\"{fill}\""), "fill=\"none\"", 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustlab_plot::theme::Theme;

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
        let svg = render_to_svg_string("flowchart LR\n  A --> B\n", &dir, mocha()).unwrap();
        assert!(
            svg.starts_with("<?xml") || svg.starts_with("<svg"),
            "expected SVG, got {:?}",
            &svg[..svg.len().min(80)]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_hit_returns_same_content() {
        let dir = tmp_dir("hit");
        let src = "flowchart LR\n  X --> Y\n";
        let a = render_to_svg_string(src, &dir, mocha()).unwrap();
        let b = render_to_svg_string(src, &dir, mocha()).unwrap();
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
        let _ = render_to_svg_string("flowchart LR\n  A --> B\n", &dir, mocha()).unwrap();
        let _ = render_to_svg_string("flowchart LR\n  A --> C\n", &dir, mocha()).unwrap();
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
        let _ = render_to_svg_string("@@@ not a diagram @@@", &dir, mocha());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn svg_output_has_no_script_tags() {
        let dir = tmp_dir("noscript");
        let svg = render_to_svg_string("flowchart LR\n  A --> B\n", &dir, mocha()).unwrap();
        assert!(!svg.contains("<script"), "SVG must not contain <script>");
        assert!(!svg.contains("onclick="), "SVG must not contain onclick=");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_to_svg_file_writes_diagram_n() {
        let dir = tmp_dir("file");
        let path = render_to_svg_file("flowchart LR\n  A --> B\n", &dir, 3, mocha()).unwrap();
        assert_eq!(path, dir.join("diagram-3.svg"));
        assert!(path.exists());
        assert!(fs::metadata(&path).unwrap().len() > 0);
        let _ = fs::remove_dir_all(&dir);
    }

    const FLOW: &str = "flowchart LR\n  A[text] --> B[tokens] --> C[ids]\n";

    fn mocha() -> &'static ThemeColors {
        Theme::Mocha.colors()
    }

    fn latte() -> &'static ThemeColors {
        Theme::Latte.colors()
    }

    /// Every `fill="…"` / `stroke="…"` value in the SVG, lowercased.
    fn paint_values(svg: &str, attr: &str) -> Vec<String> {
        let needle = format!(" {attr}=\"");
        svg.match_indices(&needle)
            .map(|(i, _)| {
                let rest = &svg[i + needle.len()..];
                rest[..rest.find('"').unwrap()].to_ascii_lowercase()
            })
            .collect()
    }

    /// The `<rect …/>` tags of the SVG.
    fn rects(svg: &str) -> Vec<&str> {
        svg.match_indices("<rect ")
            .map(|(i, _)| {
                let rest = &svg[i..];
                &rest[..rest.find('>').unwrap() + 1]
            })
            .collect()
    }

    fn assert_themed_flowchart(svg: &str, c: &ThemeColors) {
        let lower = svg.to_ascii_lowercase();
        assert!(!lower.contains("#ffffff") || c.bg.eq_ignore_ascii_case("#ffffff"));
        for stock in ["#f8fafc", "#0f172a", "#64748b", "#94a3b8"] {
            assert!(!lower.contains(stock), "stock colour {stock} left in {svg}");
        }
        let canvas = rects(svg)[0];
        assert!(
            canvas.contains("fill=\"none\""),
            "canvas not transparent: {canvas}"
        );
        let node = rects(svg)
            .into_iter()
            .find(|r| r.contains("rx=\"3\""))
            .unwrap_or_else(|| panic!("no node rect in {svg}"));
        assert!(
            node.contains(&format!("fill=\"{}\"", c.bg_secondary)),
            "node fill is not bg_secondary {}: {node}",
            c.bg_secondary
        );
        assert!(
            node.contains(&format!("stroke=\"{}\"", c.border_subtle)),
            "node stroke is not border_subtle {}: {node}",
            c.border_subtle
        );
        let fills = paint_values(svg, "fill");
        assert!(
            fills.contains(&c.text.to_ascii_lowercase()),
            "no text-coloured label"
        );
        let strokes = paint_values(svg, "stroke");
        assert!(
            strokes.contains(&c.text_dim.to_ascii_lowercase()),
            "edges not text_dim"
        );
    }

    #[test]
    fn mocha_has_no_white_canvas_and_uses_theme_roles() {
        let dir = tmp_dir("mocha");
        let svg = render_to_svg_string(FLOW, &dir, mocha()).unwrap();
        assert_themed_flowchart(&svg, mocha());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn latte_uses_theme_roles() {
        let dir = tmp_dir("latte");
        let svg = render_to_svg_string(FLOW, &dir, latte()).unwrap();
        assert_themed_flowchart(&svg, latte());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn theme_file_colours_reach_the_diagram() {
        let dir = tmp_dir("theme-file");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("brand.toml");
        fs::write(
            &path,
            "inherits = \"mocha\"\n[colors]\nbg_secondary = \"#102030\"\nborder_subtle = \"#405060\"\ntext = \"#e0e8f0\"\ntext_dim = \"#a0b0c0\"\n",
        )
        .unwrap();
        let loaded = crate::theme_file::load_path(&path, &Default::default()).unwrap();
        let c = &loaded.theme.colors;
        assert_eq!(c.bg_secondary, "#102030");
        let svg = render_to_svg_string(FLOW, &dir, c).unwrap();
        assert_themed_flowchart(&svg, c);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_is_keyed_by_theme() {
        let dir = tmp_dir("theme-key");
        let a = render_to_svg_string(FLOW, &dir, mocha()).unwrap();
        let b = render_to_svg_string(FLOW, &dir, latte()).unwrap();
        assert_ne!(a, b, "latte reused the mocha cache entry");
        assert_eq!(fs::read_dir(dir.join(".cache")).unwrap().count(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_bare_canvas_rects_turn_transparent() {
        let svg = "<svg><rect x=\"0\" y=\"0\" width=\"5\" height=\"5\" fill=\"#1e1e2e\"/>\
                   <rect x=\"1\" y=\"1\" width=\"2\" height=\"2\" rx=\"2\" ry=\"2\" fill=\"#1e1e2e\" stroke=\"none\"/>\
                   <circle r=\"2\" fill=\"#1e1e2e\"/></svg>";
        let out = clear_canvas(svg, "#1e1e2e");
        assert_eq!(
            rects(&out)[0],
            "<rect x=\"0\" y=\"0\" width=\"5\" height=\"5\" fill=\"none\"/>"
        );
        assert!(
            rects(&out)[1].contains("fill=\"#1e1e2e\""),
            "label backing changed"
        );
        assert!(
            out.contains("<circle r=\"2\" fill=\"#1e1e2e\"/>"),
            "knockout changed"
        );
    }

    const FIXED_LITERALS: [&str; 15] = [
        "#ECECFF", "#FFE6CC", "#D5E8D4", "#F8CECC", "#FFF2CC", "#E1D5E7", "#f1f1ff", "#f6f6ff",
        "#fbfbff", "#E2E8F0", "#c7c7f1", "#DEDEDE", "#333333", "#131300", "#FFFFFF",
    ];

    #[test]
    fn fixed_light_palettes_follow_every_builtin_theme() {
        let src = "timeline\n  title T\n  2024 : a\n  2025 : b\n  2026 : c\n";
        let quad = "quadrantChart\n  x-axis Low --> High\n  y-axis Low --> High\n  A: [0.3, 0.6]\n";
        for name in ["mocha", "macchiato", "frappe", "latte"] {
            let c = rustlab_plot::theme::theme_colors(name).unwrap();
            let dir = tmp_dir(&format!("fixed-{name}"));
            for diagram in [src, quad] {
                let svg = render_to_svg_string(diagram, &dir, c).unwrap();
                for lit in FIXED_LITERALS {
                    assert!(!svg.contains(&format!("=\"{lit}\"")), "{name}: {lit} left");
                }
            }
            for (_, fill) in fixed_palette(c) {
                if fill == c.border_subtle || fill == c.text_dim || fill == c.text {
                    continue;
                }
                let ratio = rustlab_plot::theme::contrast_ratio(&c.text, &fill).unwrap();
                assert!(ratio >= 4.5, "{name}: text on {fill} is {ratio:.2}:1");
            }
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn sequence_diagram_has_no_stock_colours() {
        let dir = tmp_dir("seq");
        let src = "sequenceDiagram\n  Alice->>Bob: hi\n  Note right of Bob: hm\n";
        let svg = render_to_svg_string(src, &dir, mocha())
            .unwrap()
            .to_ascii_lowercase();
        for stock in [
            "#ffffff", "#f8fafc", "#0f172a", "#64748b", "#94a3b8", "#fff7ed", "#fdba74",
        ] {
            assert!(
                !svg.contains(stock),
                "stock colour {stock} left in sequence diagram"
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
