use clap::{Parser, Subcommand, ValueEnum};
use rustlab_config::{DefaultAxis, DisplayFormat, UserSettings};
use rustlab_notebook::theme_select::{self, ContrastMode, ThemePolicy, ThemeRequest};
use rustlab_plot::{
    parse_theme, set_default_axis_y_direction, set_default_theme, AxisYDirection, ThemeColors,
};
use rustlab_script::{set_default_number_format, NumberFormat};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "rustlab-notebook",
    version = env!("CARGO_PKG_VERSION"),
    about = "Render Markdown notebooks with rustlab code blocks",
    long_about = "Render Markdown notebooks with rustlab code blocks.\n\n\
        Executes ```rustlab fenced code blocks through the evaluator, captures\n\
        text output and plots, and produces HTML, Markdown, LaTeX, PDF, or JSON.\n\
        Supports template interpolation (${expr}), KaTeX math, syntax highlighting,\n\
        and multi-notebook directory rendering with index generation.\n\n\
        Examples:\n  \
        rustlab-notebook render analysis.md                    # → analysis.html (dark theme)\n  \
        rustlab-notebook render analysis.md -t light           # → analysis.html (light theme)\n  \
        rustlab-notebook render analysis.md -f pdf             # → analysis.pdf (always light)\n  \
        rustlab-notebook render analysis.md -f latex           # → analysis.tex + SVG plots\n  \
        rustlab-notebook render analysis.md -o out.html        # custom output path\n  \
        rustlab-notebook render notebooks/                     # render all .md → .html + index\n  \
        rustlab-notebook render notebooks/ -f pdf              # all notebooks → light PDF\n\n\
        Options:\n  \
        -o, --output <PATH>    Output file or directory (default: <input_stem>.<ext>)\n  \
        -f, --format <FMT>     html (default), latex, pdf, markdown, json\n  \
        -t, --theme  <THEME>   HTML/watch theme: a builtin (mocha|macchiato|frappe|latte,\n                             \
                               aliases dark, light) or a theme file name. Default:\n                             \
                               frontmatter theme:, then rustlab.toml, then ~/.rustlabrc\n                             \
                               [notebook] theme, then mocha. LaTeX and PDF are always\n                             \
                               Catppuccin Latte on white paper.\n      \
            --theme-file <PATH> Use a TOML theme file (see `rustlab-notebook theme`).\n      \
            --obsidian         (markdown only) append an <iframe> pointing at the\n                                   \
                               sibling .html so Obsidian renders the interactive\n                                   \
                               Plotly view inline. GitHub strips iframes, so the\n                                   \
                               same .md remains safe to commit.\n\n\
        Formats:\n  \
        html      Self-contained HTML with Plotly charts and KaTeX math (default)\n  \
        latex     LaTeX .tex file + SVG plots in plots/<name>/ directory\n  \
        pdf       Compile LaTeX to PDF (always light; requires pdflatex or tectonic)\n  \
        markdown  GitHub-friendly .md with inline SVG plots — suitable for\n            \
                  committing alongside source, browsable on GitHub\n  \
        json      One notebook as JSON on stdout (single file; --stdin, --pretty)\n\n\
        Fences tagged bash, python, toml, yaml (or yml), or text are highlighted (text is uncolored). A fence with no tag is text.\n\
        Directory render shows a fixed file browser. Directory watch shows that browser and the section outline in a Contents drawer (also `\\`) that does not reserve width. A single file, LaTeX, and PDF do not show a file browser.\n\n\
        Themes:\n  \
        mocha / dark (default)  Catppuccin Mocha\n  \
        macchiato               Catppuccin Macchiato\n  \
        frappe                  Catppuccin Frappé\n  \
        latte / light           Catppuccin Latte\n  \
        Theme files: <project>/themes/NAME.toml (next to rustlab.toml) or\n  \
        ~/.config/rustlab/themes/NAME.toml. `rustlab-notebook theme list|show|check`."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliContrast {
    Warn,
    Error,
}

