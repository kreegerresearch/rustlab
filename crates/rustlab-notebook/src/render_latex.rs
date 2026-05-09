use crate::execute::Rendered;
use crate::parse::CalloutKind;
use crate::render::{notebook_md_options, transform_wikilinks};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use rustlab_plot::theme::{Theme, ThemeColors};
use std::path::Path;

/// Render executed notebook blocks into a LaTeX document string.
///
/// Plot images are written to `plot_dir` as SVG files and referenced from
/// the rendered `.tex` via `\includesvg{plot_href_prefix/plot-N}`. Splitting
/// the on-disk write location from the include path lets callers nest plots
/// under a single `plots/<stem>/` umbrella the same way the markdown emitter
/// does, without coupling the path inside `\includesvg` to the directory the
/// SVGs are written to.
pub fn render_latex(
    title: &str,
    blocks: &[Rendered],
    plot_dir: &Path,
    plot_href_prefix: &str,
    theme: &ThemeColors,
) -> String {
    let mut body = String::new();
    let mut plot_idx = 0;

    let _ = std::fs::create_dir_all(plot_dir);
    let href_prefix = plot_href_prefix.trim_end_matches('/').to_string();

    for block in blocks {
        match block {
            Rendered::Markdown(md) => {
                body.push_str(&markdown_to_latex(md));
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
                // Source code (unless hidden)
                if !hidden {
                    body.push_str("\\begin{verbatim}\n");
                    body.push_str(source);
                    body.push_str("\n\\end{verbatim}\n\n");
                }

                // Details title (LaTeX has no collapsibility — just add a label)
                if let Some(title) = details {
                    body.push_str(&format!("\\paragraph{{{}}}\n\n", escape_latex(title)));
                }

                // Text output
                let trimmed = text_output.trim();
                if !trimmed.is_empty() {
                    body.push_str("\\begin{quote}\n\\ttfamily\\small\n\\begin{verbatim}\n");
                    body.push_str(trimmed);
                    body.push_str("\n\\end{verbatim}\n\\end{quote}\n\n");
                }

                // Error
                if let Some(err) = error {
                    body.push_str(&format!(
                        "\\begin{{quote}}\n{{\\color[HTML]{{{error_hex}}}\\ttfamily\\small\n\\begin{{verbatim}}\n",
                        error_hex = &theme.error_text[1..], // strip leading '#'
                    ));
                    body.push_str(err);
                    body.push_str("\n\\end{verbatim}\n}\\end{quote}\n\n");
                }

                // Plots (one per savefig call, or one final snapshot)
                for fig in figures {
                    plot_idx += 1;
                    let plot_file = plot_dir.join(format!("plot-{plot_idx}.svg"));
                    if let Err(e) = rustlab_plot::render_figure_state_to_file_themed(
                        fig,
                        &plot_file.to_string_lossy(),
                        theme,
                    ) {
                        eprintln!("warning: could not render plot-{plot_idx}: {e}");
                        continue;
                    }
                    let width = if let Some(n) = grid_cols {
                        let w = 0.9 / *n as f64;
                        format!("{w:.2}\\textwidth")
                    } else {
                        "0.9\\textwidth".to_string()
                    };
                    body.push_str(&format!(
                        "\\begin{{center}}\n\\includesvg[width={width}]{{{}/plot-{plot_idx}}}\n\\end{{center}}\n\n",
                        href_prefix,
                    ));
                }

                // Animations cannot embed in a static PDF — emit a note
                // pointing the reader at the HTML / GIF version.
                for anim in animations {
                    let kind = match anim.format {
                        rustlab_plot::NotebookAnimationFormat::Html => "Plotly HTML",
                        rustlab_plot::NotebookAnimationFormat::Gif => "GIF",
                    };
                    body.push_str(&format!(
                        "\\begin{{quote}}\\textit{{[{kind} animation: {} frames at {:.0} fps — view in HTML output]}}\\end{{quote}}\n\n",
                        anim.frames.len(),
                        anim.fps,
                    ));
                }
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
                if let Some(title) = details {
                    body.push_str(&format!("\\paragraph{{{}}}\n\n", escape_latex(title)));
                }
                plot_idx += 1;
                emit_mermaid_latex(
                    &mut body,
                    source,
                    plot_dir,
                    &href_prefix,
                    plot_idx,
                    caption.as_deref(),
                );
            }
            Rendered::Callout {
                kind,
                title,
                content,
            } => {
                let default_label = match kind {
                    CalloutKind::Note => "Note",
                    CalloutKind::Tip => "Tip",
                    CalloutKind::Important => "Important",
                    CalloutKind::Warning => "Warning",
                    CalloutKind::Caution => "Caution",
                };
                let label = title.as_deref().unwrap_or(default_label);
                body.push_str(&format!("\\begin{{quote}}\n\\textbf{{{label}:}} "));
                body.push_str(&markdown_to_latex(content));
                body.push_str("\\end{quote}\n\n");
            }
            Rendered::ExerciseStart { number } => {
                body.push_str(&format!(
                    "\\medskip\\noindent\\textbf{{Exercise~{number}.}}\\quad\n"
                ));
            }
            Rendered::SolutionStart => {
                body.push_str("\\medskip\\noindent\\textbf{Solution.}\\quad\n");
            }
        }
    }

    let is_dark = theme as *const ThemeColors == Theme::Dark.colors() as *const ThemeColors;
    let link_hex = &theme.accent_secondary[1..]; // strip leading '#'

    let dark_preamble = if is_dark {
        let bg_hex = &theme.bg[1..];
        let text_hex = &theme.text[1..];
        format!(
            "\\usepackage{{pagecolor}}\n\
             \\definecolor{{pagebg}}{{HTML}}{{{bg_hex}}}\n\
             \\definecolor{{pagetext}}{{HTML}}{{{text_hex}}}\n\
             \\pagecolor{{pagebg}}\n\
             \\color{{pagetext}}\n"
        )
    } else {
        String::new()
    };

    format!(
        r#"\documentclass[11pt,a4paper]{{article}}
\usepackage[utf8]{{inputenc}}
\usepackage[T1]{{fontenc}}
\usepackage{{geometry}}
\geometry{{margin=1in}}
\usepackage{{graphicx}}
\usepackage{{svg}}
\usepackage{{amsmath,amssymb}}
\usepackage{{xcolor}}
\usepackage{{booktabs}}
\usepackage{{hyperref}}
\hypersetup{{colorlinks=true,linkcolor=[HTML]{{{link_hex}}},urlcolor=[HTML]{{{link_hex}}}}}
{dark_preamble}
\title{{{title}}}
\date{{\today}}

\begin{{document}}
\maketitle

{body}
\end{{document}}
"#,
        title = escape_latex(title),
        body = body,
        link_hex = link_hex,
        dark_preamble = dark_preamble,
    )
}

