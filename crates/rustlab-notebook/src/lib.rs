pub mod execute;
pub mod parse;
pub mod render;
pub mod render_latex;
pub mod render_markdown;

use rustlab_plot::theme::ThemeColors;
use std::path::PathBuf;

/// Cross-notebook navigation context passed to `render::render_html` when a
/// notebook is rendered as part of a multi-notebook directory build. All
/// fields are relative hrefs (e.g. `index.html`, `analysis.html`).
#[derive(Debug, Clone, Default)]
pub struct NotebookNav {
    pub index_href: Option<String>,
    pub prev: Option<(String, String)>,
    pub next: Option<(String, String)>,
}

/// Render a single notebook file to the chosen format.
pub fn cmd_render(input: PathBuf, output: Option<PathBuf>, format: Format, theme: &ThemeColors) {
    let source = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", input.display());
            std::process::exit(1);
        }
    };

    if let Some(dir) = input.parent() {
        if dir.as_os_str().len() > 0 {
            let _ = std::env::set_current_dir(dir);
        }
    }

    let title = extract_title(&source, &input);
    let blocks = parse::parse_notebook(&source);
    let rendered = execute::execute_notebook(&blocks);

    let ext = format.extension();
    let out_path = output.unwrap_or_else(|| {
        let stem = input.file_stem().unwrap_or_default();
        PathBuf::from(format!("{}.{ext}", stem.to_string_lossy()))
    });

    render_output(&out_path, &format, &title, &rendered, theme, None);
    print_summary(&input, &out_path, &rendered);
}

/// Render all .md files in a directory.
///
/// `index_title` overrides the auto-derived index page title. When a file named
/// `index.md` exists in `dir`, it is treated specially: its body is rendered as
/// the top of the generated `index.html` (above the notebook listing), and its
/// title supplies the default index title when `index_title` is `None`.
pub fn cmd_render_dir(
    dir: PathBuf,
    output: Option<PathBuf>,
    format: Format,
    theme: &ThemeColors,
    index_title: Option<String>,
) {
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    let out_dir = output
        .map(|o| std::path::absolute(&o).unwrap_or(o))
        .unwrap_or_else(|| dir.clone());

    let mut md_files: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map_or(false, |ext| ext == "md"))
            // README.md is project metadata for the directory itself, not a
            // notebook. Skip it so it doesn't appear in the rendered index
            // alongside real notebooks. (`index.md` is handled separately.)
            .filter(|p| p.file_name().map_or(true, |n| n != "README.md"))
            .collect(),
        Err(e) => {
            eprintln!("error: cannot read directory {}: {e}", dir.display());
            std::process::exit(1);
        }
    };
    md_files.sort();

    // Split out `index.md` so it is not listed as a notebook entry.
    let index_md_path = md_files
        .iter()
        .position(|p| p.file_name().map_or(false, |n| n == "index.md"))
        .map(|i| md_files.remove(i));

    if md_files.is_empty() && index_md_path.is_none() {
        eprintln!("warning: no .md files found in {}", dir.display());
        return;
    }

    let ext = format.extension();

    // Pass 1: read sources and collect metadata so we can sort before rendering.
    // This lets us give each notebook its prev/next neighbour in the nav.
    struct Pending {
        md_path: PathBuf,
        out_file: PathBuf,
        title: String,
        filename: String,
        order: Option<i64>,
        source: String,
    }
    let mut pending: Vec<Pending> = Vec::new();
    for md_path in &md_files {
        let source = match std::fs::read_to_string(md_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("warning: cannot read {}: {e}", md_path.display());
                continue;
            }
        };
        let (fm, _) = parse::extract_frontmatter(&source);
        let title = extract_title(&source, md_path);
        let stem = md_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let filename = format!("{stem}.{ext}");
        let out_file = out_dir.join(&filename);
        pending.push(Pending {
            md_path: md_path.clone(),
            out_file,
            title,
            filename,
            order: fm.order,
            source,
        });
    }

    // Sort the same way the index used to: order asc (None last), ties by filename.
    pending.sort_by(|a, b| match (a.order, b.order) {
        (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.filename.cmp(&b.filename)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.filename.cmp(&b.filename),
    });

    let emit_nav = matches!(format, Format::Html);
    let n = pending.len();
    for i in 0..n {
        let nav = if emit_nav {
            let prev = (i > 0).then(|| {
                (
                    pending[i - 1].title.clone(),
                    pending[i - 1].filename.clone(),
                )
            });
            let next = (i + 1 < n).then(|| {
                (
                    pending[i + 1].title.clone(),
                    pending[i + 1].filename.clone(),
                )
            });
            Some(NotebookNav {
                index_href: Some("index.html".to_string()),
                prev,
                next,
            })
        } else {
            None
        };

        let p = &pending[i];
        let _ = std::env::set_current_dir(&dir);
        let blocks = parse::parse_notebook(&p.source);
        let rendered = execute::execute_notebook(&blocks);
        render_output(&p.out_file, &format, &p.title, &rendered, theme, nav.as_ref());
        print_summary(&p.md_path, &p.out_file, &rendered);
    }

    if matches!(format, Format::Html) {
        // Resolve the index title: CLI flag > index.md title > dir name.
        let (index_body_html, index_md_title) = match index_md_path.as_ref() {
            Some(p) => read_and_render_index_md(p, &dir, theme),
            None => (String::new(), None),
        };
        let resolved_title = index_title.or(index_md_title).unwrap_or_else(|| {
            dir.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        });

        let entries_simple: Vec<(String, String)> = pending
            .iter()
            .map(|p| (p.title.clone(), p.filename.clone()))
            .collect();
        let index_html =
            generate_index_html(&resolved_title, &entries_simple, theme, &index_body_html);
        let index_path = out_dir.join("index.html");
        write_output(&index_path, index_html.as_bytes());
        println!(
            "Generated {} ({} notebooks)",
            index_path.display(),
            entries_simple.len()
        );
    }
}

