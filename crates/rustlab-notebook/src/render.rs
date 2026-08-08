use crate::execute::Rendered;
use crate::parse::CalloutKind;
use crate::widget::{WidgetDecl, WidgetKind};
use crate::NotebookNav;
use rustlab_script::WidgetValue;
use pulldown_cmark::{html::push_html, Event, Options, Parser, Tag, TagEnd};
use rustlab_plot::render_animation_inline;
use rustlab_plot::render_figure_plotly_div;
use rustlab_plot::{NotebookAnimationFormat, ThemeColors};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;

/// How cross-notebook `.md` link destinations resolve in HTML output.
///
/// The static build and the watch server publish notebooks at different
/// addresses — `<stem>.html` siblings vs `/n/<slug>` routes — so the same
/// `[next](02-filter.md)` source must emit different hrefs per mode. The
/// rewrite happens in the pulldown-cmark event stream (see
/// [`markdown_to_html_linked`]), which is what scopes it to real link
/// destinations: code spans, fenced blocks, titles, and reference-style
/// definitions are all handled by the parser, not by string matching.
///
/// Only destinations that are relative (no URL scheme, no leading `/`) and
/// end in `.md` before an optional `#fragment` are candidates. Everything
/// else — external URLs, absolute paths, images, anchors — passes through
/// untouched. A candidate that cannot be resolved (dangling target, a
/// `_partial.md`, a path outside the collection) is left exactly as
/// written: a visibly broken `.md` link beats a manufactured `.html` one
/// that 404s while looking intentional.
#[derive(Clone, Debug)]
pub enum LinkMode {
    /// Static HTML render: `foo.md` → `foo.html`.
    ///
    /// `known` is the set of collection-relative sources the build actually
    /// emits (listable notebooks plus `index.md`, which the directory build
    /// hoists into `index.html`). `Some` (directory renders) rewrites only
    /// members and leaves the rest as written; `None` (single-file renders)
    /// rewrites unconditionally — a lone file has no collection to check
    /// against, and the sibling may well be rendered separately.
    Static { known: Option<HashSet<String>> },
    /// Watch server: `foo.md` → `/n/<slug>`.
    ///
    /// `slugs` maps normalized collection-root-relative paths
    /// (`"ch1/notes.md"`) to server slugs. Keyed by path, not stem: the
    /// server walk is recursive and same-stem files in different
    /// directories hold distinct `-N` suffixed slugs. `current_rel_dir` is
    /// the linking notebook's directory relative to the root (`""` at the
    /// root), used to resolve `./` and `../`. Root `index.md` maps to `/`
    /// (it is the index page's body, not a notebook — it has no slug).
    Server {
        slugs: HashMap<String, String>,
        current_rel_dir: String,
    },
}

impl LinkMode {
    /// The mode for a standalone single-file static render.
    pub fn single_file() -> Self {
        LinkMode::Static { known: None }
    }
}

/// Rewrite one link destination per [`LinkMode`], or `None` to leave it
/// exactly as written.
fn rewrite_link_dest(dest: &str, mode: &LinkMode) -> Option<String> {
    // Scheme'd (`https:`, `mailto:`), protocol-relative (`//host/x`),
    // absolute (`/docs/x`), and pure-fragment (`#x`) destinations are not
    // notebook references.
    if dest.starts_with('/') || dest.starts_with('#') || has_url_scheme(dest) {
        return None;
    }
    let (path, fragment) = match dest.find('#') {
        Some(at) => (&dest[..at], &dest[at..]),
        None => (dest, ""),
    };
    let stem = path.strip_suffix(".md")?;
    match mode {
        LinkMode::Static { known } => {
            let normalized = normalize_rel_path("", path)?;
            if let Some(known) = known {
                if !known.contains(&normalized) {
                    return None;
                }
            }
            Some(format!("{stem}.html{fragment}"))
        }
        LinkMode::Server {
            slugs,
            current_rel_dir,
        } => {
            let normalized = normalize_rel_path(current_rel_dir, path)?;
            if normalized == "index.md" {
                // The index page's body, hoisted to the server root.
                return Some(format!("/{fragment}"));
            }
            let slug = slugs.get(&normalized)?;
            Some(format!("/n/{slug}{fragment}"))
        }
    }
}

/// Does `dest` start with a URL scheme (`scheme:` per RFC 3986) before any
/// path character?
fn has_url_scheme(dest: &str) -> bool {
    let mut chars = dest.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for c in chars {
        match c {
            ':' => return true,
            c if c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.') => continue,
            _ => return false,
        }
    }
    false
}

