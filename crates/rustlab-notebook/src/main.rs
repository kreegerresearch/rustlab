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

#[derive(Clone, PartialEq, Eq, ValueEnum)]
enum CliFormat {
    Html,
    Latex,
    Pdf,
    Markdown,
    /// Emit a JSON document on stdout describing every block plus
    /// pre-rendered HTML/SVG. Consumed by the Obsidian community plugin
    /// and other downstream tooling. Single-file input only.
    Json,
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
    /// Watch a notebook (interactive server) or directory (re-render on save)
    #[command(
        long_about = "Watch a notebook source and react to saves.\n\n\
            Two modes — picked by what you pass:\n\n  \
            Interactive server (a .md file or a directory, no --obsidian/--output):\n    \
            Spins up a local web server on http://127.0.0.1:8042 (auto-bumps\n    \
            up to +10 if busy), opens your browser, and live-reloads the\n    \
            page on every save via a WebSocket push. A single .md serves one\n    \
            notebook; a directory serves every .md under it behind an index\n    \
            page (one URL per notebook). KaTeX + Plotly assets are embedded\n    \
            in the binary so the page works fully offline. Small edits ship\n    \
            as block-level partial diffs and preserve scroll position;\n    \
            structural edits fall back to a full refresh. Source .md is never\n    \
            modified — unless you pass --editable (in-browser editor).\n\n  \
            Re-render on save (directory + --obsidian or --output):\n    \
            Long-running counterpart of `render`. Re-renders any notebook\n    \
            whose source changes, debouncing fs events. Pairs with\n    \
            --obsidian for an Obsidian Editing/Reading view loop.\n\n\
            Examples:\n  \
            rustlab-notebook watch analysis.md                             # interactive server (one notebook)\n  \
            rustlab-notebook watch notebooks/                              # interactive server (whole directory + index)\n  \
            rustlab-notebook watch analysis.md --port 9000                 # custom port (fails loud on collision)\n  \
            rustlab-notebook watch analysis.md --no-browser                # don't auto-open the browser\n  \
            rustlab-notebook watch analysis.md --editable                  # edit the .md in the browser (writes back)\n  \
            rustlab-notebook watch notebooks/ --obsidian                   # re-render on save, vault-friendly in-place\n  \
            rustlab-notebook watch notebooks/ -o vault/ --obsidian         # re-render on save, vault-native two-dir\n  \
            rustlab-notebook watch notebooks/ --debounce-ms 500            # quieter editor, slower triggers\n\n\
            Re-render-on-save is markdown-only currently."
    )]
    Watch {
        /// Notebook .md file (interactive server mode) or directory of .md
        /// files (with --obsidian / --output).
        input: PathBuf,
        /// Output directory (default: same as input). Setting --output or
        /// --obsidian switches off the interactive server and runs the
        /// existing re-render-on-save flow instead.
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
        /// (interactive server mode only) Port to bind on 127.0.0.1.
        /// Default 8042 with auto-increment up to +10 if busy; setting
        /// --port explicitly disables auto-increment (fails loud on
        /// collision).
        #[arg(long, value_name = "PORT")]
        port: Option<u16>,
        /// (interactive server mode only) Do not auto-open the browser.
        #[arg(long)]
        no_browser: bool,
        /// (interactive server mode only) Enable the in-browser editor:
        /// a split-pane CodeMirror editor whose saves write back to the
        /// source .md. This is the one interactive path that modifies
        /// source (parallels --obsidian), so it is strictly opt-in.
        #[arg(long)]
        editable: bool,
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
        /// Read notebook source from stdin instead of a file. The `input`
        /// argument is ignored when set; pass `-` as a placeholder. JSON
        /// format only — file formats require a real input path.
        #[arg(long)]
        stdin: bool,
        /// Override the directory used to resolve relative paths (embeds,
        /// frontmatter resolution). Defaults to the input file's parent
        /// (or current dir for `--stdin`). JSON format only.
        #[arg(long, value_name = "DIR")]
        cwd: Option<PathBuf>,
        /// Indent JSON output for readability. Default: compact (one line).
        #[arg(long)]
        pretty: bool,
    },
    /// Render notebooks and lint each output against trusted external linters.
    ///
    /// Drop-in CI check for projects that ship rustlab-notebook sources —
    /// catches output-side regressions (broken HTML, malformed LaTeX,
    /// unparseable PDFs) that the source-side `check` command cannot see.
    ///
    /// Examples:
    ///   rustlab-notebook validate notebooks/
    ///   rustlab-notebook validate notebooks/ --format html,pdf
    ///   rustlab-notebook validate notebooks/ --require-all --report json
    ///   rustlab-notebook validate notebooks/ --linter vnu=$HOME/jars/vnu.jar
    #[command(
        long_about = "Render notebooks and lint each output against trusted external linters.\n\n\
            Linter selection per format:\n  \
            markdown → markdownlint-cli2 (npm i -g markdownlint-cli2)\n  \
            html     → vnu ($VNU_JAR or PATH) → tidy-html5 (5.x+) fallback\n  \
            latex    → chktex\n  \
            pdf      → pdfinfo + pdftotext (smoke), qpdf --check (structure),\n             \
                       verapdf (PDF/A, opt-in via --pdf-a)\n\n\
            Each linter is shelled out only when installed; otherwise the row\n\
            reports SKIPPED + an install hint. Set --require-all to upgrade any\n\
            SKIPPED to a FAIL — useful for CI to enforce a baseline toolchain.\n\n\
            Exit codes:\n  \
            0 = clean (no findings, or only SKIPPED)\n  \
            1 = at least one linter reported FAIL\n  \
            2 = --require-all set and at least one linter is missing"
    )]
    Validate {
        /// Input .md file or directory of .md files (recursive).
        input: PathBuf,
        /// Output formats to validate (comma-separated).
        #[arg(short, long, value_delimiter = ',',
              default_values_t = vec![CliValidateFormat::Markdown,
                                      CliValidateFormat::Html,
                                      CliValidateFormat::Latex,
                                      CliValidateFormat::Pdf])]
        format: Vec<CliValidateFormat>,
        /// Report format: text (default) | json.
        #[arg(long, value_enum, default_value = "text")]
        report: CliReportFormat,
        /// Missing linter → FAIL (default: SKIPPED).
        #[arg(long)]
        require_all: bool,
        /// Also run verapdf PDF/A conformance check on PDFs.
        /// Off by default — the pipeline does not target PDF/A.
        #[arg(long)]
        pdf_a: bool,
        /// Leave the temp render dir for inspection after the run.
        #[arg(long)]
        keep_tmp: bool,
        /// Override a linter's binary path (repeatable). Format: KEY=PATH.
        /// Keys: markdownlint-cli2, markdownlint, vnu, tidy, chktex,
        /// pdfinfo, pdftotext, qpdf, verapdf. For `vnu`, pass a `.jar`
        /// path to invoke via `java -jar`.
        #[arg(long = "linter", value_name = "KEY=PATH",
              value_parser = parse_linter_override, action = clap::ArgAction::Append)]
        linter_overrides: Vec<(String, PathBuf)>,
    },
    /// Inspect, prune, or clear a persistent function-result cache.
    /// Mirrors `rustlab cache ...` so notebook-driven workflows can
    /// manage the cache without leaving the notebook binary.
    #[command(subcommand)]
    Cache(CacheCommands),
}