/// Read `index.md`, render its markdown body to HTML, and return the body
/// plus the title used for the page. Code fences inside `index.md` are
/// rendered as plain markdown (not executed) to keep the landing page
/// lightweight — put executable content in regular notebooks and link to
/// them from `index.md`.
fn read_and_render_index_md(
    path: &PathBuf,
    _dir: &PathBuf,
    _theme: &ThemeColors,
) -> (String, Option<String>) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("warning: cannot read {}: {e}", path.display());
            return (String::new(), None);
        }
    };
    let title = extract_title(&source, path);
    let (_, body_md) = parse::extract_frontmatter(&source);
    // Strip the first H1 from the body: it becomes the page title already.
    let body_without_h1 = strip_leading_h1(body_md);
    let mut opts = pulldown_cmark::Options::empty();
    opts.insert(pulldown_cmark::Options::ENABLE_TABLES);
    opts.insert(pulldown_cmark::Options::ENABLE_STRIKETHROUGH);
    let parser = pulldown_cmark::Parser::new_ext(body_without_h1, opts);
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);
    (html, Some(title))
}

fn strip_leading_h1(src: &str) -> &str {
    let mut rest = src;
    // Skip any blank lines.
    loop {
        let trimmed = rest.trim_start_matches(|c: char| c == '\n' || c == '\r');
        if trimmed.len() == rest.len() {
            break;
        } else {
            rest = trimmed;
        }
    }
    let first_line = rest.lines().next().unwrap_or("");
    if first_line.trim_start().starts_with("# ") {
        let consumed = first_line.len().min(rest.len());
        let after = &rest[consumed..];
        after.strip_prefix('\n').unwrap_or(after)
    } else {
        rest
    }
}

/// Output format.
#[derive(Clone)]
pub enum Format {
    Html,
    Latex,
    Pdf,
    /// GitHub-friendly markdown with inline SVG plots.
    ///
    /// `obsidian: true` additionally appends an `<iframe>` pointing at the
    /// sibling `<stem>.html` file so Obsidian (and any tool that respects
    /// raw HTML in markdown) can show the interactive Plotly view inline.
    /// GitHub strips iframes during sanitization, so the same `.md` is
    /// safe to commit either way.
    Markdown { obsidian: bool },
}

impl Format {
    pub fn extension(&self) -> &'static str {
        match self {
            Format::Html => "html",
            Format::Latex => "tex",
            Format::Pdf => "pdf",
            Format::Markdown { .. } => "md",
        }
    }
}

