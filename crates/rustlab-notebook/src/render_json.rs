//! JSON emitter for downstream tooling (Obsidian plugin, web viewers, etc.).
//!
//! Walks executed notebook blocks and produces a single JSON document
//! describing every block, its outputs, and any pre-rendered HTML/SVG.
//! Unlike the markdown / html emitters, this one writes no sidecar files —
//! plot SVGs are inlined directly into the document as strings. The plugin
//! consumes the JSON in-memory, so per-render disk traffic is zero
//! (beyond the SVG-via-tempfile round-trip plotters insists on).
//!
//! Schema is versioned via the top-level `version: 1` field. Optional
//! fields are non-breaking; consumers must tolerate unknown keys. See
//! `dev/plans/notebook_obsidian_plugin.md` § "Phase 1" for the contract.

use crate::execute::Rendered;
use crate::parse::CalloutKind;
use pulldown_cmark::{html::push_html, Options, Parser};
use rustlab_plot::theme::ThemeColors;
use serde::Serialize;

/// Top-level document. `version` lets downstream tools detect breaking
/// schema changes; bumped only when an existing field's meaning changes
/// or a required field is removed.
#[derive(Serialize, Debug)]
pub struct Document {
    pub version: u32,
    pub title: String,
    pub blocks: Vec<JsonBlock>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Tagged union over the block kinds, mirroring `Rendered` but with
/// pre-rendered HTML / SVG attached so the consumer doesn't need its
/// own markdown or plot renderer.
#[derive(Serialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JsonBlock {
    Markdown {
        source: String,
        html: String,
    },
    Code {
        language: &'static str,
        source: String,
        source_hash: String,
        text_output: String,
        error: Option<String>,
        plots: Vec<JsonPlot>,
        hidden: bool,
        details: Option<String>,
    },
    Mermaid {
        source: String,
        /// Inline SVG when the `mermaid` feature is enabled and the
        /// renderer succeeded; `None` otherwise (consumer falls back to
        /// rendering the source itself, e.g. via mermaid.js).
        svg: Option<String>,
        caption: Option<String>,
    },
    Callout {
        callout_type: &'static str,
        title: Option<String>,
        html: String,
    },
    ExerciseStart {
        number: usize,
    },
    SolutionStart,
}

/// One inlined plot. `format` distinguishes the representation; `data` is
/// the bytes (SVG XML as a string today; reserved for `"html"` Plotly
/// divs and `"gif"` base64 animations in future versions).
#[derive(Serialize, Debug)]
pub struct JsonPlot {
    pub format: &'static str,
    pub data: String,
}

#[derive(Serialize, Debug)]
pub struct Diagnostic {
    pub level: &'static str,
    pub message: String,
    pub block_index: usize,
}

/// Current schema version. Bumped on breaking changes only.
pub const SCHEMA_VERSION: u32 = 1;

