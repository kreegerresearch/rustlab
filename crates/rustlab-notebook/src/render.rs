use crate::execute::Rendered;
use crate::parse::CalloutKind;
use crate::NotebookNav;
use pulldown_cmark::{html::push_html, Options, Parser};
use rustlab_plot::render_animation_inline;
use rustlab_plot::render_figure_plotly_div;
use rustlab_plot::{NotebookAnimationFormat, ThemeColors};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// Render executed notebook blocks into an HTML string.
///
/// HTML inline plots stay self-contained, but animated-GIF sidecars
/// require a per-notebook plot directory: `plot_dir` is where GIF files
/// are written and `plot_href_prefix` is the relative path used inside
/// `<img src=...>` (same convention as the Markdown renderer).
///
/// `nav` is `Some` when the notebook is part of a multi-notebook directory
/// render — it carries an "← Index" link for the sidebar plus prev/next
/// footer links. `None` for single-file renders.
pub fn render_html(
    title: &str,
    blocks: &[Rendered],
    plot_dir: &Path,
    plot_href_prefix: &str,
    theme: &ThemeColors,
    nav: Option<&NotebookNav>,
) -> String {
    let _ = std::fs::create_dir_all(plot_dir);
    let href_prefix = plot_href_prefix.trim_end_matches('/').to_string();
    let mut nav_items = String::new();
    let mut body = String::new();
    let mut heading_idx = 0;
    let mut plot_idx = 0;
    let mut in_solution = false;
    let mut in_exercise = false;
    // Phase 3: per-render counter so identical-source blocks at
    // different positions get unique IDs (collision → "-N" suffix).
    // See dev/plans/notebook_interactive_server.md Phase 3.
    let mut block_id_counter: HashMap<u64, usize> = HashMap::new();

    for block in blocks {
        // Auto-close solution/exercise when we hit a new exercise or solution marker
        if matches!(block, Rendered::ExerciseStart { .. }) {
            if in_solution {
                body.push_str("</details>\n");
                in_solution = false;
            }
            if in_exercise {
                body.push_str("</div>\n");
                in_exercise = false;
            }
        }
        if matches!(block, Rendered::SolutionStart) && in_solution {
            body.push_str("</details>\n");
            in_solution = false;
        }

        match block {
            Rendered::Markdown(md) => {
                let mark = body.len();
                // Transform `[[wiki]]` / `![[embed]]` to standard markdown
                let md = transform_wikilinks(md);
                // Rewrite .md links to .html for cross-notebook references
                let md = rewrite_md_links(&md);
                // Stash math spans before CommonMark eats LaTeX backslashes
                let (md, math) = protect_math(&md);
                // Convert markdown to HTML
                let parser = Parser::new_ext(&md, notebook_md_options());
                let mut html = String::new();
                push_html(&mut html, parser);
                let html = restore_math(&html, &math);

                // Extract headings for nav and inject IDs
                let html = inject_heading_ids(&html, &mut nav_items, &mut heading_idx);

                body.push_str("<div class=\"prose\">\n");
                body.push_str(&html);
                body.push_str("</div>\n");
                finalize_block(&mut body, mark, &mut block_id_counter);
            }
            Rendered::Code {
                source,
                text_output,
                error,
                figures,
                animations,
                hidden,
                details,
                grid_cols,
            } => {
                let mark = body.len();
                body.push_str("<div class=\"code-block\">\n");

                // Source code (unless hidden)
                if !hidden {
                    body.push_str("<pre class=\"source\"><code>");
                    body.push_str(&highlight_rustlab(source));
                    body.push_str("</code></pre>\n");
                }

                // If details is set, wrap output section in a disclosure widget
                if let Some(title) = details {
                    body.push_str("<details class=\"code-details\">\n");
                    body.push_str(&format!("<summary>{}</summary>\n", escape_html(title)));
                }

                // Text output (if any)
                let trimmed_output = text_output.trim();
                if !trimmed_output.is_empty() {
                    body.push_str("<pre class=\"output\">");
                    body.push_str(&escape_html(trimmed_output));
                    body.push_str("</pre>\n");
                }

                // Error (if any)
                if let Some(err) = error {
                    body.push_str("<pre class=\"error\">");
                    body.push_str(&escape_html(err));
                    body.push_str("</pre>\n");
                }

                // Plots (one per savefig call, or one final snapshot)
                if !figures.is_empty() {
                    if let Some(n) = grid_cols {
                        body.push_str(&format!(
                            "<div class=\"image-grid\" style=\"grid-template-columns:repeat({n},1fr)\">\n"
                        ));
                        for fig in figures {
                            plot_idx += 1;
                            let div_id = format!("plot-{plot_idx}");
                            body.push_str(&render_figure_plotly_div(fig, &div_id, theme));
                            body.push('\n');
                        }
                        body.push_str("</div>\n");
                    } else {
                        for fig in figures {
                            plot_idx += 1;
                            let div_id = format!("plot-{plot_idx}");
                            let height = plot_container_height(fig.subplot_rows);
                            body.push_str(&format!(
                                "<div class=\"plot-container\" style=\"height: {height}px\">\n"
                            ));
                            body.push_str(&render_figure_plotly_div(fig, &div_id, theme));
                            body.push_str("\n</div>\n");
                        }
                    }
                }

                // Animations (one per saveanim call).
                // .html output: inline Plotly div (play/pause + slider).
                // .gif output: sidecar GIF in plot_dir, embedded via <img>.
                for anim in animations {
                    plot_idx += 1;
                    match anim.format {
                        NotebookAnimationFormat::Html => {
                            let div_id = format!("anim-{plot_idx}");
                            body.push_str("<div class=\"plot-container\">\n");
                            body.push_str(&render_animation_inline(
                                &anim.frames,
                                &div_id,
                                anim.fps,
                                theme,
                            ));
                            body.push_str("\n</div>\n");
                        }
                        NotebookAnimationFormat::Gif => {
                            let gif_path =
                                plot_dir.join(format!("anim-{plot_idx}.gif"));
                            if let Err(e) = rustlab_plot::write_animation_gif(
                                &gif_path.to_string_lossy(),
                                &anim.frames,
                                anim.fps,
                            ) {
                                eprintln!(
                                    "warning: could not write anim-{plot_idx}.gif: {e}"
                                );
                                continue;
                            }
                            body.push_str(&format!(
                                "<div class=\"plot-container\"><img src=\"{}/anim-{plot_idx}.gif\" alt=\"animation {plot_idx}\" /></div>\n",
                                href_prefix
                            ));
                        }
                    }
                }

                // Close details if open
                if details.is_some() {
                    body.push_str("</details>\n");
                }

                body.push_str("</div>\n");
                finalize_block(&mut body, mark, &mut block_id_counter);
            }
            Rendered::Mermaid {
                source,
                hidden,
                details,
                caption,
            } => {
                if *hidden {
                    continue;
                }
                let mark = body.len();
                if let Some(title) = details {
                    body.push_str("<details class=\"code-details\">\n");
                    body.push_str(&format!("<summary>{}</summary>\n", escape_html(title)));
                }
                body.push_str("<figure class=\"mermaid\">\n");
                emit_mermaid_html(&mut body, source, plot_dir);
                if let Some(cap) = caption {
                    body.push_str(&format!(
                        "<figcaption>{}</figcaption>\n",
                        escape_html(cap)
                    ));
                }
                body.push_str("</figure>\n");
                if details.is_some() {
                    body.push_str("</details>\n");
                }
                finalize_block(&mut body, mark, &mut block_id_counter);
            }
            Rendered::Callout {
                kind,
                title,
                content,
            } => {
                let mark = body.len();
                let (class, default_label) = callout_style(*kind);
                let label = title.as_deref().unwrap_or(default_label);
                body.push_str(&format!("<div class=\"callout callout-{class}\">\n"));
                body.push_str(&format!(
                    "<div class=\"callout-title\">{}</div>\n",
                    escape_html(label)
                ));
                let md = transform_wikilinks(content);
                let md = rewrite_md_links(&md);
                let (md, math) = protect_math(&md);
                let parser = Parser::new_ext(&md, notebook_md_options());
                let mut html = String::new();
                push_html(&mut html, parser);
                let html = restore_math(&html, &math);
                body.push_str(&html);
                body.push_str("</div>\n");
                finalize_block(&mut body, mark, &mut block_id_counter);
            }
            Rendered::ExerciseStart { number } => {
                body.push_str(&format!(
                    "<div class=\"exercise\">\n<div class=\"exercise-title\">Exercise {number}</div>\n"
                ));
                in_exercise = true;
            }
            Rendered::SolutionStart => {
                body.push_str("<details class=\"solution\">\n<summary>Show solution</summary>\n");
                in_solution = true;
            }
        }
    }

    // Auto-close any open solution/exercise at end of document
    if in_solution {
        body.push_str("</details>\n");
    }
    if in_exercise {
        body.push_str("</div>\n");
    }

    // Directory-mode sub-pages get a top breadcrumb bar instead of the fixed
    // sidebar — less visual weight, more horizontal room for content.
    // Single-file renders (`nav = None`) keep the sidebar with the in-page TOC.
    let use_topbar = nav.is_some();
    let body_class = if use_topbar {
        " class=\"topbar-layout\""
    } else {
        ""
    };

    let topbar_block = match nav {
        Some(n) => {
            let index_link = n
                .index_href
                .as_ref()
                .map(|href| {
                    format!(
                        "<a href=\"{href}\">&larr; Index</a>",
                        href = escape_html(href),
                    )
                })
                .unwrap_or_default();
            format!(
                "<header class=\"topbar\">{index}<span class=\"sep\">/</span><span class=\"current\">{title}</span></header>\n",
                index = index_link,
                title = escape_html(title),
            )
        }
        None => String::new(),
    };

    let sidebar_block = if use_topbar {
        String::new()
    } else {
        format!(
            "<button class=\"nav-toggle\" onclick=\"document.querySelector('nav.sidebar').classList.toggle('open')\" aria-label=\"Toggle navigation\">&#9776;</button>\n\
             <nav class=\"sidebar\">\n  <div class=\"nav-title\">{title}</div>\n{nav_items}</nav>\n",
            title = escape_html(title),
            nav_items = nav_items,
        )
    };

    let footer_nav = nav.map(|n| build_footer_nav(n)).unwrap_or_default();

    let c = theme;
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<script src="https://cdn.plot.ly/plotly-2.35.0.min.js"></script>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16.21/dist/katex.min.css">
<script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.21/dist/katex.min.js"></script>
<script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.21/dist/contrib/auto-render.min.js"
  onload="renderMathInElement(document.body, {{
    delimiters: [
      {{left: '$$', right: '$$', display: true}},
      {{left: '$', right: '$', display: false}}
    ]
  }});"></script>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
    background: {bg};
    color: {text};
    display: flex;
    min-height: 100vh;
  }}
  /* ── Navigation sidebar ── */
  nav.sidebar {{
    position: fixed;
    top: 0;
    left: 0;
    width: 220px;
    height: 100vh;
    background: {bg_secondary};
    border-right: 1px solid {border};
    padding: 1.5rem 0;
    overflow-y: auto;
    z-index: 100;
    transition: transform 0.25s ease;
  }}
  nav.sidebar .nav-title {{
    font-size: 1.1rem;
    font-weight: 700;
    color: {accent_primary};
    padding: 0 1rem 1rem;
    border-bottom: 1px solid {border};
    margin-bottom: 0.5rem;
  }}
  nav.sidebar a {{
    display: block;
    padding: 0.4rem 1rem;
    color: {text_dim};
    text-decoration: none;
    font-size: 0.9rem;
    transition: background 0.15s, color 0.15s;
  }}
  nav.sidebar a:hover {{
    background: {border};
    color: {text};
  }}
  nav.sidebar a.h2 {{
    padding-left: 1.8rem;
    font-size: 0.85rem;
  }}
  nav.sidebar a.h3 {{
    padding-left: 2.6rem;
    font-size: 0.8rem;
  }}
  /* ── Hamburger toggle (hidden on desktop) ── */
  .nav-toggle {{
    display: none;
    position: fixed;
    top: 0.7rem;
    left: 0.7rem;
    z-index: 200;
    background: {border};
    border: 1px solid {border_subtle};
    border-radius: 6px;
    color: {text};
    font-size: 1.3rem;
    width: 2.4rem;
    height: 2.4rem;
    cursor: pointer;
    line-height: 1;
  }}
  /* ── Main content ── */
  main {{
    margin-left: 220px;
    flex: 1;
    padding: 2rem 2.5rem;
    max-width: 960px;
    min-width: 0;
  }}
  /* ── Directory-mode top bar (replaces sidebar for sub-pages) ── */
  body.topbar-layout {{
    display: block;
  }}
  body.topbar-layout main {{
    margin: 0 auto;
    padding: 2rem 2.5rem;
    max-width: 960px;
  }}
  .topbar {{
    position: sticky;
    top: 0;
    z-index: 100;
    background: {bg_secondary};
    border-bottom: 1px solid {border};
    padding: 0.6rem 1.2rem;
    font-size: 0.85rem;
    color: {text_dim};
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }}
  .topbar a {{
    color: {accent_secondary};
    text-decoration: none;
  }}
  .topbar a:hover {{
    text-decoration: underline;
  }}
  .topbar .sep {{
    color: {text_dim};
  }}
  .topbar .current {{
    color: {text};
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }}
  .prose {{
    line-height: 1.7;
    margin-bottom: 1.5rem;
  }}
  .prose h1 {{
    font-size: 1.8rem;
    color: {accent_primary};
    margin: 2rem 0 1rem;
    padding-bottom: 0.4rem;
    border-bottom: 1px solid {border};
  }}
  .prose h2 {{
    font-size: 1.4rem;
    color: {accent_secondary};
    margin: 1.8rem 0 0.8rem;
  }}
  .prose h3 {{
    font-size: 1.15rem;
    color: {accent_tertiary};
    margin: 1.4rem 0 0.6rem;
  }}
  .prose p {{
    margin-bottom: 1rem;
  }}
  .prose code {{
    background: {inline_code_bg};
    padding: 0.15rem 0.4rem;
    border-radius: 3px;
    font-size: 0.9em;
  }}
  .prose table {{
    border-collapse: collapse;
    margin: 1rem 0;
  }}
  .prose th, .prose td {{
    border: 1px solid {border_subtle};
    padding: 0.5rem 0.8rem;
    text-align: left;
  }}
  .prose th {{
    background: {border};
    color: {accent_primary};
    font-weight: 600;
  }}
  .prose ul, .prose ol {{
    margin: 0.5rem 0 1rem 1.5rem;
  }}
  .prose li {{
    margin-bottom: 0.3rem;
  }}
  .prose blockquote {{
    border-left: 3px solid {accent_primary};
    padding-left: 1rem;
    color: {text_dim};
    margin: 1rem 0;
  }}
  .code-block {{
    margin-bottom: 1.5rem;
  }}
  .source {{
    background: {code_bg};
    border: 1px solid {border};
    border-radius: 6px;
    padding: 1rem;
    overflow-x: auto;
    font-family: "SF Mono", "Fira Code", "JetBrains Mono", monospace;
    font-size: 0.85rem;
    line-height: 1.5;
    color: {text};
  }}
  .output {{
    background: {output_bg};
    border: 1px solid {border};
    border-radius: 6px;
    padding: 0.8rem 1rem;
    margin-top: 0.5rem;
    color: {text_dim};
    font-family: "SF Mono", "Fira Code", "JetBrains Mono", monospace;
    font-size: 0.85rem;
    white-space: pre-wrap;
    line-height: 1.5;
  }}
  .error {{
    background: {error_bg};
    border: 1px solid {error_text};
    border-radius: 6px;
    padding: 0.8rem 1rem;
    margin-top: 0.5rem;
    color: {error_text};
    font-family: "SF Mono", "Fira Code", "JetBrains Mono", monospace;
    font-size: 0.85rem;
    white-space: pre-wrap;
  }}
  .plot-container {{
    background: {bg};
    border: 1px solid {border};
    border-radius: 8px;
    margin-top: 0.5rem;
    height: 450px;
  }}
  .plot-container > div {{
    width: 100%;
    height: 100%;
  }}
  footer {{
    color: {footer_text};
    font-size: 0.8rem;
    margin-top: 3rem;
    padding-top: 1rem;
    border-top: 1px solid {border};
  }}
  .page-nav {{
    display: flex;
    align-items: stretch;
    gap: 0.5rem;
    margin-top: 2.5rem;
    padding-top: 1.2rem;
    border-top: 1px solid {border};
  }}
  .page-nav a {{
    flex: 1 1 0;
    padding: 0.7rem 1rem;
    background: {bg_secondary};
    border: 1px solid {border};
    border-radius: 8px;
    color: {accent_secondary};
    text-decoration: none;
    font-size: 0.9rem;
    transition: background 0.15s, border-color 0.15s;
    min-width: 0;
  }}
  .page-nav a:hover {{
    background: {border};
    border-color: {accent_secondary};
  }}
  .page-nav .label {{
    display: block;
    color: {text_dim};
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.2rem;
  }}
  .page-nav .title {{
    display: block;
    color: {text};
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }}
  .page-nav .prev {{ text-align: left; }}
  .page-nav .index {{ text-align: center; align-self: center; }}
  .page-nav .next {{ text-align: right; }}
  /* ── Syntax highlighting ── */
  .syn-kw  {{ color: {syn_keyword}; }}
  .syn-fn  {{ color: {syn_function}; }}
  .syn-num {{ color: {syn_number}; }}
  .syn-str {{ color: {syn_string}; }}
  .syn-com {{ color: {syn_comment}; font-style: italic; }}
  .syn-op  {{ color: {syn_operator}; }}
  /* ── Callout blocks ── */
  .callout {{
    border-left: 4px solid;
    border-radius: 6px;
    padding: 1rem 1.2rem;
    margin: 1rem 0;
  }}
  .callout-title {{
    font-weight: 700;
    margin-bottom: 0.5rem;
    font-size: 0.95rem;
  }}
  .callout-note {{
    border-color: {accent_secondary};
    background: {bg_secondary};
  }}
  .callout-note .callout-title {{ color: {accent_secondary}; }}
  .callout-tip {{
    border-color: {accent_tertiary};
    background: {bg_secondary};
  }}
  .callout-tip .callout-title {{ color: {accent_tertiary}; }}
  .callout-important {{
    border-color: {accent_primary};
    background: {bg_secondary};
  }}
  .callout-important .callout-title {{ color: {accent_primary}; }}
  .callout-warning {{
    border-color: {error_text};
    background: {bg_secondary};
  }}
  .callout-warning .callout-title {{ color: {error_text}; }}
  .callout-caution {{
    border-color: {error_text};
    background: {bg_secondary};
  }}
  .callout-caution .callout-title {{ color: {error_text}; }}
  /* ── Exercise / solution blocks ── */
  .exercise {{
    border: 1px solid {border};
    border-radius: 8px;
    padding: 1.2rem;
    margin: 1.5rem 0;
    background: {bg_secondary};
  }}
  .exercise-title {{
    font-weight: 700;
    color: {accent_primary};
    margin-bottom: 0.8rem;
    font-size: 1.05rem;
  }}
  .solution {{
    margin-top: 1rem;
  }}
  .solution > summary {{
    cursor: pointer;
    color: {accent_secondary};
    font-weight: 600;
    padding: 0.3rem 0;
  }}
  /* ── Collapsible code output ── */
  .code-details > summary {{
    cursor: pointer;
    color: {accent_secondary};
    font-weight: 600;
    padding: 0.4rem 0;
  }}
  /* ── Image grid ── */
  .image-grid {{
    display: grid;
    gap: 0.5rem;
    margin-top: 0.5rem;
  }}
  /* ── Responsive: collapse sidebar on narrow screens ── */
  @media (max-width: 768px) {{
    nav.sidebar {{
      transform: translateX(-100%);
    }}
    nav.sidebar.open {{
      transform: translateX(0);
    }}
    .nav-toggle {{
      display: block;
    }}
    main {{
      margin-left: 0;
      padding: 3rem 1rem 2rem;
    }}
  }}