#[derive(Subcommand)]
enum CacheCommands {
    /// Print store path, entry count, and total stored bytes
    Status(CacheCommonArgs),
    /// List cached entries (key, size, version, timestamp) — never prints values
    List(CacheListArgs),
    /// Drop every cached entry; keeps the DB file
    Clear(CacheCommonArgs),
    /// Drop entries older than a duration and/or to fit a max byte cap
    Prune(CachePruneArgs),
}

#[derive(clap::Args, Clone)]
struct CacheCommonArgs {
    /// Path to the `.rcache` / `.db` store to operate on
    /// (default: `.rustlab/cache.db`)
    #[arg(long, value_name = "PATH")]
    store: Option<PathBuf>,
}

#[derive(clap::Args, Clone)]
struct CacheListArgs {
    #[command(flatten)]
    common: CacheCommonArgs,
    /// Cap the number of rows shown (newest first)
    #[arg(long, value_name = "N")]
    limit: Option<usize>,
}

#[derive(clap::Args, Clone)]
struct CachePruneArgs {
    #[command(flatten)]
    common: CacheCommonArgs,
    /// Age cutoff: drops entries older than this. Format: `30d`, `12h`,
    /// `500ms`, etc. (units: ms, s, m, h, d, w)
    #[arg(long, value_name = "DURATION")]
    older_than: Option<String>,
    /// Size cap in bytes: drops oldest entries until total ≤ this
    #[arg(long, value_name = "BYTES")]
    max_size: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, ValueEnum)]
enum CliValidateFormat {
    Markdown,
    Html,
    Latex,
    Pdf,
}

