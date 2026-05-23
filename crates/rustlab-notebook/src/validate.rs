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
pub fn cmd_validate(input: PathBuf, opts: ValidateOpts) -> ValidateOutcome {
    let fixtures = collect_fixtures(&input);
    let tmp_root = tempfile::Builder::new()
        .prefix("rustlab-validate.")
        .tempdir()
        .expect("failed to create temp dir");

    // `keep_tmp` works by intentionally leaking the TempDir guard so
    // the directory survives process exit; we print the path so the
    // user can inspect.
    let tmp_path = tmp_root.path().to_path_buf();
    if opts.keep_tmp {
        let _leaked = tmp_root.keep();
        eprintln!("→ keeping render dir for inspection: {}", tmp_path.display());
    } else {
        // Move the TempDir into an inner scope so it drops at the end.
        // We can't easily do that without restructuring; instead we'll
        // explicitly drop it after the loop completes by holding it
        // in `keep_alive` (named to make intent obvious).
        let _keep_alive = tmp_root;
        return run_loop(&fixtures, &tmp_path, &opts);
    }

    run_loop(&fixtures, &tmp_path, &opts)
}

fn run_loop(fixtures: &[PathBuf], tmp_path: &Path, opts: &ValidateOpts) -> ValidateOutcome {
    let renderer = renderer_command();
    let mut results: Vec<Finding> = Vec::new();

    for src in fixtures {
        let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();
        let fdir = tmp_path.join(&stem);
        if std::fs::create_dir_all(&fdir).is_err() {
            results.push(Finding {
                fixture: stem.clone(),
                format: Format::Markdown,
                linter: "render".into(),
                status: Status::Fail,
                detail: format!("could not create temp dir {}", fdir.display()),
            });
            continue;
        }

        for &fmt in &opts.formats {
            let out_path = fdir.join(format!("{stem}.{}", fmt.extension()));
            match render_one(&renderer, src, &out_path, fmt) {
                Ok(()) => {
                    let mut findings = lint(fmt, &stem, &out_path, opts);
                    results.append(&mut findings);
                }
                Err(detail) => {
                    results.push(Finding {
                        fixture: stem.clone(),
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

// ── fixture discovery ────────────────────────────────────────────────────────

fn collect_fixtures(input: &Path) -> Vec<PathBuf> {
    if input.is_file() {
        return vec![input.to_path_buf()];
    }
    if !input.is_dir() {
        return Vec::new();
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
    out
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
        let got = collect_fixtures(&f);
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
        let got = collect_fixtures(dir.path());
        let names: Vec<String> = got
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.md".to_string(), "b.md".to_string()]);
    }
}
