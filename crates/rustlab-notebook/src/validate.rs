//! Output-side validation for the notebook renderer.
//!
//! Renders one or more `.md` notebook sources through every requested
//! output format (html / markdown / latex / pdf) and pipes each rendered
//! artefact through a trusted external linter. The complementary
//! source-side linter is `cmd_check` in [`crate::check`].
//!
//! Designed to be a drop-in CI check for downstream projects that ship
//! rustlab-notebook sources (rustlab_em, etc.): one binary, one command,
//! no script vendoring. Catches output-side regressions (broken HTML,
//! malformed LaTeX, unparseable PDFs) that source-side checks cannot
//! see.
//!
//! Linter selection per format:
//! - **markdown** → `markdownlint-cli2` (or `markdownlint` fallback)
//! - **html** → `vnu` (via `$VNU_JAR` or PATH) → `tidy-html5` (5.x+;
//!   macOS's 2006 HTML4 `tidy` is detected and SKIPped)
//! - **latex** → `chktex`
//! - **pdf** → `pdfinfo` + `pdftotext` (smoke), `qpdf --check`
//!   (structure), and opt-in `verapdf` (PDF/A conformance via
//!   `--pdf-a`)
//!
//! Each linter is shelled out via [`std::process::Command`] only when
//! installed; otherwise it reports `Skip` with an install hint. Set
//! `require_all = true` to upgrade any `Skip` to a hard failure (CI
//! mode). Override a linter's binary path with `--linter <key>=<path>`
//! when the tool isn't on PATH.
//!
//! ## Process isolation
//!
//! Rendering is delegated to a subprocess invocation of the *current
//! binary* (`std::env::current_exe()`) running `render`. One notebook's
//! pdflatex hang or panic cannot take down the validate run; the
//! render's stdout/stderr is captured and surfaces as a `render` linter
//! failure if the exit code is non-zero.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

/// Output format under validation. Mirrors [`crate::Format`] but is
/// Copy/Eq so we can use it freely in maps and CLI parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Markdown,
    Html,
    Latex,
    Pdf,
}