fn render_output(
    out_path: &PathBuf,
    format: &Format,
    title: &str,
    rendered: &[execute::Rendered],
    theme: &ThemeColors,
    nav: Option<&NotebookNav>,
) {
    match format {
        Format::Html => {
            let (plot_dir, href_prefix) = plot_layout_for(out_path);
            let html =
                render::render_html(title, rendered, &plot_dir, &href_prefix, theme, nav);
            write_output(out_path, html.as_bytes());
        }
        Format::Markdown { obsidian } => {
            let (plot_dir, href_prefix) = plot_layout_for(out_path);
            let iframe_href = if *obsidian {
                let stem = out_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                Some(format!("{stem}.html"))
            } else {
                None
            };
            let md = render_markdown::render_markdown(
                title,
                rendered,
                &plot_dir,
                &href_prefix,
                theme,
                iframe_href.as_deref(),
            );
            write_output(out_path, md.as_bytes());
        }
        Format::Latex => {
            let (plot_dir, href_prefix) = plot_layout_for(out_path);
            let tex = render_latex::render_latex(title, rendered, &plot_dir, &href_prefix, theme);
            write_output(out_path, tex.as_bytes());
        }
        Format::Pdf => {
            // PDFs are self-contained, so compilation happens inside a temp
            // directory: the .tex source and SVG plots are intermediates the
            // user did not ask for. Only the final .pdf is copied out.
            let workdir = match tempfile::tempdir() {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("error: cannot create temp directory for PDF build: {e}");
                    std::process::exit(1);
                }
            };
            let tex_path = workdir.path().join("notebook.tex");
            let plot_dir = workdir.path().join("plots").join("notebook");
            let tex = render_latex::render_latex(
                title,
                rendered,
                &plot_dir,
                "plots/notebook",
                theme,
            );
            write_output(&tex_path, tex.as_bytes());
            compile_pdf(&tex_path, out_path);
        }
    }
}

/// Where to write plot SVGs and what relative path to embed in the rendered
/// document. Both Markdown and LaTeX use the same convention so a directory
/// of rendered notebooks groups under one `plots/` umbrella with one
/// subdirectory per notebook stem — see `docs/notebooks.md` ("Plot output
/// layout") for the rationale.
fn plot_layout_for(out_path: &PathBuf) -> (PathBuf, String) {
    let stem = out_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let parent = out_path.parent().unwrap_or(std::path::Path::new("."));
    let plot_dir = parent.join("plots").join(&stem);
    let href_prefix = format!("plots/{stem}");
    (plot_dir, href_prefix)
}

fn print_summary(input: &PathBuf, out_path: &PathBuf, rendered: &[execute::Rendered]) {
    let n_code = rendered
        .iter()
        .filter(|b| matches!(b, execute::Rendered::Code { .. }))
        .count();
    let n_plots: usize = rendered
        .iter()
        .map(|b| match b {
            execute::Rendered::Code { figures, .. } => figures.len(),
            _ => 0,
        })
        .sum();
    let n_errors = rendered
        .iter()
        .filter(|b| matches!(b, execute::Rendered::Code { error: Some(_), .. }))
        .count();

    print!(
        "Rendered {} → {} ({} code blocks, {} plots",
        input.display(),
        out_path.display(),
        n_code,
        n_plots
    );
    if n_errors > 0 {
        print!(", {} errors", n_errors);
    }
    println!(")");
}

fn write_output(path: &PathBuf, data: &[u8]) {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("error: cannot create directory {}: {e}", parent.display());
                std::process::exit(1);
            }
        }
    }
    if let Err(e) = std::fs::write(path, data) {
        eprintln!("error: cannot write {}: {e}", path.display());
        std::process::exit(1);
    }
}