</style>
</head>
<body{body_class}>
{topbar_block}{sidebar_block}<main>
{body}{footer_nav}<footer>Generated by rustlab-notebook</footer>
</main>
</body>
</html>
"##,
        title = escape_html(title),
        body_class = body_class,
        topbar_block = topbar_block,
        sidebar_block = sidebar_block,
        footer_nav = footer_nav,
        body = body,
        bg = c.bg,
        bg_secondary = c.bg_secondary,
        text = c.text,
        text_dim = c.text_dim,
        border = c.border,
        border_subtle = c.border_subtle,
        accent_primary = c.accent_primary,
        accent_secondary = c.accent_secondary,
        accent_tertiary = c.accent_tertiary,
        code_bg = c.code_bg,
        output_bg = c.output_bg,
        inline_code_bg = c.inline_code_bg,
        error_bg = c.error_bg,
        error_text = c.error_text,
        footer_text = c.footer_text,
        syn_keyword = c.syn_keyword,
        syn_function = c.syn_function,
        syn_number = c.syn_number,
        syn_string = c.syn_string,
        syn_comment = c.syn_comment,
        syn_operator = c.syn_operator,
    )
}

/// Pixel height for the `.plot-container` so that stacked subplots are not
/// crushed into the default 450px slot. Single row keeps the historical
/// 450px; each extra row adds another full slot.
fn plot_container_height(rows: usize) -> usize {
    let rows = rows.max(1);
    450 + (rows - 1) * 350
}