impl CliValidateFormat {
    fn to_format(&self) -> rustlab_notebook::validate::Format {
        match self {
            CliValidateFormat::Markdown => rustlab_notebook::validate::Format::Markdown,
            CliValidateFormat::Html => rustlab_notebook::validate::Format::Html,
            CliValidateFormat::Latex => rustlab_notebook::validate::Format::Latex,
            CliValidateFormat::Pdf => rustlab_notebook::validate::Format::Pdf,
        }
    }
}

impl std::fmt::Display for CliValidateFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            CliValidateFormat::Markdown => "markdown",
            CliValidateFormat::Html => "html",
            CliValidateFormat::Latex => "latex",
            CliValidateFormat::Pdf => "pdf",
        };
        f.write_str(s)
    }
}

#[derive(Clone, Debug, ValueEnum)]
enum CliReportFormat {
    Text,
    Json,
}

fn parse_linter_override(s: &str) -> Result<(String, PathBuf), String> {
    let (key, path) = s
        .split_once('=')
        .ok_or_else(|| format!("expected KEY=PATH, got `{s}`"))?;
    if key.is_empty() {
        return Err("linter override key is empty".to_string());
    }
    Ok((key.to_string(), PathBuf::from(path)))
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
            port,
            no_browser,
            editable,
        } => {
            let theme = match theme {
                CliTheme::Dark => Theme::Dark,
                CliTheme::Light => Theme::Light,
            };
            let colors = theme.colors();

            // Bare `watch <input>` (no --obsidian, no --output) spins up
            // the interactive server: a single .md file serves one
            // notebook; a directory serves every notebook under it behind
            // an index page. Any other combination falls through to the
            // existing two-dir / obsidian render loop.
            // Per dev/plans/notebook_interactive_server.md locked-in #1.
            if !obsidian && output.is_none() {
                if !input.exists() {
                    eprintln!("error: {} does not exist", input.display());
                    std::process::exit(2);
                }
                if attachments_dir.is_some() || no_iframe {
                    eprintln!(
                        "warning: --attachments-dir / --no-iframe only apply with --obsidian; ignored"
                    );
                }
                let opts = rustlab_notebook::server::ServerOpts {
                    port,
                    no_browser,
                    editable,
                };
                if let Err(e) = rustlab_notebook::server::start(&input, colors, opts) {
                    eprintln!("rustlab-notebook watch: {e:#}");
                    std::process::exit(1);
                }
                return;
            }

            if port.is_some() || no_browser || editable {
                eprintln!(
                    "warning: --port / --no-browser / --editable only apply to the bare-input interactive server; ignored",
                );
            }

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
            stdin,
            cwd,
            pretty,
        } => {
            let theme = match theme {
                CliTheme::Dark => Theme::Dark,
                CliTheme::Light => Theme::Light,
            };
            let colors = theme.colors();

            // JSON has stdout-only IO semantics (no output path, optional
            // stdin) so it diverges from the file-based render pipeline
            // before any of the markdown-specific option-validation runs.
            if format == CliFormat::Json {
                if output.is_some() {
                    eprintln!("warning: --output is ignored for --format json (writes to stdout)");
                }
                if title.is_some() {
                    eprintln!("warning: --title is ignored for --format json");
                }
                if obsidian || attachments_dir.is_some() || no_iframe {
                    eprintln!(
                        "warning: --obsidian / --attachments-dir / --no-iframe do not apply to --format json; ignored"
                    );
                }
                let input_arg = if stdin { None } else { Some(input) };
                rustlab_notebook::cmd_render_json(input_arg, cwd, colors, pretty);
                return;
            }

            if stdin || cwd.is_some() || pretty {
                eprintln!("warning: --stdin / --cwd / --pretty only apply to --format json; ignored");
            }

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
                CliFormat::Json => unreachable!("--format json branched to cmd_render_json above"),
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
        Command::Validate {
            input,
            format,
            report,
            require_all,
            pdf_a,
            keep_tmp,
            linter_overrides,
        } => {
            use rustlab_notebook::validate::{
                cmd_validate, ReportFormat, ValidateOpts,
            };
            let opts = ValidateOpts {
                formats: format.iter().map(|f| f.to_format()).collect(),
                report: match report {
                    CliReportFormat::Text => ReportFormat::Text,
                    CliReportFormat::Json => ReportFormat::Json,
                },
                require_all,
                pdf_a,
                keep_tmp,
                linter_overrides: linter_overrides.into_iter().collect(),
            };
            let outcome = cmd_validate(input, opts.clone());
            match opts.report {
                ReportFormat::Text => print!("{}", outcome.render_text()),
                ReportFormat::Json => println!("{}", outcome.render_json()),
            }
            let code = outcome.exit_code(require_all);
            if code != 0 {
                std::process::exit(code);
            }
        }
        Command::Cache(cmd) => {
            if let Err(e) = run_cache_command(cmd) {
                eprintln!("rustlab-notebook cache: {e:#}");
                std::process::exit(1);
            }
        }
    }
}