/// Run pdflatex/tectonic on `tex_path` (expected to live in a temp directory)
/// and copy the resulting PDF to `pdf_path`. On failure the build log is
/// copied next to `pdf_path` as `<stem>.log` so it survives the temp dir's
/// cleanup and the user has something to read.
fn compile_pdf(tex_path: &PathBuf, pdf_path: &PathBuf) {
    let tex_dir = tex_path.parent().unwrap_or(std::path::Path::new("."));

    let (cmd, args): (&str, Vec<&str>) = if which_exists("pdflatex") {
        (
            "pdflatex",
            vec![
                "-interaction=nonstopmode",
                "-halt-on-error",
                "-shell-escape",
            ],
        )
    } else if which_exists("tectonic") {
        ("tectonic", vec!["-Z", "shell-escape"])
    } else {
        eprintln!("error: neither pdflatex nor tectonic found in PATH");
        eprintln!("  Install TeX Live: https://tug.org/texlive/");
        eprintln!("  Or tectonic:      https://tectonic-typesetting.github.io/");
        std::process::exit(1);
    };

    eprintln!("Compiling PDF with {cmd}...");
    let status = std::process::Command::new(cmd)
        .args(&args)
        .arg(tex_path.file_name().unwrap())
        .current_dir(tex_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match status {
        Ok(s) if s.success() => {
            let generated = tex_path.with_extension("pdf");
            if let Some(parent) = pdf_path.parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            if let Err(e) = std::fs::copy(&generated, pdf_path) {
                eprintln!("error: cannot write {}: {e}", pdf_path.display());
                std::process::exit(1);
            }
        }
        Ok(s) => {
            let log_src = tex_path.with_extension("log");
            let log_dst = pdf_path.with_extension("log");
            let copied = std::fs::copy(&log_src, &log_dst).is_ok();
            eprintln!("error: {cmd} exited with status {s}");
            if copied {
                eprintln!("  Build log saved to {}", log_dst.display());
            } else {
                eprintln!("  (build log was not preserved)");
            }
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("error: failed to run {cmd}: {e}");
            std::process::exit(1);
        }
    }
}

fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn extract_title(source: &str, path: &PathBuf) -> String {
    // Frontmatter `title:` wins over the H1 fallback.
    let (fm, body) = parse::extract_frontmatter(source);
    if let Some(t) = fm.title {
        return t;
    }
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") && !trimmed.starts_with("## ") {
            return trimmed[2..].trim().to_string();
        }
    }
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

pub fn generate_index_html(
    page_title: &str,
    entries: &[(String, String)],
    theme: &ThemeColors,
    body_html: &str,
) -> String {
    let c = theme;
    let mut links = String::new();
    for (title, filename) in entries {
        links.push_str(&format!(
            "  <li><a href=\"{filename}\">{title}</a></li>\n",
            filename = escape_html(filename),
            title = escape_html(title),
        ));
    }

    let intro = if body_html.is_empty() {
        String::new()
    } else {
        format!("<div class=\"intro prose\">\n{body_html}</div>\n")
    };

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — Notebook Index</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
    background: {bg};
    color: {text};
    display: flex;
    justify-content: center;
    min-height: 100vh;
    padding: 3rem 1.5rem;
  }}
  main {{
    max-width: 720px;
    width: 100%;
  }}
  h1 {{
    font-size: 2rem;
    color: {accent_primary};
    margin-bottom: 0.5rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid {border};
  }}
  .subtitle {{
    color: {text_dim};
    font-size: 0.9rem;
    margin-bottom: 2rem;
  }}
  ul {{
    list-style: none;
    padding: 0;
  }}
  li {{
    margin-bottom: 0.5rem;
  }}
  a {{
    display: block;
    padding: 0.8rem 1.2rem;
    background: {bg_secondary};
    border: 1px solid {border};
    border-radius: 8px;
    color: {accent_secondary};
    text-decoration: none;
    font-size: 1.05rem;
    transition: background 0.15s, border-color 0.15s;
  }}
  a:hover {{
    background: {border};
    border-color: {accent_secondary};
  }}
  footer {{
    color: {footer_text};
    font-size: 0.8rem;
    margin-top: 3rem;
    padding-top: 1rem;
    border-top: 1px solid {border};
  }}
  .intro {{
    color: {text};
    margin-bottom: 2rem;
  }}
  .intro p, .intro ul, .intro ol {{ margin-bottom: 1rem; }}
  .intro h2 {{ color: {accent_primary}; margin: 1.5rem 0 0.5rem; }}
  .intro a {{
    display: inline; padding: 0; background: transparent; border: 0;
    color: {accent_secondary}; text-decoration: underline;
  }}
  .intro a:hover {{ background: transparent; }}
