use clap::{Parser, Subcommand, ValueEnum};
use rustlab_plot::Theme;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "rustlab-notebook",
    version = env!("CARGO_PKG_VERSION"),
    about = "Render Markdown notebooks with rustlab code blocks",
    long_about = "Render Markdown notebooks with rustlab code blocks.\n\n\
        Executes ```rustlab fenced code blocks through the evaluator, captures\n\
        text output and plots, and produces self-contained HTML, LaTeX, or PDF.\n\
        Supports template interpolation (${expr}), KaTeX math, syntax highlighting,\n\
        and multi-notebook directory rendering with index generation.\n\n\
        Examples:\n  \
        rustlab-notebook render analysis.md                    # → analysis.html (dark theme)\n  \
        rustlab-notebook render analysis.md -t light           # → analysis.html (light theme)\n  \
        rustlab-notebook render analysis.md -f pdf             # → analysis.pdf\n  \
        rustlab-notebook render analysis.md -f latex           # → analysis.tex + SVG plots\n  \
        rustlab-notebook render analysis.md -f pdf -t light    # light-themed PDF\n  \
        rustlab-notebook render analysis.md -o out.html        # custom output path\n  \
        rustlab-notebook render notebooks/                     # render all .md → .html + index\n  \
        rustlab-notebook render notebooks/ -f pdf -t light     # all notebooks → light PDF\n\n\
        Options:\n  \
        -o, --output <PATH>    Output file or directory (default: <input_stem>.<ext>)\n  \
        -f, --format <FMT>     html (default), latex, pdf, markdown\n  \
        -t, --theme  <THEME>   dark (default), light\n      \
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
struct Cli {
    #[command(subcommand)]
    command: Command,
}

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
enum Command {
    /// Render a notebook (or directory of notebooks) to HTML, LaTeX, or PDF
    #[command(
        long_about = "Render a notebook (or directory of notebooks) to HTML, LaTeX, or PDF.\n\n\
            Examples:\n  \
            rustlab-notebook render analysis.md                    # → analysis.html (dark theme)\n  \
            rustlab-notebook render analysis.md -t light           # → analysis.html (light theme)\n  \
            rustlab-notebook render analysis.md -f pdf             # → analysis.pdf\n  \
            rustlab-notebook render analysis.md -f latex           # → analysis.tex + SVG plots\n  \
            rustlab-notebook render analysis.md -f pdf -t light    # light-themed PDF\n  \
            rustlab-notebook render analysis.md -o out.html        # custom output path\n  \
            rustlab-notebook render notebooks/                     # render all .md → .html + index\n  \
            rustlab-notebook render notebooks/ -f pdf -t light     # all notebooks → light PDF\n\n\
            Options:\n  \
            -o, --output <PATH>    Output file or directory (default: <input_stem>.<ext>)\n  \
            -f, --format <FMT>     html (default), latex, pdf\n  \
            -t, --theme  <THEME>   dark (default), light\n\n\
            Formats:\n  \
            html   Self-contained HTML with Plotly charts and KaTeX math (default)\n  \
            latex  LaTeX .tex file + SVG plots in plots/<name>/ directory\n  \
            pdf    Compile LaTeX to PDF (requires pdflatex or tectonic)\n\n\
            Themes:\n  \
            dark   Catppuccin Mocha — dark background, light text (default)\n  \
            light  Catppuccin Latte — light background, dark text"
    )]
    /// Watch a directory of notebooks and re-render on save (markdown only)
    #[command(
        long_about = "Watch a directory of notebooks and re-render whenever a source file changes.\n\n\
            Pairs naturally with --obsidian: edit notes in Obsidian's Editing view,\n\
            switch to Reading view, see updated plots and text within ~500 ms.\n\n\
            Examples:\n  \
            rustlab-notebook watch notebooks/                              # → re-render to notebooks/<name>.md on save\n  \
            rustlab-notebook watch notebooks/ -o vault/ --obsidian         # vault-native output to a separate dir\n  \
            rustlab-notebook watch notebooks/ --debounce-ms 500            # quieter editor, slower triggers\n\n\
            Currently --format markdown only."
    )]
    Watch {
        /// Directory of .md files to watch
        input: PathBuf,
        /// Output directory (default: same as input)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Color theme: dark (default), light
        #[arg(short, long, value_enum, default_value = "dark")]
        theme: CliTheme,
        /// Obsidian-friendly markdown output (see `render --obsidian` for details)
        #[arg(long)]
        obsidian: bool,
        /// Override the attachments directory (with --obsidian)
        #[arg(long, value_name = "DIR")]
        attachments_dir: Option<String>,
        /// Suppress the trailing iframe (with --obsidian)
        #[arg(long)]
        no_iframe: bool,
        /// Debounce window for filesystem events (default 250 ms)
        #[arg(long, value_name = "MS", default_value = "250")]
        debounce_ms: u64,
    },
    /// Lint .md notebook source(s) for rustlab-shaped failures
    #[command(
        long_about = "Lint one or more .md notebook files for rustlab-shaped failures.\n\n\
            Exit codes:\n  \
            0 = clean (no findings, or info-only)\n  \
            1 = warnings (also exits 1 on info under --strict)\n  \
            2 = any error\n\n\
            Examples:\n  \
            rustlab-notebook check note.md\n  \
            rustlab-notebook check notebooks/         # recursive\n  \
            rustlab-notebook check note.md --fix      # auto-correct safe issues\n  \
            rustlab-notebook check notebooks/ --strict"
    )]
    Check {
        /// Input .md file or directory of .md files (recursive).
        input: PathBuf,
        /// Auto-correct findings the linter can fix (calls `clean`).
        #[arg(long)]
        fix: bool,
        /// Treat warnings (and info) as errors.
        #[arg(long)]
        strict: bool,
    },
    /// Strip rustlab-generated artifacts from .md notebook source(s)
    #[command(
        long_about = "Strip rustlab-generated artifacts from one or more .md files, leaving only \
            user-authored source. Useful for migrating between single-dir (in-place) and two-dir \
            layouts, sanitising files before commit, or recovering pristine source from a rendered \
            output.\n\n\
            Examples:\n  \
            rustlab-notebook clean note.md                 # in-place clean of one file\n  \
            rustlab-notebook clean notebooks/              # in-place clean of every .md under notebooks/\n  \
            rustlab-notebook clean note.md --check         # exit 1 if anything would change, no write"
    )]
    Clean {
        /// Input .md file or directory of .md files (recursive).
        input: PathBuf,
        /// Optional output path. Default: clean in place.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Report what would change without writing.
        #[arg(long)]
        check: bool,
    },
    Render {
        /// Input .md file or directory of .md files
        input: PathBuf,
        /// Output file or directory (default: <input_stem>.<ext> or same directory)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Output format: html (default), latex, pdf, markdown
        #[arg(short, long, value_enum, default_value = "html")]
        format: CliFormat,
        /// Color theme: dark (default), light
        #[arg(short, long, value_enum, default_value = "dark")]
        theme: CliTheme,
        /// Index page title (directory mode only). Precedence:
        /// --title > index.md H1 > parent directory name.
        #[arg(long)]
        title: Option<String>,
        /// Obsidian-friendly markdown: cross-notebook links emit as
        /// `[[wikilinks]]`, plots route to `_attachments/<stem>/`, frontmatter
        /// gains `tags: [rustlab]` / `cssclasses: [rustlab-notebook]`, and a
        /// trailing iframe to the sibling .html is appended (suppress with
        /// `--no-iframe`). Only meaningful with --format markdown.
        #[arg(long)]
        obsidian: bool,
        /// Override the attachments directory used by `--obsidian` for
        /// plot SVGs. Default: `_attachments`.
        #[arg(long, value_name = "DIR")]
        attachments_dir: Option<String>,
        /// Suppress the trailing iframe under `--obsidian`.
        #[arg(long)]
        no_iframe: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Watch {
            input,
            output,
            theme,
            obsidian,
            attachments_dir,
            no_iframe,
            debounce_ms,
        } => {
            let theme = match theme {
                CliTheme::Dark => Theme::Dark,
                CliTheme::Light => Theme::Light,
            };
            let colors = theme.colors();
            let obsidian_opts = if obsidian {
                let mut opts = rustlab_notebook::ObsidianOpts::default();
                if let Some(dir) = attachments_dir {
                    opts.attachments_dir = dir;
                }
                if no_iframe {
                    opts.iframe = false;
                }
                Some(opts)
            } else {
                None
            };
            let format = rustlab_notebook::Format::Markdown {
                obsidian: obsidian_opts,
            };
            rustlab_notebook::watch::cmd_watch(input, output, format, colors, debounce_ms);
        }
        Command::Render {
            input,
            output,
            format,
            theme,
            title,
            obsidian,
            attachments_dir,
            no_iframe,
        } => {
            let theme = match theme {
                CliTheme::Dark => Theme::Dark,
                CliTheme::Light => Theme::Light,
            };
            let colors = theme.colors();
            if obsidian && !matches!(format, CliFormat::Markdown) {
                eprintln!("warning: --obsidian only applies to --format markdown; ignored");
            }
            if (attachments_dir.is_some() || no_iframe) && !obsidian {
                eprintln!(
                    "warning: --attachments-dir / --no-iframe only apply with --obsidian; ignored"
                );
            }
            let obsidian_opts = if obsidian {
                let mut opts = rustlab_notebook::ObsidianOpts::default();
                if let Some(dir) = attachments_dir.clone() {
                    opts.attachments_dir = dir;
                }
                if no_iframe {
                    opts.iframe = false;
                }
                Some(opts)
            } else {
                None
            };
            let format = match format {
                CliFormat::Html => rustlab_notebook::Format::Html,
                CliFormat::Latex => rustlab_notebook::Format::Latex,
                CliFormat::Pdf => rustlab_notebook::Format::Pdf,
                CliFormat::Markdown => rustlab_notebook::Format::Markdown {
                    obsidian: obsidian_opts,
                },
            };
            if input.is_dir() {
                rustlab_notebook::cmd_render_dir(input, output, format, colors, title);
            } else {
                if title.is_some() {
                    eprintln!("warning: --title is only used when rendering a directory; ignored for single-file input");
                }
                rustlab_notebook::cmd_render(input, output, format, colors);
            }
        }
        Command::Clean { input, output, check } => {
            let changed = rustlab_notebook::cmd_clean(input, output, check);
            if check && changed > 0 {
                std::process::exit(1);
            }
        }
        Command::Check { input, fix, strict } => {
            let outcome = rustlab_notebook::cmd_check(input, fix, strict);
            let code = outcome.exit_code(strict);
            if code != 0 {
                std::process::exit(code);
            }
        }
    }
}