impl Format {
    pub fn extension(&self) -> &'static str {
        match self {
            Format::Markdown => "md",
            Format::Html => "html",
            Format::Latex => "tex",
            Format::Pdf => "pdf",
        }
    }

    pub fn render_flag(&self) -> &'static str {
        match self {
            Format::Markdown => "markdown",
            Format::Html => "html",
            Format::Latex => "latex",
            Format::Pdf => "pdf",
        }
    }

    pub fn all() -> Vec<Format> {
        vec![Format::Markdown, Format::Html, Format::Latex, Format::Pdf]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Status {
    Ok,
    Fail,
    Skip,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub fixture: String,
    pub format: Format,
    pub linter: String,
    pub status: Status,
    pub detail: String,
}

/// Caller-facing options. Construct from CLI args or a downstream
/// project's wrapper.
#[derive(Debug, Clone)]
pub struct ValidateOpts {
    pub formats: Vec<Format>,
    pub report: ReportFormat,
    pub require_all: bool,
    pub pdf_a: bool,
    pub keep_tmp: bool,
    /// Per-linter binary overrides (key matches the linter's lookup
    /// name: `markdownlint-cli2`, `markdownlint`, `vnu`, `tidy`,
    /// `chktex`, `pdfinfo`, `pdftotext`, `qpdf`, `verapdf`). For `vnu`,
    /// `$VNU_JAR` is also honored.
    pub linter_overrides: HashMap<String, PathBuf>,
}

impl Default for ValidateOpts {
    fn default() -> Self {
        Self {
            formats: Format::all(),
            report: ReportFormat::Text,
            require_all: false,
            pdf_a: false,
            keep_tmp: false,
            linter_overrides: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub pass: usize,
    pub fail: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidateOutcome {
    pub schema_version: u32,
    pub summary: Summary,
    pub results: Vec<Finding>,
}

impl ValidateOutcome {
    /// Process exit code matching the validator contract:
    /// - 0 = clean (or only Skip)
    /// - 1 = any Fail
    /// - 2 = `require_all` set and any Skip
    pub fn exit_code(&self, require_all: bool) -> i32 {
        if self.summary.fail > 0 {
            1
        } else if require_all && self.summary.skipped > 0 {
            2
        } else {
            0
        }
    }

    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "  {:7}  {:22}  {:8}  {:22}  {}\n",
            "STATUS", "FIXTURE", "FORMAT", "LINTER", "DETAIL"
        ));
        out.push_str(&format!(
            "  {:7}  {:22}  {:8}  {:22}  {}\n",
            "------",
            "----------------------",
            "--------",
            "----------------------",
            "------",
        ));
        for f in &self.results {
            let status = match f.status {
                Status::Ok => "OK",
                Status::Fail => "FAIL",
                Status::Skip => "SKIP",
            };
            out.push_str(&format!(
                "  {:7}  {:22}  {:8}  {:22}  {}\n",
                status,
                f.fixture,
                f.format.render_flag(),
                f.linter,
                f.detail,
            ));
        }
        out.push_str(&format!(
            "\n── summary ───────────────────────────────────────────\n  pass:    {}\n  fail:    {}\n  skipped: {}\n",
            self.summary.pass, self.summary.fail, self.summary.skipped,
        ));
        out
    }

    pub fn render_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Drive validation: collect fixtures, render each into a temp dir for
/// every requested format, pipe each artefact through the matching
/// linter, return findings.
///
/// On any render FAIL the temp directory is auto-preserved (with its
/// path printed to stderr) so the user can inspect the `<stem>.log`
/// pdflatex build log that `compile_pdf` writes next to a failed PDF.
/// Pass `keep_tmp = true` to preserve unconditionally.
pub fn cmd_validate(input: PathBuf, opts: ValidateOpts) -> ValidateOutcome {
    // Validate the input path up front: a typo or missing directory
    // would otherwise produce an empty findings set and exit 0,
    // making CI report green for work that never ran.
    let fixtures = match collect_fixtures(&input) {
        Ok(v) => v,
        Err(detail) => {
            return ValidateOutcome {
                schema_version: 1,
                summary: Summary { pass: 0, fail: 1, skipped: 0 },
                results: vec![Finding {
                    fixture: input.display().to_string(),
                    format: Format::Markdown,
                    linter: "input".into(),
                    status: Status::Fail,
                    detail,
                }],
            };
        }
    };

    let tmp_root = tempfile::Builder::new()
        .prefix("rustlab-validate.")
        .tempdir()
        .expect("failed to create temp dir");
    let tmp_path = tmp_root.path().to_path_buf();

    let outcome = run_loop(&input, &fixtures, &tmp_path, &opts);

    // Decide whether to keep the temp dir:
    //   - explicit --keep-tmp: always keep
    //   - any FAIL: keep automatically so the user can inspect logs
    //     (especially the pdflatex `<stem>.log` written next to a
    //     failed PDF)
    let auto_keep = outcome.summary.fail > 0 && !opts.keep_tmp;
    if opts.keep_tmp {
        let _leaked = tmp_root.keep();
        eprintln!(
            "→ keeping render dir for inspection: {}",
            tmp_path.display()
        );
    } else if auto_keep {
        let _leaked = tmp_root.keep();
        eprintln!(
            "→ {} failure(s) — keeping render dir for inspection: {}",
            outcome.summary.fail,
            tmp_path.display(),
        );
    }
    // Otherwise: tmp_root drops here, dir is cleaned up.

    outcome
}

fn run_loop(
    input: &Path,
    fixtures: &[PathBuf],
    tmp_path: &Path,
    opts: &ValidateOpts,
) -> ValidateOutcome {
    let renderer = renderer_command();
    let mut results: Vec<Finding> = Vec::new();
    let input_root = fixture_root(input);

    for src in fixtures {
        let display = fixture_display_name(src, &input_root);
        // Mirror the relative path under the temp dir so two fixtures
        // with the same file_stem in different subdirectories don't
        // overwrite each other (e.g. notebooks/foo.md vs
        // notebooks/sub/foo.md both rendering to `<tmp>/foo/foo.html`).
        let fdir = tmp_path.join(&display);
        if std::fs::create_dir_all(&fdir).is_err() {
            results.push(Finding {
                fixture: display.clone(),
                format: Format::Markdown,
                linter: "render".into(),
                status: Status::Fail,
                detail: format!("could not create temp dir {}", fdir.display()),
            });
            continue;
        }

        let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("out");

        for &fmt in &opts.formats {
            let out_path = fdir.join(format!("{stem}.{}", fmt.extension()));
            match render_one(&renderer, src, &out_path, fmt) {
                Ok(()) => {
                    let mut findings = lint(fmt, &display, &out_path, opts);
                    results.append(&mut findings);
                }
                Err(detail) => {
                    results.push(Finding {
                        fixture: display.clone(),
                        format: fmt,
                        linter: "render".into(),
                        status: Status::Fail,
                        detail,
                    });
                }
            }
        }
    }

    let mut summary = Summary { pass: 0, fail: 0, skipped: 0 };
    for f in &results {
        match f.status {
            Status::Ok => summary.pass += 1,
            Status::Fail => summary.fail += 1,
            Status::Skip => summary.skipped += 1,
        }
    }
    ValidateOutcome { schema_version: 1, summary, results }
}

/// Root used to derive a fixture's relative display name. For a single
/// file input we use its parent directory; for a directory input we
/// use the directory itself.
fn fixture_root(input: &Path) -> PathBuf {
    if input.is_dir() {
        input.to_path_buf()
    } else {
        input
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

/// Stable, collision-free name for a fixture, derived from its path
/// relative to `input_root` with the `.md` extension stripped. Used as
/// both the user-visible `fixture` label in findings AND the
/// per-fixture subdirectory under the temp render root.
fn fixture_display_name(fixture: &Path, input_root: &Path) -> String {
    let rel = fixture.strip_prefix(input_root).unwrap_or(fixture);
    rel.with_extension("").to_string_lossy().into_owned()
}

// ── fixture discovery ────────────────────────────────────────────────────────

fn collect_fixtures(input: &Path) -> Result<Vec<PathBuf>, String> {
    if input.is_file() {
        // Single-file mode: must actually be a markdown notebook source.
        // A non-`.md` file would silently render-fail with a cryptic
        // detail; surface the type error explicitly instead.
        if input.extension().and_then(|s| s.to_str()) != Some("md") {
            return Err(format!(
                "input `{}` is not a .md notebook (validate accepts a single .md file or a directory of .md files)",
                input.display()
            ));
        }
        return Ok(vec![input.to_path_buf()]);
    }
    if !input.is_dir() {
        // Missing path. The previous behaviour silently returned an
        // empty list, producing 0/0/0 + exit 0 — CI would report
        // green for a fixture path that doesn't exist.
        return Err(format!(
            "input `{}` does not exist (no such file or directory)",
            input.display()
        ));
    }
    // Walk recursively, collect `.md` files, skip README.md.
    let mut out = Vec::new();
    let mut stack = vec![input.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("md")
                && path.file_name().and_then(|s| s.to_str()) != Some("README.md")
            {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

// ── render dispatch ──────────────────────────────────────────────────────────

/// Path to the binary used for rendering. By default, the currently
/// running executable invokes itself; downstream test fixtures can
/// override via the `RUSTLAB_NOTEBOOK_BIN` env var (handy when the
/// validate test runs from `cargo test` and `current_exe()` is the
/// test harness rather than the production binary).
fn renderer_command() -> PathBuf {
    if let Ok(p) = std::env::var("RUSTLAB_NOTEBOOK_BIN") {
        return PathBuf::from(p);
    }
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rustlab-notebook"))
}

fn render_one(bin: &Path, src: &Path, out_path: &Path, fmt: Format) -> Result<(), String> {
    let output = Command::new(bin)
        .arg("render")
        .arg(src)
        .arg("--format")
        .arg(fmt.render_flag())
        .arg("--output")
        .arg(out_path)
        .output()
        .map_err(|e| format!("could not spawn renderer: {e}"))?;
    if !output.status.success() {
        // Pull the last meaningful line of stderr for the detail field.
        let stderr = String::from_utf8_lossy(&output.stderr);
        let last = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("renderer failed")
            .trim()
            .to_string();
        return Err(last);
    }
    Ok(())
}

// ── linter dispatch ──────────────────────────────────────────────────────────

fn lint(fmt: Format, fixture: &str, file: &Path, opts: &ValidateOpts) -> Vec<Finding> {
    match fmt {
        Format::Markdown => vec![lint_markdown(fixture, file, opts)],
        Format::Html => vec![lint_html(fixture, file, opts)],
        Format::Latex => vec![lint_latex(fixture, file, opts)],
        Format::Pdf => lint_pdf(fixture, file, opts),
    }
}

/// Look up a tool binary: honor `--linter` overrides first, then fall
/// back to PATH. Returns `None` when the tool isn't installed.
fn locate(name: &str, opts: &ValidateOpts) -> Option<PathBuf> {
    if let Some(p) = opts.linter_overrides.get(name) {
        return Some(p.clone());
    }
    which_binary(name)
}

fn which_binary(name: &str) -> Option<PathBuf> {
    let out = Command::new("which").arg(name).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() { None } else { Some(PathBuf::from(path)) }
}

fn finding(
    fixture: &str,
    format: Format,
    linter: &str,
    status: Status,
    detail: impl Into<String>,
) -> Finding {
    Finding {
        fixture: fixture.to_string(),
        format,
        linter: linter.to_string(),
        status,
        detail: detail.into(),
    }
}

// ── markdown ─────────────────────────────────────────────────────────────────

fn lint_markdown(fixture: &str, file: &Path, opts: &ValidateOpts) -> Finding {
    if let Some(bin) = locate("markdownlint-cli2", opts) {
        return run_simple_linter(
            &bin,
            &[file.as_os_str()],
            fixture,
            Format::Markdown,
            "markdownlint-cli2",
            |out| out.lines().last().unwrap_or("").to_string(),
        );
    }
    if let Some(bin) = locate("markdownlint", opts) {
        return run_simple_linter(
            &bin,
            &[file.as_os_str()],
            fixture,
            Format::Markdown,
            "markdownlint",
            |out| out.lines().last().unwrap_or("").to_string(),
        );
    }
    finding(
        fixture,
        Format::Markdown,
        "markdownlint-cli2",
        Status::Skip,
        "install: npm i -g markdownlint-cli2",
    )
}

// ── html ─────────────────────────────────────────────────────────────────────

fn lint_html(fixture: &str, file: &Path, opts: &ValidateOpts) -> Finding {
    // Prefer vnu (W3C Nu) when a jar path is configured.
    if let Some(jar) = vnu_jar(opts) {
        let out = Command::new("java")
            .arg("-jar")
            .arg(&jar)
            .arg("--errors-only")
            .arg(file)
            .output();
        return interpret_html_exit(out, fixture, "vnu");
    }
    if let Some(bin) = locate("vnu", opts) {
        let out = Command::new(&bin).arg("--errors-only").arg(file).output();
        return interpret_html_exit(out, fixture, "vnu");
    }
    if let Some(bin) = locate("tidy", opts) {
        if is_html5_tidy(&bin) {
            // tidy exits 0=clean, 1=warnings-only, 2=errors. Accept 0 and 1
            // because the embedded Plotly bundle emits legitimately-warning
            // HTML (vendor attributes) we cannot fix.
            let out = Command::new(&bin)
                .arg("-errors")
                .arg("-quiet")
                .arg(file)
                .output();
            return match out {
                Ok(out) => {
                    let code = out.status.code().unwrap_or(0);
                    if code <= 1 {
                        finding(fixture, Format::Html, "tidy-html5", Status::Ok, "")
                    } else {
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let line = stderr.lines().chain(stdout.lines())
                            .find(|l| l.starts_with("line "))
                            .unwrap_or("")
                            .to_string();
                        finding(fixture, Format::Html, "tidy-html5", Status::Fail, line)
                    }
                }
                Err(e) => finding(fixture, Format::Html, "tidy-html5", Status::Fail, format!("spawn error: {e}")),
            };
        }
        // Fall through — old tidy gets SKIPped.
    }
    finding(
        fixture,
        Format::Html,
        "vnu",
        Status::Skip,
        "install: brew install vnu (or set VNU_JAR), or brew install tidy-html5",
    )
}

fn vnu_jar(opts: &ValidateOpts) -> Option<PathBuf> {
    if let Some(p) = opts.linter_overrides.get("vnu") {
        // If the override looks like a .jar file, use it as the jar path.
        if p.extension().and_then(|s| s.to_str()) == Some("jar") {
            return Some(p.clone());
        }
    }
    if let Ok(p) = std::env::var("VNU_JAR") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    None
}

fn is_html5_tidy(bin: &Path) -> bool {
    // Banner variants we need to recognise:
    //   macOS brew tidy-html5  → "HTML Tidy for HTML5 (Mac OS X 64-bit) version 5.8.0"
    //   Ubuntu apt tidy 5.6.0  → "HTML Tidy for Linux version 5.6.0"   (no "HTML5"!)
    //   macOS 2006 /usr/bin/tidy → "HTML Tidy for Mac OS X released on 31 October 2006 …"
    //
    // Accept anything that mentions "HTML Tidy" AND either advertises
    // HTML5 explicitly OR reports a 5.x / 6.x version number. macOS's
    // 2006-vintage tidy has neither and stays SKIPped (it rejects
    // every HTML5 tag).
    Command::new(bin)
        .arg("--version")
        .output()
        .ok()
        .map(|o| {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr),
            );
            combined.contains("HTML Tidy")
                && (combined.contains("HTML5")
                    || combined.contains("version 5.")
                    || combined.contains("version 6."))
        })
        .unwrap_or(false)
}

fn interpret_html_exit(
    out: std::io::Result<std::process::Output>,
    fixture: &str,
    linter: &str,
) -> Finding {
    match out {
        Ok(out) if out.status.success() => {
            finding(fixture, Format::Html, linter, Status::Ok, "")
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let first = stderr.lines().next().unwrap_or("").to_string();
            finding(fixture, Format::Html, linter, Status::Fail, first)
        }
        Err(e) => finding(fixture, Format::Html, linter, Status::Fail, format!("spawn error: {e}")),
    }
}

// ── latex ────────────────────────────────────────────────────────────────────

fn lint_latex(fixture: &str, file: &Path, opts: &ValidateOpts) -> Finding {
    let Some(bin) = locate("chktex", opts) else {
        return finding(
            fixture,
            Format::Latex,
            "chktex",
            Status::Skip,
            "install: brew install chktex (or TeX Live)",
        );
    };
    // chktex's own exit code is 0 even on findings; count Warning/Error
    // lines for the FAIL signal.
    // chktex's own exit code is 0 even on findings, so we count its
    // Warning/Error lines. Treat *errors* as FAIL (real LaTeX syntax
    // problems) and warnings as OK — chktex warnings are stylistic
    // (straight quotes, double spaces, math-spacing) and inevitable in
    // notebook-renderer output, where titles and captions come from
    // user-authored prose. Symmetric with the tidy-html5 path that
    // tolerates Plotly bundle warnings.
    let out = Command::new(&bin).arg("-q").arg(file).output();
    match out {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let warnings = stdout.lines().filter(|l| l.starts_with("Warning")).count();
            let errors = stdout.lines().filter(|l| l.starts_with("Error")).count();
            if errors == 0 {
                let detail = if warnings > 0 {
                    format!("{warnings} warning(s) (ignored)")
                } else {
                    String::new()
                };
                finding(fixture, Format::Latex, "chktex", Status::Ok, detail)
            } else {
                finding(
                    fixture,
                    Format::Latex,
                    "chktex",
                    Status::Fail,
                    format!("{errors} error(s), {warnings} warning(s)"),
                )
            }
        }
        Err(e) => finding(fixture, Format::Latex, "chktex", Status::Fail, format!("spawn error: {e}")),
    }
}

// ── pdf ──────────────────────────────────────────────────────────────────────

fn lint_pdf(fixture: &str, file: &Path, opts: &ValidateOpts) -> Vec<Finding> {
    let mut out = Vec::new();
    out.push(lint_pdf_smoke(fixture, file, opts));
    out.push(lint_pdf_qpdf(fixture, file, opts));
    if opts.pdf_a {
        out.push(lint_pdf_verapdf(fixture, file, opts));
    }
    out
}

fn lint_pdf_smoke(fixture: &str, file: &Path, opts: &ValidateOpts) -> Finding {
    let pdfinfo = locate("pdfinfo", opts);
    let pdftotext = locate("pdftotext", opts);
    if pdfinfo.is_none() && pdftotext.is_none() {
        return finding(
            fixture,
            Format::Pdf,
            "pdfinfo+pdftotext",
            Status::Skip,
            "install: brew install poppler (apt: poppler-utils)",
        );
    }

    if let Some(bin) = pdfinfo {
        let out = Command::new(&bin).arg(file).output();
        match out {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let pages: Option<u32> = stdout
                    .lines()
                    .find_map(|l| l.strip_prefix("Pages:"))
                    .and_then(|s| s.trim().parse().ok());
                if pages.unwrap_or(0) < 1 {
                    return finding(
                        fixture,
                        Format::Pdf,
                        "pdfinfo+pdftotext",
                        Status::Fail,
                        "pdfinfo: zero pages",
                    );
                }
            }
            Ok(_) => {
                return finding(
                    fixture,
                    Format::Pdf,
                    "pdfinfo+pdftotext",
                    Status::Fail,
                    "pdfinfo: parse failed",
                );
            }
            Err(e) => {
                return finding(
                    fixture,
                    Format::Pdf,
                    "pdfinfo+pdftotext",
                    Status::Fail,
                    format!("pdfinfo: spawn error: {e}"),
                );
            }
        }
    }

    if let Some(bin) = pdftotext {
        let out = Command::new(&bin).arg(file).arg("-").output();
        match out {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                let stripped: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                if stripped.is_empty() {
                    return finding(
                        fixture,
                        Format::Pdf,
                        "pdfinfo+pdftotext",
                        Status::Fail,
                        "pdftotext: empty extraction",
                    );
                }
            }
            _ => {
                // pdftotext failure is treated as smoke-fail too.
                return finding(
                    fixture,
                    Format::Pdf,
                    "pdfinfo+pdftotext",
                    Status::Fail,
                    "pdftotext: extraction failed",
                );
            }
        }
    }

    finding(fixture, Format::Pdf, "pdfinfo+pdftotext", Status::Ok, "")
}

fn lint_pdf_qpdf(fixture: &str, file: &Path, opts: &ValidateOpts) -> Finding {
    let Some(bin) = locate("qpdf", opts) else {
        return finding(
            fixture,
            Format::Pdf,
            "qpdf",
            Status::Skip,
            "install: brew install qpdf (apt: qpdf)",
        );
    };
    // qpdf exit codes: 0 = no errors/warnings, 2 = errors, 3 = warnings.
    // Treat 0 and 3 as OK (warnings include advisories we don't gate on).
    let out = Command::new(&bin).arg("--check").arg(file).output();
    match out {
        Ok(out) => {
            let code = out.status.code().unwrap_or(0);
            if code == 0 || code == 3 {
                finding(fixture, Format::Pdf, "qpdf", Status::Ok, "")
            } else {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let line = stdout
                    .lines()
                    .find(|l| l.to_lowercase().contains("error"))
                    .unwrap_or("qpdf reported errors")
                    .to_string();
                finding(fixture, Format::Pdf, "qpdf", Status::Fail, line)
            }
        }
        Err(e) => finding(fixture, Format::Pdf, "qpdf", Status::Fail, format!("spawn error: {e}")),
    }
}

fn lint_pdf_verapdf(fixture: &str, file: &Path, opts: &ValidateOpts) -> Finding {
    let Some(bin) = locate("verapdf", opts) else {
        return finding(
            fixture,
            Format::Pdf,
            "verapdf(PDF/A)",
            Status::Skip,
            "install: https://verapdf.org/software/ (Java + jar)",
        );
    };
    let out = Command::new(&bin)
        .arg("--format")
        .arg("text")
        .arg(file)
        .output();
    match out {
        Ok(out) if out.status.success() => {
            finding(fixture, Format::Pdf, "verapdf(PDF/A)", Status::Ok, "")
        }
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let last = stdout.lines().last().unwrap_or("").to_string();
            finding(fixture, Format::Pdf, "verapdf(PDF/A)", Status::Fail, last)
        }
        Err(e) => finding(
            fixture,
            Format::Pdf,
            "verapdf(PDF/A)",
            Status::Fail,
            format!("spawn error: {e}"),
        ),
    }
}

// ── shared helper ────────────────────────────────────────────────────────────

fn run_simple_linter(
    bin: &Path,
    args: &[&std::ffi::OsStr],
    fixture: &str,
    format: Format,
    linter: &str,
    summarise_failure: impl FnOnce(&str) -> String,
) -> Finding {
    let out = Command::new(bin).args(args).output();
    match out {
        Ok(out) if out.status.success() => finding(fixture, format, linter, Status::Ok, ""),
        Ok(out) => {
            let combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
            let detail = summarise_failure(&combined);
            finding(fixture, format, linter, Status::Fail, detail)
        }
        Err(e) => finding(fixture, format, linter, Status::Fail, format!("spawn error: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixtures_dir() -> PathBuf {
        let manifest = env!("CARGO_MANIFEST_DIR");
        PathBuf::from(manifest).join("tests/fixtures/bad")
    }

    fn opts_default() -> ValidateOpts {
        ValidateOpts::default()
    }

    // ── lint_pdf_smoke ────────────────────────────────────────────────────

    #[test]
    fn lint_pdf_smoke_rejects_invalid_pdf() {
        // Only meaningful when pdfinfo is installed; otherwise SKIPped
        // and the assertion that "this isn't OK" still holds.
        let bad = fixtures_dir().join("bad.pdf");
        if !bad.exists() {
            return; // fixtures not provisioned in this build env
        }
        let f = lint_pdf_smoke("bad", &bad, &opts_default());
        assert_ne!(f.status, Status::Ok, "smoke check accepted a non-PDF: {f:?}");
    }

    // ── is_html5_tidy banner detection ────────────────────────────────────
    //
    // Spawn a tiny shell script that prints a fixed banner and exits, so we
    // can exercise the version-string match against every banner variant we
    // care about without depending on the host's actual tidy install.

    #[cfg(unix)]
    fn fake_tidy(banner: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new()
            .prefix("fake-tidy-")
            .tempfile()
            .unwrap();
        // Echo to stdout regardless of args.
        writeln!(f, "#!/bin/sh\ncat <<'EOF'\n{banner}\nEOF\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        f
    }

    #[test]
    #[cfg(unix)]
    fn is_html5_tidy_accepts_ubuntu_5_6_banner() {
        // Ubuntu 24.04 (noble) ships tidy 5.6.0 with this exact banner —
        // notice it has no "HTML5" substring, which is why the original
        // check skipped tidy on every Linux CI run.
        let fake = fake_tidy("HTML Tidy for Linux version 5.6.0");
        assert!(is_html5_tidy(fake.path()));
    }

    #[test]
    #[cfg(unix)]
    fn is_html5_tidy_accepts_brew_html5_banner() {
        let fake = fake_tidy("HTML Tidy for HTML5 (Mac OS X 64-bit) version 5.8.0");
        assert!(is_html5_tidy(fake.path()));
    }

    #[test]
    #[cfg(unix)]
    fn is_html5_tidy_rejects_macos_2006_banner() {
        // /usr/bin/tidy on every macOS install — HTML4 era, rejects <nav>,
        // <main>, <footer>, etc. Must NOT be detected as html5-capable.
        let fake = fake_tidy(
            "HTML Tidy for Mac OS X released on 31 October 2006 - Apple Inc. build 11418",
        );
        assert!(!is_html5_tidy(fake.path()));
    }

    #[test]
    #[cfg(unix)]
    fn is_html5_tidy_rejects_unrelated_binary() {
        let fake = fake_tidy("some other tool v1.2.3");
        assert!(!is_html5_tidy(fake.path()));
    }

    // ── lint_latex via fake chktex ────────────────────────────────────────

    #[cfg(unix)]
    fn fake_chktex(script: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new()
            .prefix("fake-chktex-")
            .tempfile()
            .unwrap();
        writeln!(f, "#!/bin/sh\n{script}").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        f
    }

    #[test]
    #[cfg(unix)]
    fn lint_latex_warnings_only_returns_ok_with_ignored_detail() {
        // chktex's exit code is 0 even on findings, so we count
        // Warning/Error lines. Warnings-only must yield OK (with a
        // detail noting the warning count for visibility) — matching
        // the tidy-html5 wrapper that already tolerates Plotly bundle
        // warnings. Real LaTeX syntax bugs are reported as Error.
        let fake = fake_chktex(
            "cat <<'EOF'\nWarning 1 in foo.tex line 1: Use \\(...\\) instead\nWarning 2 in foo.tex line 2: Use \\(...\\) instead\nEOF",
        );
        let mut opts = opts_default();
        opts.linter_overrides.insert("chktex".into(), fake.path().to_path_buf());
        let f = lint_latex("test", Path::new("/dev/null"), &opts);
        assert_eq!(f.status, Status::Ok, "{f:?}");
        assert!(f.detail.contains("2 warning"), "{f:?}");
    }

    #[test]
    #[cfg(unix)]
    fn lint_latex_errors_still_fail() {
        let fake = fake_chktex("cat <<'EOF'\nError 1 in foo.tex line 1: Unmatched brace\nEOF");
        let mut opts = opts_default();
        opts.linter_overrides.insert("chktex".into(), fake.path().to_path_buf());
        let f = lint_latex("test", Path::new("/dev/null"), &opts);
        assert_eq!(f.status, Status::Fail, "{f:?}");
        assert!(f.detail.contains("1 error"), "{f:?}");
    }

    #[test]
    #[cfg(unix)]
    fn lint_latex_clean_run_returns_ok_no_detail() {
        let fake = fake_chktex(":"); // no-op, no output
        let mut opts = opts_default();
        opts.linter_overrides.insert("chktex".into(), fake.path().to_path_buf());
        let f = lint_latex("test", Path::new("/dev/null"), &opts);
        assert_eq!(f.status, Status::Ok, "{f:?}");
        assert_eq!(f.detail, "");
    }

    // ── locate() / linter overrides ───────────────────────────────────────

    #[test]
    fn locate_prefers_override_over_path() {
        let mut opts = opts_default();
        let fake = PathBuf::from("/nonexistent/markdownlint-cli2");
        opts.linter_overrides.insert("markdownlint-cli2".into(), fake.clone());
        assert_eq!(locate("markdownlint-cli2", &opts), Some(fake));
    }

    #[test]
    fn locate_falls_back_to_path() {
        let opts = opts_default();
        // `which` itself should always be on PATH on Unix.
        let found = locate("which", &opts);
        assert!(found.is_some(), "which not found on PATH?");
    }

    // ── exit code mapping ─────────────────────────────────────────────────

    #[test]
    fn exit_code_zero_when_clean() {
        let o = ValidateOutcome {
            schema_version: 1,
            summary: Summary { pass: 3, fail: 0, skipped: 0 },
            results: vec![],
        };
        assert_eq!(o.exit_code(false), 0);
        assert_eq!(o.exit_code(true), 0);
    }

    #[test]
    fn exit_code_one_on_fail_regardless_of_require_all() {
        let o = ValidateOutcome {
            schema_version: 1,
            summary: Summary { pass: 1, fail: 2, skipped: 1 },
            results: vec![],
        };
        assert_eq!(o.exit_code(false), 1);
        assert_eq!(o.exit_code(true), 1);
    }

    #[test]
    fn exit_code_two_when_require_all_and_skipped() {
        let o = ValidateOutcome {
            schema_version: 1,
            summary: Summary { pass: 1, fail: 0, skipped: 2 },
            results: vec![],
        };
        assert_eq!(o.exit_code(false), 0);
        assert_eq!(o.exit_code(true), 2);
    }

    // ── reporters ─────────────────────────────────────────────────────────

    #[test]
    fn render_json_has_schema_version_and_summary() {
        let o = ValidateOutcome {
            schema_version: 1,
            summary: Summary { pass: 1, fail: 1, skipped: 1 },
            results: vec![Finding {
                fixture: "x".into(),
                format: Format::Pdf,
                linter: "qpdf".into(),
                status: Status::Fail,
                detail: "bad".into(),
            }],
        };
        let json = o.render_json();
        assert!(json.contains("\"schema_version\": 1"));
        assert!(json.contains("\"summary\""));
        assert!(json.contains("\"results\""));
        assert!(json.contains("\"status\": \"FAIL\""));
        assert!(json.contains("\"format\": \"pdf\""));
    }

    #[test]
    fn render_text_table_has_header_and_summary() {
        let o = ValidateOutcome {
            schema_version: 1,
            summary: Summary { pass: 1, fail: 0, skipped: 0 },
            results: vec![Finding {
                fixture: "x".into(),
                format: Format::Html,
                linter: "tidy-html5".into(),
                status: Status::Ok,
                detail: "".into(),
            }],
        };
        let text = o.render_text();
        assert!(text.contains("STATUS"));
        assert!(text.contains("FIXTURE"));
        assert!(text.contains("LINTER"));
        assert!(text.contains("OK"));
        assert!(text.contains("pass:    1"));
    }

    // ── fixture discovery ────────────────────────────────────────────────

    #[test]
    fn collect_fixtures_single_file() {
        let f = std::env::temp_dir().join("rustlab-validate-test-single.md");
        std::fs::File::create(&f).unwrap().write_all(b"# x").unwrap();
        let got = collect_fixtures(&f).expect("single .md file is valid");
        assert_eq!(got, vec![f.clone()]);
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn collect_fixtures_directory_skips_readme() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "x").unwrap();
        std::fs::write(dir.path().join("b.md"), "y").unwrap();
        std::fs::write(dir.path().join("README.md"), "skip me").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "skip me too").unwrap();
        let got = collect_fixtures(dir.path()).expect("valid dir");
        let names: Vec<String> = got
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.md".to_string(), "b.md".to_string()]);
    }

    // ── Regression: Bug 2 — collect_fixtures must reject a missing path
    // rather than silently returning [] and exiting 0. Typo'd CI input
    // would otherwise report green for work that never ran.
    #[test]
    fn collect_fixtures_errors_on_missing_path() {
        let bogus = std::env::temp_dir().join("rustlab-validate-no-such-path-xyz");
        let _ = std::fs::remove_dir_all(&bogus);
        let _ = std::fs::remove_file(&bogus);
        let err = collect_fixtures(&bogus).unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
    }

    // ── Regression: Bug 4 — single-file mode rejects non-.md inputs
    // up front rather than letting render fail with a cryptic detail.
    #[test]
    fn collect_fixtures_errors_on_non_md_single_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("notes.txt");
        std::fs::write(&f, "not a notebook").unwrap();
        let err = collect_fixtures(&f).unwrap_err();
        assert!(err.contains("not a .md"), "{err}");
    }

    // ── cmd_validate surfaces the input error as a Fail finding ────────
    #[test]
    fn cmd_validate_returns_fail_on_missing_input() {
        let bogus = std::env::temp_dir().join("rustlab-validate-still-no-such-path");
        let _ = std::fs::remove_dir_all(&bogus);
        let _ = std::fs::remove_file(&bogus);
        let outcome = cmd_validate(bogus.clone(), ValidateOpts::default());
        assert_eq!(outcome.summary.fail, 1);
        assert_eq!(outcome.summary.pass, 0);
        assert_eq!(outcome.results.len(), 1);
        let f = &outcome.results[0];
        assert_eq!(f.status, Status::Fail);
        assert_eq!(f.linter, "input");
        assert_eq!(outcome.exit_code(false), 1);
    }

    // ── Regression: Bug 1 — fixture_display_name uses path relative to
    // the input root, so two .md files with the same file_stem in
    // different subdirectories produce distinct findings (no temp-dir
    // collision, no merged report row).
    #[test]
    fn fixture_display_name_disambiguates_same_stem_in_subdirs() {
        let root = Path::new("/some/root");
        let a = Path::new("/some/root/foo.md");
        let b = Path::new("/some/root/sub/foo.md");
        let na = fixture_display_name(a, root);
        let nb = fixture_display_name(b, root);
        assert_ne!(na, nb, "got duplicate display names: {na} vs {nb}");
        assert_eq!(na, "foo");
        // The exact separator may be platform-specific; just assert
        // it contains the disambiguating segment.
        assert!(nb.contains("foo"));
        assert!(nb.contains("sub"));
    }

    #[test]
    fn fixture_display_name_for_single_file_input_uses_just_stem() {
        // Single-file input: root = file's parent, relative = filename.
        let input = Path::new("/some/dir/foo.md");
        let root = fixture_root(input);
        assert_eq!(fixture_display_name(input, &root), "foo");
    }
}