</style>
</head>
<body>
<main>
<h1>{title}</h1>
<p class="subtitle">{count} notebook{plural}</p>
{intro}<ul>
{links}</ul>
<footer>Generated by rustlab-notebook</footer>
</main>
</body>
</html>
"##,
        title = escape_html(page_title),
        count = entries.len(),
        plural = if entries.len() == 1 { "" } else { "s" },
        intro = intro,
        links = links,
        bg = c.bg,
        bg_secondary = c.bg_secondary,
        text = c.text,
        text_dim = c.text_dim,
        border = c.border,
        accent_primary = c.accent_primary,
        accent_secondary = c.accent_secondary,
        footer_text = c.footer_text,
    )
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustlab_plot::Theme;

    #[test]
    fn extract_title_from_heading() {
        let source = "# My Analysis\n\nSome text.";
        let title = extract_title(source, &PathBuf::from("analysis.md"));
        assert_eq!(title, "My Analysis");
    }

    #[test]
    fn extract_title_fallback_to_filename() {
        let source = "No heading here.";
        let title = extract_title(source, &PathBuf::from("my_report.md"));
        assert_eq!(title, "my_report");
    }

    #[test]
    fn extract_title_ignores_h2() {
        let source = "## Sub Heading\n\nText.";
        let title = extract_title(source, &PathBuf::from("test.md"));
        assert_eq!(title, "test");
    }

    #[test]
    fn generate_index_basic() {
        let entries = vec![
            ("Filter Analysis".to_string(), "filter.html".to_string()),
            ("Quick Look".to_string(), "quick.html".to_string()),
        ];
        let html = generate_index_html("notebooks", &entries, Theme::Dark.colors(), "");
        assert!(html.contains("notebooks"));
        assert!(html.contains("2 notebooks"));
        assert!(html.contains("href=\"filter.html\""));
        assert!(html.contains("Filter Analysis"));
        assert!(html.contains("href=\"quick.html\""));
        assert!(html.contains("Quick Look"));
        assert!(html.contains("Generated by rustlab-notebook"));
    }

    #[test]
    fn generate_index_single() {
        let entries = vec![("Solo".to_string(), "solo.html".to_string())];
        let html = generate_index_html("test", &entries, Theme::Dark.colors(), "");
        assert!(html.contains("1 notebook"));
        assert!(!html.contains("notebooks")); // singular
    }

    #[test]
    fn generate_index_empty() {
        let html = generate_index_html("empty", &[], Theme::Dark.colors(), "");
        assert!(html.contains("0 notebooks"));
    }

    #[test]
    fn generate_index_escapes_html() {
        let entries = vec![("A <script> & \"test\"".to_string(), "test.html".to_string())];
        let html = generate_index_html("dir", &entries, Theme::Dark.colors(), "");
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("&amp;"));
    }

    #[test]
    fn generate_index_includes_body_html() {
        let entries = vec![("A".to_string(), "a.html".to_string())];
        let body = "<p>Intro paragraph.</p>\n";
        let html = generate_index_html("dir", &entries, Theme::Dark.colors(), body);
        assert!(html.contains("<p>Intro paragraph.</p>"));
        assert!(html.contains("class=\"intro"));
    }

    #[test]
    fn generate_index_no_intro_when_body_empty() {
        let entries = vec![("A".to_string(), "a.html".to_string())];
        let html = generate_index_html("dir", &entries, Theme::Dark.colors(), "");
        assert!(!html.contains("class=\"intro"));
    }

    #[test]
    fn generate_index_uses_custom_title() {
        let entries = vec![("A".to_string(), "a.html".to_string())];
        let html = generate_index_html("My Book", &entries, Theme::Dark.colors(), "");
        assert!(html.contains("<h1>My Book</h1>"));
        assert!(html.contains("<title>My Book"));
    }

    #[test]
    fn extract_title_from_frontmatter_wins_over_h1() {
        let source = "---\ntitle: FM Wins\n---\n# H1 Loses\n";
        let title = extract_title(source, &PathBuf::from("x.md"));
        assert_eq!(title, "FM Wins");
    }

    #[test]
    fn extract_title_frontmatter_quoted() {
        let source = "---\ntitle: \"Quoted Title\"\n---\n";
        let title = extract_title(source, &PathBuf::from("x.md"));
        assert_eq!(title, "Quoted Title");
    }

    #[test]
    fn extract_title_h1_when_no_frontmatter_title() {
        let source = "---\norder: 3\n---\n# Real Title\n";
        let title = extract_title(source, &PathBuf::from("x.md"));
        assert_eq!(title, "Real Title");
    }
}