/// CSS class suffix and default-title label for each callout kind.
pub(crate) fn callout_style(kind: CalloutKind) -> (&'static str, &'static str) {
    match kind {
        CalloutKind::Note => ("note", "Note"),
        CalloutKind::Tip => ("tip", "Tip"),
        CalloutKind::Important => ("important", "Important"),
        CalloutKind::Warning => ("warning", "Warning"),
        CalloutKind::Caution => ("caution", "Caution"),
    }
}

/// Pulldown-cmark feature set used by every notebook markdown parse —
/// the GFM superset that GitHub and Obsidian both render natively.
/// Format-specific renderers (e.g. LaTeX) can layer extra flags on top.
pub(crate) fn notebook_md_options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    opts
}

/// Render a Mermaid block into the HTML body. Inline SVG on success;
/// verbatim source in a `<pre>` on failure or when the `mermaid` feature
/// is disabled at build time.
fn emit_mermaid_html(body: &mut String, source: &str, _plot_dir: &std::path::Path) {
    #[cfg(feature = "mermaid")]
    {
        match crate::mermaid::render_to_svg_string(source, _plot_dir) {
            Ok(svg) => {
                body.push_str(&strip_xml_decl(&svg));
                body.push('\n');
                return;
            }
            Err(e) => {
                eprintln!("warning: mermaid render failed, embedding source: {e}");
            }
        }
    }
    #[cfg(not(feature = "mermaid"))]
    {
        warn_mermaid_disabled_once();
    }
    body.push_str("<pre class=\"mermaid-source\"><code>");
    body.push_str(&escape_html(source));
    body.push_str("</code></pre>\n");
}

/// Strip an XML declaration `<?xml ... ?>` from the start of an SVG
/// string so it inlines cleanly inside HTML. Whitespace before the
/// declaration is preserved as-is (the renderer typically emits none).
#[cfg_attr(not(feature = "mermaid"), allow(dead_code))]
fn strip_xml_decl(svg: &str) -> &str {
    let trimmed = svg.trim_start();
    if let Some(rest) = trimmed.strip_prefix("<?xml") {
        if let Some(end) = rest.find("?>") {
            return rest[end + 2..].trim_start();
        }
    }
    svg
}

#[cfg(not(feature = "mermaid"))]
fn warn_mermaid_disabled_once() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        eprintln!(
            "warning: rustlab-notebook built without 'mermaid' feature. \
             Mermaid blocks rendered as verbatim source."
        );
    }
}

fn build_footer_nav(nav: &NotebookNav) -> String {
    if nav.prev.is_none() && nav.next.is_none() && nav.index_href.is_none() {
        return String::new();
    }
    let mut out = String::from("<nav class=\"page-nav\">\n");
    if let Some((title, href)) = &nav.prev {
        out.push_str(&format!(
            "  <a class=\"prev\" href=\"{href}\"><span class=\"label\">&larr; Previous</span><span class=\"title\">{title}</span></a>\n",
            href = escape_html(href),
            title = escape_html(title),
        ));
    }
    if let Some(href) = &nav.index_href {
        out.push_str(&format!(
            "  <a class=\"index\" href=\"{href}\"><span class=\"title\">Index</span></a>\n",
            href = escape_html(href),
        ));
    }
    if let Some((title, href)) = &nav.next {
        out.push_str(&format!(
            "  <a class=\"next\" href=\"{href}\"><span class=\"label\">Next &rarr;</span><span class=\"title\">{title}</span></a>\n",
            href = escape_html(href),
            title = escape_html(title),
        ));
    }
    out.push_str("</nav>\n");
    out
}

/// Scan HTML for <h1>–<h3> tags. For each heading found:
/// 1. Inject an `id` attribute so nav links can scroll to it.
/// 2. Append a nav link to `nav`.
/// Returns the modified HTML.
fn inject_heading_ids(html: &str, nav: &mut String, idx: &mut usize) -> String {
    let mut result = html.to_string();
    for tag in ["h1", "h2", "h3"] {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let mut search_from = 0;
        loop {
            let Some(start) = result[search_from..].find(&open) else {
                break;
            };
            let abs_open = search_from + start;
            let content_start = abs_open + open.len();
            let Some(rel_end) = result[content_start..].find(&close) else {
                break;
            };
            let content = result[content_start..content_start + rel_end].to_string();
            let clean = strip_tags(&content);
            if !clean.is_empty() {
                *idx += 1;
                let id = format!("heading-{idx}");
                // Replace <hN> with <hN id="heading-N">
                let new_open = format!("<{tag} id=\"{id}\">");
                result.replace_range(abs_open..abs_open + open.len(), &new_open);
                // Build nav link
                nav.push_str(&format!(
                    "  <a href=\"#{id}\" class=\"{tag}\">{text}</a>\n",
                    id = id,
                    tag = tag,
                    text = escape_html(&clean),
                ));
                search_from = abs_open + new_open.len() + rel_end + close.len();
            } else {
                search_from = content_start + rel_end + close.len();
            }
        }
    }
    result
}

/// Strip HTML tags from a string.
fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── Syntax highlighting ─────────────────────────────────────────────────────

const KEYWORDS: &[&str] = &[
    "function",
    "end",
    "return",
    "if",
    "elseif",
    "else",
    "for",
    "while",
    "switch",
    "case",
    "otherwise",
];

/// Produce syntax-highlighted HTML for a rustlab code snippet.
/// Returns HTML with <span class="syn-*"> wrappers (already escaped).
fn highlight_rustlab(source: &str) -> String {
    let mut out = String::with_capacity(source.len() * 2);
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        // Comment: % to end of line
        if ch == '%' {
            out.push_str("<span class=\"syn-com\">");
            while i < len && chars[i] != '\n' {
                push_escaped_char(&mut out, chars[i]);
                i += 1;
            }
            out.push_str("</span>");
            continue;
        }

        // String: "..." or '...' (single-char or multi-char)
        if ch == '"' || (ch == '\'' && is_string_quote(&chars, i)) {
            let quote = ch;
            out.push_str("<span class=\"syn-str\">");
            push_escaped_char(&mut out, ch);
            i += 1;
            while i < len && chars[i] != quote && chars[i] != '\n' {
                push_escaped_char(&mut out, chars[i]);
                i += 1;
            }
            if i < len && chars[i] == quote {
                push_escaped_char(&mut out, chars[i]);
                i += 1;
            }
            out.push_str("</span>");
            continue;
        }

        // Dot-operators: .* ./ .^ .'
        if ch == '.' && i + 1 < len && matches!(chars[i + 1], '*' | '/' | '^' | '\'') {
            out.push_str("<span class=\"syn-op\">");
            push_escaped_char(&mut out, ch);
            push_escaped_char(&mut out, chars[i + 1]);
            out.push_str("</span>");
            i += 2;
            continue;
        }

        // Number: digits, optionally with . or e
        if ch.is_ascii_digit() || (ch == '.' && i + 1 < len && chars[i + 1].is_ascii_digit()) {
            out.push_str("<span class=\"syn-num\">");
            while i < len
                && (chars[i].is_ascii_digit()
                    || chars[i] == '.'
                    || chars[i] == 'e'
                    || chars[i] == 'E'
                    || ((chars[i] == '+' || chars[i] == '-')
                        && i > 0
                        && (chars[i - 1] == 'e' || chars[i - 1] == 'E')))
            {
                push_escaped_char(&mut out, chars[i]);
                i += 1;
            }
            // Trailing 'i' or 'j' for complex literals
            if i < len && (chars[i] == 'i' || chars[i] == 'j') {
                push_escaped_char(&mut out, chars[i]);
                i += 1;
            }
            out.push_str("</span>");
            continue;
        }

        // Identifier or keyword
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();

            if KEYWORDS.contains(&word.as_str()) {
                out.push_str("<span class=\"syn-kw\">");
                out.push_str(&escape_html(&word));
                out.push_str("</span>");
            } else if i < len && chars[i] == '(' {
                // Function call
                out.push_str("<span class=\"syn-fn\">");
                out.push_str(&escape_html(&word));
                out.push_str("</span>");
            } else {
                out.push_str(&escape_html(&word));
            }
            continue;
        }

        // Operators
        if is_operator(ch) {
            out.push_str("<span class=\"syn-op\">");
            // Handle two-char operators
            if i + 1 < len {
                let next = chars[i + 1];
                let two: String = [ch, next].iter().collect();
                if matches!(two.as_str(), "==" | "~=" | "<=" | ">=" | "&&" | "||") {
                    push_escaped_char(&mut out, ch);
                    push_escaped_char(&mut out, next);
                    i += 2;
                    out.push_str("</span>");
                    continue;
                }
            }
            push_escaped_char(&mut out, ch);
            i += 1;
            out.push_str("</span>");
            continue;
        }

        // Everything else (whitespace, parens, etc.)
        push_escaped_char(&mut out, ch);
        i += 1;
    }

    out
}