/// Convert a markdown string to LaTeX using pulldown-cmark events.
fn markdown_to_latex(md: &str) -> String {
    let md = transform_wikilinks(md);
    let mut opts = notebook_md_options();
    opts.insert(Options::ENABLE_MATH);
    let parser = Parser::new_ext(&md, opts);

    let mut out = String::new();
    let mut in_table = false;
    #[allow(unused_assignments)]
    let mut table_alignments: Vec<pulldown_cmark::Alignment> = Vec::new();
    let mut table_cell_idx: usize = 0;
    let mut table_in_head = false;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    let cmd = match level {
                        HeadingLevel::H1 => "section",
                        HeadingLevel::H2 => "subsection",
                        HeadingLevel::H3 => "subsubsection",
                        _ => "paragraph",
                    };
                    out.push_str(&format!("\\{cmd}{{"));
                }
                Tag::Paragraph => {}
                Tag::Emphasis => out.push_str("\\emph{"),
                Tag::Strong => out.push_str("\\textbf{"),
                Tag::CodeBlock(_) => {
                    // Fenced code blocks in markdown (non-rustlab) treated as verbatim
                    out.push_str("\\begin{verbatim}\n");
                }
                Tag::BlockQuote(_) => out.push_str("\\begin{quote}\n"),
                Tag::List(Some(1)) => out.push_str("\\begin{enumerate}\n"),
                Tag::List(Some(_)) => out.push_str("\\begin{enumerate}\n"),
                Tag::List(None) => out.push_str("\\begin{itemize}\n"),
                Tag::Item => out.push_str("\\item "),
                Tag::Table(alignments) => {
                    in_table = true;
                    table_alignments = alignments;
                    let cols: String = table_alignments
                        .iter()
                        .map(|a| match a {
                            pulldown_cmark::Alignment::Left | pulldown_cmark::Alignment::None => {
                                'l'
                            }
                            pulldown_cmark::Alignment::Center => 'c',
                            pulldown_cmark::Alignment::Right => 'r',
                        })
                        .collect();
                    out.push_str(&format!("\\begin{{tabular}}{{{cols}}}\n\\toprule\n"));
                }
                Tag::TableHead => {
                    table_in_head = true;
                    table_cell_idx = 0;
                }
                Tag::TableRow => {
                    table_cell_idx = 0;
                }
                Tag::TableCell => {
                    if table_cell_idx > 0 {
                        out.push_str(" & ");
                    }
                }
                Tag::Link { dest_url, .. } => {
                    out.push_str(&format!("\\href{{{}}}", dest_url));
                    out.push('{');
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => out.push_str("}\n\n"),
                TagEnd::Paragraph => out.push_str("\n\n"),
                TagEnd::Emphasis => out.push('}'),
                TagEnd::Strong => out.push('}'),
                TagEnd::CodeBlock => out.push_str("\\end{verbatim}\n\n"),
                TagEnd::BlockQuote(_) => out.push_str("\\end{quote}\n"),
                TagEnd::List(true) => out.push_str("\\end{enumerate}\n\n"),
                TagEnd::List(false) => out.push_str("\\end{itemize}\n\n"),
                TagEnd::Item => out.push('\n'),
                TagEnd::Table => {
                    out.push_str("\\bottomrule\n\\end{tabular}\n\n");
                    in_table = false;
                }
                TagEnd::TableHead => {
                    out.push_str(" \\\\\n\\midrule\n");
                    table_in_head = false;
                }
                TagEnd::TableRow => {
                    if !table_in_head {
                        out.push_str(" \\\\\n");
                    }
                }
                TagEnd::TableCell => {
                    table_cell_idx += 1;
                }
                TagEnd::Link => out.push('}'),
                _ => {}
            },
            Event::Text(text) => {
                if in_table {
                    // Don't escape $ in tables — formulas should pass through
                    out.push_str(&escape_latex_preserving_math(&text));
                } else {
                    out.push_str(&escape_latex_preserving_math(&text));
                }
            }
            Event::Code(code) => {
                out.push_str(&format!("\\texttt{{{}}}", escape_latex(&code)));
            }
            Event::SoftBreak => out.push('\n'),
            Event::HardBreak => out.push_str("\\\\\n"),
            Event::InlineMath(math) => {
                out.push('$');
                out.push_str(&math);
                out.push('$');
            }
            Event::DisplayMath(math) => {
                out.push_str("\\[\n");
                out.push_str(&math);
                out.push_str("\n\\]\n");
            }
            Event::Html(html) => {
                // HTML comments / directives — skip
                let _ = html;
            }
            _ => {}
        }
    }

    out
}