/// Render executed notebook blocks into the JSON document tree.
/// `theme` is forwarded to the SVG backend so emitted plots colour-match
/// what an HTML render of the same notebook would produce.
pub fn render_json(title: &str, blocks: &[Rendered], theme: &ThemeColors) -> Document {
    let mut json_blocks = Vec::with_capacity(blocks.len());
    let mut diagnostics = Vec::new();

    for (idx, block) in blocks.iter().enumerate() {
        match block {
            Rendered::Markdown(md) => {
                json_blocks.push(JsonBlock::Markdown {
                    source: md.clone(),
                    html: markdown_to_html(md),
                });
            }
            Rendered::Code {
                source,
                text_output,
                error,
                figures,
                animations: _,
                hidden,
                details,
                grid_cols: _,
            } => {
                let mut plots = Vec::with_capacity(figures.len());
                for (fig_idx, fig) in figures.iter().enumerate() {
                    match figure_to_svg_string(fig, theme) {
                        Ok(svg) => plots.push(JsonPlot {
                            format: "svg",
                            data: svg,
                        }),
                        Err(e) => diagnostics.push(Diagnostic {
                            level: "warn",
                            message: format!("plot {fig_idx} in block {idx}: {e}"),
                            block_index: idx,
                        }),
                    }
                }
                json_blocks.push(JsonBlock::Code {
                    language: "rustlab",
                    source: source.clone(),
                    source_hash: hash_source(source),
                    text_output: text_output.clone(),
                    error: error.clone(),
                    plots,
                    hidden: *hidden,
                    details: details.clone(),
                });
            }
            Rendered::Mermaid {
                source,
                hidden,
                details: _,
                caption,
            } => {
                if *hidden {
                    continue;
                }
                let svg = render_mermaid_svg(source, idx, &mut diagnostics);
                json_blocks.push(JsonBlock::Mermaid {
                    source: source.clone(),
                    svg,
                    caption: caption.clone(),
                });
            }
            Rendered::Callout {
                kind,
                title,
                content,
            } => {
                let html = format!(
                    "<div class=\"callout callout-{}\">{}{}</div>",
                    callout_class(kind),
                    title
                        .as_ref()
                        .map(|t| format!("<div class=\"callout-title\">{}</div>", escape_html(t)))
                        .unwrap_or_default(),
                    markdown_to_html(content),
                );
                json_blocks.push(JsonBlock::Callout {
                    callout_type: callout_tag(kind),
                    title: title.clone(),
                    html,
                });
            }
            Rendered::ExerciseStart { number } => {
                json_blocks.push(JsonBlock::ExerciseStart { number: *number });
            }
            Rendered::SolutionStart => {
                json_blocks.push(JsonBlock::SolutionStart);
            }
        }
    }

    Document {
        version: SCHEMA_VERSION,
        title: title.to_string(),
        blocks: json_blocks,
        diagnostics,
    }
}

fn markdown_to_html(md: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(md, opts);
    let mut html = String::new();
    push_html(&mut html, parser);
    html
}

fn figure_to_svg_string(
    fig: &rustlab_plot::FigureState,
    theme: &ThemeColors,
) -> Result<String, String> {
    let tmp = tempfile::Builder::new()
        .prefix("rustlab-json-plot-")
        .suffix(".svg")
        .tempfile()
        .map_err(|e| format!("tempfile: {e}"))?;
    rustlab_plot::render_figure_state_to_file_themed(fig, &tmp.path().to_string_lossy(), theme)
        .map_err(|e| format!("render: {e}"))?;
    std::fs::read_to_string(tmp.path()).map_err(|e| format!("read: {e}"))
}

#[cfg(feature = "mermaid")]
fn render_mermaid_svg(
    source: &str,
    block_index: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    // Mermaid renderer wants a plot_dir to stash its hash-keyed cache.
    // Use a per-process scratch dir under the tempdir so consecutive
    // renders share the cache (mermaid render is slow — ~hundreds of ms).
    let dir = std::env::temp_dir().join("rustlab-json-mermaid");
    let _ = std::fs::create_dir_all(&dir);
    match crate::mermaid::render_to_svg_string(source, &dir) {
        Ok(svg) => Some(svg),
        Err(e) => {
            diagnostics.push(Diagnostic {
                level: "warn",
                message: format!("mermaid render failed: {e}"),
                block_index,
            });
            None
        }
    }
}

#[cfg(not(feature = "mermaid"))]
fn render_mermaid_svg(
    _source: &str,
    block_index: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    diagnostics.push(Diagnostic {
        level: "info",
        message: "mermaid feature not enabled; svg omitted".to_string(),
        block_index,
    });
    None
}

fn callout_tag(kind: &CalloutKind) -> &'static str {
    match kind {
        CalloutKind::Note => "NOTE",
        CalloutKind::Tip => "TIP",
        CalloutKind::Important => "IMPORTANT",
        CalloutKind::Warning => "WARNING",
        CalloutKind::Caution => "CAUTION",
    }
}