/// Determine if a single quote at position `i` starts a string literal
/// (as opposed to being the transpose operator).
fn is_string_quote(chars: &[char], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let prev = chars[i - 1];
    // After ), ], identifier char, or digit — it's transpose
    if prev == ')' || prev == ']' || prev.is_ascii_alphanumeric() || prev == '_' || prev == '.' {
        return false;
    }
    true
}

fn is_operator(ch: char) -> bool {
    matches!(
        ch,
        '+' | '-' | '*' | '/' | '\\' | '^' | '=' | '<' | '>' | '~' | '&' | '|' | ':' | ';' | ','
    )
}

fn push_escaped_char(out: &mut String, ch: char) {
    match ch {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        _ => out.push(ch),
    }
}

/// Rewrite relative `.md` links to `.html` in markdown text.
/// Converts `](something.md)` to `](something.html)` for cross-notebook links.
fn rewrite_md_links(md: &str) -> String {
    md.replace(".md)", ".html)").replace(".md#", ".html#")
}

/// Transform Obsidian-style wikilinks and embeds into standard markdown so
/// the committed `book/*.md` renders correctly on GitHub (where `[[...]]`
/// is literal text) and the HTML pipeline can run them through
/// `rewrite_md_links` like ordinary `.md` references.
///
/// Mappings:
/// - `[[Foo]]`              → `[Foo](Foo.md)`
/// - `[[Foo|Bar]]`          → `[Bar](Foo.md)`
/// - `[[Foo#Section]]`      → `[Foo § Section](Foo.md#section)`
/// - `[[Foo#Section|Bar]]`  → `[Bar](Foo.md#section)`
/// - `![[image.png]]`       → `![](image.png)`
/// - `![[image.png|alt]]`   → `![alt](image.png)`
///
/// The target gets a `.md` extension when it has none (i.e. ordinary
/// notebook references); embeds (`![[...]]`) keep the path as written so
/// they reference image/asset files. Skips fenced code blocks and inline
/// code spans so wiki-syntax inside ` ```mermaid ` or `` `[[Foo]]` `` is
/// preserved verbatim.
pub(crate) fn transform_wikilinks(md: &str) -> String {
    let s = md.as_bytes();
    let n = s.len();
    let mut out = String::with_capacity(n);
    let mut i = 0;
    let mut copied_to = 0; // byte index of next un-flushed source char
    let mut in_fence: Option<(u8, usize)> = None; // (fence char, fence len)

    // The triggers (`[`, `!`, `\``, fence opens) are all ASCII, so a byte
    // scan never lands inside a multi-byte UTF-8 sequence. We copy spans
    // verbatim from `md` as `&str` slices to keep non-ASCII bytes intact.

    while i < n {
        // At the start of a line, update fence state. Lines inside a
        // fenced code block (and the fence delimiters themselves) pass
        // through verbatim — flush via the trailing copy at end of loop.
        if i == 0 || s[i - 1] == b'\n' {
            if let Some((fc, len)) = in_fence {
                if is_close_fence(&s[i..line_end(s, i)], fc, len) {
                    in_fence = None;
                }
            } else if let Some((_after, fc, len)) = detect_fence_open(s, i) {
                in_fence = Some((fc, len));
            }
        }
        if in_fence.is_some() {
            i += 1;
            continue;
        }

        // Inline code span: skip forward to the closing backtick (or
        // end of line). Spans pass through verbatim.
        if s[i] == b'`' {
            i += 1;
            while i < n && s[i] != b'`' && s[i] != b'\n' {
                i += 1;
            }
            if i < n && s[i] == b'`' {
                i += 1;
            }
            continue;
        }

        // Embed: `![[…]]`
        if i + 3 < n && s[i] == b'!' && s[i + 1] == b'[' && s[i + 2] == b'[' {
            if let Some(close) = find_double_close(s, i + 3) {
                out.push_str(&md[copied_to..i]);
                out.push_str(&render_embed(&md[i + 3..close]));
                i = close + 2;
                copied_to = i;
                continue;
            }
        }

        // Link: `[[…]]`
        if i + 1 < n && s[i] == b'[' && s[i + 1] == b'[' {
            if let Some(close) = find_double_close(s, i + 2) {
                out.push_str(&md[copied_to..i]);
                out.push_str(&render_wikilink(&md[i + 2..close]));
                i = close + 2;
                copied_to = i;
                continue;
            }
        }

        i += 1;
    }
    out.push_str(&md[copied_to..]);
    out
}