/// Escape special LaTeX characters, but preserve $...$ math delimiters.
fn escape_latex_preserving_math(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_math = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '$' {
            in_math = !in_math;
            out.push('$');
            i += 1;
            continue;
        }
        if in_math {
            out.push(ch);
        } else {
            match ch {
                '&' => out.push_str("\\&"),
                '%' => out.push_str("\\%"),
                '#' => out.push_str("\\#"),
                '_' => out.push_str("\\_"),
                '{' => out.push_str("\\{"),
                '}' => out.push_str("\\}"),
                '~' => out.push_str("\\textasciitilde{}"),
                '^' => out.push_str("\\textasciicircum{}"),
                '\\' => out.push_str("\\textbackslash{}"),
                _ => out.push(ch),
            }
        }
        i += 1;
    }

    out
}

/// Escape special LaTeX characters (no math preservation).
fn escape_latex(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("\\&"),
            '%' => out.push_str("\\%"),
            '#' => out.push_str("\\#"),
            '_' => out.push_str("\\_"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            '\\' => out.push_str("\\textbackslash{}"),
            '$' => out.push_str("\\$"),
            _ => out.push(ch),
        }
    }
    out
}

/// Render a Mermaid block into the LaTeX body. On success, writes
/// `<plot_dir>/diagram-<idx>.svg` and emits a `\begin{figure}…\includesvg…`
/// float. On failure or with the `mermaid` feature disabled, falls back to
/// `\begin{verbatim}` containing the source.
fn emit_mermaid_latex(
    body: &mut String,
    source: &str,
    #[cfg_attr(not(feature = "mermaid"), allow(unused_variables))] plot_dir: &Path,
    #[cfg_attr(not(feature = "mermaid"), allow(unused_variables))] href_prefix: &str,
    #[cfg_attr(not(feature = "mermaid"), allow(unused_variables))] diagram_idx: usize,
    #[cfg_attr(not(feature = "mermaid"), allow(unused_variables))] caption: Option<&str>,
) {
    #[cfg(feature = "mermaid")]
    {
        match crate::mermaid::render_to_svg_file(source, plot_dir, diagram_idx) {
            Ok(_) => {
                body.push_str("\\begin{figure}[htbp]\n  \\centering\n  ");
                body.push_str(&format!(
                    "\\includesvg[width=0.8\\linewidth]{{{href_prefix}/diagram-{diagram_idx}}}\n"
                ));
                if let Some(cap) = caption {
                    body.push_str(&format!("  \\caption{{{}}}\n", escape_latex(cap)));
                }
                body.push_str("\\end{figure}\n\n");
                return;
            }
            Err(e) => {
                eprintln!(
                    "warning: mermaid render failed for diagram-{diagram_idx}, embedding source: {e}"
                );
            }
        }
    }
    #[cfg(not(feature = "mermaid"))]
    {
        warn_mermaid_disabled_once_latex();
    }
    body.push_str("\\begin{verbatim}\n");
    body.push_str(source);
    body.push_str("\n\\end{verbatim}\n\n");
}