/// Join `target` onto `base_dir` and normalize `.`/`..` components, all in
/// `/`-separated collection-relative terms. `None` when `..` escapes the
/// collection root — such a link cannot be resolved against the listing.
fn normalize_rel_path(base_dir: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = base_dir.split('/').filter(|p| !p.is_empty()).collect();
    for comp in target.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

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
///
/// `link` chooses how `.md` cross-notebook references resolve — static
/// `.html` siblings or server `/n/<slug>` routes. See [`LinkMode`].
pub fn render_html(
    title: &str,
    blocks: &[Rendered],
    plot_dir: &Path,
    plot_href_prefix: &str,
    theme: &ThemeColors,
    nav: Option<&NotebookNav>,
    link: &LinkMode,
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
    // Executable-block ordinal (Code + Mermaid, hidden included) — must
    // stay aligned with `execute_core`'s cache-slot indexing. Code
    // sections are stamped with it (`data-code-idx`) so the interactive
    // server's ▶ Run can address the block; inert in static output.
    let mut exec_idx = 0usize;

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
                // (wikilinks emit `.md` targets for the link resolver)
                let md = transform_wikilinks(md);
                // Convert markdown to HTML (math-protected shared pipeline),
                // resolving cross-notebook `.md` links per `link` mode
                let html = markdown_to_html_linked(&md, Some(link));

                // Extract headings for nav and inject IDs
                let html = inject_heading_ids(&html, &mut nav_items, &mut heading_idx);

                body.push_str("<div class=\"prose\">\n");
                body.push_str(&html);
                body.push_str("</div>\n");
                finalize_block(&mut body, mark, &mut block_id_counter, "");
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
                let attrs = format!(" data-code-idx=\"{exec_idx}\"");
                finalize_block(&mut body, mark, &mut block_id_counter, &attrs);
                exec_idx += 1;
            }
            Rendered::Mermaid {
                source,
                hidden,
                details,
                caption,
            } => {
                // Mermaid occupies a cache slot even when hidden — advance
                // the ordinal before the skip so code stamps stay aligned.
                exec_idx += 1;
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
                finalize_block(&mut body, mark, &mut block_id_counter, "");
            }
            Rendered::Widget { decl, value } => {
                let mark = body.len();
                body.push_str(&render_widget_html(decl, value));
                finalize_block(&mut body, mark, &mut block_id_counter, "");
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
                let html = markdown_to_html_linked(&md, Some(link));
                body.push_str(&html);
                body.push_str("</div>\n");
                finalize_block(&mut body, mark, &mut block_id_counter, "");
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

    // One layout for both modes. The two navs answer different questions and
    // are both always present: the topbar moves BETWEEN notebooks, the
    // sidebar moves WITHIN one. Previously they were mutually exclusive, so a
    // directory render dropped the in-page TOC entirely — the heading anchors
    // were still emitted, and the sidebar CSS still shipped, with nothing
    // linking to them. A reader should not be able to tell from the page how
    // the file was rendered.
    let topbar_block = {
        let (prev_link, index_link, next_link) = match nav {
            Some(n) => (
                n.prev
                    .as_ref()
                    .map(|(t, href)| {
                        format!(
                            "<a class=\"prev\" href=\"{href}\">&larr; {t}</a>",
                            href = escape_html(href),
                            t = escape_html(t),
                        )
                    })
                    .unwrap_or_default(),
                n.index_href
                    .as_ref()
                    .map(|href| {
                        format!(
                            "<a class=\"index\" href=\"{href}\">Index</a><span class=\"sep\">/</span>",
                            href = escape_html(href),
                        )
                    })
                    .unwrap_or_default(),
                n.next
                    .as_ref()
                    .map(|(t, href)| {
                        format!(
                            "<a class=\"next\" href=\"{href}\">{t} &rarr;</a>",
                            href = escape_html(href),
                            t = escape_html(t),
                        )
                    })
                    .unwrap_or_default(),
            ),
            // Single-file render: same chrome, just nothing to page to.
            None => (String::new(), String::new(), String::new()),
        };
        format!(
            "<header class=\"topbar\">{prev}<span class=\"crumb\">{index}<span class=\"current\">{title}</span></span>{next}</header>\n",
            prev = prev_link,
            index = index_link,
            next = next_link,
            title = escape_html(title),
        )
    };

    // A notebook with no h1–h3 has nothing to put in a TOC; emitting the
    // sidebar anyway costs 220px of empty chrome. `no-toc` lets main reclaim it.
    let has_toc = !nav_items.trim().is_empty();
    let body_class = if has_toc { "" } else { " class=\"no-toc\"" };
    let sidebar_block = if has_toc {
        format!(
            "<button class=\"nav-toggle\" onclick=\"document.querySelector('nav.sidebar').classList.toggle('open')\" aria-label=\"Toggle navigation\">&#9776;</button>\n\
             <nav class=\"sidebar\">\n  <div class=\"nav-title\">{title}</div>\n{nav_items}</nav>\n",
            title = escape_html(title),
            nav_items = nav_items,
        )
    } else {
        String::new()
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
      {{left: '\\[', right: '\\]', display: true}},
      {{left: '\\(', right: '\\)', display: false}}
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
  /* Height of the fixed topbar; the sidebar and main both clear it. */
  :root {{
    --topbar-h: 2.6rem;
  }}
  /* ── Navigation sidebar (in-page TOC) ── */
  nav.sidebar {{
    position: fixed;
    top: var(--topbar-h);
    left: 0;
    width: 220px;
    height: calc(100vh - var(--topbar-h));
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
    /* Headings can be long, and KaTeX typesets any inline math in them
       (auto-render walks the whole body, sidebar included). Wrap rather
       than let either overflow the fixed 220px column. */
    overflow-wrap: anywhere;
  }}
  nav.sidebar .katex {{
    font-size: 0.95em;
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
    /* Collection pages used to centre their column (`margin: 0 auto`); keep
       that on wide displays rather than stranding the prose against the
       sidebar with dead space to its right. */
    margin-right: auto;
    flex: 1;
    padding: calc(var(--topbar-h) + 2rem) 2.5rem 2rem;
    max-width: 960px;
    min-width: 0;
  }}
  /* ── Topbar (between-notebook nav) ──
     Fixed rather than sticky: it spans the full width above the sidebar, so
     it cannot participate in the body's flex row. */
  .topbar {{
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: var(--topbar-h);
    z-index: 150;
    background: {bg_secondary};
    border-bottom: 1px solid {border};
    padding: 0 1.2rem;
    font-size: 0.85rem;
    color: {text_dim};
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }}
  .topbar a {{
    color: {accent_secondary};
    text-decoration: none;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }}
  .topbar a:hover {{
    text-decoration: underline;
  }}
  /* Prev/next flank the breadcrumb and yield space to it when cramped. */
  .topbar a.prev, .topbar a.next {{
    flex: 0 1 auto;
    max-width: 22%;
  }}
  .topbar a.next {{
    margin-left: auto;
    text-align: right;
  }}
  .topbar .crumb {{
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
    /* Claim the leftover width. Flex items default to min-width:auto and
       refuse to shrink below their content, so without this the fixed-size
       prev/next links won and the title — the one label saying where you
       are — collapsed to nothing on a narrow screen. `.current` keeps
       min-width:0 so it ellipsises instead. */
    flex: 1 1 auto;
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
  /* Anchor targets must clear the fixed topbar. Without this a link scrolls
     its target to y=0, which is behind the bar — the heading you clicked is
     the one thing you cannot see. Applies to EVERY id, not just the
     generated `heading-N` ones: explicit `{{#anchor}}` headings, footnote
     back-references and block ids are all link targets too. */
  [id] {{
    scroll-margin-top: calc(var(--topbar-h) + 0.75rem);
  }}
  /* No headings, no sidebar: give the content the full width back. */
  body.no-toc main {{
    margin-left: 0;
    margin-right: auto;
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
  /* ── Interactive widgets ── */
  .rl-widget {{
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin: 0.75rem 0;
    padding: 0.6rem 0.9rem;
    border: 1px solid {border};
    border-radius: 6px;
    background: {bg_secondary};
  }}
  .rl-widget-label {{
    font-weight: 600;
    font-size: 0.9rem;
    white-space: nowrap;
  }}
  .rl-widget-input {{
    flex: 1;
    accent-color: {accent_primary};
  }}
  .rl-widget-value {{
    font-variant-numeric: tabular-nums;
    min-width: 4ch;
    text-align: right;
    color: {accent_primary};
  }}
  .rl-widget-options {{
    flex-wrap: wrap;
  }}
  .rl-widget-choice {{
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.9rem;
    white-space: nowrap;
  }}
  .rl-widget input[type="number"] {{
    flex: 0 0 auto;
    width: 7rem;
    padding: 0.2rem 0.4rem;
    border: 1px solid {border};
    border-radius: 4px;
    background: {bg};
    color: {text};
  }}
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
      padding: calc(var(--topbar-h) + 1rem) 1rem 2rem;
    }}
    /* Leave room for the hamburger, which sits over the topbar's left end. */
    .topbar {{
      padding-left: 3rem;
    }}
    .topbar a.prev, .topbar a.next {{
      max-width: 28%;
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
    let class = match kind {
        CalloutKind::Note => "note",
        CalloutKind::Tip => "tip",
        CalloutKind::Important => "important",
        CalloutKind::Warning => "warning",
        CalloutKind::Caution => "caution",
    };
    (class, kind.default_label())
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

/// Parse `md` with `opts`, demoting GFM strikethrough spans delimited by a
/// *single* tilde back to literal `~` text events.
///
/// pulldown-cmark's `ENABLE_STRIKETHROUGH` accepts one-tilde runs (`~word~`)
/// as well as `~~word~~`, so prose like "swap ~this~ out" renders struck
/// through. Only double-tilde spans should be strikethrough; the parser has
/// no option for that, so the delimiter width is checked against each span's
/// source range instead.
pub(crate) fn parse_single_tilde_safe<'a>(md: &'a str, opts: Options) -> Vec<Event<'a>> {
    let bytes = md.as_bytes();
    let mut events = Vec::new();
    // Parallel to the parser's open-strikethrough nesting: `true` entries are
    // single-tilde spans being demoted to literal text.
    let mut demoted: Vec<bool> = Vec::new();
    for (event, range) in Parser::new_ext(md, opts).into_offset_iter() {
        match event {
            Event::Start(Tag::Strikethrough) => {
                // The range covers the whole span, delimiters included.
                let single = range.end > range.start + 1
                    && bytes[range.start] == b'~'
                    && bytes[range.start + 1] != b'~';
                demoted.push(single);
                if single {
                    events.push(Event::Text("~".into()));
                } else {
                    events.push(event);
                }
            }
            Event::End(TagEnd::Strikethrough) => {
                if demoted.pop().unwrap_or(false) {
                    events.push(Event::Text("~".into()));
                } else {
                    events.push(event);
                }
            }
            other => events.push(other),
        }
    }
    events
}

/// Shared markdown → HTML pipeline used for HTML prose, callouts, and the
/// JSON target's `html` fields: math spans are stashed before CommonMark can
/// eat their backslashes (see [`protect_math`]), single-tilde strikethrough
/// is demoted to literal text, and math is restored with the currency-safe
/// `\(…\)` / `\[…\]` delimiters.
pub(crate) fn markdown_to_html(md: &str) -> String {
    markdown_to_html_linked(md, None)
}

/// [`markdown_to_html`] with cross-notebook link resolution.
///
/// Destinations are rewritten on `Event::Start(Tag::Link)` — after the
/// parser has resolved reference-style definitions and separated titles,
/// and only for real links: text, code spans, and fenced blocks never
/// produce link events, so `` `[x](a.md)` `` stays byte-identical. Images
/// (`Tag::Image`) are deliberately untouched — `.md` is not an image
/// format, and embeds were already expanded upstream.
pub(crate) fn markdown_to_html_linked(md: &str, link: Option<&LinkMode>) -> String {
    let (protected, math) = protect_math(md);
    let mut events = parse_single_tilde_safe(&protected, notebook_md_options());
    if let Some(mode) = link {
        rewrite_link_events(&mut events, mode);
    }
    let mut html = String::new();
    push_html(&mut html, events.into_iter());
    restore_math(&html, &math)
}

/// Apply [`LinkMode`] resolution to every link-open event in place. Shared
/// with the index-page body renderer, which runs its own event pipeline.
pub(crate) fn rewrite_link_events(events: &mut [Event<'_>], mode: &LinkMode) {
    for ev in events.iter_mut() {
        if let Event::Start(Tag::Link { dest_url, .. }) = ev {
            if let Some(new) = rewrite_link_dest(dest_url, mode) {
                *dest_url = new.into();
            }
        }
    }
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
///
/// ONE pass, taking the earliest heading of any level from the cursor. The
/// old shape — a full-document pass per level — emitted every h1 to the nav
/// before any h2 and numbered `heading-N` in that grouped order, so any
/// prose block with more than one heading (the common case: contiguous
/// prose between code fences is a single block) got a sidebar whose order
/// and implied nesting contradicted the page.
fn inject_heading_ids(html: &str, nav: &mut String, idx: &mut usize) -> String {
    let mut result = html.to_string();
    let mut search_from = 0;
    loop {
        // Match `<hN>` *and* `<hN ...>`: pulldown-cmark emits an explicit
        // id for `## Title {#anchor}`, and matching only the bare tag
        // skipped those headings entirely — no TOC entry at all, which
        // hit exactly the notebooks using anchors for deep links.
        let found = ["h1", "h2", "h3"]
            .iter()
            .filter_map(|t| {
                find_heading_open(&result[search_from..], t)
                    .map(|(rel, len)| (*t, search_from + rel, len))
            })
            .min_by_key(|&(_, at, _)| at);
        let Some((tag, abs_open, open_len)) = found else {
            break;
        };
        let close = format!("</{tag}>");
        let content_start = abs_open + open_len;
        let Some(rel_end) = result[content_start..].find(&close) else {
            // Unterminated heading: step past it so another level can still
            // match later in the document.
            search_from = content_start;
            continue;
        };
        let content = result[content_start..content_start + rel_end].to_string();
        let clean = strip_tags(&content);
        if !clean.is_empty() {
            let open_tag = result[abs_open..abs_open + open_len].to_string();
            // An explicit `{#anchor}` already produced an id, and other
            // pages may link to it — keep it and point the TOC there
            // rather than overwriting someone's stable anchor.
            let (id, new_open) = match existing_id(&open_tag) {
                Some(existing) => (existing, open_tag.clone()),
                None => {
                    *idx += 1;
                    let id = format!("heading-{idx}");
                    let inner = open_tag
                        .trim_start_matches('<')
                        .trim_end_matches('>')
                        .to_string();
                    (id.clone(), format!("<{inner} id=\"{id}\">"))
                }
            };
            result.replace_range(abs_open..abs_open + open_len, &new_open);
            // `clean` came out of already-rendered HTML, so its entities
            // are encoded already — `strip_tags` only removes tag spans
            // and leaves them alone. Escaping again turned a heading's
            // `&` into `&amp;amp;`, which the sidebar displayed literally.
            nav.push_str(&format!(
                "  <a href=\"#{id}\" class=\"{tag}\">{text}</a>\n",
                id = id,
                tag = tag,
                text = clean,
            ));
            search_from = abs_open + new_open.len() + rel_end + close.len();
        } else {
            search_from = content_start + rel_end + close.len();
        }
    }
    result
}

/// Find the next `<hN>` or `<hN ...>` open tag, returning `(offset, len)`.
fn find_heading_open(hay: &str, tag: &str) -> Option<(usize, usize)> {
    let needle = format!("<{tag}");
    let mut from = 0;
    while let Some(rel) = hay[from..].find(&needle) {
        let start = from + rel;
        let after = start + needle.len();
        match hay[after..].chars().next() {
            // `<h1>` or `<h1 id=…>` — but not `<h11>` or `<hr>`.
            Some('>') => return Some((start, after - start + 1)),
            Some(c) if c.is_whitespace() => {
                // Quote-aware: a raw-HTML heading can carry `>` inside an
                // attribute value (`<h2 title="a > b">`), and cutting the
                // tag there spliced the injected id into the attribute.
                let end = end_of_open_tag(hay, start)?;
                return Some((start, end - start));
            }
            _ => from = after,
        }
    }
    None
}

/// Byte offset just past the `>` closing the tag that opens at `start`
/// (`s[start]` must be `<`). `>` inside quoted attribute values does not
/// terminate the tag. `None` when unterminated.
fn end_of_open_tag(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        match quote {
            Some(q) => {
                if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'>' => return Some(i + 1),
                _ => {}
            },
        }
    }
    None
}

/// The value of an `id="…"` attribute already present on an open tag.
///
/// Boundary-checked: `id` must start its own attribute name. A bare
/// substring search matched `data-id="…"`, stealing an anchor nothing on
/// the page carries — the heading got no id and the TOC entry went dead.
fn existing_id(open_tag: &str) -> Option<String> {
    let bytes = open_tag.as_bytes();
    let mut from = 0;
    while let Some(rel) = open_tag[from..].find("id=\"") {
        let at = from + rel;
        if at > 0 && bytes[at - 1].is_ascii_whitespace() {
            let vstart = at + 4;
            let end = open_tag[vstart..].find('"')?;
            return Some(open_tag[vstart..vstart + end].to_string());
        }
        from = at + 4;
    }
    None
}

/// Remove HTML tag spans, leaving text and entity references intact.
///
/// Three things here are deliberately NOT treated as tags:
/// - **Restored math.** `markdown_to_html_linked` restores `\(…\)` / `\[…\]`
///   spans as plain text *after* pulldown escaping, so `$a<b$` arrives here
///   as a literal `\(a<b\)`. Treating that `<` as a tag opener swallowed to
///   the next `>` — `$a<b$ and $c>d$` spliced into `\(ad\)`, a label the
///   heading never said, with delimiters balanced so nothing looked wrong.
///   Delimited math is copied verbatim.
/// - **A `<` not followed by a tag-name character** (`$E > 0$`, `a < b`):
///   plain text in HTML's data state, kept.
/// - **Comments** are skipped to their `-->`, not to the first `>` — a
///   comment containing `>` leaked its tail into the label.
///
/// Real tag spans are skipped quote-aware, matching [`end_of_open_tag`].
fn strip_tags(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        // Math span: copy verbatim through its closing delimiter.
        if chars[i] == '\\' && matches!(chars.get(i + 1), Some('(') | Some('[')) {
            let closer = if chars[i + 1] == '(' { ')' } else { ']' };
            out.push(chars[i]);
            out.push(chars[i + 1]);
            i += 2;
            while i < chars.len() {
                if chars[i] == '\\' && chars.get(i + 1) == Some(&closer) {
                    out.push('\\');
                    out.push(closer);
                    i += 2;
                    break;
                }
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if chars[i] == '<'
            && chars
                .get(i + 1)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == '/' || *c == '!')
        {
            // Comment: skip to `-->` (or end if unterminated).
            if chars[i..].starts_with(&['<', '!', '-', '-']) {
                match (i + 4..chars.len().saturating_sub(2))
                    .find(|&j| chars[j..].starts_with(&['-', '-', '>']))
                {
                    Some(j) => i = j + 3,
                    None => break,
                }
                continue;
            }
            // Tag: skip quote-aware to its closing '>' (or end).
            let mut quote: Option<char> = None;
            i += 1;
            while i < chars.len() {
                let c = chars[i];
                match quote {
                    Some(q) => {
                        if c == q {
                            quote = None;
                        }
                    }
                    None => match c {
                        '"' | '\'' => quote = Some(c),
                        '>' => {
                            i += 1;
                            break;
                        }
                        _ => {}
                    },
                }
                i += 1;
            }
        } else {
            out.push(chars[i]);
            i += 1;
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

/// Transform Obsidian-style wikilinks and embeds into standard markdown so
/// the committed `book/*.md` renders correctly on GitHub (where `[[...]]`
/// is literal text) and the HTML pipeline can resolve them through
/// [`rewrite_link_dest`] like ordinary `.md` references.
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

        // Inline math: $ ... $ (KaTeX-style, single line). On table rows the
        // scan stops at cell boundaries so a bare `$N` in one cell can't
        // swallow the `|` up to a math span in a later cell.
        if b == b'$' && is_inline_math_open(s, i) {
            if let Some(close) = find_inline_close(s, i + 1, line_is_table_row(s, i)) {
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

/// Convert a stashed math span from its authored `$…$` / `$$…$$` form to
/// the `\(…\)` / `\[…\]` delimiters the client-side KaTeX auto-render is
/// configured to recognize. Keeping bare `$` *out* of the emitted HTML is
/// what stops prose dollar amounts (e.g. "$260 … $65") from being
/// re-parsed as inline math in the browser: the server already decides
/// what is and isn't math (see [`protect_math`]), so the client must not
/// second-guess it with a naive `$…$` scan.
///
/// `$$` is matched before `$` so display spans aren't mis-split. Anything
/// that doesn't look delimited is returned unchanged (defensive — every
/// real stash entry carries its delimiters).
fn to_katex_delimiters(original: &str) -> String {
    if original.len() >= 4 && original.starts_with("$$") && original.ends_with("$$") {
        format!("\\[{}\\]", &original[2..original.len() - 2])
    } else if original.len() >= 2 && original.starts_with('$') && original.ends_with('$') {
        format!("\\({}\\)", &original[1..original.len() - 1])
    } else {
        original.to_string()
    }
}

/// Restore math placeholders in rendered HTML, rewriting each span's
/// delimiters to the currency-safe `\(…\)` / `\[…\]` form (see
/// [`to_katex_delimiters`]).
fn restore_math(html: &str, stash: &[String]) -> String {
    if stash.is_empty() {
        return html.to_string();
    }
    let mut out = html.to_string();
    for (idx, original) in stash.iter().enumerate() {
        out = out.replace(&math_placeholder(idx), &to_katex_delimiters(original));
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

/// True if the line containing byte `i` looks like a GFM table row:
/// at most 3 leading spaces followed by `|`.
fn line_is_table_row(s: &[u8], i: usize) -> bool {
    let ls = s[..i]
        .iter()
        .rposition(|&c| c == b'\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let mut j = ls;
    while j < s.len() && s[j] == b' ' && j - ls < 3 {
        j += 1;
    }
    j < s.len() && s[j] == b'|'
}

/// Find closing `$` for an inline span starting at `start`. Same line only.
/// Closing `$` must be preceded by non-whitespace and not followed by a digit
/// (KaTeX convention to avoid swallowing prices like "$5").
///
/// With `stop_at_pipe` (set on table-row lines) an unescaped `|` ends the
/// scan: a table cell's `|` boundary must never be swallowed into a math
/// span. Literal pipes inside table-cell math need `\|` or `\mid`, matching
/// GFM's own escaping rule for pipes in cells.
fn find_inline_close(s: &[u8], start: usize, stop_at_pipe: bool) -> Option<usize> {
    let n = s.len();
    let mut j = start;
    while j < n && s[j] != b'\n' {
        if s[j] == b'\\' && j + 1 < n {
            j += 2;
            continue;
        }
        if stop_at_pipe && s[j] == b'|' {
            return None;
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
/// Render a `rustlab-widget` declaration as an interactive HTML control.
/// The page's WS client (`server/ws.rs`) delegates `input` events on
/// `.rl-widget` controls to a `{"kind":"widget_update",…}` message; the
/// server re-renders and pushes the result back. The `value`/`<output>`
/// reflect the control's current value so a full re-render is consistent.
fn render_widget_html(decl: &WidgetDecl, value: &WidgetValue) -> String {
    let name_attr = escape_html(&decl.name);
    let id = format!("rlw-{}", escape_html(&decl.name));
    let label = escape_html(decl.label.as_deref().unwrap_or(&decl.name));
    match &decl.kind {
        WidgetKind::Slider {
            min,
            max,
            step,
            default,
        } => {
            let cur = value.as_number().unwrap_or(*default);
            let step_attr = step.map(|s| format!(" step=\"{s}\"")).unwrap_or_default();
            format!(
                "<form class=\"rl-widget\" data-widget-name=\"{name_attr}\" data-widget-type=\"slider\">\n\
                 <label class=\"rl-widget-label\" for=\"{id}\">{label}</label>\n\
                 <input class=\"rl-widget-input\" type=\"range\" id=\"{id}\" name=\"{name_attr}\" min=\"{min}\" max=\"{max}\"{step_attr} value=\"{cur}\">\n\
                 <output class=\"rl-widget-value\" for=\"{id}\">{cur}</output>\n\
                 </form>\n"
            )
        }
        WidgetKind::Number {
            min,
            max,
            step,
            default,
        } => {
            let cur = value.as_number().unwrap_or(*default);
            let min_attr = min.map(|m| format!(" min=\"{m}\"")).unwrap_or_default();
            let max_attr = max.map(|m| format!(" max=\"{m}\"")).unwrap_or_default();
            let step_attr = step.map(|s| format!(" step=\"{s}\"")).unwrap_or_default();
            format!(
                "<form class=\"rl-widget\" data-widget-name=\"{name_attr}\" data-widget-type=\"number\">\n\
                 <label class=\"rl-widget-label\" for=\"{id}\">{label}</label>\n\
                 <input class=\"rl-widget-input\" type=\"number\" id=\"{id}\" name=\"{name_attr}\"{min_attr}{max_attr}{step_attr} value=\"{cur}\">\n\
                 </form>\n"
            )
        }
        WidgetKind::Option { choices, default } => {
            let cur = value.as_text().unwrap_or_else(|| default.clone());
            let mut radios = String::new();
            for (i, choice) in choices.iter().enumerate() {
                let choice_attr = escape_html(choice);
                let rid = format!("{id}-{i}");
                let checked = if choice == &cur { " checked" } else { "" };
                radios.push_str(&format!(
                    "<label class=\"rl-widget-choice\"><input class=\"rl-widget-input\" type=\"radio\" id=\"{rid}\" name=\"{name_attr}\" value=\"{choice_attr}\"{checked}> {choice_attr}</label>\n"
                ));
            }
            format!(
                "<form class=\"rl-widget rl-widget-options\" data-widget-name=\"{name_attr}\" data-widget-type=\"option\">\n\
                 <span class=\"rl-widget-label\">{label}</span>\n\
                 {radios}</form>\n"
            )
        }
    }
}

/// `<section class="rl-block" id="b-<hash>">…</section>`, and
/// replaces the chunk in `body`. ID is the low 32 bits of the
/// chunk's `DefaultHasher` digest rendered as 8 hex chars; if
/// the same hash already appeared in this render the suffix
/// `-N` disambiguates (per locked-in #14 of the plan, position
/// is the collision tiebreaker).
///
/// `extra_attrs` is spliced verbatim into the opening tag after the id
/// (pass `""` for none; a leading space when non-empty). It is *not*
/// hashed — the id stays a pure content hash, so stamping a section
/// with e.g. `data-code-idx` never changes its identity.
///
/// Empty / whitespace-only chunks emit nothing (matches the
/// existing renderer's behaviour for skipped blocks).
fn finalize_block(
    body: &mut String,
    mark: usize,
    counter: &mut HashMap<u64, usize>,
    extra_attrs: &str,
) {
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
    let open = format!("<section class=\"rl-block\" id=\"{id}\"{extra_attrs}>\n");
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
    fn inject_heading_ids_follows_document_order() {
        // The old per-level passes emitted every h1 before any h2, so a
        // block with mixed levels got a sidebar whose order and implied
        // nesting contradicted the page (docs/notebooks.md itself nested
        // Quick Start's subsections under the wrong h2), and heading-N ids
        // were non-monotonic down the page.
        let mut nav = String::new();
        let mut idx = 0;
        let out = inject_heading_ids(
            "<h1>Title</h1><h2>Section A</h2><h3>A.1</h3><h2>Section B</h2>",
            &mut nav,
            &mut idx,
        );
        let labels: Vec<&str> = nav
            .lines()
            .filter_map(|l| l.split('>').nth(1).and_then(|s| s.split('<').next()))
            .collect();
        assert_eq!(
            labels,
            vec!["Title", "Section A", "A.1", "Section B"],
            "nav must follow document order: {nav}"
        );
        // Ids number in document order too — A.1 is heading-3, B heading-4.
        assert!(out.contains("<h3 id=\"heading-3\">A.1</h3>"), "{out}");
        assert!(out.contains("<h2 id=\"heading-4\">Section B</h2>"), "{out}");
        // Level ascending within one block (h2 before h1) must also hold.
        let mut nav = String::new();
        let mut idx = 0;
        inject_heading_ids("<h2>Sub</h2><h1>Main</h1>", &mut nav, &mut idx);
        let first = nav.lines().next().unwrap_or_default();
        assert!(first.contains("Sub"), "h2 emitted first in doc order: {nav}");
    }

    #[test]
    fn inject_heading_ids_ignores_data_id_attributes() {
        // `existing_id` substring-matched `data-id="…"`, so the heading got
        // no injected id and the TOC linked to an anchor nothing carries.
        let mut nav = String::new();
        let mut idx = 0;
        let out = inject_heading_ids("<h2 data-id=\"zzz\">Data attr</h2>", &mut nav, &mut idx);
        assert!(
            out.contains("id=\"heading-1\""),
            "generated id missing — data-id stole the anchor: {out}"
        );
        assert!(
            nav.contains("href=\"#heading-1\""),
            "TOC must target the injected id, not data-id: {nav}"
        );
    }

    #[test]
    fn inject_heading_ids_handles_gt_inside_quoted_attributes() {
        // The open-tag scan cut at the first `>` even inside a quoted
        // attribute value, splicing the injected id into the attribute and
        // leaking `b">` into the heading text.
        let mut nav = String::new();
        let mut idx = 0;
        let out = inject_heading_ids(
            "<h2 title=\"a > b\">Raw attr heading</h2>",
            &mut nav,
            &mut idx,
        );
        assert!(
            out.contains("<h2 title=\"a > b\" id=\"heading-1\">Raw attr heading</h2>"),
            "id must be appended after the quoted attribute: {out}"
        );
        assert!(nav.contains(">Raw attr heading</a>"), "label corrupted: {nav}");
    }

    #[test]
    fn strip_tags_keeps_unspaced_math_comparisons() {
        // `$a<b$` reaches strip_tags as `\(a<b\)`; `<b` looked like a tag
        // opener and the scan swallowed to the `>` in the NEXT math span,
        // splicing `\(a<b\) and \(c>d\)` into `\(ad\)` — balanced
        // delimiters, silently wrong label.
        assert_eq!(
            strip_tags(r"Bounds \(a<b\) and \(c>d\)"),
            r"Bounds \(a<b\) and \(c>d\)"
        );
        // Truncation form: no later `>` at all.
        assert_eq!(strip_tags(r"Regime \(a<b\)"), r"Regime \(a<b\)");
        // Real tags around math still strip.
        assert_eq!(strip_tags(r"<em>x</em> \(a<b\)"), r"x \(a<b\)");
    }

    #[test]
    fn strip_tags_skips_comments_to_their_real_close() {
        // The generic tag-skip stopped at the first `>`, so a comment
        // containing `>` leaked its tail into the TOC label.
        assert_eq!(strip_tags("Comment <!-- a > b --> tail"), "Comment  tail");
        // Unterminated comment: drop the rest rather than leak it.
        assert_eq!(strip_tags("x <!-- open"), "x ");
    }

    #[test]
    fn inject_heading_ids_keeps_inline_math_intact() {
        // strip_tags treated every `<` as a tag opener, so a comparison
        // inside heading math swallowed the rest of the label — leaving an
        // unmatched `\(` that KaTeX could not close — and a `>` was deleted
        // outright, making the TOC assert the opposite of the heading.
        let mut nav = String::new();
        let mut idx = 0;
        inject_heading_ids(
            "<h2>Regime \\(T < T_c\\)</h2><h2>Threshold \\(E > 0\\)</h2>",
            &mut nav,
            &mut idx,
        );
        assert!(nav.contains("Regime \\(T < T_c\\)"), "math truncated: {nav}");
        assert!(nav.contains("Threshold \\(E > 0\\)"), "operator lost: {nav}");
    }

    #[test]
    fn inject_heading_ids_honours_explicit_anchors() {
        // `## Title {#anchor}` renders as `<h2 id="anchor">`. Matching only
        // the bare `<h2>` skipped those headings entirely — no TOC entry —
        // and overwriting the id would break cross-notebook deep links.
        let mut nav = String::new();
        let mut idx = 0;
        let out = inject_heading_ids("<h2 id=\"filters\">Filter Analysis</h2>", &mut nav, &mut idx);
        assert!(out.contains("id=\"filters\""), "explicit id was overwritten");
        assert!(nav.contains("href=\"#filters\""), "anchor heading missing from TOC: {nav}");
        assert!(nav.contains("Filter Analysis"));
    }

    #[test]
    fn inject_heading_ids_does_not_double_escape_entities() {
        // The heading text is a slice of already-rendered HTML, so `&` has
        // been encoded once. Escaping it again produced `&amp;amp;`, and the
        // sidebar rendered a literal "&amp;" — e.g. quantum_lab's
        // "Hyperfine Structure & Qubit Selection".
        let mut nav = String::new();
        let mut idx = 0;
        inject_heading_ids("<h1>Structure &amp; Selection</h1>", &mut nav, &mut idx);
        assert!(
            nav.contains("Structure &amp; Selection"),
            "nav label lost its single-escaped entity: {nav}"
        );
        assert!(
            !nav.contains("&amp;amp;"),
            "nav label was escaped twice: {nav}"
        );
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

    /// The inline cell editor seeds its buffer from the rendered
    /// `<pre class="source">`'s textContent — i.e. the highlighted HTML
    /// with tags stripped and entities unescaped. That round trip must
    /// reproduce the block source byte-for-byte, or Shift+Enter would
    /// save a silently mangled block. Pin it here: strip + unescape of
    /// `highlight_rustlab(src)` == `src` for awkward inputs.
    #[test]
    fn highlight_round_trips_to_source_text() {
        // Reverse of `escape_html`: &amp; must be restored LAST, since
        // escape encodes it first.
        fn unescape(s: &str) -> String {
            s.replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&amp;", "&")
        }
        let cases = [
            "x = 1;",
            "for k = 1:3\n  disp(k)\nend",
            "s = \"quoted <html> & ampersand\";",
            "a = b & c; d = a < b; % comment with 'quotes'\ny = sin(x)",
            "m = [1, 2; 3, 4];\n\n% blank line above\nz = m';",
        ];
        for src in cases {
            let highlighted = highlight_rustlab(src);
            assert_eq!(
                unescape(&strip_tags(&highlighted)),
                src,
                "highlight must preserve every source character"
            );
        }
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
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
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
        let html = render_html("T", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
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
        let html = render_html("T", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
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
        let h1 = render_html("T", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        let h2 = render_html("T", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        let id1 = h1.split("id=\"b-").nth(1).unwrap().split('"').next().unwrap();
        let id2 = h2.split("id=\"b-").nth(1).unwrap().split('"').next().unwrap();
        assert_eq!(id1, id2, "block id changed between identical renders");
    }

    // ── data-code-idx stamping (cell execution) ──

    fn code(source: &str) -> Rendered {
        Rendered::Code {
            source: source.to_string(),
            text_output: String::new(),
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
        }
    }

    #[test]
    fn code_sections_stamped_with_executable_ordinal() {
        let blocks = vec![
            Rendered::Markdown("prose".to_string()),
            code("a = 1"),
            Rendered::Markdown("more prose".to_string()),
            code("b = 2"),
        ];
        let html = render_html("T", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        assert!(html.contains("data-code-idx=\"0\""), "first code block is idx 0");
        assert!(html.contains("data-code-idx=\"1\""), "second code block is idx 1");
        // Prose sections carry no ordinal.
        assert_eq!(html.matches("data-code-idx=").count(), 2);
    }

    #[test]
    fn hidden_mermaid_advances_the_ordinal() {
        // Hidden mermaid emits no section but occupies a cache slot — the
        // following code block's stamp must skip its index.
        let blocks = vec![
            code("a = 1"),
            Rendered::Mermaid {
                source: "flowchart LR\nA-->B".to_string(),
                hidden: true,
                details: None,
                caption: None,
            },
            code("b = 2"),
        ];
        let html = render_html("T", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        assert!(html.contains("data-code-idx=\"0\""));
        assert!(
            html.contains("data-code-idx=\"2\""),
            "code after hidden mermaid must be idx 2, not 1:\n{html}"
        );
        assert!(!html.contains("data-code-idx=\"1\""), "slot 1 is the hidden mermaid");
    }

    #[test]
    fn visible_mermaid_section_is_not_stamped() {
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\nA-->B".to_string(),
            hidden: false,
            details: None,
            caption: None,
        }];
        let html = render_html("T", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        assert!(!html.contains("data-code-idx="), "mermaid gets no Run affordance");
    }

    #[test]
    fn stamping_does_not_change_block_ids() {
        // The ordinal attribute is spliced into the open tag only — the
        // content-hash id must be identical to what an unstamped prose
        // block with the same content would get. Compare a code block's
        // id across two renders where its *ordinal* differs (an earlier
        // code block was inserted): same content → same id.
        // Identifiers survive `highlight_rustlab` verbatim; operators and
        // numbers get wrapped in spans, so match on the bare name only.
        let plot = std::path::PathBuf::from("/tmp/rustlab_test_plots");
        let solo = render_html("T", &[code("xyzzy = 1")], &plot, "plots", test_theme(), None, &LinkMode::single_file());
        let shifted = render_html(
            "T",
            &[code("unrelated = 2"), code("xyzzy = 1")],
            &plot,
            "plots",
            test_theme(),
            None,
            &LinkMode::single_file(),
        );
        let id_of = |html: &str, marker: &str| -> String {
            // Find the section whose body contains `marker`, return its id.
            html.split("<section class=\"rl-block\" id=\"")
                .skip(1)
                .find(|chunk| chunk.contains(marker))
                .and_then(|chunk| chunk.split('"').next())
                .unwrap_or_default()
                .to_string()
        };
        let a = id_of(&solo, "xyzzy");
        let b = id_of(&shifted, "xyzzy");
        assert!(!a.is_empty());
        assert_eq!(a, b, "content-hash id must not depend on the ordinal stamp");
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
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
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
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
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
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
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
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        // Source shown, but no output div
        assert!(html.contains("class=\"source\""));
        assert!(!html.contains("class=\"output\""));
    }

    #[test]
    fn render_html_katex_included() {
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        assert!(html.contains("katex"));
        assert!(html.contains("auto-render"));
    }

    #[test]
    fn render_html_plotly_included() {
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        assert!(html.contains("plotly"));
    }

    #[test]
    fn render_html_nav_toggle() {
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        assert!(html.contains("nav-toggle"));
    }

    #[test]
    fn render_html_title_escaped() {
        let html = render_html("A <script> & \"test\"", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
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
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        assert!(html.contains("syn-kw"));
        assert!(html.contains("syn-fn"));
        assert!(html.contains("syn-num"));
    }

    #[test]
    fn render_html_nav_from_headings() {
        let blocks = vec![Rendered::Markdown(
            "# Section One\n\n## Sub Section".to_string(),
        )];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        assert!(html.contains("heading-1"));
        assert!(html.contains("heading-2"));
        assert!(html.contains("Section One"));
        assert!(html.contains("Sub Section"));
    }

    // ── cross-notebook link resolution (LinkMode) ──
    //
    // The old `rewrite_md_links` was a raw string replace on unparsed
    // markdown: it corrupted external URLs ending in `.md`, rewrote inside
    // code spans, and missed titled and reference-style links entirely.
    // These tests pin the resolver's contract at both altitudes: the
    // destination-level rules here, the full-pipeline behaviour below.

    fn server_mode(pairs: &[(&str, &str)], current_rel_dir: &str) -> LinkMode {
        LinkMode::Server {
            slugs: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            current_rel_dir: current_rel_dir.to_string(),
        }
    }

    #[test]
    fn link_dest_static_swaps_md_for_html() {
        let mode = LinkMode::single_file();
        assert_eq!(
            rewrite_link_dest("filter.md", &mode).as_deref(),
            Some("filter.html")
        );
        // Fragment survives verbatim.
        assert_eq!(
            rewrite_link_dest("other.md#intro", &mode).as_deref(),
            Some("other.html#intro")
        );
        // Non-.md destinations pass through untouched.
        assert_eq!(rewrite_link_dest("image.png", &mode), None);
        assert_eq!(rewrite_link_dest("#local-anchor", &mode), None);
    }

    #[test]
    fn link_dest_never_touches_external_or_absolute_urls() {
        // The old string replace corrupted every one of these to `.html`.
        for mode in [
            LinkMode::single_file(),
            server_mode(&[("doc.md", "doc")], ""),
        ] {
            for dest in [
                "https://example.com/README.md",
                "https://github.com/x/y/blob/main/doc.md",
                "http://host/doc.md#frag",
                "mailto:someone@example.md",
                "//host/proto-relative.md",
                "/docs/absolute.md",
            ] {
                assert_eq!(
                    rewrite_link_dest(dest, &mode),
                    None,
                    "{dest} must pass through unrewritten"
                );
            }
        }
    }

    #[test]
    fn link_dest_static_collection_only_rewrites_emitted_siblings() {
        // Directory renders know exactly which .html files they emit; a
        // link to anything else (partial, dangling target, subdirectory)
        // is left as written — visibly broken beats a manufactured 404.
        let known: HashSet<String> = ["01-intro.md".to_string(), "index.md".to_string()]
            .into_iter()
            .collect();
        let mode = LinkMode::Static { known: Some(known) };
        assert_eq!(
            rewrite_link_dest("01-intro.md", &mode).as_deref(),
            Some("01-intro.html")
        );
        assert_eq!(
            rewrite_link_dest("index.md", &mode).as_deref(),
            Some("index.html")
        );
        assert_eq!(rewrite_link_dest("_setup.md", &mode), None, "partial");
        assert_eq!(rewrite_link_dest("nope.md", &mode), None, "dangling");
        assert_eq!(
            rewrite_link_dest("sub/foo.md", &mode),
            None,
            "static dir build is non-recursive; sub/foo.html is never emitted"
        );
    }

    #[test]
    fn link_dest_server_resolves_to_slug_routes() {
        let mode = server_mode(&[("01-intro.md", "01-intro"), ("02-filter.md", "02-filter")], "");
        assert_eq!(
            rewrite_link_dest("02-filter.md", &mode).as_deref(),
            Some("/n/02-filter")
        );
        assert_eq!(
            rewrite_link_dest("./02-filter.md#setup", &mode).as_deref(),
            Some("/n/02-filter#setup")
        );
        // index.md is the index page's body, not a notebook: it has no
        // slug and must land on the server root.
        assert_eq!(rewrite_link_dest("index.md", &mode).as_deref(), Some("/"));
        // Dangling and partial targets: left exactly as written.
        assert_eq!(rewrite_link_dest("nope.md", &mode), None);
        assert_eq!(rewrite_link_dest("_setup.md", &mode), None);
    }

    #[test]
    fn link_dest_server_resolves_nested_paths_by_path_not_stem() {
        // ch1/01-intro.md and 01-intro.md share a stem but hold distinct
        // slugs; the link must resolve to the slug of the file the path
        // names. A stem-keyed map fails this test.
        let mode = server_mode(
            &[
                ("01-intro.md", "01-intro"),
                ("ch1/01-intro.md", "01-intro-2"),
                ("ch2/notes.md", "notes"),
            ],
            "ch1",
        );
        assert_eq!(
            rewrite_link_dest("01-intro.md", &mode).as_deref(),
            Some("/n/01-intro-2"),
            "relative to ch1/, `01-intro.md` is the ch1 file"
        );
        assert_eq!(
            rewrite_link_dest("../01-intro.md", &mode).as_deref(),
            Some("/n/01-intro")
        );
        assert_eq!(
            rewrite_link_dest("../ch2/notes.md", &mode).as_deref(),
            Some("/n/notes")
        );
        // `..` escaping the collection root cannot resolve — left alone.
        assert_eq!(rewrite_link_dest("../../outside.md", &mode), None);
    }

    /// Render one markdown block through the full HTML pipeline in `mode`.
    fn render_md_linked(src: &str, mode: &LinkMode) -> String {
        let blocks = vec![Rendered::Markdown(src.to_string())];
        render_html(
            "T",
            &blocks,
            &std::path::PathBuf::from("/tmp/rustlab_test_plots"),
            "plots",
            test_theme(),
            None,
            mode,
        )
    }

    #[test]
    fn pipeline_rewrites_titled_links() {
        // `[x](a.md "title")` never matched the old `.md)` string replace,
        // so titled cross-references shipped broken in BOTH modes.
        let html = render_md_linked(
            r#"[x](02-filter.md "Filter lesson")"#,
            &LinkMode::single_file(),
        );
        assert!(
            html.contains(r#"href="02-filter.html" title="Filter lesson""#),
            "titled link not rewritten with title preserved: {html}"
        );
        let html = render_md_linked(
            r#"[x](02-filter.md "Filter lesson")"#,
            &server_mode(&[("02-filter.md", "02-filter")], ""),
        );
        assert!(
            html.contains(r#"href="/n/02-filter" title="Filter lesson""#),
            "titled link not resolved to slug route: {html}"
        );
    }

    #[test]
    fn pipeline_rewrites_reference_style_links() {
        // `[flt]: 02-filter.md` has no `.md)` for a string replace to see;
        // the parser resolves the reference before the Link event fires.
        let src = "See [the filter][flt] lesson.\n\n[flt]: 02-filter.md#setup\n";
        let html = render_md_linked(src, &LinkMode::single_file());
        assert!(
            html.contains(r#"href="02-filter.html#setup""#),
            "reference-style link not rewritten: {html}"
        );
        let html = render_md_linked(src, &server_mode(&[("02-filter.md", "02-filter")], ""));
        assert!(
            html.contains(r#"href="/n/02-filter#setup""#),
            "reference-style link not resolved to slug route: {html}"
        );
    }

    #[test]
    fn pipeline_leaves_code_spans_and_fences_byte_identical() {
        // The old replace ran on raw markdown BEFORE parsing, so
        // `` `[link](a.md)` `` displayed as `[link](a.html)` — a rendered
        // code example asserting a rewrite the reader never wrote.
        let src = "Inline `[link](a.md)` span.\n\n```text\nsee [x](a.md)\n```\n";
        for mode in [
            LinkMode::single_file(),
            server_mode(&[("a.md", "a")], ""),
        ] {
            let html = render_md_linked(src, &mode);
            assert!(
                html.contains("[link](a.md)"),
                "inline code span was rewritten: {html}"
            );
            assert!(
                html.contains("see [x](a.md)"),
                "fenced block was rewritten: {html}"
            );
            assert!(!html.contains("a.html"), "code content leaked a rewrite: {html}");
        }
    }

    #[test]
    fn pipeline_keeps_external_md_urls_intact() {
        // Confirmed corruption before the resolver: this exact form
        // rendered as `https://example.com/README.html`.
        let html = render_md_linked(
            "[readme](https://example.com/README.md)",
            &LinkMode::single_file(),
        );
        assert!(
            html.contains(r#"href="https://example.com/README.md""#),
            "external URL was corrupted: {html}"
        );
    }

    #[test]
    fn pipeline_bare_md_angle_reference_is_literal_text() {
        // CommonMark autolinks require a scheme: `<02-filter.md>` is plain
        // text, not a link. Pinned so nobody "fixes" it with string
        // matching later.
        let html = render_md_linked("see <02-filter.md> here", &LinkMode::single_file());
        assert!(!html.contains("href=\"02-filter"), "text became a link: {html}");
        assert!(
            html.contains("&lt;02-filter.md&gt;"),
            "angle reference should render as literal text: {html}"
        );
    }

    #[test]
    fn pipeline_resolves_wikilinks_through_the_same_seam() {
        // `[[02-filter#Setup|the setup]]` → transform_wikilinks emits
        // `[the setup](02-filter.md#setup)` (fragment lowercased) → the
        // resolver routes it per mode. The fragment must match the id the
        // explicit-anchor pipeline emits for `## Setup {#setup}`.
        let src = "see [[02-filter#Setup|the setup]]";
        let html = render_md_linked(src, &LinkMode::single_file());
        assert!(
            html.contains(r#"href="02-filter.html#setup""#),
            "wikilink not resolved statically: {html}"
        );
        let html = render_md_linked(src, &server_mode(&[("02-filter.md", "02-filter")], ""));
        assert!(
            html.contains(r#"href="/n/02-filter#setup""#),
            "wikilink not resolved to slug route: {html}"
        );
    }

    #[test]
    fn pipeline_resolves_links_inside_callouts() {
        // Callout bodies run through their own markdown_to_html call
        // (the second rewrite site) — same resolution applies.
        let blocks = vec![Rendered::Callout {
            kind: CalloutKind::Note,
            title: None,
            content: "see [next](02-filter.md)".to_string(),
        }];
        let html = render_html(
            "T",
            &blocks,
            &std::path::PathBuf::from("/tmp/rustlab_test_plots"),
            "plots",
            test_theme(),
            None,
            &server_mode(&[("02-filter.md", "02-filter")], ""),
        );
        assert!(
            html.contains(r#"href="/n/02-filter""#),
            "callout link not resolved: {html}"
        );
    }

    #[test]
    fn pipeline_explicit_anchor_target_exists_end_to_end() {
        // The served `#setup` fragment lands on a real id: this PR's
        // explicit-anchor preservation keeps `## Setup {#setup}` as
        // `id="setup"` rather than renumbering it `heading-N`.
        let html = render_md_linked("## Setup {#setup}", &LinkMode::single_file());
        assert!(
            html.contains(r#"id="setup""#),
            "explicit anchor id missing — fragment links would dangle: {html}"
        );
    }

    #[test]
    fn render_html_rewrites_md_links() {
        let blocks = vec![Rendered::Markdown(
            "See [other](other.md) for details".to_string(),
        )];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
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
    fn restore_math_rewrites_display_delimiters() {
        // `$$…$$` round-trips through the placeholder back to math, but with
        // the delimiters rewritten to `\[…\]` (currency-safe client form).
        // The inner content — including the `\\` row separator — is verbatim.
        let src = r"$$a \\ b$$";
        let (rewritten, stash) = protect_math(src);
        let restored = restore_math(&rewritten, &stash);
        assert_eq!(restored, r"\[a \\ b\]");
    }

    #[test]
    fn restore_math_rewrites_inline_delimiters() {
        let src = r"see $x^2$ here";
        let (rewritten, stash) = protect_math(src);
        let restored = restore_math(&rewritten, &stash);
        assert_eq!(restored, r"see \(x^2\) here");
        assert!(!restored.contains('$'), "no bare $ may survive into HTML");
    }

    #[test]
    fn restore_math_leaves_currency_prose_untouched() {
        // protect_math doesn't treat unpaired/price `$` as math, so it never
        // reaches the stash; the prose passes through with literal `$` and no
        // `\(`/`\[` delimiters — the browser's auto-render (which only knows
        // `\(`/`\[`) will leave it alone. This is the currency-katex bug fix.
        let src = "Safe harbor target: $260,876 for the year ($65,219 / quarter).";
        let (rewritten, stash) = protect_math(src);
        assert!(stash.is_empty(), "currency must not be stashed as math");
        let restored = restore_math(&rewritten, &stash);
        assert_eq!(restored, src);
        assert!(!restored.contains(r"\("), "no inline math delimiter injected");
    }

    #[test]
    fn to_katex_delimiters_handles_both_forms() {
        assert_eq!(to_katex_delimiters("$x$"), r"\(x\)");
        assert_eq!(to_katex_delimiters("$$x$$"), r"\[x\]");
        // Degenerate empty display span.
        assert_eq!(to_katex_delimiters("$$$$"), r"\[\]");
        // Not delimited → unchanged.
        assert_eq!(to_katex_delimiters("plain"), "plain");
    }

    #[test]
    fn render_html_preserves_matrix_row_separator() {
        let blocks = vec![Rendered::Markdown(
            r"$$\begin{pmatrix}0 & 1 \\ 1 & 0\end{pmatrix}$$".to_string(),
        )];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
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
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
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
        assert_eq!(restored, r"before \[\] after");
    }

    // ── single-tilde strikethrough demotion (audit S1/S2) ──

    #[test]
    fn single_tilde_stays_literal() {
        for (src, tilde_text) in [
            ("id ~foo~ and ~bar~ keys", "id ~foo~ and ~bar~ keys"),
            ("swap ~this~ out", "swap ~this~ out"),
            ("func(~x~)", "func(~x~)"),
        ] {
            let html = markdown_to_html(src);
            assert!(!html.contains("<del>"), "struck through: {src:?} → {html:?}");
            assert!(html.contains(tilde_text), "tildes lost: {src:?} → {html:?}");
        }
    }

    #[test]
    fn double_tilde_still_strikethrough() {
        let html = markdown_to_html("this is ~~struck~~ text");
        assert!(html.contains("<del>struck</del>"), "{html:?}");
    }

    #[test]
    fn double_tilde_nested_single_stays_literal() {
        let html = markdown_to_html("~~outer ~inner~ outer~~");
        assert!(html.contains("<del>outer ~inner~ outer</del>"), "{html:?}");
    }

    #[test]
    fn benign_tildes_unaffected() {
        for src in [
            "takes ~5 minutes to ~10 minutes",
            "~/dotfiles and ~/bin",
            "pH ~7",
            "20~30 range",
            "intraword a~b here",
        ] {
            let html = markdown_to_html(src);
            assert!(!html.contains("<del>"), "struck through: {src:?} → {html:?}");
        }
    }

    #[test]
    fn tilde_wrapped_math_not_struck() {
        // Audit S2: the stashed math placeholder is flanking-eligible, so
        // single tildes used to pair around it.
        let html = markdown_to_html("wrap ~$x$~ here");
        assert!(!html.contains("<del>"), "{html:?}");
        assert!(html.contains(r"~\(x\)~"), "{html:?}");

        let html = markdown_to_html(r"a ~$\alpha$~ b");
        assert!(!html.contains("<del>"), "{html:?}");
        assert!(html.contains(r"~\(\alpha\)~"), "{html:?}");

        let html = markdown_to_html("cost ~$5~$10 span");
        assert!(!html.contains("<del>"), "{html:?}");
        assert!(html.contains("cost ~$5~$10 span"), "{html:?}");
    }

    // ── table cells vs inline math (audit D2) ──

    #[test]
    fn table_row_bare_price_does_not_swallow_cell_boundary() {
        let src = "| a | b |\n|---|---|\n| $5 | $x$ |";
        let html = markdown_to_html(src);
        assert!(html.contains("<td>$5</td>"), "{html:?}");
        assert!(html.contains(r"<td>\(x\)</td>"), "{html:?}");
    }

    #[test]
    fn table_row_math_then_price_still_works() {
        let src = "| a | b |\n|---|---|\n| $y=2$ | cost $9 |";
        let html = markdown_to_html(src);
        assert!(html.contains(r"<td>\(y=2\)</td>"), "{html:?}");
        assert!(html.contains("<td>cost $9</td>"), "{html:?}");
    }

    #[test]
    fn prose_math_with_pipe_still_protected() {
        // Only table rows terminate the scan at `|`; prose keeps `$P(A|B)$`.
        let (_, stash) = protect_math("prob $P(A|B)$ here");
        assert_eq!(stash, vec!["$P(A|B)$".to_string()]);
    }

    // ── Cross-notebook navigation (Option B) ──

    #[test]
    fn render_html_no_nav_for_single_file() {
        let blocks = vec![Rendered::Markdown("# Alpha\n\n## Beta\n".to_string())];
        let html = render_html("Test", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        // Same chrome as a collection page — there is just nothing to page to.
        assert!(html.contains("class=\"topbar\""));
        assert!(html.contains("<nav class=\"sidebar\">"));
        assert!(html.contains("class=\"nav-title\""));
        // No cross-notebook affordances when the notebook stands alone.
        assert!(!html.contains("class=\"page-nav\""));
        assert!(!html.contains("class=\"prev\""));
        assert!(!html.contains("class=\"next\""));
        assert!(!html.contains("href=\"index.html\""));
    }

    #[test]
    fn no_page_emits_the_removed_topbar_layout_class() {
        // The class is gone, but a rule keyed off it survived in
        // server/page.rs and left the source/edit toolbar sitting on top of
        // the "Next" link. Nothing may reference it again.
        let blocks = vec![Rendered::Markdown("# Alpha\n".to_string())];
        let nav = NotebookNav {
            index_href: Some("index.html".to_string()),
            prev: None,
            next: None,
        };
        for n in [None, Some(&nav)] {
            let html = render_html("T", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), n, &LinkMode::single_file());
            assert!(!html.contains("topbar-layout"), "stale layout class emitted");
        }
    }

    #[test]
    fn render_html_topbar_breadcrumb_when_nav_provided() {
        let nav = NotebookNav {
            index_href: Some("index.html".to_string()),
            prev: None,
            next: None,
        };
        let blocks = vec![Rendered::Markdown("# Filter Analysis\n".to_string())];
        let html = render_html("Filter Analysis", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), Some(&nav), &LinkMode::single_file());
        // Topbar present with breadcrumb.
        assert!(html.contains("class=\"topbar\""));
        assert!(html.contains("href=\"index.html\""));
        assert!(html.contains("class=\"sep\""));
        assert!(html.contains("class=\"current\""));
        assert!(html.contains("Filter Analysis"));
        // The assertions above all held under the old mutually-exclusive
        // layout too; these are what make the test fail on a revert.
        assert!(html.contains("<nav class=\"sidebar\">"), "sidebar dropped");
        assert!(!html.contains("topbar-layout"), "stale layout class");
    }

    #[test]
    fn notebook_without_headings_gets_no_empty_sidebar() {
        // Emitting the sidebar unconditionally cost 220px of chrome holding
        // nothing but the title. `no-toc` gives that width back to content.
        let blocks = vec![Rendered::Markdown("just prose, no headings.\n".to_string())];
        let html = render_html("Solo", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        assert!(!html.contains("<nav class=\"sidebar\">"), "empty sidebar emitted");
        assert!(html.contains("<body class=\"no-toc\">"));
        // The topbar is still there — chrome stays consistent.
        assert!(html.contains("class=\"topbar\""));
    }

    #[test]
    fn heading_anchors_clear_the_fixed_topbar() {
        // Without scroll-margin-top a sidebar link scrolls its heading to
        // y=0, behind the fixed topbar — the heading you clicked is the one
        // thing you cannot see.
        let blocks = vec![Rendered::Markdown("# Alpha\n".to_string())];
        let html = render_html("T", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), None, &LinkMode::single_file());
        assert!(
            html.contains("scroll-margin-top"),
            "heading anchors would land under the topbar"
        );
    }

    #[test]
    fn collection_pages_keep_the_in_page_toc() {
        // The two navs answer different questions and must coexist. A
        // directory render used to drop the sidebar entirely, so a long
        // lesson lost its table of contents — while still emitting the
        // heading anchors and shipping the sidebar CSS, both unreachable.
        let nav = NotebookNav {
            index_href: Some("index.html".to_string()),
            prev: Some(("Lesson 08".to_string(), "08.html".to_string())),
            next: Some(("Lesson 10".to_string(), "10.html".to_string())),
        };
        let blocks = vec![Rendered::Markdown(
            "# Alpha\n\n## Beta\n\n## Gamma\n".to_string(),
        )];
        let html = render_html("Lesson 09", &blocks, &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), Some(&nav), &LinkMode::single_file());
        // Between-notebook nav.
        assert!(html.contains("class=\"topbar\""));
        assert!(html.contains("href=\"08.html\""), "prev link missing");
        assert!(html.contains("href=\"10.html\""), "next link missing");
        // Within-notebook nav, on the same page.
        assert!(html.contains("<nav class=\"sidebar\">"), "sidebar dropped");
        assert!(
            html.contains("href=\"#heading-1\""),
            "heading anchors emitted but nothing links to them"
        );
        assert!(html.contains("id=\"heading-1\""));
    }

    #[test]
    fn render_html_topbar_escapes_current_title() {
        let nav = NotebookNav {
            index_href: Some("index.html".to_string()),
            prev: None,
            next: None,
        };
        let html = render_html("A <script> & \"x\"", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), Some(&nav), &LinkMode::single_file());
        assert!(html.contains("A &lt;script&gt; &amp; &quot;x&quot;"));
    }

    #[test]
    fn render_html_footer_nav_middle_page() {
        let nav = NotebookNav {
            index_href: Some("index.html".to_string()),
            prev: Some(("Intro".to_string(), "intro.html".to_string())),
            next: Some(("Analysis".to_string(), "analysis.html".to_string())),
        };
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), Some(&nav), &LinkMode::single_file());
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
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), Some(&nav), &LinkMode::single_file());
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
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), Some(&nav), &LinkMode::single_file());
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
        let html = render_html("Test", &[], &std::path::PathBuf::from("/tmp/rustlab_test_plots"), "plots", test_theme(), Some(&nav), &LinkMode::single_file());
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
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None, &LinkMode::single_file());
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
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None, &LinkMode::single_file());
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
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None, &LinkMode::single_file());
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
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None, &LinkMode::single_file());
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
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None, &LinkMode::single_file());
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
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None, &LinkMode::single_file());
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
        let html = render_html("T", &blocks, &dir, "plots", test_theme(), None, &LinkMode::single_file());
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
            &LinkMode::single_file(),
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
            &LinkMode::single_file(),
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
            &LinkMode::single_file(),
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