/// Find the byte index of the next `]]` starting at `from`, on the same
/// line as `from` (wikilinks don't span lines).
fn find_double_close(s: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < s.len() {
        match s[i] {
            b'\n' => return None,
            b']' if s[i + 1] == b']' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Render the inside of `[[...]]` as standard markdown.
fn render_wikilink(inner: &str) -> String {
    let (target, alias) = match inner.split_once('|') {
        Some((t, a)) => (t.trim(), Some(a.trim())),
        None => (inner.trim(), None),
    };
    let (path, anchor) = match target.split_once('#') {
        Some((p, a)) => (p.trim(), Some(a.trim())),
        None => (target, None),
    };
    let dest = if path_has_extension(path) {
        path.to_string()
    } else {
        format!("{path}.md")
    };
    let anchor_url = anchor.map(|a| format!("#{}", slugify(a))).unwrap_or_default();
    let text = match (alias, anchor) {
        (Some(a), _) => a.to_string(),
        (None, Some(a)) => format!("{path} § {a}"),
        (None, None) => path.to_string(),
    };
    format!("[{text}]({dest}{anchor_url})")
}

/// Render the inside of `![[...]]` as a standard markdown image.
fn render_embed(inner: &str) -> String {
    let (path, alt) = match inner.split_once('|') {
        Some((p, a)) => (p.trim(), a.trim()),
        None => (inner.trim(), ""),
    };
    format!("![{alt}]({path})")
}

/// Heuristic: a path "has an extension" if its last `/`-segment contains
/// a `.`. Good enough for the embed/notebook split — `notes.md`, `img.png`
/// both true; `My Note`, `Sub/Note` both false.
fn path_has_extension(path: &str) -> bool {
    let tail = path.rsplit('/').next().unwrap_or(path);
    tail.contains('.')
}

/// Lowercase + replace runs of non-alphanumerics with `-`. Matches how
/// pulldown-cmark / GitHub generate heading anchors so `[[Foo#My Section]]`
/// resolves to the same `#my-section` the heading produces.
fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_dash = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            last_was_dash = false;
        } else if !last_was_dash && !out.is_empty() {
            out.push('-');
            last_was_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

// ── Math protection ─────────────────────────────────────────────────────────
// CommonMark consumes `\\` → `\`, which destroys LaTeX row separators inside
// `$$...$$`. We replace math spans with placeholders before parsing and
// restore them after, so KaTeX sees the original LaTeX. PUA characters survive
// pulldown-cmark and `escape_html` unchanged.

fn math_placeholder(idx: usize) -> String {
    format!("\u{E000}M{idx}\u{E001}")
}

/// Replace `$$...$$` and `$...$` math spans with opaque placeholders.
/// Returns the rewritten markdown plus the stashed originals (delimiters
/// included). Skips fenced code blocks and inline code spans, and respects
/// `\$` escapes per CommonMark.
fn protect_math(md: &str) -> (String, Vec<String>) {
    let s = md.as_bytes();
    let n = s.len();
    let mut out = String::with_capacity(n);
    let mut stash: Vec<String> = Vec::new();
    let mut i = 0;
    let mut at_line_start = true;

    while i < n {
        // Fenced code block opening at start of line (0–3 leading spaces, then ``` or ~~~).
        if at_line_start {
            if let Some((after_open, fence_char, fence_len)) = detect_fence_open(s, i) {
                // Copy through end of opening line.
                let eol = line_end(s, i);
                out.push_str(&md[i..eol]);
                i = eol;
                // Consume body until close fence (or EOF).
                while i < n {
                    let next = line_end(s, i);
                    let line = &md[i..next];
                    out.push_str(line);
                    i = next;
                    if is_close_fence(line.as_bytes(), fence_char, fence_len) {
                        break;
                    }
                }
                at_line_start = true;
                let _ = after_open; // unused; kept for symmetry/clarity
                continue;
            }
        }

        let b = s[i];

        // Inline code span: matched run of N backticks.
        if b == b'`' {
            let run_start = i;
            while i < n && s[i] == b'`' {
                i += 1;
            }
            let open_len = i - run_start;
            // Find a matching closing run of the same length.
            let mut j = i;
            let mut close: Option<(usize, usize)> = None;
            while j < n {
                if s[j] == b'`' {
                    let cs = j;
                    while j < n && s[j] == b'`' {
                        j += 1;
                    }
                    if j - cs == open_len {
                        close = Some((cs, j));
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            if let Some((_, ce)) = close {
                out.push_str(&md[run_start..ce]);
                at_line_start = ce > 0 && s[ce - 1] == b'\n';
                i = ce;
                continue;
            }
            // Unclosed run: treat as literal text.
            out.push_str(&md[run_start..i]);
            at_line_start = false;
            continue;
        }

        // CommonMark backslash escape of $ or `: copy verbatim, do not enter math.
        if b == b'\\' && i + 1 < n && (s[i + 1] == b'$' || s[i + 1] == b'`') {
            out.push('\\');
            out.push(s[i + 1] as char);
            i += 2;
            at_line_start = false;
            continue;
        }

        // Display math: $$ ... $$
        if b == b'$' && i + 1 < n && s[i + 1] == b'$' {
            if let Some(close) = find_display_close(s, i + 2) {
                let original = &md[i..close + 2];
                let idx = stash.len();
                stash.push(original.to_string());
                out.push_str(&math_placeholder(idx));
                // Track newlines consumed.
                if md[i..close + 2].contains('\n') {
                    at_line_start = s[close + 1] == b'\n';
                } else {
                    at_line_start = false;
                }
                i = close + 2;
                continue;
            }
        }

        // Inline math: $ ... $ (KaTeX-style, single line).
        if b == b'$' && is_inline_math_open(s, i) {
            if let Some(close) = find_inline_close(s, i + 1) {
                let original = &md[i..close + 1];
                let idx = stash.len();
                stash.push(original.to_string());
                out.push_str(&math_placeholder(idx));
                i = close + 1;
                at_line_start = false;
                continue;
            }
        }

        // Default: copy one byte verbatim. We only branch on ASCII delimiters
        // ($, `, \), so bytes >= 0x80 are UTF-8 continuation bytes from the
        // source — they must be appended raw, not via `b as char` (which would
        // reinterpret each byte as a Latin-1 code point and mojibake any
        // non-ASCII text). Writing the raw byte preserves the source's UTF-8
        // encoding; the final buffer is valid UTF-8 because `md` is.
        unsafe {
            out.as_mut_vec().push(b);
        }
        at_line_start = b == b'\n';
        i += 1;
    }

    (out, stash)
}

/// Restore math placeholders in rendered HTML.
fn restore_math(html: &str, stash: &[String]) -> String {
    if stash.is_empty() {
        return html.to_string();
    }
    let mut out = html.to_string();
    for (idx, original) in stash.iter().enumerate() {
        out = out.replace(&math_placeholder(idx), original);
    }
    out
}

/// If `i` is at the start of a fenced code block opener, return
/// `(byte_after_opener, fence_char, fence_len)`. Otherwise None.
fn detect_fence_open(s: &[u8], i: usize) -> Option<(usize, u8, usize)> {
    let n = s.len();
    let mut j = i;
    let mut spaces = 0;
    while j < n && s[j] == b' ' && spaces < 4 {
        j += 1;
        spaces += 1;
    }
    if spaces >= 4 || j >= n {
        return None;
    }
    let fc = s[j];
    if fc != b'`' && fc != b'~' {
        return None;
    }
    let start = j;
    while j < n && s[j] == fc {
        j += 1;
    }
    let len = j - start;
    if len < 3 {
        return None;
    }
    Some((j, fc, len))
}

/// True if `line` is a closing fence for an open fence of `fc`/`min_len`.
fn is_close_fence(line: &[u8], fc: u8, min_len: usize) -> bool {
    let mut i = 0;
    let mut spaces = 0;
    while i < line.len() && line[i] == b' ' && spaces < 4 {
        i += 1;
        spaces += 1;
    }
    if spaces >= 4 {
        return false;
    }
    let start = i;
    while i < line.len() && line[i] == fc {
        i += 1;
    }
    if i - start < min_len {
        return false;
    }
    while i < line.len() {
        match line[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            _ => return false,
        }
    }
    true
}

fn line_end(s: &[u8], i: usize) -> usize {
    s[i..]
        .iter()
        .position(|&c| c == b'\n')
        .map(|p| i + p + 1)
        .unwrap_or(s.len())
}

/// Find closing `$$` after `start`, honoring `\\` and `\$` escapes.
fn find_display_close(s: &[u8], start: usize) -> Option<usize> {
    let n = s.len();
    let mut j = start;
    while j + 1 < n {
        if s[j] == b'\\' {
            j += 2;
            continue;
        }
        if s[j] == b'$' && s[j + 1] == b'$' {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// KaTeX-style inline math opener: `$` followed by a non-whitespace,
/// non-`$` byte. Avoids triggering on prose like "$5 and $10".
fn is_inline_math_open(s: &[u8], i: usize) -> bool {
    if i + 1 >= s.len() {
        return false;
    }
    let nx = s[i + 1];
    if nx == b'$' {
        return false;
    }
    !nx.is_ascii_whitespace()
}

/// Find closing `$` for an inline span starting at `start`. Same line only.
/// Closing `$` must be preceded by non-whitespace and not followed by a digit
/// (KaTeX convention to avoid swallowing prices like "$5").
fn find_inline_close(s: &[u8], start: usize) -> Option<usize> {
    let n = s.len();
    let mut j = start;
    while j < n && s[j] != b'\n' {
        if s[j] == b'\\' && j + 1 < n {
            j += 2;
            continue;
        }
        if s[j] == b'$' {
            let prev_ok = j > start && !s[j - 1].is_ascii_whitespace();
            let next_ok = j + 1 >= n || !s[j + 1].is_ascii_digit();
            if prev_ok && next_ok {
                return Some(j);
            }
        }
        j += 1;
    }
    None
}

/// Phase 3 block-wrapping helper. Called at the end of every
/// match arm in [`render_html`] that emits a *diffable* block
/// (Markdown / Code / Mermaid / Callout — *not* the structural
/// markers ExerciseStart / SolutionStart).
///
/// Looks at the bytes pushed to `body` since `mark`, treats them
/// as the rendered chunk for one block, wraps them in
/// `<section class="rl-block" id="b-<hash>">…</section>`, and
/// replaces the chunk in `body`. ID is the low 32 bits of the
/// chunk's `DefaultHasher` digest rendered as 8 hex chars; if
/// the same hash already appeared in this render the suffix
/// `-N` disambiguates (per locked-in #14 of the plan, position
/// is the collision tiebreaker).
///
/// Empty / whitespace-only chunks emit nothing (matches the
/// existing renderer's behaviour for skipped blocks).
fn finalize_block(body: &mut String, mark: usize, counter: &mut HashMap<u64, usize>) {
    if body.len() <= mark {
        return;
    }
    let chunk_len = body.len() - mark;
    if body[mark..].chars().all(char::is_whitespace) {
        body.truncate(mark);
        return;
    }

    let mut hasher = DefaultHasher::new();
    body[mark..].hash(&mut hasher);
    let raw = hasher.finish();
    let prefix = format!("{:08x}", raw as u32);
    let n = counter.entry(raw).or_insert(0);
    let id = if *n == 0 {
        format!("b-{prefix}")
    } else {
        format!("b-{prefix}-{n}", n = *n)
    };
    *n += 1;

    // Splice: insert opening section tag at `mark`, append closing
    // tag. Using `String::insert_str` here means the chunk doesn't
    // need to be cloned out and back in.
    let open = format!("<section class=\"rl-block\" id=\"{id}\">\n");
    body.insert_str(mark, &open);
    let _ = chunk_len; // (kept for debugging — closing tag goes at end)
    body.push_str("</section>\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execute::Rendered;
    use rustlab_plot::Theme;

    fn test_theme() -> &'static ThemeColors {
        Theme::Dark.colors()
    }

    // ── escape_html ──

    #[test]
    fn escape_html_special_chars() {
        assert_eq!(
            escape_html("<b>\"a & b\"</b>"),
            "&lt;b&gt;&quot;a &amp; b&quot;&lt;/b&gt;"
        );
    }

    #[test]
    fn escape_html_passthrough() {
        assert_eq!(escape_html("hello world 123"), "hello world 123");
    }

    // ── strip_tags ──

    #[test]
    fn strip_tags_basic() {
        assert_eq!(strip_tags("<b>bold</b> text"), "bold text");
    }

    #[test]
    fn strip_tags_nested() {
        assert_eq!(strip_tags("<a href=\"#\"><em>link</em></a>"), "link");
    }

    #[test]
    fn strip_tags_no_tags() {
        assert_eq!(strip_tags("plain text"), "plain text");
    }

    // ── inject_heading_ids ──

    #[test]
    fn inject_heading_ids_h1() {
        let mut nav = String::new();
        let mut idx = 0;
        let result = inject_heading_ids("<h1>Title</h1>", &mut nav, &mut idx);
        assert!(result.contains("id=\"heading-1\""));
        assert!(nav.contains("href=\"#heading-1\""));
        assert!(nav.contains("class=\"h1\""));
        assert_eq!(idx, 1);
    }

    #[test]
    fn inject_heading_ids_multiple_levels() {
        let mut nav = String::new();
        let mut idx = 0;
        let html = "<h1>A</h1><h2>B</h2><h3>C</h3>";
        let result = inject_heading_ids(html, &mut nav, &mut idx);
        assert!(result.contains("id=\"heading-1\""));
        assert!(result.contains("id=\"heading-2\""));
        assert!(result.contains("id=\"heading-3\""));
        assert!(nav.contains("class=\"h1\""));
        assert!(nav.contains("class=\"h2\""));
        assert!(nav.contains("class=\"h3\""));
        assert_eq!(idx, 3);
    }

    #[test]
    fn inject_heading_ids_no_headings() {
        let mut nav = String::new();
        let mut idx = 0;
        let result = inject_heading_ids("<p>no headings</p>", &mut nav, &mut idx);
        assert_eq!(result, "<p>no headings</p>");
        assert!(nav.is_empty());
        assert_eq!(idx, 0);
    }

    #[test]
    fn inject_heading_ids_with_inner_tags() {
        let mut nav = String::new();
        let mut idx = 0;
        let result = inject_heading_ids("<h1><em>Styled</em> Title</h1>", &mut nav, &mut idx);
        assert!(result.contains("id=\"heading-1\""));
        // Nav text should be stripped of tags
        assert!(nav.contains("Styled Title"));
    }

    // ── is_string_quote ──

    #[test]
    fn string_quote_at_start() {
        let chars: Vec<char> = "'hello'".chars().collect();
        assert!(is_string_quote(&chars, 0));
    }

    #[test]
    fn transpose_after_paren() {
        let chars: Vec<char> = "x)'".chars().collect();
        assert!(!is_string_quote(&chars, 2));
    }

    #[test]
    fn transpose_after_identifier() {
        let chars: Vec<char> = "A'".chars().collect();
        assert!(!is_string_quote(&chars, 1));
    }

    #[test]
    fn string_quote_after_operator() {
        let chars: Vec<char> = "='hello'".chars().collect();
        assert!(is_string_quote(&chars, 1));
    }

    #[test]
    fn string_quote_after_space() {
        let chars: Vec<char> = " 'hello'".chars().collect();
        assert!(is_string_quote(&chars, 1));
    }

    // ── highlight_rustlab ──

    #[test]
    fn highlight_keywords() {
        let out = highlight_rustlab("if x end");
        assert!(out.contains("<span class=\"syn-kw\">if</span>"));
        assert!(out.contains("<span class=\"syn-kw\">end</span>"));
    }

    #[test]
    fn highlight_all_keywords() {
        for kw in KEYWORDS {
            let out = highlight_rustlab(kw);
            assert!(out.contains("syn-kw"), "keyword {kw} not highlighted");
        }
    }

    #[test]
    fn highlight_function_call() {
        let out = highlight_rustlab("plot(x)");
        assert!(out.contains("<span class=\"syn-fn\">plot</span>"));
    }

    #[test]
    fn highlight_identifier_not_function() {
        let out = highlight_rustlab("x = 1");
        assert!(!out.contains("syn-fn"));
        assert!(!out.contains("syn-kw"));
        assert_eq!(out.contains("x"), true);
    }

    #[test]
    fn highlight_numbers() {
        let out = highlight_rustlab("42");
        assert!(out.contains("<span class=\"syn-num\">42</span>"));
    }

    #[test]
    fn highlight_float() {
        let out = highlight_rustlab("3.14");
        assert!(out.contains("<span class=\"syn-num\">3.14</span>"));
    }

    #[test]
    fn highlight_scientific_notation() {
        let out = highlight_rustlab("1.5e-3");
        assert!(out.contains("<span class=\"syn-num\">1.5e-3</span>"));
    }

    #[test]
    fn highlight_complex_literal() {
        let out = highlight_rustlab("2.5j");
        assert!(out.contains("<span class=\"syn-num\">2.5j</span>"));
    }

    #[test]
    fn highlight_leading_dot_number() {
        let out = highlight_rustlab(".5");
        assert!(out.contains("<span class=\"syn-num\">.5</span>"));
    }

    #[test]
    fn highlight_string_double() {
        let out = highlight_rustlab("\"hello\"");
        assert!(out.contains("<span class=\"syn-str\">&quot;hello&quot;</span>"));
    }

    #[test]
    fn highlight_string_single() {
        let out = highlight_rustlab("x = 'world'");
        assert!(out.contains("<span class=\"syn-str\">'world'</span>"));
    }

    #[test]
    fn highlight_comment() {
        let out = highlight_rustlab("% a comment");
        assert!(out.contains("<span class=\"syn-com\">"));
        assert!(out.contains("a comment"));
    }

    #[test]
    fn highlight_comment_stops_at_newline() {
        let out = highlight_rustlab("% comment\nx = 1");
        // The comment span should not include the next line
        assert!(out.contains("</span>\nx"));
    }

    #[test]
    fn highlight_operators() {
        let out = highlight_rustlab("x + y");
        assert!(out.contains("<span class=\"syn-op\">+</span>"));
    }

    #[test]
    fn highlight_two_char_operators() {
        for op in &[".*", "./", ".^", "==", "~=", "<=", ">=", "&&", "||"] {
            let out = highlight_rustlab(op);
            // Should be a single span, not two separate ones
            assert!(
                out.contains(&format!(
                    "<span class=\"syn-op\">{}</span>",
                    op.replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;")
                )),
                "two-char op {op} not highlighted as unit"
            );
        }
    }

    #[test]
    fn highlight_transpose_not_string() {
        let out = highlight_rustlab("x'");
        // After identifier, ' is transpose — should NOT be a string
        assert!(!out.contains("syn-str"));
    }

    #[test]
    fn highlight_special_chars_escaped() {
        let out = highlight_rustlab("x < y & z");
        assert!(out.contains("&lt;"));
        assert!(out.contains("&amp;"));
    }

    #[test]
    fn highlight_empty() {
        assert_eq!(highlight_rustlab(""), "");
    }

    #[test]
    fn highlight_multiline() {
        let out = highlight_rustlab("for k = 1:3\n  disp(k)\nend");
        assert!(out.contains("<span class=\"syn-kw\">for</span>"));
        assert!(out.contains("<span class=\"syn-kw\">end</span>"));
        assert!(out.contains("<span class=\"syn-fn\">disp</span>"));
    }

    // ── render_html (integration) ──

    #[test]
    fn render_html_basic_structure() {
        let blocks = vec![Rendered::Markdown("# Hello".to_string())];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<title>Test</title>"));
        assert!(html.contains("class=\"prose\""));
        assert!(html.contains("Generated by rustlab-notebook"));
    }

    // ── Phase 3: stable block-id wrapping ──

    #[test]
    fn render_html_wraps_blocks_in_rl_block_section() {
        let blocks = vec![
            Rendered::Markdown("hello".to_string()),
            Rendered::Markdown("world".to_string()),
        ];
        let html = render_html("T", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        // Each prose block lives inside a rl-block section.
        let opens: Vec<_> = html.matches("<section class=\"rl-block\" id=\"b-").collect();
        assert_eq!(opens.len(), 2, "expected 2 block wrappers, full html:\n{html}");
        // The pre-existing prose div is preserved inside the section.
        assert!(html.contains("class=\"prose\""));
    }

    #[test]
    fn render_html_block_ids_suffix_on_collision() {
        let blocks = vec![
            Rendered::Markdown("dup".to_string()),
            Rendered::Markdown("dup".to_string()),
            Rendered::Markdown("unique".to_string()),
        ];
        let html = render_html("T", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        // The two `dup` blocks have identical content → identical
        // 8-char hashes → second gets the "-1" suffix.
        let suffixed = html.matches("\" id=\"b-").count();
        assert_eq!(suffixed, 3);
        assert!(
            html.matches("-1\">").count() >= 1,
            "expected a collision-suffixed id (…-1) in:\n{html}",
        );
    }

    #[test]
    fn render_html_block_ids_stable_across_renders() {
        let blocks = vec![Rendered::Markdown("stable content".to_string())];
        let h1 = render_html("T", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        let h2 = render_html("T", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        let id1 = h1.split("id=\"b-").nth(1).unwrap().split('"').next().unwrap();
        let id2 = h2.split("id=\"b-").nth(1).unwrap().split('"').next().unwrap();
        assert_eq!(id1, id2, "block id changed between identical renders");
    }

    #[test]
    fn render_html_code_block() {
        let blocks = vec![Rendered::Code {
            source: "x = 42".to_string(),
            text_output: "ans = 42".to_string(),
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
        }];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        assert!(html.contains("class=\"source\""));
        assert!(html.contains("class=\"output\""));
        assert!(html.contains("ans = 42"));
    }

    #[test]
    fn render_html_error_block() {
        let blocks = vec![Rendered::Code {
            source: "bad".to_string(),
            text_output: String::new(),
            error: Some("undefined variable".to_string()),
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
        }];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        assert!(html.contains("class=\"error\""));
        assert!(html.contains("undefined variable"));
    }

    #[test]
    fn render_html_hidden_block() {
        let blocks = vec![Rendered::Code {
            source: "secret = 42".to_string(),
            text_output: "ans = 42".to_string(),
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: true,
            details: None,
            grid_cols: None,
        }];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        // Source should not appear
        assert!(!html.contains("secret = 42"));
        assert!(!html.contains("class=\"source\""));
        // But output should still appear
        assert!(html.contains("ans = 42"));
    }

    #[test]
    fn render_html_empty_output_not_shown() {
        let blocks = vec![Rendered::Code {
            source: "x = 1;".to_string(),
            text_output: "   \n  ".to_string(), // whitespace only
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
        }];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        // Source shown, but no output div
        assert!(html.contains("class=\"source\""));
        assert!(!html.contains("class=\"output\""));
    }

    #[test]
    fn render_html_katex_included() {
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        assert!(html.contains("katex"));
        assert!(html.contains("auto-render"));
    }

    #[test]
    fn render_html_plotly_included() {
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        assert!(html.contains("plotly"));
    }

    #[test]
    fn render_html_nav_toggle() {
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        assert!(html.contains("nav-toggle"));
    }

    #[test]
    fn render_html_title_escaped() {
        let html = render_html("A <script> & \"test\"", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        assert!(html.contains("A &lt;script&gt; &amp; &quot;test&quot;"));
    }

    #[test]
    fn render_html_syntax_highlighting_in_code() {
        let blocks = vec![Rendered::Code {
            source: "for k = 1:10\n  plot(k)\nend".to_string(),
            text_output: String::new(),
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
        }];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        assert!(html.contains("syn-kw"));
        assert!(html.contains("syn-fn"));
        assert!(html.contains("syn-num"));
    }

    #[test]
    fn render_html_nav_from_headings() {
        let blocks = vec![Rendered::Markdown(
            "# Section One\n\n## Sub Section".to_string(),
        )];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        assert!(html.contains("heading-1"));
        assert!(html.contains("heading-2"));
        assert!(html.contains("Section One"));
        assert!(html.contains("Sub Section"));
    }

    // ── rewrite_md_links ──

    #[test]
    fn rewrite_md_links_basic() {
        assert_eq!(
            rewrite_md_links("See [filter](filter.md) for details"),
            "See [filter](filter.html) for details"
        );
    }

    #[test]
    fn rewrite_md_links_with_anchor() {
        assert_eq!(
            rewrite_md_links("[section](other.md#intro)"),
            "[section](other.html#intro)"
        );
    }

    #[test]
    fn rewrite_md_links_no_md() {
        let input = "No links here.";
        assert_eq!(rewrite_md_links(input), input);
    }

    #[test]
    fn rewrite_md_links_multiple() {
        assert_eq!(
            rewrite_md_links("[a](a.md) and [b](b.md)"),
            "[a](a.html) and [b](b.html)"
        );
    }

    #[test]
    fn render_html_rewrites_md_links() {
        let blocks = vec![Rendered::Markdown(
            "See [other](other.md) for details".to_string(),
        )];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        assert!(html.contains("other.html"));
        assert!(!html.contains("other.md"));
    }

    // ── protect_math / restore_math ──

    #[test]
    fn protect_math_display_preserves_double_backslash() {
        let src = r"text $$\begin{pmatrix}0 & 1 \\ 1 & 0\end{pmatrix}$$ more";
        let (rewritten, stash) = protect_math(src);
        assert_eq!(stash.len(), 1);
        assert!(stash[0].contains(r"\\"), "stashed math lost row separator");
        assert!(!rewritten.contains('$'), "delimiters should be removed");
    }

    #[test]
    fn protect_math_inline_basic() {
        let src = "the value $x = 1$ is set";
        let (rewritten, stash) = protect_math(src);
        assert_eq!(stash, vec!["$x = 1$".to_string()]);
        assert!(!rewritten.contains('$'));
    }

    #[test]
    fn protect_math_skips_whitespace_padded_dollars() {
        // KaTeX rule: opening $ followed by whitespace is not math.
        let src = "I have $ 5 dollars";
        let (_, stash) = protect_math(src);
        assert!(stash.is_empty());
    }

    #[test]
    fn protect_math_skips_prices() {
        // Closing $ followed by digit is not math.
        let src = "costs $5 and $10";
        let (_, stash) = protect_math(src);
        assert!(stash.is_empty());
    }

    #[test]
    fn protect_math_respects_escaped_dollar() {
        let src = r"price is \$5 even";
        let (rewritten, stash) = protect_math(src);
        assert!(stash.is_empty());
        assert!(rewritten.contains(r"\$5"));
    }

    #[test]
    fn protect_math_skips_inside_fenced_code() {
        let src = "```\n$$ a \\\\ b $$\n```\nafter";
        let (rewritten, stash) = protect_math(src);
        assert!(
            stash.is_empty(),
            "math inside code fence must not be stashed"
        );
        assert!(rewritten.contains("$$ a \\\\ b $$"));
    }

    #[test]
    fn protect_math_skips_inside_inline_code() {
        let src = "use `$$x$$` for display math";
        let (_, stash) = protect_math(src);
        assert!(stash.is_empty());
    }

    #[test]
    fn protect_math_multiline_display() {
        let src = "intro\n$$\nA = \\begin{pmatrix}\n1 & 2 \\\\\n3 & 4\n\\end{pmatrix}\n$$\noutro";
        let (rewritten, stash) = protect_math(src);
        assert_eq!(stash.len(), 1);
        assert!(stash[0].contains("\\\\"));
        assert!(rewritten.contains("intro\n"));
        assert!(rewritten.contains("\noutro"));
    }

    #[test]
    fn restore_math_round_trip() {
        let src = r"$$a \\ b$$";
        let (rewritten, stash) = protect_math(src);
        let restored = restore_math(&rewritten, &stash);
        assert_eq!(restored, src);
    }

    #[test]
    fn render_html_preserves_matrix_row_separator() {
        let blocks = vec![Rendered::Markdown(
            r"$$\begin{pmatrix}0 & 1 \\ 1 & 0\end{pmatrix}$$".to_string(),
        )];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        // The `\\` must reach the rendered HTML so KaTeX can split rows.
        assert!(
            html.contains(r"\\"),
            "matrix row separator lost; KaTeX will collapse rows"
        );
    }

    #[test]
    fn render_html_callout_preserves_math_backslashes() {
        let blocks = vec![Rendered::Callout {
            kind: CalloutKind::Note,
            title: None,
            content: r"see $$a \\ b$$".to_string(),
        }];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        assert!(html.contains(r"\\"));
    }

    #[test]
    fn protect_math_unclosed_display_left_alone() {
        let src = "open $$ but no close";
        let (rewritten, stash) = protect_math(src);
        assert!(stash.is_empty());
        assert_eq!(rewritten, src);
    }

    #[test]
    fn protect_math_aligned_environment_preserves_each_row() {
        let src = r"$$\begin{aligned} a &= 1 \\ b &= 2 \\ c &= 3 \end{aligned}$$";
        let (_, stash) = protect_math(src);
        assert_eq!(stash.len(), 1);
        assert_eq!(
            stash[0].matches(r"\\").count(),
            2,
            "expected 2 row separators in aligned environment, got {:?}",
            stash[0]
        );
    }

    #[test]
    fn protect_math_inline_smallmatrix_preserves_separator() {
        let src = r"see $\begin{smallmatrix}a \\ b\end{smallmatrix}$ inline";
        let (_, stash) = protect_math(src);
        assert_eq!(stash.len(), 1);
        assert!(
            stash[0].contains(r"\\"),
            "inline smallmatrix lost row separator: {:?}",
            stash[0]
        );
    }

    #[test]
    fn protect_math_cases_preserves_each_branch() {
        let src = r"$$f(x) = \begin{cases} 0 & x<0 \\ 1 & x \ge 0 \end{cases}$$";
        let (_, stash) = protect_math(src);
        assert_eq!(stash.len(), 1);
        assert_eq!(
            stash[0].matches(r"\\").count(),
            1,
            "cases environment lost branch separator: {:?}",
            stash[0]
        );
    }

    #[test]
    fn protect_math_empty_display_span() {
        // `$$$$` is a degenerate empty display span. Whatever protect_math
        // does with it, the round-trip must not panic and restore_math must
        // reconstruct the input verbatim.
        let src = "before $$$$ after";
        let (rewritten, stash) = protect_math(src);
        let restored = restore_math(&rewritten, &stash);
        assert_eq!(restored, src);
    }

    // ── Cross-notebook navigation (Option B) ──

    #[test]
    fn render_html_no_nav_for_single_file() {
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None);
        // Single-file renders keep the sidebar layout, no topbar.
        assert!(!html.contains("class=\"page-nav\""));
        assert!(!html.contains("&larr; Index"));
        assert!(!html.contains("class=\"topbar\""));
        assert!(!html.contains("class=\"topbar-layout\""));
        assert!(html.contains("<nav class=\"sidebar\">"));
        assert!(html.contains("class=\"nav-title\""));
    }

    #[test]
    fn render_html_topbar_breadcrumb_when_nav_provided() {
        let nav = NotebookNav {
            index_href: Some("index.html".to_string()),
            prev: None,
            next: None,
        };
        let html = render_html("Filter Analysis", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), Some(&nav));
        // Topbar present with breadcrumb.
        assert!(html.contains("class=\"topbar-layout\""));
        assert!(html.contains("class=\"topbar\""));
        assert!(html.contains("href=\"index.html\""));
        assert!(html.contains("&larr; Index"));
        assert!(html.contains("class=\"sep\""));
        assert!(html.contains("class=\"current\""));
        assert!(html.contains("Filter Analysis"));
        // Sidebar removed.
        assert!(!html.contains("<nav class=\"sidebar\">"));
        assert!(!html.contains("class=\"nav-title\""));
        assert!(!html.contains("class=\"nav-toggle\""));
    }

    #[test]
    fn render_html_topbar_escapes_current_title() {
        let nav = NotebookNav {
            index_href: Some("index.html".to_string()),
            prev: None,
            next: None,
        };
        let html = render_html("A <script> & \"x\"", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), Some(&nav));
        assert!(html.contains("A &lt;script&gt; &amp; &quot;x&quot;"));
    }

    #[test]
    fn render_html_footer_nav_middle_page() {
        let nav = NotebookNav {
            index_href: Some("index.html".to_string()),
            prev: Some(("Intro".to_string(), "intro.html".to_string())),
            next: Some(("Analysis".to_string(), "analysis.html".to_string())),
        };
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), Some(&nav));
        assert!(html.contains("class=\"page-nav\""));
        assert!(html.contains("class=\"prev\""));
        assert!(html.contains("href=\"intro.html\""));
        assert!(html.contains("Intro"));
        assert!(html.contains("class=\"index\""));
        assert!(html.contains("class=\"next\""));
        assert!(html.contains("href=\"analysis.html\""));
        assert!(html.contains("Analysis"));
    }

    #[test]
    fn render_html_footer_nav_first_page_no_prev() {
        let nav = NotebookNav {
            index_href: Some("index.html".to_string()),
            prev: None,
            next: Some(("Next One".to_string(), "next.html".to_string())),
        };
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), Some(&nav));
        assert!(html.contains("class=\"page-nav\""));
        assert!(!html.contains("class=\"prev\""));
        assert!(html.contains("class=\"next\""));
    }

    #[test]
    fn render_html_footer_nav_last_page_no_next() {
        let nav = NotebookNav {
            index_href: Some("index.html".to_string()),
            prev: Some(("Earlier".to_string(), "earlier.html".to_string())),
            next: None,
        };
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), Some(&nav));
        assert!(html.contains("class=\"prev\""));
        assert!(!html.contains("class=\"next\""));
    }

    #[test]
    fn render_html_footer_nav_escapes_titles() {
        let nav = NotebookNav {
            index_href: Some("index.html".to_string()),
            prev: Some(("A & <b>".to_string(), "p.html".to_string())),
            next: None,
        };
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), Some(&nav));
        assert!(html.contains("A &amp; &lt;b&gt;"));
        assert!(!html.contains("<b>"));
    }

    // ── Mermaid blocks ──

    fn mermaid_plot_dir(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "rustlab_render_html_mermaid_{}_{}",
            std::process::id(),
            tag,
        ));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn render_html_mermaid_inline_svg() {
        let dir = mermaid_plot_dir("inline");
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B\n".to_string(),
            hidden: false,
            details: None,
            caption: None,
        }];
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None);
        assert!(html.contains("<figure class=\"mermaid\">"));
        assert!(html.contains("<svg"), "expected inline <svg> tag");
        assert!(!html.contains("<?xml"), "XML decl should be stripped");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_html_mermaid_no_cdn_script() {
        // Regression: must never re-introduce a CDN dependency for Mermaid.
        let dir = mermaid_plot_dir("nocdn");
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B\n".to_string(),
            hidden: false,
            details: None,
            caption: None,
        }];
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None);
        assert!(!html.contains("cdn.jsdelivr.net/npm/mermaid"));
        assert!(!html.contains("mermaid.initialize("));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn render_html_mermaid_caption_emitted() {
        let dir = mermaid_plot_dir("caption");
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B\n".to_string(),
            hidden: false,
            details: None,
            caption: Some("Signal flow".to_string()),
        }];
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None);
        assert!(html.contains("<figcaption>Signal flow</figcaption>"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn render_html_mermaid_details_wrap() {
        let dir = mermaid_plot_dir("details");
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B\n".to_string(),
            hidden: false,
            details: Some("Architecture".to_string()),
            caption: None,
        }];
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None);
        assert!(html.contains("<details class=\"code-details\">"));
        assert!(html.contains("<summary>Architecture</summary>"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_html_mermaid_hidden_omits() {
        let dir = mermaid_plot_dir("hidden");
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B\n".to_string(),
            hidden: true,
            details: None,
            caption: None,
        }];
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None);
        assert!(!html.contains("<figure class=\"mermaid\">"));
        assert!(!html.contains("<svg"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn render_html_multiple_mermaid_blocks() {
        let dir = mermaid_plot_dir("multi");
        let blocks = vec![
            Rendered::Mermaid {
                source: "flowchart LR\n  A --> B\n".to_string(),
                hidden: false,
                details: None,
                caption: None,
            },
            Rendered::Mermaid {
                source: "flowchart TD\n  X --> Y\n".to_string(),
                hidden: false,
                details: None,
                caption: None,
            },
        ];
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None);
        let figs = html.matches("<figure class=\"mermaid\">").count();
        assert_eq!(figs, 2, "expected two mermaid figures, got {figs}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(feature = "mermaid"))]
    #[test]
    fn render_html_mermaid_feature_disabled_falls_back_to_source() {
        let dir = mermaid_plot_dir("disabled");
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B\n".to_string(),
            hidden: false,
            details: None,
            caption: None,
        }];
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None);
        assert!(html.contains("class=\"mermaid-source\""));
        assert!(html.contains("flowchart LR"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plot_container_height_scales_with_rows() {
        assert_eq!(plot_container_height(0), 450);
        assert_eq!(plot_container_height(1), 450);
        assert_eq!(plot_container_height(2), 800);
        assert_eq!(plot_container_height(3), 1150);
        assert_eq!(plot_container_height(4), 1500);
    }

    // ── GFM-superset markdown features (Phase B) ──
    //
    // These pin the parser flag set in `notebook_md_options()` so anyone
    // who turns one off accidentally fails the test. They exercise the
    // canonical GFM features GitHub and Obsidian both render natively.

    fn render_md(src: &str) -> String {
        let blocks = vec![Rendered::Markdown(src.to_string())];
        render_html(
            "T",
            &blocks,
            &std::path::PathBuf::from("/tmp/rustlab_test_plots"),
            "plots",
            test_theme(),
            None,
        )
    }

    #[test]
    fn render_html_footnote_reference_and_definition() {
        let html = render_md("Cite[^src].\n\n[^src]: Smith 2024.");
        assert!(
            html.contains(r##"href="#src""##) || html.contains(r##"href="#fn-src""##),
            "footnote reference link missing: {html}"
        );
        assert!(
            html.contains("Smith 2024"),
            "footnote definition body missing: {html}"
        );
    }

    #[test]
    fn render_html_task_list_unchecked() {
        let html = render_md("- [ ] todo");
        assert!(
            html.contains("type=\"checkbox\""),
            "task-list checkbox missing: {html}"
        );
        assert!(
            !html.contains("checked=\"\""),
            "unchecked box should not be checked: {html}"
        );
    }

    #[test]
    fn render_html_task_list_checked() {
        let html = render_md("- [x] done");
        assert!(html.contains("type=\"checkbox\""), "checkbox missing: {html}");
        assert!(html.contains("checked"), "checked attr missing: {html}");
    }

    #[test]
    fn render_html_heading_explicit_id() {
        // `{#custom}` after a heading produces `id="custom"` rather than
        // the auto-slug. Note: `inject_heading_ids` rewrites the id, so
        // we just assert the explicit slug shows up somewhere usable.
        let html = render_md("# Filter Analysis {#filters}");
        assert!(
            html.contains("Filter Analysis"),
            "heading text missing: {html}"
        );
        assert!(
            html.contains(r#"id="filters""#),
            "explicit heading id missing: {html}"
        );
    }

    // ── Callout rendering for GFM-native kinds + custom title ──

    fn render_callout(kind: CalloutKind, title: Option<&str>, content: &str) -> String {
        let blocks = vec![Rendered::Callout {
            kind,
            title: title.map(String::from),
            content: content.to_string(),
        }];
        render_html(
            "T",
            &blocks,
            &std::path::PathBuf::from("/tmp/rustlab_test_plots"),
            "plots",
            test_theme(),
            None,
        )
    }

    #[test]
    fn render_html_callout_important_kind() {
        let html = render_callout(CalloutKind::Important, None, "key fact");
        assert!(html.contains("callout-important"));
        assert!(html.contains(">Important<"));
    }

    #[test]
    fn render_html_callout_caution_kind() {
        let html = render_callout(CalloutKind::Caution, None, "danger");
        assert!(html.contains("callout-caution"));
        assert!(html.contains(">Caution<"));
    }

    #[test]
    fn render_html_callout_custom_title_overrides_label() {
        let html = render_callout(CalloutKind::Tip, Some("Heads up"), "body");
        assert!(html.contains(">Heads up<"));
        assert!(!html.contains(">Tip<"));
    }

    // ── Wikilink / embed transform (Phase C) ──

    #[test]
    fn wikilink_simple() {
        assert_eq!(transform_wikilinks("see [[Foo]]."), "see [Foo](Foo.md).");
    }

    #[test]
    fn wikilink_with_alias() {
        assert_eq!(
            transform_wikilinks("see [[Foo|the bar]]."),
            "see [the bar](Foo.md)."
        );
    }

    #[test]
    fn wikilink_with_anchor() {
        assert_eq!(
            transform_wikilinks("see [[Foo#Section Two]]."),
            "see [Foo § Section Two](Foo.md#section-two)."
        );
    }

    #[test]
    fn wikilink_alias_and_anchor() {
        assert_eq!(
            transform_wikilinks("see [[Foo#Section|the bit]]."),
            "see [the bit](Foo.md#section)."
        );
    }

    #[test]
    fn wikilink_keeps_existing_extension() {
        // `[[diagram.svg]]` already has an extension — don't append `.md`.
        assert_eq!(
            transform_wikilinks("see [[diagram.svg]]"),
            "see [diagram.svg](diagram.svg)"
        );
    }

    #[test]
    fn embed_simple() {
        assert_eq!(
            transform_wikilinks("![[image.png]]"),
            "![](image.png)"
        );
    }

    #[test]
    fn embed_with_alt() {
        assert_eq!(
            transform_wikilinks("![[image.png|alt text]]"),
            "![alt text](image.png)"
        );
    }

    #[test]
    fn wikilink_inside_inline_code_left_alone() {
        assert_eq!(
            transform_wikilinks("write `[[Foo]]` for a wikilink"),
            "write `[[Foo]]` for a wikilink"
        );
    }

    #[test]
    fn wikilink_inside_fenced_code_left_alone() {
        let src = "```\n[[Foo]]\n```\nThen [[Bar]].";
        let out = transform_wikilinks(src);
        assert!(out.contains("```\n[[Foo]]\n```"));
        assert!(out.contains("[Bar](Bar.md)"));
    }

    #[test]
    fn wikilink_unmatched_close_left_alone() {
        // No closing `]]` on the line — pass through unchanged.
        assert_eq!(
            transform_wikilinks("see [[Foo and stop"),
            "see [[Foo and stop"
        );
    }

    #[test]
    fn wikilink_html_pipeline_resolves_to_html() {
        // Source `[[Foo]]` round-trips through the HTML pipeline as a link
        // to `Foo.html` (the existing `rewrite_md_links` makes the swap).
        let blocks = vec![Rendered::Markdown("see [[Foo]] for details.".to_string())];
        let html = render_html(
            "T",
            &blocks,
            &std::path::PathBuf::from("/tmp/rustlab_test_plots"),
            "plots",
            test_theme(),
            None,
        );
        assert!(html.contains(r#"href="Foo.html""#), "expected .html href: {html}");
        assert!(html.contains(">Foo</a>"));
    }

    #[test]
    fn wikilink_preserves_utf8_around_transform() {
        // Em-dash, super/subscript digits, and other multi-byte UTF-8 must
        // survive the byte-level scan untouched. Regression for an early
        // byte-as-char emit that produced mojibake on em-dashes.
        let src = "intro — see [[Foo]] for 10⁵ samples ≈ ε.";
        let out = transform_wikilinks(src);
        assert_eq!(out, "intro — see [Foo](Foo.md) for 10⁵ samples ≈ ε.");
    }

    #[test]
    fn slugify_matches_github_anchor_style() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("Already-dashed"), "already-dashed");
        assert_eq!(slugify("with punctuation!?"), "with-punctuation");
        assert_eq!(slugify("multi   spaces"), "multi-spaces");
    }

    #[test]
    fn notebook_md_options_includes_gfm_superset() {
        let opts = notebook_md_options();
        assert!(opts.contains(Options::ENABLE_TABLES));
        assert!(opts.contains(Options::ENABLE_STRIKETHROUGH));
        assert!(opts.contains(Options::ENABLE_FOOTNOTES));
        assert!(opts.contains(Options::ENABLE_TASKLISTS));
        assert!(opts.contains(Options::ENABLE_HEADING_ATTRIBUTES));
    }
}