#[cfg(not(feature = "mermaid"))]
fn warn_mermaid_disabled_once_latex() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        eprintln!(
            "warning: rustlab-notebook built without 'mermaid' feature. \
             Mermaid blocks rendered as verbatim source in LaTeX/PDF output."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execute::Rendered;

    fn light() -> &'static ThemeColors {
        Theme::Light.colors()
    }

    // ── escape_latex ──

    #[test]
    fn escape_latex_special_chars() {
        assert_eq!(escape_latex("a & b"), "a \\& b");
        assert_eq!(escape_latex("100%"), "100\\%");
        assert_eq!(escape_latex("#1"), "\\#1");
        assert_eq!(escape_latex("x_1"), "x\\_1");
        assert_eq!(escape_latex("{x}"), "\\{x\\}");
        assert_eq!(escape_latex("~"), "\\textasciitilde{}");
        assert_eq!(escape_latex("^"), "\\textasciicircum{}");
        assert_eq!(escape_latex("\\"), "\\textbackslash{}");
        assert_eq!(escape_latex("$5"), "\\$5");
    }

    #[test]
    fn escape_latex_passthrough() {
        assert_eq!(escape_latex("hello world"), "hello world");
    }

    // ── escape_latex_preserving_math ──

    #[test]
    fn preserve_math_inline() {
        let result = escape_latex_preserving_math("value of $x_1$ is 5%");
        assert_eq!(result, "value of $x_1$ is 5\\%");
    }

    #[test]
    fn preserve_math_multiple() {
        let result = escape_latex_preserving_math("$a$ and $b$");
        assert_eq!(result, "$a$ and $b$");
    }

    #[test]
    fn preserve_math_no_math() {
        let result = escape_latex_preserving_math("a & b");
        assert_eq!(result, "a \\& b");
    }

    #[test]
    fn preserve_math_special_inside_math() {
        // Inside $...$, special chars should NOT be escaped
        let result = escape_latex_preserving_math("$x_{max}$");
        assert_eq!(result, "$x_{max}$");
    }

    #[test]
    fn preserve_math_empty() {
        assert_eq!(escape_latex_preserving_math(""), "");
    }

    // ── markdown_to_latex ──

    #[test]
    fn md_to_latex_heading_h1() {
        let out = markdown_to_latex("# Title");
        assert!(out.contains("\\section{Title}"));
    }

    #[test]
    fn md_to_latex_heading_h2() {
        let out = markdown_to_latex("## Sub");
        assert!(out.contains("\\subsection{Sub}"));
    }

    #[test]
    fn md_to_latex_heading_h3() {
        let out = markdown_to_latex("### Sub Sub");
        assert!(out.contains("\\subsubsection{Sub Sub}"));
    }

    #[test]
    fn md_to_latex_emphasis() {
        let out = markdown_to_latex("*italic*");
        assert!(out.contains("\\emph{italic}"));
    }

    #[test]
    fn md_to_latex_strong() {
        let out = markdown_to_latex("**bold**");
        assert!(out.contains("\\textbf{bold}"));
    }

    #[test]
    fn md_to_latex_inline_code() {
        let out = markdown_to_latex("`x = 1`");
        assert!(out.contains("\\texttt{"));
    }

    #[test]
    fn md_to_latex_code_block() {
        let out = markdown_to_latex("```\ncode here\n```");
        assert!(out.contains("\\begin{verbatim}"));
        assert!(out.contains("\\end{verbatim}"));
    }

    #[test]
    fn md_to_latex_unordered_list() {
        let out = markdown_to_latex("- item one\n- item two");
        assert!(out.contains("\\begin{itemize}"));
        assert!(out.contains("\\item"));
        assert!(out.contains("\\end{itemize}"));
    }

    #[test]
    fn md_to_latex_ordered_list() {
        let out = markdown_to_latex("1. first\n2. second");
        assert!(out.contains("\\begin{enumerate}"));
        assert!(out.contains("\\item"));
        assert!(out.contains("\\end{enumerate}"));
    }

    #[test]
    fn md_to_latex_blockquote() {
        let out = markdown_to_latex("> quoted text");
        assert!(out.contains("\\begin{quote}"));
        assert!(out.contains("\\end{quote}"));
    }

    #[test]
    fn md_to_latex_link() {
        let out = markdown_to_latex("[click](https://example.com)");
        assert!(out.contains("\\href{https://example.com}"));
        assert!(out.contains("{click}"));
    }

    #[test]
    fn md_to_latex_table() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |";
        let out = markdown_to_latex(md);
        assert!(out.contains("\\begin{tabular}"));
        assert!(out.contains("\\toprule"));
        assert!(out.contains("\\midrule"));
        assert!(out.contains("\\bottomrule"));
        assert!(out.contains("\\end{tabular}"));
        assert!(out.contains(" & "));
    }

    #[test]
    fn md_to_latex_inline_math() {
        let out = markdown_to_latex("The value $x^2$ is large.");
        assert!(out.contains("$x^2$"));
    }

    #[test]
    fn md_to_latex_display_math() {
        let out = markdown_to_latex("$$E = mc^2$$");
        assert!(out.contains("\\[\nE = mc^2\n\\]"));
    }

    #[test]
    fn md_to_latex_special_chars_escaped() {
        let out = markdown_to_latex("Use 100% of the CPU & GPU");
        assert!(out.contains("100\\%"));
        assert!(out.contains("\\&"));
    }

    #[test]
    fn md_to_latex_paragraph() {
        let out = markdown_to_latex("Para one.\n\nPara two.");
        // Paragraphs should be separated
        assert!(out.contains("Para one."));
        assert!(out.contains("Para two."));
    }

    #[test]
    fn md_to_latex_empty() {
        assert_eq!(markdown_to_latex(""), "");
    }

    // ── render_latex (integration) ──

    #[test]
    fn render_latex_preamble() {
        let tex = render_latex(
            "Test Title",
            &[],
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
        );
        assert!(tex.contains("\\documentclass"));
        assert!(tex.contains("\\usepackage{graphicx}"));
        assert!(tex.contains("\\usepackage{svg}"));
        assert!(tex.contains("\\usepackage{amsmath,amssymb}"));
        assert!(tex.contains("\\usepackage{booktabs}"));
        assert!(tex.contains("\\begin{document}"));
        assert!(tex.contains("\\end{document}"));
        assert!(tex.contains("\\maketitle"));
    }

    #[test]
    fn render_latex_title_escaped() {
        let tex = render_latex(
            "A & B",
            &[],
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
        );
        assert!(tex.contains("\\title{A \\& B}"));
    }

    #[test]
    fn render_latex_code_block() {
        let blocks = vec![Rendered::Code {
            source: "x = 42".to_string(),
            text_output: String::new(),
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
        }];
        let tex = render_latex(
            "Test",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
        );
        assert!(tex.contains("\\begin{verbatim}\nx = 42\n\\end{verbatim}"));
    }

    #[test]
    fn render_latex_hidden_block() {
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
        let tex = render_latex(
            "Test",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
        );
        // Source should not appear in verbatim
        assert!(!tex.contains("secret = 42"));
        // But text output should
        assert!(tex.contains("ans = 42"));
    }

    #[test]
    fn render_latex_text_output() {
        let blocks = vec![Rendered::Code {
            source: "x = 1".to_string(),
            text_output: "ans = 1".to_string(),
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
        }];
        let tex = render_latex(
            "Test",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
        );
        assert!(tex.contains("\\begin{quote}"));
        assert!(tex.contains("ans = 1"));
    }

    #[test]
    fn render_latex_empty_output_not_shown() {
        let blocks = vec![Rendered::Code {
            source: "x = 1;".to_string(),
            text_output: "   \n  ".to_string(),
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
        }];
        let tex = render_latex(
            "Test",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
        );
        // Only one verbatim (source), no quote block for output
        let verbatim_count = tex.matches("\\begin{verbatim}").count();
        assert_eq!(verbatim_count, 1);
        assert!(!tex.contains("\\begin{quote}"));
    }

    #[test]
    fn render_latex_error() {
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
        let tex = render_latex(
            "Test",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
        );
        assert!(tex.contains("\\color[HTML]{"));
        assert!(tex.contains("undefined variable"));
    }

    #[test]
    fn render_latex_markdown_section() {
        let blocks = vec![Rendered::Markdown(
            "## Analysis\n\nSome text with $x^2$ math.".to_string(),
        )];
        let tex = render_latex(
            "Test",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
        );
        assert!(tex.contains("\\subsection{Analysis}"));
        assert!(tex.contains("$x^2$"));
    }

    // ── Mermaid blocks ──

    fn mermaid_plot_dir(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "rustlab_render_latex_mermaid_{}_{}",
            std::process::id(),
            tag,
        ));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn mermaid_emits_figure_with_includesvg() {
        let dir = mermaid_plot_dir("fig");
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B\n".to_string(),
            hidden: false,
            details: None,
            caption: None,
        }];
        let tex = render_latex("T", &blocks, &dir, "plots/test", light());
        assert!(tex.contains("\\begin{figure}[htbp]"));
        assert!(tex.contains("\\includesvg[width=0.8\\linewidth]{plots/test/diagram-1}"));
        assert!(tex.contains("\\end{figure}"));
        assert!(dir.join("diagram-1.svg").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn mermaid_caption_present_when_set() {
        let dir = mermaid_plot_dir("cap");
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B\n".to_string(),
            hidden: false,
            details: None,
            caption: Some("Signal flow".to_string()),
        }];
        let tex = render_latex("T", &blocks, &dir, "plots/test", light());
        assert!(tex.contains("\\caption{Signal flow}"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "mermaid")]
    #[test]
    fn mermaid_no_caption_omits_command() {
        let dir = mermaid_plot_dir("nocap");
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B\n".to_string(),
            hidden: false,
            details: None,
            caption: None,
        }];
        let tex = render_latex("T", &blocks, &dir, "plots/test", light());
        assert!(!tex.contains("\\caption{"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mermaid_hidden_omits_figure() {
        let dir = mermaid_plot_dir("hidden");
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B\n".to_string(),
            hidden: true,
            details: None,
            caption: None,
        }];
        let tex = render_latex("T", &blocks, &dir, "plots/test", light());
        assert!(!tex.contains("\\begin{figure}"));
        assert!(!tex.contains("\\includesvg"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(feature = "mermaid"))]
    #[test]
    fn mermaid_feature_disabled_emits_verbatim() {
        let dir = mermaid_plot_dir("disabled");
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B\n".to_string(),
            hidden: false,
            details: None,
            caption: None,
        }];
        let tex = render_latex("T", &blocks, &dir, "plots/test", light());
        assert!(tex.contains("\\begin{verbatim}"));
        assert!(tex.contains("flowchart LR"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