fn callout_class(kind: &CalloutKind) -> &'static str {
    match kind {
        CalloutKind::Note => "note",
        CalloutKind::Tip => "tip",
        CalloutKind::Important => "important",
        CalloutKind::Warning => "warning",
        CalloutKind::Caution => "caution",
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Source-hash format mirrors the rest of the codebase: a 16-char hex
/// digest prefixed with `blake3:` when blake3 is available (the `mermaid`
/// feature pulls it in), or a `siphash:` digest from `DefaultHasher`
/// otherwise. Either form is stable across runs of the same binary on
/// the same input; the plugin treats it as an opaque cache key.
#[cfg(feature = "mermaid")]
fn hash_source(s: &str) -> String {
    let h = blake3::hash(s.as_bytes());
    format!("blake3:{}", &h.to_hex()[..16])
}

#[cfg(not(feature = "mermaid"))]
fn hash_source(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("siphash:{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::CalloutKind;
    use rustlab_plot::theme::Theme;

    fn theme() -> &'static ThemeColors {
        Theme::Dark.colors()
    }

    #[test]
    fn json_schema_version_is_1() {
        let doc = render_json("t", &[], theme());
        assert_eq!(doc.version, 1);
        assert_eq!(SCHEMA_VERSION, 1);
    }

    #[test]
    fn json_minimal_notebook_has_required_fields() {
        let blocks = vec![
            Rendered::Markdown("# Hello\n\nProse.".to_string()),
            Rendered::Code {
                source: "x = 1".to_string(),
                text_output: "x = 1".to_string(),
                error: None,
                figures: vec![],
                animations: vec![],
                hidden: false,
                details: None,
                grid_cols: None,
            },
        ];
        let doc = render_json("My Notebook", &blocks, theme());
        let json = serde_json::to_string(&doc).unwrap();
        // Round-trips through serde_json::Value.
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["title"], "My Notebook");
        assert_eq!(v["blocks"].as_array().unwrap().len(), 2);
        assert_eq!(v["blocks"][0]["kind"], "markdown");
        assert_eq!(v["blocks"][1]["kind"], "code");
        assert!(v["diagnostics"].is_array());
    }

    #[test]
    fn json_markdown_includes_pre_rendered_html() {
        let blocks = vec![Rendered::Markdown("# Heading\n\nA *para*.".to_string())];
        let doc = render_json("t", &blocks, theme());
        let v = serde_json::to_value(&doc).unwrap();
        let html = v["blocks"][0]["html"].as_str().unwrap();
        assert!(html.contains("<h1>"), "got: {html}");
        assert!(html.contains("<em>para</em>"), "got: {html}");
    }

    #[test]
    fn json_code_block_includes_source_hash() {
        let blocks = vec![Rendered::Code {
            source: "x = 1 + 2".to_string(),
            text_output: String::new(),
            error: None,
            figures: vec![],
            animations: vec![],
            hidden: false,
            details: None,
            grid_cols: None,
        }];
        let doc = render_json("t", &blocks, theme());
        let v = serde_json::to_value(&doc).unwrap();
        let hash = v["blocks"][0]["source_hash"].as_str().unwrap();
        assert!(
            hash.starts_with("blake3:") || hash.starts_with("siphash:"),
            "unexpected hash prefix: {hash}"
        );
        // Stable across calls.
        let doc2 = render_json("t", &blocks, theme());
        assert_eq!(doc.blocks.len(), 1);
        assert_eq!(doc2.blocks.len(), 1);
        let v2 = serde_json::to_value(&doc2).unwrap();
        assert_eq!(v["blocks"][0]["source_hash"], v2["blocks"][0]["source_hash"]);
    }

    #[test]
    fn json_error_block_serialises_with_error_field_set() {
        let blocks = vec![Rendered::Code {
            source: "broken".to_string(),
            text_output: String::new(),
            error: Some("undefined name 'broken'".to_string()),
            figures: vec![],
            animations: vec![],
            hidden: false,
            details: None,
            grid_cols: None,
        }];
        let doc = render_json("t", &blocks, theme());
        let v = serde_json::to_value(&doc).unwrap();
        assert_eq!(v["blocks"][0]["error"], "undefined name 'broken'");
    }

    #[test]
    fn json_callout_emits_html_pre_rendered() {
        let blocks = vec![Rendered::Callout {
            kind: CalloutKind::Note,
            title: Some("Heads up".to_string()),
            content: "Just *so* you know.".to_string(),
        }];
        let doc = render_json("t", &blocks, theme());
        let v = serde_json::to_value(&doc).unwrap();
        assert_eq!(v["blocks"][0]["kind"], "callout");
        assert_eq!(v["blocks"][0]["callout_type"], "NOTE");
        assert_eq!(v["blocks"][0]["title"], "Heads up");
        let html = v["blocks"][0]["html"].as_str().unwrap();
        assert!(html.contains("callout-note"));
        assert!(html.contains("<em>so</em>"));
        assert!(html.contains("Heads up"));
    }

    #[test]
    fn json_plot_emitted_as_svg_string() {
        use rustlab_plot::{FigureState, SeriesColor};
        let mut fig = FigureState::new();
        fig.current_mut().series.push(rustlab_plot::Series {
            label: "y".to_string(),
            x_data: vec![0.0, 1.0, 2.0],
            y_data: vec![0.0, 1.0, 0.5],
            color: SeriesColor::Blue,
            style: rustlab_plot::LineStyle::Solid,
            kind: rustlab_plot::PlotKind::Line,
        });
        let blocks = vec![Rendered::Code {
            source: "plot([0 1 2], [0 1 .5])".to_string(),
            text_output: String::new(),
            error: None,
            figures: vec![fig],
            animations: vec![],
            hidden: false,
            details: None,
            grid_cols: None,
        }];
        let doc = render_json("t", &blocks, theme());
        let v = serde_json::to_value(&doc).unwrap();
        let plots = v["blocks"][0]["plots"].as_array().unwrap();
        assert_eq!(plots.len(), 1);
        assert_eq!(plots[0]["format"], "svg");
        let svg = plots[0]["data"].as_str().unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"), "got: {}", &svg[..80.min(svg.len())]);
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn json_exercise_and_solution_pass_through() {
        let blocks = vec![
            Rendered::ExerciseStart { number: 3 },
            Rendered::SolutionStart,
        ];
        let doc = render_json("t", &blocks, theme());
        let v = serde_json::to_value(&doc).unwrap();
        assert_eq!(v["blocks"][0]["kind"], "exercise_start");
        assert_eq!(v["blocks"][0]["number"], 3);
        assert_eq!(v["blocks"][1]["kind"], "solution_start");
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn json_mermaid_emits_inline_svg() {
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B".to_string(),
            hidden: false,
            details: None,
            caption: Some("a flow".to_string()),
        }];
        let doc = render_json("t", &blocks, theme());
        let v = serde_json::to_value(&doc).unwrap();
        assert_eq!(v["blocks"][0]["kind"], "mermaid");
        assert_eq!(v["blocks"][0]["caption"], "a flow");
        let svg = v["blocks"][0]["svg"].as_str().unwrap();
        assert!(svg.contains("<svg"), "got: {}", &svg[..120.min(svg.len())]);
    }

    #[cfg(not(feature = "mermaid"))]
    #[test]
    fn json_mermaid_without_feature_reports_diagnostic() {
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B".to_string(),
            hidden: false,
            details: None,
            caption: None,
        }];
        let doc = render_json("t", &blocks, theme());
        let v = serde_json::to_value(&doc).unwrap();
        assert_eq!(v["blocks"][0]["kind"], "mermaid");
        assert!(v["blocks"][0]["svg"].is_null());
        let diags = v["diagnostics"].as_array().unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0]["block_index"], 0);
    }
}