impl CliContrast {
    fn mode(self) -> ContrastMode {
        match self {
            CliContrast::Warn => ContrastMode::Warn,
            CliContrast::Error => ContrastMode::Error,
        }
    }
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

#[derive(Subcommand)]
enum Command {
    /// Watch notebooks (interactive server) or a directory (re-render on save)
    #[command(long_about = "Watch a notebook source and react to saves.\n\n\
            Two modes — picked by what you pass:\n\n  \
            Interactive server (a .md file, a directory, or several files; no --obsidian/--output):\n    \
            Spins up a local web server on http://127.0.0.1:8042 (auto-bumps\n    \
            up to +10 if busy), opens your browser, and live-reloads the\n    \
            page on every save via a WebSocket push. A single .md serves one\n    \
            notebook; a directory serves every .md under it behind an index\n    \
            page (one URL per notebook). Several files (.md notebooks and\n    \
            .rlab scripts) are served together under their common folder,\n    \
            in command-line order; the browser opens the first one. A .rlab\n    \
            script is a read-only page that runs the script as one cell.\n    \
            Directories cannot be mixed with files. KaTeX + Plotly assets are embedded\n    \
            in the binary so the page works fully offline. Small edits ship\n    \
            as block-level partial diffs and preserve scroll position;\n    \
            structural edits fall back to a full refresh. Every code block\n    \
            has a ▶ Run button that force-re-executes it and everything\n    \
            below it (upstream replays from cache). Source .md is never\n    \
            modified — unless you pass --editable (in-browser editor +\n    \
            inline cell editing: ✎ Edit a block, Shift+Enter writes it\n    \
            back into the .md and runs it).\n\n  \
            Re-render on save (directory + --obsidian or --output):\n    \
            Long-running counterpart of `render`. Re-renders any notebook\n    \
            whose source changes, debouncing fs events. Pairs with\n    \
            --obsidian for an Obsidian Editing/Reading view loop.\n\n\
            Examples:\n  \
            rustlab-notebook watch analysis.md                             # interactive server (one notebook)\n  \
            rustlab-notebook watch notebooks/                              # interactive server (whole directory + index)\n  \
            rustlab-notebook watch intro.md ch2/filters.md demo.rlab       # interactive server (these files, common folder)\n  \
            rustlab-notebook watch analysis.md --port 9000                 # custom port (fails loud on collision)\n  \
            rustlab-notebook watch analysis.md --no-browser                # don't auto-open the browser\n  \
            rustlab-notebook watch analysis.md --editable                  # edit the .md in the browser (writes back)\n  \
            rustlab-notebook watch notebooks/ --obsidian                   # re-render on save, vault-friendly in-place\n  \
            rustlab-notebook watch notebooks/ -o vault/ --obsidian         # re-render on save, vault-native two-dir\n  \
            rustlab-notebook watch notebooks/ --debounce-ms 500            # quieter editor, slower triggers\n\n\
            Re-render-on-save is markdown-only currently.\n\n\
            A directory watch shows a file browser of file names, together with the section outline, in one Contents drawer (also `\\`). It does not reserve width. Pages are /n/<relative-path> without .md. An old /n/<stem> redirects only when that name is unique; otherwise the page lists the matches. A single file stays /n/<stem> and has the outline drawer, not the file browser.\n\n\
            Several files are browsed the same way, as a tree rooted at their common folder: /n/ch2/filters for ch2/filters.md, /n/demo.rlab for a script. Only the listed files are served. Each notebook's file I/O is jailed to its own folder (--jail-root widens it). A missing path, a directory among files, or another file type is an error.")]
    Watch {
        /// A notebook .md file, a directory of notebooks, or several .md /
        /// .rlab files (interactive server). The re-render-on-save mode
        /// (--obsidian / --output) takes one directory.
        #[arg(required = true, value_name = "PATH")]
        input: Vec<PathBuf>,
        /// Output directory (default: same as input). Setting --output or
        /// --obsidian switches off the interactive server and runs the
        /// existing re-render-on-save flow instead.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// HTML and watch theme: a builtin (mocha|macchiato|frappe|latte;
        /// aliases dark, light) or a theme file name from <project>/themes
        /// or ~/.config/rustlab/themes. Overrides frontmatter `theme:`,
        /// rustlab.toml, and ~/.rustlabrc [notebook] theme (default mocha).
        /// LaTeX and PDF ignore this and always use Catppuccin Latte.
        #[arg(short = 't', long, value_name = "THEME", conflicts_with = "theme_file")]
        theme: Option<String>,
        /// Use this TOML theme file. Same precedence as --theme.
        #[arg(long, value_name = "PATH", conflicts_with = "theme")]
        theme_file: Option<PathBuf>,
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
        /// The default opens it unless `CI` is set (no TTY check).
        #[arg(long)]
        no_browser: bool,
        /// (interactive server mode only) Enable the in-browser editor:
        /// a split-pane CodeMirror editor whose saves write back to the
        /// source .md, plus inline cell editing (✎ Edit on each code
        /// block; Shift+Enter saves the block into the .md and runs it).
        /// This is the one interactive path that modifies source
        /// (parallels --obsidian), so it is strictly opt-in.
        #[arg(long)]
        editable: bool,
        /// Widen the path jail for notebook file I/O (`load`, `save`,
        /// `savefig`, `saveanim`, `run`, embeds). Default: the watched
        /// directory, or each notebook's own directory for one or several
        /// files. Paths that resolve outside the jail fail with
        /// "path escapes notebook directory".
        #[arg(long, value_name = "DIR")]
        jail_root: Option<PathBuf>,
        /// Show highlights and comments. `on` (default for watch) or `off`.
        /// `--no-comments` is the same as `--comments off`.
        #[arg(long, value_name = "on|off", num_args = 0..=1, default_missing_value = "on", conflicts_with = "no_comments")]
        comments: Option<String>,
        /// Hide highlights and comments (the markup stays so the page
        /// checkbox can reveal it). Same as `--comments off`.
        #[arg(long = "no-comments", conflicts_with = "comments")]
        no_comments: bool,
        /// Right-click menu to highlight and comment. Implied by `--editable`.
        /// Does not turn on the source editor.
        #[arg(long)]
        annotate: bool,
        /// Enable git comparison. Off unless this flag or `[notebook] git_diff`
        /// is set. Git reads the repository config; use it only on a
        /// repository you trust.
        #[arg(long)]
        git_diff: bool,
        /// Default before side once git diff is enabled: each page opens
        /// compared with REF. Bare `--diff` uses HEAD (uncommitted changes).
        /// Does not enable git by itself. The menu can change it per page
        /// (`?changes=uncommitted|branch|last|custom&base=<ref>&compare=<ref>`).
        /// Ignored by the re-render-on-save watcher.
        #[arg(long, num_args = 0..=1, default_missing_value = "HEAD", value_name = "REF")]
        diff: Option<String>,
        /// Let `cache enable` open or create the SQLite function cache.
        /// Overrides `[cache] enabled = false`.
        #[arg(long, conflicts_with = "no_cache")]
        cache: bool,
        /// Do not open the SQLite function cache, even when `[cache] enabled`
        /// is true.
        #[arg(long, conflicts_with = "cache")]
        no_cache: bool,
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
    /// Unwrap highlights and drop comments, leaving GitHub-ready Markdown.
    ///
    /// Does not execute the notebook. Code fences, inline code, and math
    /// are copied unchanged. `==text==` becomes `text`. Every `%%` comment
    /// (inline, block, cell, reply) is removed.
    #[command(
        long_about = "Unwrap ==highlights== and delete %%comments%% from notebook Markdown.\n\n\
            The notebook is not executed. Fenced code, inline code, and math are copied\n\
            unchanged, so `a == b` and a string containing %% stay literal.\n\n\
            A file is not modified unless you pass --in-place or -o. With neither flag,\n\
            cleaned Markdown is written to stdout (and a terminal is told the file was\n\
            left unchanged). This is not `clean`: `clean` removes the generated header\n\
            and output sentinels, and it leaves %% comments in the file.\n\n\
            Examples:\n  \
            rustlab-notebook strip note.md                 # cleaned Markdown on stdout\n  \
            rustlab-notebook strip note.md -o clean.md\n  \
            rustlab-notebook strip note.md --in-place      # rewrite note.md\n  \
            rustlab-notebook strip notebooks/ -o out/      # directory needs -o or --in-place"
    )]
    Strip {
        /// Input .md file or directory of .md files (recursive).
        input: PathBuf,
        /// Write here instead of stdout. A directory input writes each
        /// file under this directory, keeping relative paths.
        #[arg(short, long, conflicts_with = "in_place")]
        output: Option<PathBuf>,
        /// Replace the input file (or each file under a directory).
        #[arg(long, conflicts_with = "output")]
        in_place: bool,
    },
    /// Render a notebook (or a directory of notebooks) to HTML, Markdown, LaTeX, PDF, or JSON
    #[command(
        long_about = "Render a notebook (or a directory of notebooks) to HTML, Markdown, LaTeX, PDF, or JSON.\n\n\
            Examples:\n  \
            rustlab-notebook render analysis.md                    # → analysis.html (dark theme)\n  \
            rustlab-notebook render analysis.md -t light           # → analysis.html (latte)\n  \
            rustlab-notebook render analysis.md -t macchiato       # → Catppuccin Macchiato\n  \
            rustlab-notebook render analysis.md -f pdf             # → analysis.pdf (always Latte)\n  \
            rustlab-notebook render analysis.md -f latex           # → analysis.tex + SVG plots\n  \
            rustlab-notebook render analysis.md -f markdown        # → analysis.md + SVG plots\n  \
            rustlab-notebook render analysis.md -f json            # JSON on stdout (single file)\n  \
            rustlab-notebook render analysis.md -o out.html        # custom output path\n  \
            rustlab-notebook render notebooks/                     # render all .md → .html + index\n  \
            rustlab-notebook render notebooks/ -f pdf              # all notebooks → Latte PDF\n  \
            rustlab-notebook render analysis.md --no-toc --no-comments --index /notebooks/ Notebooks\n  \
            rustlab-notebook render analysis.md --strict            # exit 1 if a cell errors\n  \
            rustlab-notebook render analysis.md --theme-file course.toml --theme-contrast=error\n\n\
            Options:\n  \
            -o, --output <PATH>    Output file or directory (default: <input_stem>.<ext>)\n  \
            -f, --format <FMT>     html (default), latex, pdf, markdown, json\n  \
            -t, --theme  <THEME>   HTML/watch theme: a builtin (mocha|macchiato|frappe|latte;\n                                 \
                                   aliases dark, light) or a theme file name. Default:\n                                 \
                                   frontmatter theme:, rustlab.toml, ~/.rustlabrc, mocha.\n                                 \
                                   LaTeX and PDF are always Latte on white paper.\n      \
                --theme-file <PATH>    TOML theme file instead of a name.\n      \
                --theme-contrast <warn|error>  error: exit 1 before rendering when the\n                                 \
                                   theme fails a WCAG contrast check (default warn).\n\n\
            Formats:\n  \
            html      Self-contained HTML with Plotly charts and KaTeX math (default)\n  \
            markdown  GitHub-friendly .md with inline SVG plots\n  \
            latex     LaTeX .tex file + SVG plots in plots/<name>/ directory\n  \
            pdf       Compile LaTeX to PDF (always Latte on white paper; requires pdflatex or tectonic)\n  \
            json      JSON on stdout for one notebook (--stdin, --cwd, --pretty). No --output file.\n\n\
            Fences tagged bash, python, toml, yaml (or yml), or text are highlighted (text is uncolored). A fence with no tag is text.\n\
            Directory render shows a fixed file browser. Directory watch shows that browser and the section outline in a Contents drawer (also `\\`) that does not reserve width. A single file, LaTeX, and PDF do not show a file browser.\n\n\
            Themes:\n  \
            mocha / dark (default)  Catppuccin Mocha\n  \
            macchiato               Catppuccin Macchiato\n  \
            frappe                  Catppuccin Frappé\n  \
            latte / light           Catppuccin Latte"
    )]
    Render {
        /// Input .md file or directory of .md files
        input: PathBuf,
        /// Output file or directory (default: <input_stem>.<ext> or same directory)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Output format: html (default), latex, pdf, markdown, json
        #[arg(short, long, value_enum, default_value = "html")]
        format: CliFormat,
        /// HTML and watch theme: a builtin (mocha|macchiato|frappe|latte;
        /// aliases dark, light) or a theme file name from <project>/themes
        /// or ~/.config/rustlab/themes. Overrides frontmatter `theme:`,
        /// rustlab.toml, and ~/.rustlabrc [notebook] theme (default mocha).
        /// LaTeX and PDF ignore this and always use Catppuccin Latte.
        #[arg(short = 't', long, value_name = "THEME", conflicts_with = "theme_file")]
        theme: Option<String>,
        /// Use this TOML theme file. Same precedence as --theme.
        #[arg(long, value_name = "PATH", conflicts_with = "theme")]
        theme_file: Option<PathBuf>,
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
        /// Widen the path jail for notebook file I/O (`load`, `save`,
        /// `savefig`, `saveanim`, `run`, embeds). Default: the input
        /// directory in directory mode, or the notebook's own directory
        /// for a single file. Paths that resolve outside the jail fail
        /// with "path escapes notebook directory".
        #[arg(long, value_name = "DIR")]
        jail_root: Option<PathBuf>,
        /// Show highlights and comments. `on` or `off`. Bare `--comments`
        /// means on. HTML, markdown, and JSON default to on; LaTeX and
        /// PDF default to off. Overrides `[notebook] comments` in the rc file.
        #[arg(long, value_name = "on|off", num_args = 0..=1, default_missing_value = "on", conflicts_with = "no_comments")]
        comments: Option<String>,
        /// Hide highlights and comments. Same as `--comments off`.
        /// HTML omits the marks and the checkbox. PDF is already off
        /// unless `--comments` is passed.
        #[arg(long = "no-comments", conflicts_with = "comments")]
        no_comments: bool,
        /// Markdown only. `keep` (default) leaves `==` and `%%`.
        /// `footnotes` turns notes into GitHub footnotes and highlights
        /// into `<mark>`. `callouts` turns notes into `> [!note]` and
        /// leaves `==` so Obsidian still shows highlights. Cannot be
        /// combined with `--no-comments`.
        #[arg(long, value_name = "keep|footnotes|callouts")]
        comments_style: Option<String>,
        /// Parent link on a single HTML notebook. The first value is the
        /// href (`/notebooks/` or `https://…`). The optional second value
        /// is the label (default: Index). Refused for a directory and for
        /// every format other than HTML. `javascript:`, `data:`,
        /// `vbscript:`, and `blob:` are rejected and no file is written.
        #[arg(long, num_args = 1..=2, value_names = ["HREF", "LABEL"])]
        index: Option<Vec<String>>,
        /// Exit with status 1 when any code cell errors. The notebook is
        /// still written, and the summary still reports the error count.
        /// Off by default, so a render whose cells fail keeps exit status 0.
        #[arg(long)]
        strict: bool,
        /// `warn` (default) prints each failing contrast pair and renders.
        /// `error` exits 1 when the theme fails any pair. Unrelated to
        /// --strict, which is about cell errors.
        #[arg(long, value_enum, value_name = "warn|error", default_value = "warn")]
        theme_contrast: CliContrast,
        /// Omit the heading sidebar. Single HTML notebook only.
        #[arg(long)]
        no_toc: bool,
        /// Let `cache enable` open or create the SQLite function cache.
        /// Overrides `[cache] enabled = false`.
        #[arg(long, conflicts_with = "no_cache")]
        cache: bool,
        /// Do not open the SQLite function cache, even when `[cache] enabled`
        /// is true.
        #[arg(long, conflicts_with = "cache")]
        no_cache: bool,
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
    /// List, inspect, and check notebook themes (builtins and TOML files).
    #[command(
        subcommand,
        long_about = "List, inspect, and check notebook themes.\n\n\
            A theme is a builtin (mocha, macchiato, frappe, latte; aliases dark, light) or a\n\
            TOML file NAME.toml in <project>/themes (the directory holding the nearest\n\
            rustlab.toml, or its [notebook] theme_dir) or ~/.config/rustlab/themes\n\
            ($XDG_CONFIG_HOME/rustlab/themes). A file sets `inherits` and overrides\n\
            [palette], [colors], [typography], [shape], and [space]. See docs/notebooks.md.\n\n\
            Examples:\n  \
            rustlab-notebook theme list\n  \
            rustlab-notebook theme show course\n  \
            rustlab-notebook theme check course          # exit 1 on a failing pair\n  \
            rustlab-notebook theme check themes/course.toml"
    )]
    Theme(ThemeCommands),
}

