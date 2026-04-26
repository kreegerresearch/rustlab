use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use rustlab_plot::Theme;
use std::path::PathBuf;

#[derive(Clone, ValueEnum)]
enum CliFormat {
    Html,
    Latex,
    Pdf,
    Markdown,
}

#[derive(Clone, ValueEnum)]
enum CliTheme {
    Dark,
    Light,
}

#[derive(Subcommand)]
pub enum NotebookCommands {
    /// Render a notebook (or directory of notebooks) to HTML, LaTeX, or PDF
    #[command(
        long_about = "Render a notebook (or directory of notebooks) to HTML, LaTeX, or PDF.\n\n\
            Examples:\n  \
            rustlab notebook render analysis.md                    # → analysis.html (dark theme)\n  \
            rustlab notebook render analysis.md -t light           # → analysis.html (light theme)\n  \
            rustlab notebook render analysis.md -f pdf             # → analysis.pdf\n  \
            rustlab notebook render analysis.md -f latex           # → analysis.tex + SVG plots\n  \
            rustlab notebook render analysis.md -f pdf -t light    # light-themed PDF\n  \
            rustlab notebook render analysis.md -o out.html        # custom output path\n  \
            rustlab notebook render notebooks/                     # render all .md → .html + index\n  \
            rustlab notebook render notebooks/ -f pdf -t light     # all notebooks → light PDF\n  \
            rustlab notebook render notebooks/ -f markdown --obsidian  # Obsidian-friendly .md with iframe to sibling .html\n\n\
            Options:\n  \
            -o, --output <PATH>    Output file or directory (default: <input_stem>.<ext>)\n  \
            -f, --format <FMT>     html (default), latex, pdf, markdown\n  \
            -t, --theme  <THEME>   dark (default), light\n  \
                --obsidian         (markdown only) append an <iframe> pointing at the\n                                   \
                                   sibling .html so Obsidian renders the interactive\n                                   \
                                   Plotly view inline. GitHub strips iframes, so the\n                                   \
                                   same .md remains safe to commit.\n\n\
            Formats:\n  \
            html      Self-contained HTML with Plotly charts and KaTeX math (default)\n  \
            latex     LaTeX .tex file + SVG plots in plots/<name>/ directory\n  \
            pdf       Compile LaTeX to PDF (requires pdflatex or tectonic)\n  \
            markdown  GitHub-friendly .md with inline SVG plots — suitable for\n            \
                      committing alongside source, browsable on GitHub\n\n\
            Themes:\n  \
            dark   Catppuccin Mocha — dark background, light text (default)\n  \
            light  Catppuccin Latte — light background, dark text"
    )]
    Render(RenderArgs),
}

#[derive(Args)]
pub struct RenderArgs {
    /// Input .md file or directory of .md files
    input: PathBuf,
    /// Output file or directory
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Output format: html (default), latex, pdf, markdown
    #[arg(short, long, value_enum, default_value = "html")]
    format: CliFormat,
    /// Color theme: dark (default), light
    #[arg(short, long, value_enum, default_value = "dark")]
    theme: CliTheme,
    /// Index page title (directory mode only). Precedence: --title >
    /// index.md H1 > parent directory name.
    #[arg(long)]
    title: Option<String>,
    /// Obsidian-friendly markdown: append an iframe to the sibling .html.
    /// Only meaningful with --format markdown; ignored otherwise.
    #[arg(long)]
    obsidian: bool,
}

pub fn execute(cmd: NotebookCommands) -> Result<()> {
    match cmd {
        NotebookCommands::Render(args) => {
            let theme = match args.theme {
                CliTheme::Dark => Theme::Dark,
                CliTheme::Light => Theme::Light,
            };
            let colors = theme.colors();
            if args.obsidian && !matches!(args.format, CliFormat::Markdown) {
                eprintln!("warning: --obsidian only applies to --format markdown; ignored");
            }
            let format = match args.format {
                CliFormat::Html => rustlab_notebook::Format::Html,
                CliFormat::Latex => rustlab_notebook::Format::Latex,
                CliFormat::Pdf => rustlab_notebook::Format::Pdf,
                CliFormat::Markdown => rustlab_notebook::Format::Markdown {
                    obsidian: args.obsidian,
                },
            };
            if args.input.is_dir() {
                rustlab_notebook::cmd_render_dir(
                    args.input,
                    args.output,
                    format,
                    colors,
                    args.title,
                );
            } else {
                if args.title.is_some() {
                    eprintln!("warning: --title is only used when rendering a directory; ignored for single-file input");
                }
                rustlab_notebook::cmd_render(args.input, args.output, format, colors);
            }
            Ok(())
        }
    }
}