/// Per-project default. Resolved against CWD at command time.
const CACHE_DEFAULT_PATH: &str = ".rustlab/cache.db";

fn resolve_cache_path(common: &CacheCommonArgs) -> PathBuf {
    common
        .store
        .clone()
        .unwrap_or_else(|| PathBuf::from(CACHE_DEFAULT_PATH))
}

fn open_cache(path: &std::path::Path, must_exist: bool) -> anyhow::Result<rustlab_cache::Store> {
    use anyhow::Context;
    if must_exist && !path.exists() {
        anyhow::bail!(
            "no cache file at {} (use `rustlab-notebook render` after `cache enable` in your notebook, or pass --store PATH)",
            path.display(),
        );
    }
    rustlab_cache::Store::open(path).with_context(|| format!("opening cache at {}", path.display()))
}

fn run_cache_command(cmd: CacheCommands) -> anyhow::Result<()> {
    use anyhow::Context;
    match cmd {
        CacheCommands::Status(args) => {
            let path = resolve_cache_path(&args);
            if !path.exists() {
                println!("cache: no store at {}", path.display());
                return Ok(());
            }
            let store = open_cache(&path, true)?;
            let schema = store
                .schema_meta("version")?
                .unwrap_or_else(|| "<absent>".to_string());
            let rl_version = store
                .schema_meta("rustlab_version")?
                .unwrap_or_else(|| "<absent>".to_string());
            println!("cache: {}", path.display());
            println!("  schema version: {schema}");
            println!("  rustlab version: {rl_version}");
            println!("  entries: {}", store.entry_count()?);
            println!("  stored bytes: {}", store.total_bytes()?);
            if store.is_disabled() {
                println!("  status: DISABLED (schema is newer than this binary supports)");
            }
        }
        CacheCommands::List(args) => {
            let path = resolve_cache_path(&args.common);
            let store = open_cache(&path, true)?;
            let rows = store.list_entries(args.limit)?;
            if rows.is_empty() {
                println!("cache: no entries in {}", path.display());
                return Ok(());
            }
            println!(
                "{:<20}  {:>9}  {:>10}  {:>10}  {:>9}  created_at",
                "fn name", "entry_id", "input_hash", "bytes", "rl_ver"
            );
            for row in rows {
                let fn_name = row.fn_name.unwrap_or_else(|| "<unknown>".to_string());
                println!(
                    "{:<20}  {:>9}  {:>10}  {:>10}  {:>9}  {}",
                    fn_name,
                    row.entry_id_short,
                    row.input_hash_short,
                    row.bytes,
                    row.rustlab_version,
                    row.created_at,
                );
            }
        }
        CacheCommands::Clear(args) => {
            let path = resolve_cache_path(&args);
            let store = open_cache(&path, true)?;
            let n = store.clear()?;
            println!("cache: cleared {n} entries from {}", path.display());
        }
        CacheCommands::Prune(args) => {
            let path = resolve_cache_path(&args.common);
            let store = open_cache(&path, true)?;
            let did_specify = args.older_than.is_some() || args.max_size.is_some();
            let mut total: usize = 0;
            let mut notes: Vec<String> = Vec::new();
            if let Some(s) = args.older_than.as_deref() {
                let secs = rustlab_cache::parse_duration_secs(s)
                    .with_context(|| format!("--older-than {s}"))?;
                let n = store.prune_older_than(secs)?;
                total += n;
                notes.push(format!("{n} older than {secs}s"));
            }
            if let Some(max) = args.max_size {
                let n = store.prune_to_max_size(max)?;
                total += n;
                notes.push(format!("{n} to fit max_size={max}"));
            }
            if !did_specify {
                const THIRTY_DAYS: u64 = 30 * 24 * 60 * 60;
                let n = store.prune_older_than(THIRTY_DAYS)?;
                total += n;
                notes.push(format!("{n} older than 30 days (default)"));
            }
            println!(
                "cache: pruned {total} entries from {} ({})",
                path.display(),
                notes.join(", "),
            );
        }
    }
    Ok(())
}