#[derive(Subcommand)]
enum ThemeCommands {
    /// Builtins and discovered theme files, with where each came from.
    List(ThemeDirArgs),
    /// The resolved tokens of a theme, as CSS declarations.
    Show {
        /// Theme name, or a path to a .toml file.
        theme: String,
        #[command(flatten)]
        dir: ThemeDirArgs,
    },
    /// Validate a theme and print its WCAG contrast table. Exit 1 on a
    /// validation error or any failing pair.
    Check {
        /// Theme name, or a path to a .toml file.
        theme: String,
        #[command(flatten)]
        dir: ThemeDirArgs,
    },
}

#[derive(clap::Args, Clone)]
struct ThemeDirArgs {
    /// Where the rustlab.toml search starts (default: current directory).
    #[arg(long, value_name = "DIR")]
    dir: Option<PathBuf>,
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

fn apply_cache_policy(cache: bool, no_cache: bool, settings: &UserSettings) {
    let cli = if no_cache {
        Some(false)
    } else if cache {
        Some(true)
    } else {
        None
    };
    rustlab_script::set_host_persistent_cache(Some(rustlab_script::resolve_persistent_cache(
        cli,
        settings.cache_enabled(),
    )));
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
    // Parse first so `--help` / `--version` still work when the rc file
    // is missing or invalid. Settings load after clap returns.
    let cli = Cli::parse();
    let (settings, rc_path) = match load_and_apply_user_config() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rustlab-notebook: {e:#}");
            std::process::exit(2);
        }
    };
    // Default for validate/check and any path that does not take the flags.
    // Render and watch override this from `--cache` / `--no-cache`.
    rustlab_script::set_host_persistent_cache(Some(settings.cache_enabled()));
    match cli.command {
        Command::Watch {
            input,
            output,
            theme,
            theme_file,
            obsidian,
            attachments_dir,
            no_iframe,
            debounce_ms,
            port,
            no_browser,
            editable,
            jail_root,
            comments,
            no_comments,
            annotate,
            git_diff,
            diff,
            cache,
            no_cache,
        } => {
            apply_cache_policy(cache, no_cache, &settings);
            let inputs = input;
            let input = inputs[0].clone();
            if inputs.len() > 1 && (obsidian || output.is_some()) {
                eprintln!(
                    "error: --obsidian / --output re-render one directory; several paths need the interactive server (drop --obsidian / --output)"
                );
                std::process::exit(2);
            }
            let colors = resolve_theme_or_exit(
                &input,
                theme,
                theme_file,
                &settings,
                rc_path.as_deref(),
                ContrastMode::Warn,
            );
            // Explicit --jail-root: install on this thread for the re-render
            // loop; the interactive server threads it through ServerOpts.
            let _jail = cli_jail_guard(jail_root.as_ref(), &inputs);

            // Bare `watch <input>` (no --obsidian, no --output) spins up
            // the interactive server: a single .md file serves one
            // notebook; a directory serves every notebook under it behind
            // an index page; several files are served together under
            // their common folder. Any other combination falls through to
            // the existing two-dir / obsidian render loop.
            // Per dev/plans/notebook_interactive_server.md locked-in #1.
            if !obsidian && output.is_none() {
                let set = match rustlab_notebook::server::fileset::resolve_watch_inputs(&inputs) {
                    Ok(set) => set,
                    Err(e) => {
                        eprintln!("error: {e:#}");
                        std::process::exit(2);
                    }
                };
                if attachments_dir.is_some() || no_iframe {
                    eprintln!(
                        "warning: --attachments-dir / --no-iframe only apply with --obsidian; ignored"
                    );
                }
                let comments_on =
                    resolve_comments(comments.as_deref(), no_comments, &settings).unwrap_or(true);
                let git_on = git_diff || settings.notebook_git_diff();
                let opts = rustlab_notebook::server::ServerOpts {
                    port,
                    no_browser,
                    editable,
                    jail_root,
                    annotate: annotate || editable,
                    comments_on,
                    diff,
                    git_diff: git_on,
                };
                if let Err(e) = rustlab_notebook::server::start_set(set, colors, opts) {
                    eprintln!("rustlab-notebook watch: {e:#}");
                    std::process::exit(1);
                }
                return;
            }

            if port.is_some() || no_browser || editable || annotate || diff.is_some() || git_diff {
                eprintln!(
                    "warning: --port / --no-browser / --editable / --annotate / --diff / --git-diff only apply to the bare-input interactive server; ignored",
                );
            }
            rustlab_notebook::set_comment_display(resolve_comments(
                comments.as_deref(),
                no_comments,
                &settings,
            ));

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
            theme_file,
            title,
            obsidian,
            attachments_dir,
            no_iframe,
            stdin,
            cwd,
            pretty,
            jail_root,
            comments,
            no_comments,
            comments_style,
            index,
            no_toc,
            strict,
            theme_contrast,
            cache,
            no_cache,
        } => {
            apply_cache_policy(cache, no_cache, &settings);
            rustlab_notebook::set_render_strict(strict);
            let project_from = match (&cwd, stdin) {
                (Some(d), _) => d.clone(),
                (None, true) => PathBuf::from("."),
                (None, false) => input.clone(),
            };
            let colors = resolve_theme_or_exit(
                &project_from,
                theme,
                theme_file,
                &settings,
                rc_path.as_deref(),
                theme_contrast.mode(),
            );
            let (index_href, index_label) = match index {
                Some(values) => (values.first().cloned(), values.get(1).cloned()),
                None => (None, None),
            };
            let chrome = rustlab_notebook::SingleHtmlOpts {
                index_href,
                index_label,
                no_toc,
            };
            if let Err(e) = chrome.check(input.is_dir(), format == CliFormat::Html) {
                eprintln!("error: {e}");
                std::process::exit(2);
            }
            let comments_on = resolve_comments(comments.as_deref(), no_comments, &settings);
            rustlab_notebook::set_comment_display(comments_on);
            if let Err(e) = resolve_comments_style(
                comments_style.as_deref(),
                comments_on == Some(false),
                format == CliFormat::Markdown,
            ) {
                eprintln!("error: {e}");
                std::process::exit(2);
            }
            // Explicit --jail-root applies to every render path below
            // (single file, directory, JSON); directory renders otherwise
            // default to the collection root inside cmd_render_dir.
            let _jail = cli_jail_guard(jail_root.as_ref(), std::slice::from_ref(&input));

            // JSON has stdout-only IO semantics (no output path, optional)
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
                eprintln!(
                    "warning: --stdin / --cwd / --pretty only apply to --format json; ignored"
                );
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
            let result = if input.is_dir() {
                rustlab_notebook::cmd_render_dir(input, output, format, colors, title)
            } else {
                if title.is_some() {
                    eprintln!("warning: --title is only used when rendering a directory; ignored for single-file input");
                }
                rustlab_notebook::cmd_render_with(input, output, format, colors, &chrome)
            };
            if let Err(e) = result {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Command::Clean {
            input,
            output,
            check,
        } => {
            let changed = rustlab_notebook::cmd_clean(input, output, check);
            if check && changed > 0 {
                std::process::exit(1);
            }
        }
        Command::Strip {
            input,
            output,
            in_place,
        } => {
            if let Err(e) = rustlab_notebook::cmd_strip(input, output, in_place) {
                eprintln!("error: {e}");
                std::process::exit(2);
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
            use rustlab_notebook::validate::{cmd_validate, ReportFormat, ValidateOpts};
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
        Command::Theme(cmd) => {
            let code = run_theme_command(cmd);
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
            "no cache file at {} (pass --cache, or set [cache] enabled = true, then render a notebook with `cache enable`; or pass --store PATH)",
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

fn load_and_apply_user_config() -> anyhow::Result<(UserSettings, Option<PathBuf>)> {
    let loaded = rustlab_config::load()?;
    let where_ = loaded
        .source
        .path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "rustlab config".to_string());
    for key in &loaded.unknown_keys {
        eprintln!("warning: {where_}: unknown setting '{key}' (ignored)");
    }
    for warning in &loaded.warnings {
        eprintln!("warning: {where_}: {warning}");
    }
    apply_process_defaults(&loaded.settings);
    let path = loaded.source.path().map(PathBuf::from);
    Ok((loaded.settings, path))
}

fn apply_process_defaults(settings: &UserSettings) {
    set_default_number_format(match settings.display_format() {
        DisplayFormat::Short => NumberFormat::Short,
        DisplayFormat::Long => NumberFormat::Long,
        DisplayFormat::Hex => NumberFormat::Hex,
        DisplayFormat::Commas => NumberFormat::Commas,
    });
    set_default_axis_y_direction(match settings.default_axis() {
        DefaultAxis::Ij => AxisYDirection::Ij,
        DefaultAxis::Xy => AxisYDirection::Xy,
    });
    // Notebook binary: page theme and un-themed savefig share notebook_theme
    // (`[notebook] theme`, else `[plot] theme`, else dark).
    // A theme-file name is resolved per command; until then use its
    // builtin ancestor, or mocha.
    set_default_theme(parse_theme(&settings.notebook_theme()).unwrap_or_default());
    // `[notebook] code` — initial source disclosure. Missing key is open.
    rustlab_notebook::render::set_rc_source_open(settings.notebook_code_open());
}

/// Install an explicit `--jail-root` for renders on this thread. The
/// directory must exist (a typo here would otherwise silently fall back
/// to the default jail and confuse the user later). Warns when `input`
/// is not inside the root, because then even the notebooks' own relative
/// paths (`savefig("x.svg")`) would be rejected.
fn cli_jail_guard(
    dir: Option<&PathBuf>,
    inputs: &[PathBuf],
) -> Option<rustlab_notebook::execute::JailRootGuard> {
    let dir = dir?;
    let root = match std::fs::canonicalize(dir) {
        Ok(p) if p.is_dir() => p,
        Ok(p) => {
            eprintln!("error: --jail-root {} is not a directory", p.display());
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("error: --jail-root {}: {e}", dir.display());
            std::process::exit(2);
        }
    };
    for input in inputs {
        if let Ok(ci) = std::fs::canonicalize(input) {
            if !ci.starts_with(&root) {
                eprintln!(
                    "warning: --jail-root {} does not contain {}; notebook-relative paths \
                     (savefig, load, …) will be rejected — pass an ancestor of the notebooks",
                    root.display(),
                    ci.display()
                );
            }
        }
    }
    Some(rustlab_notebook::execute::JailRootGuard::new(Some(root)))
}

/// `-t NAME` wins; otherwise `~/.rustlabrc` `[notebook] theme` (then
/// `[plot] theme`, then mocha). Both spell the same names, so the rc value
/// maps through the plot crate's parser rather than a second table.
/// `Some` forces comments on or off. `None` leaves the format default
/// (HTML/markdown/JSON/watch on, LaTeX/PDF off) unless the rc file set one.
fn resolve_comments(
    comments: Option<&str>,
    no_comments: bool,
    settings: &UserSettings,
) -> Option<bool> {
    if no_comments {
        return Some(false);
    }
    if let Some(s) = comments {
        return Some(match s {
            "on" => true,
            "off" => false,
            other => {
                eprintln!("error: --comments expected on or off, got {other}");
                std::process::exit(2);
            }
        });
    }
    settings.notebook_comments()
}

/// `keep` is the default and is valid on every format. `footnotes` and
/// `callouts` are markdown-only and are not a strip.
fn resolve_comments_style(
    style: Option<&str>,
    comments_off: bool,
    markdown: bool,
) -> Result<(), String> {
    let name = style.unwrap_or("keep");
    if name != "keep" {
        if !markdown {
            return Err(
                "--comments-style applies only to --format markdown (use `strip` or --no-comments)"
                    .into(),
            );
        }
        if comments_off {
            return Err(format!(
                "--comments-style={name} cannot be combined with --no-comments"
            ));
        }
    }
    rustlab_notebook::set_markdown_comment_style(Some(name))
}

/// Theme for this command, by the precedence in `theme_select`: flag, then
/// (per notebook) frontmatter, then `rustlab.toml`, then the rc file, then
/// mocha. Installs the policy renders use for frontmatter, and the plot
/// default for un-themed `savefig` (the theme's builtin ancestor).
fn resolve_theme_or_exit(
    input: &std::path::Path,
    cli_theme: Option<String>,
    cli_file: Option<PathBuf>,
    settings: &UserSettings,
    rc_path: Option<&std::path::Path>,
    contrast: ContrastMode,
) -> &'static ThemeColors {
    match resolve_theme(input, cli_theme, cli_file, settings, rc_path, contrast) {
        Ok(c) => c,
        Err((code, e)) => {
            eprintln!("error: {e}");
            std::process::exit(code);
        }
    }
}

fn resolve_theme(
    input: &std::path::Path,
    cli_theme: Option<String>,
    cli_file: Option<PathBuf>,
    settings: &UserSettings,
    rc_path: Option<&std::path::Path>,
    contrast: ContrastMode,
) -> Result<&'static ThemeColors, (i32, String)> {
    let project = load_project_config(&theme_select::project_start(input)).map_err(|e| (2, e))?;
    let search = theme_select::search_for(project.as_ref());
    let rc_set = settings.notebook.theme.is_some() || settings.plot.theme.is_some();
    let req = ThemeRequest {
        cli_theme,
        cli_file,
        project_theme: project
            .as_ref()
            .and_then(|p| p.notebook_theme.clone().map(|t| (t, p.path.clone()))),
        rc_theme: rc_set.then(|| (settings.notebook_theme(), rc_path.map(PathBuf::from))),
    };
    let loaded = theme_select::select_default(&req, &search).map_err(|e| (2, e))?;
    if let Some(root) = loaded.chain.last().and_then(|n| parse_theme(n)) {
        set_default_theme(root);
    }
    // A contrast refusal is a failed check (1), not a usage error (2).
    let colors = theme_select::activate(loaded, contrast).map_err(|e| (1, e))?;
    theme_select::install_policy(ThemePolicy {
        forced: req.forced(),
        search,
        contrast,
    });
    Ok(colors)
}

fn load_project_config(
    start: &std::path::Path,
) -> Result<Option<rustlab_config::ProjectConfig>, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let project =
        rustlab_config::load_project(start, home.as_deref()).map_err(|e| e.to_string())?;
    if let Some(p) = &project {
        for key in &p.unknown_keys {
            eprintln!(
                "warning: {}: unknown setting '{key}' (ignored)",
                p.path.display()
            );
        }
    }
    Ok(project)
}

fn run_theme_command(cmd: ThemeCommands) -> i32 {
    let dir_of = |d: &ThemeDirArgs| d.dir.clone().unwrap_or_else(|| PathBuf::from("."));
    let start = match &cmd {
        ThemeCommands::List(d) => dir_of(d),
        ThemeCommands::Show { dir, .. } | ThemeCommands::Check { dir, .. } => dir_of(dir),
    };
    let project = match load_project_config(&start) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    let search = theme_select::search_for(project.as_ref());
    let mut out = std::io::stdout();
    match cmd {
        ThemeCommands::List(_) => theme_select::cmd_theme_list(&search, &mut out),
        ThemeCommands::Show { theme, .. } => {
            theme_select::cmd_theme_show(&theme, &search, &mut out)
        }
        ThemeCommands::Check { theme, .. } => {
            theme_select::cmd_theme_check(&theme, &search, &mut out)
        }
    }
}
