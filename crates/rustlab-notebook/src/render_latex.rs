use crate::execute::Rendered;
use crate::render::{notebook_md_options, parse_single_tilde_safe, transform_wikilinks};
use pulldown_cmark::{Event, HeadingLevel, Options, Tag, TagEnd};
use rustlab_plot::theme::{Theme, ThemeColors};
use std::path::Path;

/// Render executed notebook blocks into a LaTeX document string.
///
/// Plot images are written to `plot_dir` as SVG files and referenced from
/// the rendered `.tex` via `\includegraphics{plot_href_prefix/plot-N}`
/// (PDF companions are produced by fixed-argv Inkscape before TeX runs).
/// Splitting the on-disk write location from the include path lets callers
/// nest plots under a single `plots/<stem>/` umbrella the same way the
/// markdown emitter does, without coupling the path inside
/// `\includegraphics` to the directory the SVGs are written to.
pub fn render_latex(
    title: &str,
    blocks: &[Rendered],
    plot_dir: &Path,
    plot_href_prefix: &str,
    theme: &ThemeColors,
    link: &crate::render::LinkMode,
) -> String {
    let mut body = String::new();
    let mut plot_idx = 0;
    let mut in_exercise = false;

    let _ = std::fs::create_dir_all(plot_dir);
    let href_prefix = plot_href_prefix.trim_end_matches('/').to_string();

    // Printed pages are Catppuccin Latte on white paper. `-t` and the
    // rc file still theme HTML and `notebook watch`; they do not theme
    // LaTeX or PDF. A dark `pagecolor` left body text black and unreadable.
    let _html_theme = theme;
    let theme = Theme::Light.colors();

    for block in blocks {
        match block {
            Rendered::Markdown(md) => {
                body.push_str(&markdown_to_latex(md, link));
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
                // HTML-only. PDF always shows the source; `code:` is ignored.
                source_open: _,
            } => {
                // Source, printed text, and errors are separate breakable
                // panels. Plots stay full width after the panels. `code:`
                // is HTML-only; PDF always shows the source.
                let trimmed = text_output.trim();
                if !hidden {
                    body.push_str(
                        "\\noindent{\\sffamily\\footnotesize\\textcolor{rldim}{rustlab}}\\par\\nopagebreak\n",
                    );
                    body.push_str("\\begin{rlsource}\n");
                    // Colored with \textcolor — never minted, which would
                    // need shell-escape. Text mode, so the tokens stay escaped.
                    body.push_str(&emit_highlighted_source(source));
                    body.push_str("\\end{rlsource}\n\n");
                }

                // No collapsible disclosure. The title sits on its own line,
                // outside the accent rule, so it does not notch the panel.
                if let Some(title) = details {
                    body.push_str(&emit_details_label(title));
                }

                if !trimmed.is_empty() {
                    body.push_str("\\begin{rloutput}\n\\begin{rlverb}\n");
                    body.push_str(&neutralize_verb_end(trimmed));
                    body.push_str("\n\\end{rlverb}\n\\end{rloutput}\n\n");
                }

                if let Some(err) = error {
                    body.push_str("\\begin{rlerror}\n\\begin{rlverb}\n");
                    body.push_str(&neutralize_verb_end(err));
                    body.push_str("\n\\end{rlverb}\n\\end{rlerror}\n\n");
                }

                emit_figures(
                    &mut body,
                    figures,
                    grid_cols.as_ref().copied(),
                    plot_dir,
                    &href_prefix,
                    theme,
                    &mut plot_idx,
                );

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
                    body.push_str(&emit_details_label(title));
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
            Rendered::Widget { decl, value } => {
                // Static export: render the widget's label and current value.
                let label = decl.label.as_deref().unwrap_or(&decl.name);
                let val = match value {
                    rustlab_script::WidgetValue::Number(n) => format!("{n}"),
                    rustlab_script::WidgetValue::Text(s) => s.clone(),
                };
                body.push_str(&format!(
                    "\\textbf{{{}:}} \\texttt{{{}}}\n\n",
                    escape_latex(label),
                    escape_latex(&val),
                ));
            }
            Rendered::Callout {
                kind,
                title,
                content,
            } => {
                let label = title.as_deref().unwrap_or(kind.default_label());
                let frame = callout_frame(kind);
                body.push_str(&format!(
                    "\\begin{{rlcallout}}{{{frame}}}{{{}}}\n",
                    escape_latex(label),
                ));
                body.push_str(&markdown_to_latex(content, link));
                body.push_str("\\end{rlcallout}\n\n");
            }
            Rendered::ExerciseStart { number } => {
                if in_exercise {
                    body.push_str("\\end{rlexercise}\n\n");
                }
                body.push_str(&format!(
                    "\\begin{{rlexercise}}\n{{\\sffamily\\bfseries\\textcolor{{rlh1}}{{Exercise {number}.}}}}\\par\\smallskip\n"
                ));
                in_exercise = true;
            }
            Rendered::SolutionStart => {
                body.push_str(
                    "\\par\\medskip\\noindent\\textcolor{rllink}{\\textbf{Solution}}\\par\\nopagebreak\\smallskip\n",
                );
            }
        }
    }
    if in_exercise {
        body.push_str("\\end{rlexercise}\n\n");
    }

    let palette = latex_palette(theme);

    format!(
        r#"\documentclass[11pt,a4paper]{{article}}
\usepackage[utf8]{{inputenc}}
\usepackage[T1]{{fontenc}}
\usepackage{{lmodern}}
\usepackage{{geometry}}
\geometry{{margin=1in}}
\usepackage{{graphicx}}
\usepackage{{amsmath,amssymb}}
\usepackage{{newunicodechar}}
% Map common math / Greek / arrow Unicode characters that appear in
% notebook prose and code-block output. Without these, pdflatex with
% [utf8]{{inputenc}} rejects the character with "not set up for use".
% Source: the failing characters observed across examples/notebooks/.
% Math relations and operators
\newunicodechar{{≈}}{{\ensuremath{{\approx}}}}
\newunicodechar{{≡}}{{\ensuremath{{\equiv}}}}
\newunicodechar{{≤}}{{\ensuremath{{\le}}}}
\newunicodechar{{≥}}{{\ensuremath{{\ge}}}}
\newunicodechar{{≠}}{{\ensuremath{{\ne}}}}
\newunicodechar{{±}}{{\ensuremath{{\pm}}}}
\newunicodechar{{∓}}{{\ensuremath{{\mp}}}}
\newunicodechar{{×}}{{\ensuremath{{\times}}}}
\newunicodechar{{÷}}{{\ensuremath{{\div}}}}
\newunicodechar{{−}}{{\ensuremath{{-}}}}
\newunicodechar{{∇}}{{\ensuremath{{\nabla}}}}
\newunicodechar{{∂}}{{\ensuremath{{\partial}}}}
\newunicodechar{{∞}}{{\ensuremath{{\infty}}}}
\newunicodechar{{∫}}{{\ensuremath{{\int}}}}
\newunicodechar{{∑}}{{\ensuremath{{\sum}}}}
\newunicodechar{{∏}}{{\ensuremath{{\prod}}}}
\newunicodechar{{√}}{{\ensuremath{{\sqrt{{}}}}}}
\newunicodechar{{∠}}{{\ensuremath{{\angle}}}}
\newunicodechar{{∩}}{{\ensuremath{{\cap}}}}
% Greek letters (lowercase + selected uppercase)
\newunicodechar{{α}}{{\ensuremath{{\alpha}}}}
\newunicodechar{{β}}{{\ensuremath{{\beta}}}}
\newunicodechar{{γ}}{{\ensuremath{{\gamma}}}}
\newunicodechar{{Γ}}{{\ensuremath{{\Gamma}}}}
\newunicodechar{{δ}}{{\ensuremath{{\delta}}}}
\newunicodechar{{Δ}}{{\ensuremath{{\Delta}}}}
\newunicodechar{{ε}}{{\ensuremath{{\varepsilon}}}}
\newunicodechar{{η}}{{\ensuremath{{\eta}}}}
\newunicodechar{{θ}}{{\ensuremath{{\theta}}}}
\newunicodechar{{Θ}}{{\ensuremath{{\Theta}}}}
\newunicodechar{{λ}}{{\ensuremath{{\lambda}}}}
\newunicodechar{{Λ}}{{\ensuremath{{\Lambda}}}}
\newunicodechar{{μ}}{{\ensuremath{{\mu}}}}
\newunicodechar{{π}}{{\ensuremath{{\pi}}}}
\newunicodechar{{Π}}{{\ensuremath{{\Pi}}}}
\newunicodechar{{σ}}{{\ensuremath{{\sigma}}}}
\newunicodechar{{Σ}}{{\ensuremath{{\Sigma}}}}
\newunicodechar{{φ}}{{\ensuremath{{\varphi}}}}
\newunicodechar{{Φ}}{{\ensuremath{{\Phi}}}}
\newunicodechar{{ψ}}{{\ensuremath{{\psi}}}}
\newunicodechar{{Ψ}}{{\ensuremath{{\Psi}}}}
\newunicodechar{{Ω}}{{\ensuremath{{\Omega}}}}
\newunicodechar{{ω}}{{\ensuremath{{\omega}}}}
% Arrows
\newunicodechar{{⇒}}{{\ensuremath{{\Rightarrow}}}}
\newunicodechar{{⇔}}{{\ensuremath{{\Leftrightarrow}}}}
\newunicodechar{{→}}{{\ensuremath{{\to}}}}
\newunicodechar{{←}}{{\ensuremath{{\leftarrow}}}}
\newunicodechar{{↔}}{{\ensuremath{{\leftrightarrow}}}}
\newunicodechar{{↗}}{{\ensuremath{{\nearrow}}}}
\newunicodechar{{↘}}{{\ensuremath{{\searrow}}}}
\newunicodechar{{↙}}{{\ensuremath{{\swarrow}}}}
\newunicodechar{{↖}}{{\ensuremath{{\nwarrow}}}}
% Superscripts and units. Full 0–9 range so notebooks using subscript
% / superscript notation (variable indexing, exponents, footnote
% numerals) compile cleanly — pdflatex with [utf8]{{inputenc}} rejects
% undeclared codepoints fatally, so partial coverage is brittle.
\newunicodechar{{⁰}}{{\ensuremath{{^0}}}}
\newunicodechar{{¹}}{{\ensuremath{{^1}}}}
\newunicodechar{{²}}{{\ensuremath{{^2}}}}
\newunicodechar{{³}}{{\ensuremath{{^3}}}}
\newunicodechar{{⁴}}{{\ensuremath{{^4}}}}
\newunicodechar{{⁵}}{{\ensuremath{{^5}}}}
\newunicodechar{{⁶}}{{\ensuremath{{^6}}}}
\newunicodechar{{⁷}}{{\ensuremath{{^7}}}}
\newunicodechar{{⁸}}{{\ensuremath{{^8}}}}
\newunicodechar{{⁹}}{{\ensuremath{{^9}}}}
\newunicodechar{{₀}}{{\ensuremath{{_0}}}}
\newunicodechar{{₁}}{{\ensuremath{{_1}}}}
\newunicodechar{{₂}}{{\ensuremath{{_2}}}}
\newunicodechar{{₃}}{{\ensuremath{{_3}}}}
\newunicodechar{{₄}}{{\ensuremath{{_4}}}}
\newunicodechar{{₅}}{{\ensuremath{{_5}}}}
\newunicodechar{{₆}}{{\ensuremath{{_6}}}}
\newunicodechar{{₇}}{{\ensuremath{{_7}}}}
\newunicodechar{{₈}}{{\ensuremath{{_8}}}}
\newunicodechar{{₉}}{{\ensuremath{{_9}}}}
\newunicodechar{{°}}{{\ensuremath{{^{{\circ}}}}}}
\newunicodechar{{µ}}{{\ensuremath{{\mu}}}}
% Combining diacritics — combining chars overlay on the preceding
% glyph, but as a `\newunicodechar` substitution they're already
% standalone. Emit empty so pdflatex doesn't fatal; authors who need
% true overlay (x̄, x̃, …) should use `$\bar{{x}}$` / `$\tilde{{x}}$`
% in source rather than relying on Unicode combining sequences.
\newunicodechar{{̄}}{{}}
% Marks and check glyphs (commonly used in LLM/ML notebook output,
% architecture diagrams, prose). `\checkmark` is provided by amssymb
% (already loaded above).
\newunicodechar{{✓}}{{\ensuremath{{\checkmark}}}}
\newunicodechar{{✗}}{{$\times$}}
% Punctuation, dashes, ellipsis
\newunicodechar{{§}}{{\S{{}}}}
\newunicodechar{{·}}{{\ensuremath{{\cdot}}}}
\newunicodechar{{—}}{{\textemdash{{}}}}
\newunicodechar{{–}}{{\textendash{{}}}}
\newunicodechar{{…}}{{\ensuremath{{\ldots}}}}
\newunicodechar{{ï}}{{\"\i{{}}}}
% Box-drawing characters appear in REPL/console output AND in
% architecture diagrams in ML/LLM lesson prose. Map to ASCII so the
% layout reads sensibly through the LaTeX render (pdfTeX can't render
% the actual box-drawing glyphs without a Unicode-aware font setup).
\newunicodechar{{─}}{{-}}
\newunicodechar{{│}}{{|}}
\newunicodechar{{┌}}{{+}}
\newunicodechar{{┐}}{{+}}
\newunicodechar{{└}}{{+}}
\newunicodechar{{┘}}{{+}}
\newunicodechar{{├}}{{+}}
\newunicodechar{{┤}}{{+}}
\newunicodechar{{┬}}{{+}}
\newunicodechar{{┴}}{{+}}
\newunicodechar{{┼}}{{+}}
\usepackage[table]{{xcolor}}
{palette}
\usepackage{{tcolorbox}}
\tcbuselibrary{{breakable,skins}}
\usepackage{{fancyvrb}}
\usepackage{{sectsty}}
\usepackage{{float}}
\usepackage{{booktabs}}
\usepackage[normalem]{{ulem}}
\usepackage{{hyperref}}
\hypersetup{{colorlinks=true,linkcolor=rllink,urlcolor=rllink}}
\DefineVerbatimEnvironment{{rlverb}}{{Verbatim}}{{fontsize=\small}}
\tcbset{{
  rlbase/.style={{
    breakable,
    enhanced,
    arc=1.5mm,
    boxrule=0.4pt,
    leftrule=3pt,
    left=2.2mm,
    right=2mm,
    top=1.1mm,
    bottom=1.1mm,
    before skip=0.2em,
    after skip=0.2em,
    colframe=rlborder,
    overlay unbroken and first={{
      \draw[rlrule,line width=2.6pt] ([xshift=1.3pt]frame.north west) -- ([xshift=1.3pt]frame.south west);
    }},
    overlay middle and last={{
      \draw[rlrule,line width=2.6pt] ([xshift=1.3pt]frame.north west) -- ([xshift=1.3pt]frame.south west);
    }},
  }},
}}
\newtcolorbox{{rlsource}}{{rlbase, colback=rlcodebg, colupper=rltext, after skip=0.28em}}
\newtcolorbox{{rloutput}}{{rlbase, colback=rloutbg, colupper=rldim, before skip=0.22em}}
\newtcolorbox{{rlerror}}{{rlbase, colback=rlerrbg, colupper=rlerrfg, colframe=rlerrfg, before skip=0.22em}}
\newtcolorbox{{rlcallout}}[2]{{
  breakable, enhanced, arc=1.5mm,
  boxrule=0pt, leftrule=4pt,
  colback=rlpanel, colframe=#1, coltitle=#1,
  fonttitle=\bfseries\sffamily,
  title={{#2}},
  left=2.5mm, right=2.5mm, top=1.4mm, bottom=1.4mm,
  before skip=0.75em, after skip=0.75em,
}}
\newtcolorbox{{rlexercise}}{{
  breakable, enhanced, arc=2mm,
  boxrule=0.6pt, colframe=rlborder, colback=rlpanel,
  left=2.5mm, right=2.5mm, top=1.5mm, bottom=1.5mm,
  before skip=0.9em, after skip=0.9em,
}}
\sectionfont{{\color{{rlh1}}\normalfont\Large\bfseries}}
\subsectionfont{{\color{{rlh2}}\normalfont\large\bfseries}}
\subsubsectionfont{{\color{{rlh3}}\normalfont\normalsize\bfseries}}
\setcounter{{secnumdepth}}{{-1}}
\title{{{title}}}
\date{{}}

\begin{{document}}
\color{{rltext}}
\maketitle

{body}
\end{{document}}
"#,
        title = escape_latex(title),
        body = body,
        palette = palette,
    )
}

fn latex_palette(theme: &ThemeColors) -> String {
    let colors = [
        ("rltext", theme.text),
        ("rldim", theme.text_dim),
        ("rlh1", theme.accent_primary),
        ("rlh2", theme.accent_secondary),
        ("rlh3", theme.accent_tertiary),
        ("rlrule", theme.accent_primary),
        ("rllink", theme.accent_secondary),
        ("rlkw", theme.syn_keyword),
        ("rlfn", theme.syn_function),
        ("rlnum", theme.syn_number),
        ("rlstr", theme.syn_string),
        ("rlcom", theme.syn_comment),
        ("rlop", theme.syn_operator),
        ("rlcodebg", theme.code_bg),
        ("rloutbg", theme.output_bg),
        ("rlpanel", theme.bg_secondary),
        ("rlcodepill", theme.inline_code_bg),
        ("rlthead", theme.inline_code_bg),
        ("rlborder", theme.border),
        ("rlerrbg", theme.error_bg),
        ("rlerrfg", theme.error_text),
    ];
    let mut out = String::new();
    for (name, hex) in colors {
        out.push_str(&format!(
            "\\definecolor{{{name}}}{{HTML}}{{{}}}\n",
            html_hex(hex)
        ));
    }
    out
}

fn callout_frame(kind: &crate::parse::CalloutKind) -> &'static str {
    use crate::parse::CalloutKind;
    match kind {
        CalloutKind::Note => "rlh2",
        CalloutKind::Tip => "rlh3",
        CalloutKind::Important => "rlh1",
        CalloutKind::Warning | CalloutKind::Caution => "rlerrfg",
    }
}

/// Details title on its own line, in link blue, outside any panel rule.
fn emit_details_label(title: &str) -> String {
    format!(
        "\\par\\noindent\\textcolor{{rllink}}{{\\textbf{{{}}}}}\\par\\nopagebreak\n\n",
        escape_latex(title)
    )
}

/// `fancyvrb` ends at the literal `\end{rlverb}`. Break that sequence in
/// printed output so a notebook cannot close the environment early.
fn neutralize_verb_end(text: &str) -> String {
    text.replace("\\end{rlverb}", "\\end {rlverb}")
}

fn emit_figures(
    body: &mut String,
    figures: &[rustlab_plot::FigureState],
    grid_cols: Option<usize>,
    plot_dir: &Path,
    href_prefix: &str,
    theme: &ThemeColors,
    plot_idx: &mut usize,
) {
    if figures.is_empty() {
        return;
    }
    // Cap a requested row at 4. Wider requests wrap, and each cell shrinks
    // so the row still fits. A short last row stays left-aligned because
    // `\hfill` is only inserted between cells of a row.
    let cols = grid_cols.map(|n| n.clamp(1, 4));
    if cols.is_some() {
        body.push_str("\\noindent\n");
    }
    for (i, fig) in figures.iter().enumerate() {
        *plot_idx += 1;
        let plot_file = plot_dir.join(format!("plot-{}.svg", *plot_idx));
        if let Err(e) = rustlab_plot::render_figure_state_to_file_themed(
            fig,
            &plot_file.to_string_lossy(),
            theme,
        ) {
            eprintln!("warning: could not render plot-{}: {e}", *plot_idx);
            continue;
        }
        if let Some(n) = cols {
            let width = 0.96 / n as f64;
            if i > 0 && i % n == 0 {
                body.push_str("\\par\\vspace{0.45em}\n\\noindent\n");
            } else if i > 0 {
                body.push_str("\\hfill\n");
            }
            body.push_str(&format!(
                "\\begin{{minipage}}[t]{{{width:.3}\\textwidth}}\\centering\n\\includegraphics[width=\\linewidth]{{{href_prefix}/plot-{}}}\\end{{minipage}}%\n",
                *plot_idx,
            ));
        } else {
            body.push_str(&format!(
                "\\begin{{center}}\n\\includegraphics[width=0.9\\textwidth]{{{href_prefix}/plot-{}}}\\end{{center}}\n\n",
                *plot_idx,
            ));
        }
    }
    if cols.is_some() {
        body.push_str("\\par\n\n");
    }
}

/// Convert a markdown string to LaTeX using pulldown-cmark events.
///
/// Cross-notebook `.md` links resolve through the shared [`LinkMode`]
/// contract, but to sibling `.pdf` artifacts (the files a directory PDF
/// build emits) and with fragments dropped — the generated PDFs carry no
/// named destinations for markdown anchors. `\href{a.md}` pointed a PDF
/// reader at the raw source file, which ships nowhere.
fn markdown_to_latex(md: &str, link: &crate::render::LinkMode) -> String {
    let md = transform_wikilinks(md);
    let mut opts = notebook_md_options();
    opts.insert(Options::ENABLE_MATH);
    // Same single-tilde demotion as the HTML target: `~word~` stays literal
    // prose; only `~~word~~` becomes strikethrough.
    let events = parse_single_tilde_safe(&md, opts);

    let mut out = String::new();
    #[allow(unused_assignments)]
    let mut table_alignments: Vec<pulldown_cmark::Alignment> = Vec::new();
    let mut table_cell_idx: usize = 0;
    let mut table_in_head = false;

    for event in events {
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
                Tag::Strikethrough => out.push_str("\\sout{"),
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
                    out.push_str("\\rowcolor{rlthead}");
                }
                Tag::TableRow => {
                    table_cell_idx = 0;
                }
                Tag::TableCell => {
                    if table_cell_idx > 0 {
                        out.push_str(" & ");
                    }
                    if table_in_head {
                        out.push_str("\\textcolor{rlh1}{\\textbf{");
                    }
                }
                Tag::Link { dest_url, .. } => {
                    let dest = crate::render::rewrite_link_dest_pdf(&dest_url, link)
                        .unwrap_or_else(|| dest_url.to_string());
                    out.push_str(&format!("\\href{{{}}}", escape_href_dest(&dest)));
                    out.push('{');
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => out.push_str("}\n\n"),
                TagEnd::Paragraph => out.push_str("\n\n"),
                TagEnd::Emphasis => out.push('}'),
                TagEnd::Strong => out.push('}'),
                TagEnd::Strikethrough => out.push('}'),
                TagEnd::CodeBlock => out.push_str("\\end{verbatim}\n\n"),
                TagEnd::BlockQuote(_) => out.push_str("\\end{quote}\n"),
                TagEnd::List(true) => out.push_str("\\end{enumerate}\n\n"),
                TagEnd::List(false) => out.push_str("\\end{itemize}\n\n"),
                TagEnd::Item => out.push('\n'),
                TagEnd::Table => {
                    out.push_str("\\bottomrule\n\\end{tabular}\n\n");
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
                    if table_in_head {
                        out.push_str("}}");
                    }
                    table_cell_idx += 1;
                }
                TagEnd::Link => out.push('}'),
                _ => {}
            },
            Event::Text(text) => {
                // With `Options::ENABLE_MATH` on, pulldown-cmark delivers
                // inline math via `Event::InlineMath` and display math via
                // `Event::DisplayMath`. Anything that survives into a Text
                // event is literal prose — including a backslash-escaped
                // `\$` from the source, which arrives here as a bare `$`
                // that must be escaped to `\$`, not preserved as a math
                // delimiter.
                out.push_str(&escape_latex(&text));
            }
            Event::Code(code) => {
                let esc = escape_latex(&code);
                // Bookmark text cannot carry a colorbox. The visible pill
                // matches the HTML inline-code background.
                out.push_str("\\texorpdfstring{\\colorbox{rlcodepill}{\\texttt{");
                out.push_str(&esc);
                out.push_str("}}}{\\texttt{");
                out.push_str(&esc);
                out.push_str("}}");
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
            Event::Html(html) | Event::InlineHtml(html) => {
                // Raw HTML is never passed through to TeX (XSS / write18
                // surface). HTML comments (author notes, directives that
                // survived parsing) are dropped; other markup is emitted as
                // escaped text so authors still see it.
                let visible = strip_html_comments(&html);
                if !visible.trim().is_empty() {
                    out.push_str(&escape_latex(&visible));
                }
            }
            _ => {}
        }
    }

    out
}

/// Escape an `\href` DESTINATION.
///
/// hyperref tolerates most URL characters, but `#` and `%` are fatal
/// whenever the `\href` expands inside an already-tokenized macro argument
/// — `\section{}`, `\textbf{}`, `\emph{}` — which is exactly where markdown
/// puts links ("see **[setup](#setup)**", a link in a heading). A same-page
/// anchor or any external URL with a fragment or percent-escape then kills
/// the whole PDF build. `{`/`}` would unbalance the argument; `\` starts a
/// control sequence. Everything else (`_ & ~` spaces) compiles fine in all
/// contexts, verified against tectonic — do not over-escape, hyperref
/// treats the argument as a URL, not text.
fn escape_href_dest(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '#' => out.push_str("\\#"),
            '%' => out.push_str("\\%"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\\' => out.push_str("\\textbackslash{}"),
            _ => out.push(ch),
        }
    }
    out
}

fn html_hex(color: &str) -> &str {
    color.strip_prefix('#').unwrap_or(color)
}

/// Colored rustlab source. Each token is passed through [`escape_latex`]
/// and wrapped in `\textcolor`. Output stays in text mode so the
/// preamble's `\newunicodechar` mappings still apply (they do not fire
/// inside `verbatim`). No `minted` / shell-escape.
fn emit_highlighted_source(source: &str) -> String {
    use rustlab_script::highlight::HlKind;
    // Not `flushleft`: that trivlist clears `\everypar`, which drops
    // the cell's accent rule. Ragged right plus `\obeylines` keeps one
    // paragraph per source line so the rule hook fires on each of them.
    let mut out = String::from(
        "{\\ttfamily\\setlength{\\parindent}{0pt}\\setlength{\\parskip}{0pt}%\n\
         \\setlength{\\rightskip}{0pt plus 1fil}\\obeylines\\obeyspaces\n",
    );
    for span in rustlab_script::highlight::highlight(source) {
        let text = &source[span.start..span.end];
        let escaped = escape_latex(text);
        let color = match span.kind {
            HlKind::Keyword => "rlkw",
            HlKind::Function => "rlfn",
            HlKind::Number => "rlnum",
            HlKind::String => "rlstr",
            HlKind::Comment => {
                // Stay in the typewriter family. `\textit` would switch to
                // roman italic and break the mono column.
                out.push_str("{\\itshape\\textcolor{rlcom}{");
                out.push_str(&escaped);
                out.push_str("}}");
                continue;
            }
            HlKind::Operator => "rlop",
            HlKind::Text => {
                out.push_str(&escaped);
                continue;
            }
        };
        out.push_str("\\textcolor{");
        out.push_str(color);
        out.push_str("}{");
        out.push_str(&escaped);
        out.push('}');
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("}\n\n");
    out
}

/// Escape special LaTeX characters (no math preservation — math is
/// delivered through `Event::InlineMath` / `Event::DisplayMath` with
/// `Options::ENABLE_MATH`, so anything reaching us in a `Text` event is
/// literal prose).
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

/// Remove `<!-- … -->` spans (an unterminated comment swallows the rest).
fn strip_html_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        rest = match rest[start + 4..].find("-->") {
            Some(end) => &rest[start + 4 + end + 3..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// Render a Mermaid block into the LaTeX body. On success, writes
/// `<plot_dir>/diagram-<idx>.svg` (converted to `.pdf` before TeX runs)
/// and emits a `\begin{figure}…\includegraphics…` float. On failure or
/// with the `mermaid` feature disabled, falls back to `\begin{verbatim}`
/// containing the source.
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
                // `[H]` (float package) keeps the figure with its heading.
                // `[htbp]` floated the diagram above the section that
                // introduced it.
                body.push_str("\\begin{figure}[H]\n  \\centering\n  ");
                body.push_str(&format!(
                    "\\includegraphics[width=0.8\\linewidth]{{{href_prefix}/diagram-{diagram_idx}}}\n"
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

    // ── markdown_to_latex ──

    #[test]
    fn md_to_latex_heading_h1() {
        let out = markdown_to_latex("# Title", &crate::render::LinkMode::single_file());
        assert!(out.contains("\\section{Title}"));
    }

    #[test]
    fn md_to_latex_heading_h2() {
        let out = markdown_to_latex("## Sub", &crate::render::LinkMode::single_file());
        assert!(out.contains("\\subsection{Sub}"));
    }

    #[test]
    fn md_to_latex_heading_h3() {
        let out = markdown_to_latex("### Sub Sub", &crate::render::LinkMode::single_file());
        assert!(out.contains("\\subsubsection{Sub Sub}"));
    }

    #[test]
    fn md_to_latex_emphasis() {
        let out = markdown_to_latex("*italic*", &crate::render::LinkMode::single_file());
        assert!(out.contains("\\emph{italic}"));
    }

    #[test]
    fn md_to_latex_strong() {
        let out = markdown_to_latex("**bold**", &crate::render::LinkMode::single_file());
        assert!(out.contains("\\textbf{bold}"));
    }

    #[test]
    fn md_to_latex_strikethrough() {
        // Audit S3: double-tilde strikethrough must survive as \sout{…}.
        let out = markdown_to_latex(
            "this is ~~struck~~ text",
            &crate::render::LinkMode::single_file(),
        );
        assert!(out.contains("\\sout{struck}"), "{out:?}");
    }

    #[test]
    fn md_to_latex_single_tilde_stays_literal() {
        // Audit S1 (LaTeX target): `~single~` is prose, and the tildes
        // must not vanish — they come out escaped.
        let out = markdown_to_latex("a ~single~ tilde", &crate::render::LinkMode::single_file());
        assert!(!out.contains("\\sout"), "{out:?}");
        assert!(
            out.contains("\\textasciitilde{}single\\textasciitilde{}"),
            "{out:?}"
        );
    }

    #[test]
    fn md_to_latex_inline_code() {
        let out = markdown_to_latex("`x = 1`", &crate::render::LinkMode::single_file());
        assert!(out.contains("\\texttt{"));
    }

    #[test]
    fn md_to_latex_code_block() {
        let out = markdown_to_latex(
            "```\ncode here\n```",
            &crate::render::LinkMode::single_file(),
        );
        assert!(out.contains("\\begin{verbatim}"));
        assert!(out.contains("\\end{verbatim}"));
    }

    #[test]
    fn md_to_latex_unordered_list() {
        let out = markdown_to_latex(
            "- item one\n- item two",
            &crate::render::LinkMode::single_file(),
        );
        assert!(out.contains("\\begin{itemize}"));
        assert!(out.contains("\\item"));
        assert!(out.contains("\\end{itemize}"));
    }

    #[test]
    fn md_to_latex_ordered_list() {
        let out = markdown_to_latex(
            "1. first\n2. second",
            &crate::render::LinkMode::single_file(),
        );
        assert!(out.contains("\\begin{enumerate}"));
        assert!(out.contains("\\item"));
        assert!(out.contains("\\end{enumerate}"));
    }

    #[test]
    fn md_to_latex_blockquote() {
        let out = markdown_to_latex("> quoted text", &crate::render::LinkMode::single_file());
        assert!(out.contains("\\begin{quote}"));
        assert!(out.contains("\\end{quote}"));
    }

    #[test]
    fn md_to_latex_link() {
        let out = markdown_to_latex(
            "[click](https://example.com)",
            &crate::render::LinkMode::single_file(),
        );
        assert!(out.contains("\\href{https://example.com}"));
        assert!(out.contains("{click}"));
    }

    #[test]
    fn md_to_latex_escapes_href_destinations() {
        // Unescaped `#`/`%` in an \href destination is a fatal TeX error
        // whenever the link sits inside \section{}, \textbf{} or \emph{} —
        // i.e. a same-page anchor in a heading or bold text, ordinary
        // markdown. Compile-verified against tectonic in all three
        // contexts.
        let single = crate::render::LinkMode::single_file();
        let out = markdown_to_latex("## Jump to [Setup](#setup)", &single);
        assert!(
            out.contains("\\href{\\#setup}"),
            "unescaped # in heading: {out}"
        );
        let out = markdown_to_latex("**[deal](https://ex.com/50%off)**", &single);
        assert!(
            out.contains("\\href{https://ex.com/50\\%off}"),
            "unescaped %: {out}"
        );
        let out = markdown_to_latex("[frag](https://ex.com/p#sec)", &single);
        assert!(out.contains("\\href{https://ex.com/p\\#sec}"), "{out}");
        // Underscores are fine everywhere — do not over-escape.
        let out = markdown_to_latex("[n](my_notes.md)", &single);
        assert!(
            out.contains("\\href{my_notes.pdf}"),
            "over-escaped _: {out}"
        );
    }

    #[test]
    fn md_to_latex_resolves_notebook_links_to_pdf_siblings() {
        // `\href{a.md}` pointed a PDF reader at the raw source file, which
        // ships nowhere. Cross-notebook links target the sibling `.pdf`
        // artifacts a directory PDF build emits, with fragments dropped —
        // the generated PDFs carry no named destinations for anchors.
        let single = crate::render::LinkMode::single_file();
        let out = markdown_to_latex("[next](02-filter.md)", &single);
        assert!(out.contains("\\href{02-filter.pdf}"), "{out}");
        let out = markdown_to_latex("[setup](02-filter.md#setup)", &single);
        assert!(
            out.contains("\\href{02-filter.pdf}") && !out.contains("#setup}"),
            "fragment must be dropped for PDF targets: {out}"
        );
        // Titled and reference-style links resolve too (parser-level).
        let out = markdown_to_latex("[x](02-filter.md \"Filter\")", &single);
        assert!(out.contains("\\href{02-filter.pdf}"), "{out}");
        let out = markdown_to_latex("See [x][r].\n\n[r]: 02-filter.md\n", &single);
        assert!(out.contains("\\href{02-filter.pdf}"), "{out}");
        // Wikilinks route through the same seam.
        let out = markdown_to_latex("see [[02-filter]]", &single);
        assert!(out.contains("\\href{02-filter.pdf}"), "{out}");
    }

    #[test]
    fn md_to_latex_leaves_external_and_unemitted_targets_alone() {
        // External URLs ending .md must not be corrupted, and in a
        // directory build only emitted siblings rewrite — partials and
        // dangling targets are left exactly as written. index.md is NOT a
        // valid PDF target (no index.pdf is generated).
        let single = crate::render::LinkMode::single_file();
        let out = markdown_to_latex("[readme](https://example.com/README.md)", &single);
        assert!(
            out.contains("\\href{https://example.com/README.md}"),
            "{out}"
        );
        // Inline code with a link is verbatim, not a link.
        let out = markdown_to_latex("code `[x](a.md)` here", &single);
        assert!(!out.contains("\\href"), "code span became a link: {out}");

        let known: std::collections::HashSet<String> =
            ["01-intro.md".to_string()].into_iter().collect();
        let dir_mode = crate::render::LinkMode::Static {
            known: Some(known),
            current_rel_dir: String::new(),
        };
        let out = markdown_to_latex("[p](_setup.md) [g](nope.md) [h](index.md)", &dir_mode);
        assert!(
            out.contains("\\href{_setup.md}"),
            "partial rewritten: {out}"
        );
        assert!(out.contains("\\href{nope.md}"), "dangling rewritten: {out}");
        assert!(
            out.contains("\\href{index.md}"),
            "index.md rewritten but no index.pdf exists: {out}"
        );
        let out = markdown_to_latex("[i](01-intro.md)", &dir_mode);
        assert!(out.contains("\\href{01-intro.pdf}"), "{out}");
    }

    #[test]
    fn md_to_latex_table() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |";
        let out = markdown_to_latex(md, &crate::render::LinkMode::single_file());
        assert!(out.contains("\\begin{tabular}"));
        assert!(out.contains("\\toprule"));
        assert!(out.contains("\\midrule"));
        assert!(out.contains("\\bottomrule"));
        assert!(out.contains("\\end{tabular}"));
        assert!(out.contains(" & "));
        assert!(out.contains("\\rowcolor{rlthead}"));
        assert!(out.contains("\\textcolor{rlh1}{\\textbf{"));
    }

    #[test]
    fn md_to_latex_inline_math() {
        let out = markdown_to_latex(
            "The value $x^2$ is large.",
            &crate::render::LinkMode::single_file(),
        );
        assert!(out.contains("$x^2$"));
    }

    #[test]
    fn md_to_latex_display_math() {
        let out = markdown_to_latex("$$E = mc^2$$", &crate::render::LinkMode::single_file());
        assert!(out.contains("\\[\nE = mc^2\n\\]"));
    }

    #[test]
    fn md_to_latex_special_chars_escaped() {
        let out = markdown_to_latex(
            "Use 100% of the CPU & GPU",
            &crate::render::LinkMode::single_file(),
        );
        assert!(out.contains("100\\%"));
        assert!(out.contains("\\&"));
    }

    #[test]
    fn md_to_latex_paragraph() {
        let out = markdown_to_latex(
            "Para one.\n\nPara two.",
            &crate::render::LinkMode::single_file(),
        );
        // Paragraphs should be separated
        assert!(out.contains("Para one."));
        assert!(out.contains("Para two."));
    }

    #[test]
    fn md_to_latex_empty() {
        assert_eq!(
            markdown_to_latex("", &crate::render::LinkMode::single_file()),
            ""
        );
    }

    // ── regression: Bug B — escaped `\$` in markdown prose stays literal in
    // LaTeX. Before the fix, `\$` reached us as a bare `$` in a Text event
    // and `escape_latex_preserving_math` toggled math mode, breaking
    // template_interpolation.md at the `Use \${...}` paragraph.
    #[test]
    fn md_to_latex_escaped_dollar_stays_literal() {
        let out = markdown_to_latex(
            r"literal: \${not_evaluated}.",
            &crate::render::LinkMode::single_file(),
        );
        // Every `$` in the output must be preceded by a backslash —
        // otherwise it opens math mode and pdflatex fails with
        // "Missing $ inserted".
        for (i, ch) in out.char_indices() {
            if ch == '$' {
                let prev = out[..i].chars().next_back();
                assert_eq!(
                    prev,
                    Some('\\'),
                    "unescaped `$` at offset {i} in output: {out:?}"
                );
            }
        }
        // Sanity: the escaped form should be present at all.
        assert!(out.contains("\\$"), "no literal \\$ in output: {out:?}");
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
            &crate::render::LinkMode::single_file(),
        );
        assert!(tex.contains("\\documentclass"));
        assert!(tex.contains("\\usepackage{graphicx}"));
        assert!(tex.contains("\\usepackage{graphicx}"));
        assert!(!tex.contains("\\usepackage{svg}"));
        assert!(tex.contains("\\usepackage{amsmath,amssymb}"));
        assert!(tex.contains("\\usepackage{booktabs}"));
        assert!(tex.contains("\\usepackage[normalem]{ulem}"));
        assert!(tex.contains("\\begin{document}"));
        assert!(tex.contains("\\end{document}"));
        assert!(tex.contains("\\maketitle"));
    }

    // Security: TeX shell-escape / svg.sty are gone. SVGs are converted
    // to PDF by fixed-argv Inkscape before pdflatex runs; the preamble
    // must use graphicx only.
    #[test]
    fn render_latex_preamble_has_no_svg_shell_escape() {
        let tex = render_latex(
            "x",
            &[],
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(!tex.contains("\\usepackage{svg}"));
        assert!(!tex.contains("\\svgsetup"));
        assert!(!tex.contains("shell-escape"));
        assert!(tex.contains("\\usepackage{graphicx}"));
    }

    // Regression: Bug D — preamble must declare common math/Greek Unicode
    // characters so pdflatex with [utf8]{inputenc} doesn't reject body
    // text or verbatim contents containing ≈, ∇, π, Ω, ⇒, ×, etc.
    // (Previously broke 7 of the example notebooks.)
    #[test]
    fn render_latex_preamble_declares_unicode_math_chars() {
        let tex = render_latex(
            "x",
            &[],
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(tex.contains("\\usepackage{newunicodechar}"));
        // Spot-check the characters that broke real notebooks.
        // Includes the 7 chars from the rustlab_llm bug report
        // (η ┬ ₁ ↘ ⁵ ✓ plus combining-macron U+0304), the original
        // anchor set, and the wider 0–9 sub/super range — pdflatex
        // fatals on any undeclared codepoint, so partial coverage is
        // brittle and easy to break.
        for ch in [
            '≈', '∇', 'π', 'Ω', '⇒', '×', 'η', '┬', '₁', '↘', '⁵', '✓', '⁰', '¹', '⁴', '⁹', '₀',
            '₉', '↗', '↙', '✗', '┼',
        ] {
            assert!(
                tex.contains(&format!("\\newunicodechar{{{ch}}}")),
                "preamble missing declaration for U+{:04X}",
                ch as u32,
            );
        }
    }

    #[test]
    fn render_latex_title_escaped() {
        let tex = render_latex(
            "A & B",
            &[],
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
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
            source_open: None,
        }];
        let tex = render_latex(
            "Test",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(tex.contains("\\textcolor{rlnum}{42}"), "{tex}");
        assert!(tex.contains("\\textcolor{rlop}{=}"), "{tex}");
        assert!(tex.contains("\\definecolor{rlkw}{HTML}{"), "{tex}");
        // Source cells are colored text, not verbatim. Markdown fences
        // elsewhere still use verbatim.
        assert!(
            !tex.contains("\\begin{verbatim}"),
            "source cell must not be verbatim:\n{tex}"
        );
        assert!(!tex.contains("minted"));
        assert!(!tex.contains("shell-escape"));
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
            source_open: None,
        }];
        let tex = render_latex(
            "Test",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        // Source should not appear, and neither should the rustlab label.
        assert!(!tex.contains("secret = 42"));
        assert!(!tex.contains("rustlab"));
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
            source_open: None,
        }];
        let tex = render_latex(
            "Test",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(tex.contains("\\begin{rlsource}"));
        assert!(tex.contains("\\begin{rloutput}"));
        assert!(tex.contains("\\begin{rlverb}"));
        assert!(tex.contains("\\end{rlverb}"));
        assert!(tex.contains("ans = 1"));
        let cell = tex
            .split("\\begin{rloutput}")
            .nth(1)
            .unwrap()
            .split("\\end{rloutput}")
            .next()
            .unwrap();
        assert!(cell.contains("ans = 1"), "{cell}");
        assert!(
            tex.contains("rustlab"),
            "source panel carries the rustlab label"
        );
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
            source_open: None,
        }];
        let tex = render_latex(
            "Test",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        // Source is colored, not verbatim; empty output adds no quote.
        assert_eq!(tex.matches("\\begin{verbatim}").count(), 0);
        assert!(!tex.contains("\\begin{quote}"));
    }

    #[test]
    fn render_latex_source_escapes_specials_and_colors_comments() {
        let blocks = vec![Rendered::Code {
            source: "x_1 = 1 # comment % also\n</span><script>\n<img onerror=\"x\">".to_string(),
            text_output: String::new(),
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
            source_open: None,
        }];
        let tex = render_latex(
            "Test",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(tex.contains("{\\itshape\\textcolor{rlcom}{"), "{tex}");
        assert!(tex.contains("\\#"), "{tex}");
        assert!(tex.contains("\\_"), "{tex}");
        assert!(tex.contains("\\%"), "{tex}");
        assert!(tex.contains("\\obeylines"), "{tex}");
        assert!(!tex.contains("flushleft"), "{tex}");
        assert!(!tex.contains("\\begin{verbatim}"), "{tex}");
        assert!(!tex.contains("minted"), "{tex}");
        assert!(!tex.contains("shell-escape"), "{tex}");
        // The markup is escaped text, not a TeX command.
        assert!(tex.contains("span"), "{tex}");
        assert!(!tex.contains("</span><script>"), "{tex}");
    }

    #[test]
    fn render_latex_colored_source_compiles_without_shell_escape() {
        if !crate::pdf_compile::which_exists("pdflatex") {
            return;
        }
        let blocks = vec![Rendered::Code {
            source: "x_1 = 1 # comment % also\nhold on\nplot(x_1)\n".to_string(),
            text_output: String::new(),
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
            source_open: None,
        }];
        let tex = render_latex(
            "Color",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        let args = crate::pdf_compile::pdf_engine_args("pdflatex").unwrap();
        assert!(
            !crate::pdf_compile::args_enable_shell_escape(&args),
            "{args:?}"
        );
        let dir = tempfile::tempdir().unwrap();
        let tex_path = dir.path().join("nb.tex");
        std::fs::write(&tex_path, &tex).unwrap();
        let status = std::process::Command::new("pdflatex")
            .args(args)
            .arg(format!("-output-directory={}", dir.path().display()))
            .arg(&tex_path)
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("pdflatex");
        if !status.success() {
            let log = std::fs::read_to_string(dir.path().join("nb.log")).unwrap_or_default();
            panic!("pdflatex failed:\n{log}\n--- tex ---\n{tex}");
        }
        assert!(dir.path().join("nb.pdf").exists());
    }

    #[test]
    fn render_latex_indents_source_and_output_not_figures() {
        use rustlab_plot::{FigureState, LineStyle, PlotKind, Series, SeriesColor};
        let mut fig = FigureState::new();
        fig.subplots[0].series.push(Series {
            label: String::new(),
            x_data: vec![0.0, 1.0],
            y_data: vec![0.0, 1.0],
            color: SeriesColor::Blue,
            style: LineStyle::Solid,
            kind: PlotKind::Line,
        });
        let dir = std::env::temp_dir().join(format!("rl_cell_plot_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let blocks = vec![Rendered::Code {
            source: "x = 1".to_string(),
            text_output: "ans = 1".to_string(),
            error: Some("nope".to_string()),
            figures: vec![fig],
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
            // Collapsed is an HTML-only initial state. PDF still shows the source.
            source_open: Some(false),
        }];
        let tex = render_latex(
            "Test",
            &blocks,
            &dir,
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        let source_box = tex
            .split("\\begin{rlsource}")
            .nth(1)
            .expect("rlsource")
            .split("\\end{rlsource}")
            .next()
            .unwrap();
        let output_box = tex
            .split("\\begin{rloutput}")
            .nth(1)
            .expect("rloutput")
            .split("\\end{rloutput}")
            .next()
            .unwrap();
        let error_box = tex
            .split("\\begin{rlerror}")
            .nth(1)
            .expect("rlerror")
            .split("\\end{rlerror}")
            .next()
            .unwrap();
        assert!(source_box.contains("\\textcolor{"), "{source_box}");
        assert!(output_box.contains("ans = 1"), "{output_box}");
        assert!(error_box.contains("nope"), "{error_box}");
        assert!(!source_box.contains("\\includegraphics"), "{source_box}");
        let after = tex.split("\\end{rlerror}").nth(1).unwrap_or("");
        assert!(
            after.contains("\\includegraphics"),
            "figure should follow the panels:\n{tex}"
        );
        assert!(tex.contains("\\definecolor{rlrule}{HTML}{8839ef}"));
        assert!(tex.contains("breakable"));
        assert!(!tex.contains("\\begin{rlcell}"));
        assert!(!tex.contains("minted"));
        assert!(!tex.contains("shell-escape"));
        assert!(
            !tex.contains("<details") && !tex.contains("rl-src"),
            "HTML source disclosure must not leak into LaTeX"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_latex_long_cell_breaks_across_pages() {
        if !crate::pdf_compile::which_exists("pdflatex") {
            return;
        }
        let source = (0..80)
            .map(|i| format!("x = {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let text_output = (0..40)
            .map(|i| format!("ans = {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let blocks = vec![Rendered::Code {
            source,
            text_output,
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
            source_open: None,
        }];
        let tex = render_latex(
            "Long",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(tex.contains("\\begin{rlsource}"));
        assert!(tex.contains("\\begin{rloutput}"));
        assert!(tex.contains("breakable"));
        assert!(
            !tex.contains("\\begin{minipage}"),
            "a page-breaking cell must not be a minipage"
        );
        let args = crate::pdf_compile::pdf_engine_args("pdflatex").unwrap();
        assert!(!crate::pdf_compile::args_enable_shell_escape(&args));
        let dir = tempfile::tempdir().unwrap();
        let tex_path = dir.path().join("nb.tex");
        std::fs::write(&tex_path, &tex).unwrap();
        let output = std::process::Command::new("pdflatex")
            .args(args)
            .arg(format!("-output-directory={}", dir.path().display()))
            .arg(&tex_path)
            .current_dir(dir.path())
            .output()
            .expect("pdflatex");
        let log = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() {
            let file_log = std::fs::read_to_string(dir.path().join("nb.log")).unwrap_or_default();
            panic!("pdflatex failed:\n{file_log}\n{log}");
        }
        let pages = log
            .split("Output written on")
            .nth(1)
            .and_then(|s| s.split('(').nth(1))
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.trim_end_matches(',').parse::<u32>().ok())
            .unwrap_or(0);
        assert!(
            pages >= 2,
            "long cell should span a page, got {pages}: {log}"
        );
        assert!(dir.path().join("nb.pdf").exists());
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
            source_open: None,
        }];
        let tex = render_latex(
            "Test",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(tex.contains("\\begin{rlerror}"));
        assert!(tex.contains("\\definecolor{rlerrfg}{HTML}{d20f39}"));
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
            &crate::render::LinkMode::single_file(),
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
    fn mermaid_emits_figure_with_includegraphics() {
        let dir = mermaid_plot_dir("fig");
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\n  A --> B\n".to_string(),
            hidden: false,
            details: None,
            caption: None,
        }];
        let tex = render_latex(
            "T",
            &blocks,
            &dir,
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(tex.contains("\\begin{figure}[H]"));
        assert!(tex.contains("\\includegraphics[width=0.8\\linewidth]{plots/test/diagram-1}"));
        assert!(!tex.contains("\\includesvg"));
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
        let tex = render_latex(
            "T",
            &blocks,
            &dir,
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
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
        let tex = render_latex(
            "T",
            &blocks,
            &dir,
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
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
        let tex = render_latex(
            "T",
            &blocks,
            &dir,
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
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
        let tex = render_latex(
            "T",
            &blocks,
            &dir,
            "plots/test",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(tex.contains("\\begin{verbatim}"));
        assert!(tex.contains("flowchart LR"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn dark() -> &'static ThemeColors {
        Theme::Dark.colors()
    }

    fn compile_tex(tex: &str) -> u32 {
        let args = crate::pdf_compile::pdf_engine_args("pdflatex").unwrap();
        assert!(
            !crate::pdf_compile::args_enable_shell_escape(&args),
            "{args:?}"
        );
        let dir = tempfile::tempdir().unwrap();
        let tex_path = dir.path().join("nb.tex");
        std::fs::write(&tex_path, tex).unwrap();
        let output = std::process::Command::new("pdflatex")
            .args(args)
            .arg(format!("-output-directory={}", dir.path().display()))
            .arg(&tex_path)
            .current_dir(dir.path())
            .output()
            .expect("pdflatex");
        if !output.status.success() {
            let file_log = std::fs::read_to_string(dir.path().join("nb.log")).unwrap_or_default();
            panic!(
                "pdflatex failed:\n{file_log}\n{}\n--- tex ---\n{tex}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
        assert!(dir.path().join("nb.pdf").exists());
        let log = String::from_utf8_lossy(&output.stdout);
        log.split("Output written on")
            .nth(1)
            .and_then(|s| s.split('(').nth(1))
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.trim_end_matches(',').parse::<u32>().ok())
            .unwrap_or(0)
    }

    #[test]
    fn render_latex_always_light_on_white_paper() {
        let tex = render_latex(
            "Dark request",
            &[],
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            dark(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(tex.contains("\\definecolor{rltext}{HTML}{4c4f69}"), "{tex}");
        assert!(tex.contains("\\definecolor{rlkw}{HTML}{8839ef}"));
        assert!(tex.contains("\\definecolor{rlcodebg}{HTML}{dce0e8}"));
        assert!(tex.contains("\\definecolor{rloutbg}{HTML}{e6e9ef}"));
        assert!(tex.contains("\\definecolor{rldim}{HTML}{6c6f85}"));
        assert!(tex.contains("\\definecolor{rlerrbg}{HTML}{fce4e4}"));
        assert!(tex.contains("\\definecolor{rlh2}{HTML}{1e66f5}"));
        assert!(tex.contains("\\definecolor{rlh3}{HTML}{179299}"));
        assert!(
            !tex.contains("cba6f7"),
            "dark Mocha keyword must not leak: {tex}"
        );
        assert!(!tex.contains("pagecolor"));
        assert!(!tex.contains("\\today"));
        assert!(tex.contains("\\date{}"));
        assert!(tex.contains("\\setcounter{secnumdepth}{-1}"));
        assert!(tex.contains("\\usepackage{lmodern}"));
        assert!(tex.contains("\\usepackage[table]{xcolor}"));
        assert!(tex.contains("tcolorbox"));
        assert!(tex.contains("fancyvrb"));
        assert!(tex.contains("sectsty"));
        assert!(tex.contains("\\usepackage{float}"));
        assert!(!tex.contains("shell-escape"));
        assert!(!tex.contains("minted"));
    }

    #[test]
    fn render_latex_callout_details_and_exercise() {
        use crate::parse::CalloutKind;
        let blocks = vec![
            Rendered::Callout {
                kind: CalloutKind::Warning,
                title: Some("a_b & 100%".to_string()),
                content: "See `x_1`.".to_string(),
            },
            Rendered::Code {
                source: "hidden = 1".to_string(),
                text_output: "value_1 = 50%\n\\end{rlverb}\n".to_string(),
                error: None,
                figures: Vec::new(),
                animations: Vec::new(),
                hidden: true,
                details: Some("costs $5 & more".to_string()),
                grid_cols: None,
                source_open: Some(false),
            },
            Rendered::ExerciseStart { number: 2 },
            Rendered::Markdown("What is `1 + 2`?".to_string()),
            Rendered::SolutionStart,
            Rendered::Markdown("Three.".to_string()),
        ];
        let tex = render_latex(
            "T",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            dark(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(tex.contains("\\begin{rlcallout}{rlerrfg}{a\\_b \\& 100\\%}"));
        assert!(tex.contains("\\colorbox{rlcodepill}"));
        assert!(
            tex.contains("\\end {rlverb}"),
            "verb end in output must be neutralized"
        );
        assert!(tex.contains("value_1 = 50%"));
        assert!(!tex.contains("hidden = 1"));
        assert!(tex.contains("\\textcolor{rllink}{\\textbf{costs \\$5 \\& more}}"));
        assert!(tex.contains("\\begin{rlexercise}"));
        assert!(tex.contains("Exercise 2."));
        assert!(tex.contains("\\textbf{Solution}"));
        assert!(tex.contains("\\end{rlexercise}"));
        // Collapsed is HTML-only. This cell is hidden via `hide`, not `code:`.
        assert!(tex.contains("source_open") == false);
    }

    #[test]
    fn render_latex_grid_uses_capped_minipages() {
        use rustlab_plot::{FigureState, LineStyle, PlotKind, Series, SeriesColor};
        let mut fig = FigureState::new();
        fig.subplots[0].series.push(Series {
            label: String::new(),
            x_data: vec![0.0, 1.0],
            y_data: vec![0.0, 1.0],
            color: SeriesColor::Blue,
            style: LineStyle::Solid,
            kind: PlotKind::Line,
        });
        let dir = std::env::temp_dir().join(format!("rl_grid_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let blocks = vec![Rendered::Code {
            source: "plot(x)".to_string(),
            text_output: String::new(),
            error: None,
            figures: vec![fig.clone(), fig.clone(), fig.clone()],
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: Some(2),
            source_open: None,
        }];
        let tex = render_latex(
            "Grid",
            &blocks,
            &dir,
            "plots/grid",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        assert_eq!(tex.matches("\\begin{minipage}").count(), 3);
        assert!(tex.contains("\\begin{minipage}[t]{0.480\\textwidth}"));
        assert!(tex.contains("\\hfill"));
        assert!(
            tex.contains("\\par\\vspace{0.45em}"),
            "the third plot wraps to a left-aligned second row:\n{tex}"
        );
        let capped = vec![Rendered::Code {
            source: "plot(x)".to_string(),
            text_output: String::new(),
            error: None,
            figures: vec![fig.clone()],
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: Some(6),
            source_open: None,
        }];
        let capped_tex = render_latex(
            "Cap",
            &capped,
            &dir,
            "plots/grid",
            light(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(
            capped_tex.contains("\\begin{minipage}[t]{0.240\\textwidth}"),
            "six columns shrink to four: {capped_tex}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_latex_long_output_box_breaks_across_pages() {
        if !crate::pdf_compile::which_exists("pdflatex") {
            return;
        }
        let text_output = (0..90)
            .map(|i| format!("ans_{i} = {i}%"))
            .collect::<Vec<_>>()
            .join("\n");
        let blocks = vec![Rendered::Code {
            source: "x = 1".to_string(),
            text_output,
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
            source_open: None,
        }];
        let tex = render_latex(
            "Long output",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            dark(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(tex.contains("\\begin{rloutput}"));
        assert!(tex.contains("breakable"));
        let pages = compile_tex(&tex);
        assert!(
            pages >= 2,
            "long printed output should span a page, got {pages}"
        );
    }

    #[test]
    fn render_latex_parity_sample_compiles_without_shell_escape() {
        if !crate::pdf_compile::which_exists("pdflatex") {
            return;
        }
        use crate::parse::CalloutKind;
        let blocks = vec![
            Rendered::Markdown(
                "# Signal notebook\n\nInline `fft(x)` and a [link](https://example.com).\n\n\
                 | Window | Taps |\n| --- | ---: |\n| Hann | 63 |\n\n\
                 > quoted prose\n"
                    .to_string(),
            ),
            Rendered::Callout {
                kind: CalloutKind::Note,
                title: None,
                content: "DC gain is $1$.".to_string(),
            },
            Rendered::Callout {
                kind: CalloutKind::Tip,
                title: Some("Hint".to_string()),
                content: "Call `seed(1)`.".to_string(),
            },
            Rendered::Callout {
                kind: CalloutKind::Warning,
                title: None,
                content: "Rollett $K < 1$.".to_string(),
            },
            Rendered::Code {
                source: "n = 64 # bins\nh = fir_lowpass(31, 1000, 8000, \"hann\")\n".to_string(),
                text_output: "ans = -0.02\n".to_string(),
                error: Some("intentional failure".to_string()),
                figures: Vec::new(),
                animations: Vec::new(),
                hidden: false,
                details: Some("Filter coefficients".to_string()),
                grid_cols: None,
                source_open: Some(false),
            },
            Rendered::ExerciseStart { number: 1 },
            Rendered::Markdown("What is `1 + 2`?".to_string()),
            Rendered::SolutionStart,
            Rendered::Code {
                source: "print(1 + 2)".to_string(),
                text_output: "3".to_string(),
                error: None,
                figures: Vec::new(),
                animations: Vec::new(),
                hidden: false,
                details: None,
                grid_cols: None,
                source_open: None,
            },
        ];
        let tex = render_latex(
            "PDF parity sample",
            &blocks,
            std::path::Path::new("/tmp/test_plots"),
            "plots/test",
            dark(),
            &crate::render::LinkMode::single_file(),
        );
        assert!(tex.contains("\\begin{rlsource}"));
        assert!(tex.contains("\\begin{rloutput}"));
        assert!(tex.contains("\\begin{rlerror}"));
        assert!(tex.contains("\\begin{rlcallout}{rlh2}{Note}"));
        assert!(tex.contains("\\begin{rlcallout}{rlh3}{Hint}"));
        assert!(tex.contains("\\begin{rlcallout}{rlerrfg}{Warning}"));
        assert!(tex.contains("rustlab"));
        assert_eq!(
            tex.matches("\\begin{rlsource}").count(),
            2,
            "code: collapsed is ignored; both cells show their source"
        );
        let pages = compile_tex(&tex);
        assert!(pages >= 1, "parity sample produced no pages");
    }
}
