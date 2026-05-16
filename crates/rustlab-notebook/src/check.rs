//! Notebook linter — `rustlab-notebook check`.
//!
//! Targeted lint passes that catch rustlab-shaped failures in `.md`
//! notebook sources. Not a generic markdown validator — markdown is
//! forgiving by design and "valid" is renderer-specific, so a general
//! checker would spend more time fighting false positives than fixing
//! real bugs. Instead each check here pins one *specific* failure mode
//! that has bitten us in practice or that the renderer can't surface
//! up-front.
//!
//! Severity levels:
//! - `Error` → the renderer will produce wrong output or fail. Must
//!   fix before commit / publish.
//! - `Warning` → likely wrong, recoverable on re-render. Worth fixing.
//! - `Info` → stylistic / vault-specific. Optional.
//!
//! Exit-code mapping (used by `cmd_check`):
//! - 0 = clean (no findings, or only Info).
//! - 1 = warnings only (or any Info in `--strict`).
//! - 2 = any error.
//!
//! Auto-fix: a subset of findings are `auto_fixable`. `--fix` runs
//! `cmd_clean` over the file, which strips sentinel-bounded regions,
//! deduplicates the generated header, and removes legacy artifacts —
//! covering checks E001 and W001.

use crate::embed;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: Severity,
    /// 1-based source line, when the check can attribute one.
    pub line: Option<usize>,
    /// Stable rule code, e.g. `"rustlab:E001"`. Greppable.
    pub code: &'static str,
    pub message: String,
    /// `true` when `--fix` (i.e. `cmd_clean`) resolves this finding.
    pub auto_fixable: bool,
}

/// Run every check against `source` and return findings in source-order.
///
/// `file` is the on-disk path; used for resolving relative plot URLs.
/// `host_dir` is the source's directory; used for resolving `![[…]]`
/// references against sibling notebooks.
/// `root_dir` is the vault root (top of a `notebook watch` tree); used
/// for fallback embed resolution.
pub fn check_source(
    source: &str,
    file: &Path,
    host_dir: &Path,
    root_dir: &Path,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    findings.extend(check_unmatched_sentinels(source));
    findings.extend(check_unclosed_rustlab_fences(source));
    findings.extend(check_frontmatter_terminated(source));
    findings.extend(check_duplicate_generated_headers(source));
    findings.extend(check_mismatched_details(source));
    findings.extend(check_unresolved_embeds(source, host_dir, root_dir));
    findings.extend(check_plot_urls_resolve(source, file));
    // Stable order: by (line, code).
    findings.sort_by_key(|f| (f.line.unwrap_or(0), f.code));
    findings
}

// ── individual checks ──────────────────────────────────────────────────────

/// **E001** — Output-region sentinels (`<!-- rustlab:output-start -->` /
/// `<!-- rustlab:output-end -->`) must be paired. A dangling sentinel
/// either truncates output rendering or leaks the marker into the
/// rendered document.
///
/// Not auto-fixable: `cmd_clean` only strips *well-formed* sentinel
/// pairs; a dangling sentinel by definition has no matching partner
/// and requires the user to decide whether to delete the orphan
/// marker or re-render the notebook from clean source.
pub fn check_unmatched_sentinels(source: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut depth: i32 = 0;
    let mut last_open_line: Option<usize> = None;
    for (idx, line) in source.lines().enumerate() {
        let line_no = idx + 1;
        if line.contains(crate::OUTPUT_BLOCK_START) {
            depth += 1;
            last_open_line = Some(line_no);
        }
        if line.contains(crate::OUTPUT_BLOCK_END) {
            if depth == 0 {
                findings.push(Finding {
                    severity: Severity::Error,
                    line: Some(line_no),
                    code: "rustlab:E001",
                    message:
                        "rustlab:output-end sentinel without a matching output-start above"
                            .to_string(),
                    auto_fixable: false,
                });
            } else {
                depth -= 1;
            }
        }
    }
    if depth > 0 {
        findings.push(Finding {
            severity: Severity::Error,
            line: last_open_line,
            code: "rustlab:E001",
            message: format!(
                "{} rustlab:output-start sentinel(s) without a matching output-end below",
                depth
            ),
            auto_fixable: false,
        });
    }
    findings
}

/// **E002** — A ` ```rustlab ` code fence opened but never closed.
/// Anything below it becomes part of the rustlab block until end of
/// file, almost certainly not what the author intended.
pub fn check_unclosed_rustlab_fences(source: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut in_fence = false;
    let mut fence_lang = String::new();
    let mut fence_open_line = 0;
    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if in_fence {
                in_fence = false;
                fence_lang.clear();
            } else {
                in_fence = true;
                fence_lang = rest.trim().to_string();
                fence_open_line = idx + 1;
            }
        }
    }
    if in_fence {
        let kind = if fence_lang.is_empty() {
            "fenced".to_string()
        } else {
            format!("`{fence_lang}`")
        };
        findings.push(Finding {
            severity: Severity::Error,
            line: Some(fence_open_line),
            code: "rustlab:E002",
            message: format!("{kind} code fence opened but never closed"),
            auto_fixable: false,
        });
    }
    findings
}

/// **E003** — `![[Target]]` references must resolve through the embed
/// expander. Targets that the expander would fail to find render as
/// inline error callouts at render time; surfacing them up-front lets
/// a CI hook catch them before publish.
pub fn check_unresolved_embeds(
    source: &str,
    host_dir: &Path,
    root_dir: &Path,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (idx, line) in source.lines().enumerate() {
        for (_, _, eref) in embed::find_embed_refs_in_line(line) {
            // Image / svg / png references are vault attachments, not
            // notebook embeds. They get a separate plot-URL check.
            if !embed::is_markdown_target(&eref.target) {
                continue;
            }
            if embed::resolve_target(&eref.target, host_dir, root_dir).is_err() {
                findings.push(Finding {
                    severity: Severity::Error,
                    line: Some(idx + 1),
                    code: "rustlab:E003",
                    message: format!(
                        "unresolved embed `![[{}]]` — target file not found",
                        eref.target
                    ),
                    auto_fixable: false,
                });
            }
        }
    }
    findings
}

/// **E004** — YAML frontmatter opened with `---` at the top of the
/// file must be terminated by a second `---` line. Without it the
/// parser silently falls through and treats the body as starting with
/// the `---` line, losing every key.
pub fn check_frontmatter_terminated(source: &str) -> Vec<Finding> {
    let starts_with_dashes = source.starts_with("---\n") || source.starts_with("---\r\n");
    if !starts_with_dashes {
        return Vec::new();
    }
    let after_open = source.splitn(2, '\n').nth(1).unwrap_or("");
    let closed = after_open.lines().any(|l| l.trim() == "---");
    if closed {
        return Vec::new();
    }
    vec![Finding {
        severity: Severity::Error,
        line: Some(1),
        code: "rustlab:E004",
        message: "frontmatter opened with `---` but never closed with a second `---`"
            .to_string(),
        auto_fixable: false,
    }]
}

/// **E005** — A rendered notebook's plot URL points at a file that
/// does not exist on disk. Catches partial writes, watcher race
/// fallout, and stale references after a content edit invalidated the
/// hashed filename. Re-rendering the notebook regenerates the
/// referenced file.
pub fn check_plot_urls_resolve(source: &str, file: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let host_dir = file.parent().unwrap_or_else(|| Path::new("."));
    for (idx, line) in source.lines().enumerate() {
        for (alt, url) in parse_markdown_image_refs(line) {
            // Skip external URLs.
            if url.contains("://") || url.starts_with("data:") || url.starts_with('#') {
                continue;
            }
            // We only care about plot-shaped references — `plot-N-<hash>.svg`
            // and `anim-N-<hash>.<ext>`. Other relative image references
            // (user-authored screenshots, etc.) are out of scope for a
            // plot-linter check.
            let filename = url.rsplit('/').next().unwrap_or(url);
            if !(filename.starts_with("plot-") || filename.starts_with("anim-")) {
                continue;
            }
            let resolved = host_dir.join(url);
            if !resolved.exists() {
                findings.push(Finding {
                    severity: Severity::Error,
                    line: Some(idx + 1),
                    code: "rustlab:E005",
                    message: format!(
                        "plot reference `![{}]({})` does not exist on disk — re-render the notebook",
                        alt, url
                    ),
                    auto_fixable: false,
                });
            }
        }
    }
    findings
}

/// **W001** — More than one `<!-- Generated by rustlab-notebook -->`
/// header in the same file. The renderer emits exactly one and the
/// pre-parse strip removes prior occurrences, so duplicates indicate
/// either a hand edit or a pre-fix legacy file. Auto-fixable via
/// `cmd_clean`.
pub fn check_duplicate_generated_headers(source: &str) -> Vec<Finding> {
    let count = source.matches(crate::GENERATED_HEADER).count();
    if count <= 1 {
        return Vec::new();
    }
    vec![Finding {
        severity: Severity::Warning,
        line: None,
        code: "rustlab:W001",
        message: format!(
            "{count} `Generated by rustlab-notebook` headers in this file — expected at most 1"
        ),
        auto_fixable: true,
    }]
}

/// **W002** — `<details>` and `</details>` tag counts must balance.
/// A mismatch typically means a hand-authored disclosure widget lost
/// its closing tag.
pub fn check_mismatched_details(source: &str) -> Vec<Finding> {
    // Cheap substring count; `<details>` and `</details>` are
    // sufficiently unique that we don't need a full HTML parser.
    let opens = source.matches("<details>").count();
    let closes = source.matches("</details>").count();
    if opens == closes {
        return Vec::new();
    }
    let msg = if opens > closes {
        format!(
            "{} `<details>` opening tag(s) without matching `</details>`",
            opens - closes
        )
    } else {
        format!(
            "{} `</details>` closing tag(s) without matching `<details>`",
            closes - opens
        )
    };
    vec![Finding {
        severity: Severity::Warning,
        line: None,
        code: "rustlab:W002",
        message: msg,
        auto_fixable: false,
    }]
}

// ── helpers ────────────────────────────────────────────────────────────────

/// Parse every `![alt](url)` reference on `line`. Skips reference-style
/// links and `[text](url)` (non-image) links. Returns `(alt, url)` pairs.
fn parse_markdown_image_refs(line: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        // Find `![`.
        if bytes[i] == b'!' && i + 1 < n && bytes[i + 1] == b'[' {
            let alt_start = i + 2;
            // Match `]`, then `(`, then `)`.
            if let Some(alt_end_rel) = line[alt_start..].find(']') {
                let alt_end = alt_start + alt_end_rel;
                if alt_end + 1 < n && bytes[alt_end + 1] == b'(' {
                    let url_start = alt_end + 2;
                    if let Some(url_end_rel) = line[url_start..].find(')') {
                        let url_end = url_start + url_end_rel;
                        out.push((&line[alt_start..alt_end], &line[url_start..url_end]));
                        i = url_end + 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

/// Drive the linter over a path (file or directory). Returns the
/// aggregated findings so callers can format / count / exit-code.
pub struct LintRun {
    pub files: Vec<(PathBuf, Vec<Finding>)>,
}

impl LintRun {
    pub fn error_count(&self) -> usize {
        self.files
            .iter()
            .flat_map(|(_, fs)| fs.iter())
            .filter(|f| f.severity == Severity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.files
            .iter()
            .flat_map(|(_, fs)| fs.iter())
            .filter(|f| f.severity == Severity::Warning)
            .count()
    }

    pub fn info_count(&self) -> usize {
        self.files
            .iter()
            .flat_map(|(_, fs)| fs.iter())
            .filter(|f| f.severity == Severity::Info)
            .count()
    }

    pub fn has_auto_fixable(&self) -> bool {
        self.files
            .iter()
            .flat_map(|(_, fs)| fs.iter())
            .any(|f| f.auto_fixable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn assert_codes(findings: &[Finding], expected: &[&str]) {
        let got: Vec<&str> = findings.iter().map(|f| f.code).collect();
        assert_eq!(got, expected, "findings = {:#?}", findings);
    }

    // ── E001: unmatched sentinels ─────────────────────────────────────────

    #[test]
    fn e001_clean_paired_sentinels_no_findings() {
        let src = format!(
            "# Demo\n\n{s}\nbody\n{e}\n",
            s = crate::OUTPUT_BLOCK_START,
            e = crate::OUTPUT_BLOCK_END,
        );
        assert!(check_unmatched_sentinels(&src).is_empty());
    }

    #[test]
    fn e001_dangling_start_fires() {
        let src = format!("# Demo\n\n{}\nbody\n", crate::OUTPUT_BLOCK_START);
        assert_codes(&check_unmatched_sentinels(&src), &["rustlab:E001"]);
    }

    #[test]
    fn e001_dangling_end_fires() {
        let src = format!("# Demo\n\nbody\n{}\n", crate::OUTPUT_BLOCK_END);
        assert_codes(&check_unmatched_sentinels(&src), &["rustlab:E001"]);
    }

    // ── E002: unclosed rustlab fence ──────────────────────────────────────

    #[test]
    fn e002_clean_fence_no_findings() {
        let src = "# Demo\n\n```rustlab\nx = 1\n```\n";
        assert!(check_unclosed_rustlab_fences(src).is_empty());
    }

    #[test]
    fn e002_unclosed_rustlab_fence_fires() {
        let src = "# Demo\n\n```rustlab\nx = 1\nmore prose with no closing fence\n";
        assert_codes(&check_unclosed_rustlab_fences(src), &["rustlab:E002"]);
    }

    // ── E003: unresolved embeds ──────────────────────────────────────────

    #[test]
    fn e003_resolves_when_target_exists() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("target.md"), "x").unwrap();
        let host = dir.path().join("host.md");
        let src = "see ![[target]]\n";
        let findings = check_unresolved_embeds(src, dir.path(), dir.path());
        // Drop `host.md` from scope but pass dir.path() — embed expander walks the dir.
        let _ = host;
        assert!(findings.is_empty(), "{findings:#?}");
    }

    #[test]
    fn e003_unresolved_embed_fires() {
        let dir = TempDir::new().unwrap();
        let src = "see ![[missing]]\n";
        let findings = check_unresolved_embeds(src, dir.path(), dir.path());
        assert_codes(&findings, &["rustlab:E003"]);
    }

    // ── E004: unterminated frontmatter ───────────────────────────────────

    #[test]
    fn e004_clean_frontmatter_no_findings() {
        let src = "---\ntitle: ok\n---\n\n# Body\n";
        assert!(check_frontmatter_terminated(src).is_empty());
    }

    #[test]
    fn e004_unterminated_frontmatter_fires() {
        let src = "---\ntitle: oops never closes\n\n# Body\n";
        assert_codes(&check_frontmatter_terminated(src), &["rustlab:E004"]);
    }

    #[test]
    fn e004_no_frontmatter_no_findings() {
        // A leading `---` without newline isn't frontmatter — it's a
        // horizontal rule. Don't false-positive.
        let src = "# Title\n\nbody\n";
        assert!(check_frontmatter_terminated(src).is_empty());
    }

    // ── E005: plot URL resolution ────────────────────────────────────────

    #[test]
    fn e005_existing_plot_no_finding() {
        let dir = TempDir::new().unwrap();
        let plots = dir.path().join("plots/note");
        fs::create_dir_all(&plots).unwrap();
        fs::write(plots.join("plot-1-deadbeef.svg"), "<svg/>").unwrap();
        let note = dir.path().join("note.md");
        let src = "![plot 1](plots/note/plot-1-deadbeef.svg)\n";
        let findings = check_plot_urls_resolve(src, &note);
        assert!(findings.is_empty(), "{findings:#?}");
    }

    #[test]
    fn e005_missing_plot_fires() {
        let dir = TempDir::new().unwrap();
        let note = dir.path().join("note.md");
        let src = "![plot 1](plots/note/plot-1-cafebabe.svg)\n";
        let findings = check_plot_urls_resolve(src, &note);
        assert_codes(&findings, &["rustlab:E005"]);
    }

    #[test]
    fn e005_user_image_ref_not_in_scope() {
        // Non-plot-shaped image references (user attachments) are not
        // checked — only `plot-…` / `anim-…` filenames are in scope.
        let dir = TempDir::new().unwrap();
        let note = dir.path().join("note.md");
        let src = "![diagram](attachments/diagram.svg)\n";
        let findings = check_plot_urls_resolve(src, &note);
        assert!(findings.is_empty());
    }

    #[test]
    fn e005_external_url_skipped() {
        let dir = TempDir::new().unwrap();
        let note = dir.path().join("note.md");
        let src = "![remote](https://example.com/img.png)\n";
        let findings = check_plot_urls_resolve(src, &note);
        assert!(findings.is_empty());
    }

    // ── W001: duplicate generated headers ────────────────────────────────

    #[test]
    fn w001_single_header_no_findings() {
        let src = format!("{}\n\n# Body\n", crate::GENERATED_HEADER);
        assert!(check_duplicate_generated_headers(&src).is_empty());
    }

    #[test]
    fn w001_duplicate_headers_fires() {
        let src = format!(
            "{h}\n\n{h}\n\n# Body\n",
            h = crate::GENERATED_HEADER
        );
        assert_codes(&check_duplicate_generated_headers(&src), &["rustlab:W001"]);
    }

    // ── W002: mismatched details ─────────────────────────────────────────

    #[test]
    fn w002_balanced_details_no_findings() {
        let src = "<details>\n<summary>x</summary>\nbody\n</details>\n";
        assert!(check_mismatched_details(src).is_empty());
    }

    #[test]
    fn w002_missing_close_fires() {
        let src = "<details>\n<summary>x</summary>\nbody (no close)\n";
        assert_codes(&check_mismatched_details(src), &["rustlab:W002"]);
    }

    #[test]
    fn w002_orphan_close_fires() {
        let src = "spurious </details>\n";
        assert_codes(&check_mismatched_details(src), &["rustlab:W002"]);
    }

    // ── parser ───────────────────────────────────────────────────────────

    #[test]
    fn parse_image_refs_basic() {
        let refs = parse_markdown_image_refs("![alt](url)");
        assert_eq!(refs, vec![("alt", "url")]);
    }

    #[test]
    fn parse_image_refs_ignores_plain_links() {
        let refs = parse_markdown_image_refs("[text](https://example.com) and ![real](img.svg)");
        assert_eq!(refs, vec![("real", "img.svg")]);
    }

    // ── check_source aggregator ──────────────────────────────────────────

    #[test]
    fn check_source_returns_findings_sorted_by_line() {
        let dir = TempDir::new().unwrap();
        let note = dir.path().join("n.md");
        // Two checks fire: E004 on line 1 (unterminated frontmatter), and
        // E001 on a later line.
        let src = format!(
            "---\ntitle: oops\n\n# Body\n\n{}\n",
            crate::OUTPUT_BLOCK_START,
        );
        let findings = check_source(&src, &note, dir.path(), dir.path());
        let codes: Vec<&str> = findings.iter().map(|f| f.code).collect();
        assert!(codes.contains(&"rustlab:E001"));
        assert!(codes.contains(&"rustlab:E004"));
        // Sorted by (line, code) — E004 (line 1) before E001 (later).
        let e004_pos = codes.iter().position(|c| *c == "rustlab:E004").unwrap();
        let e001_pos = codes.iter().position(|c| *c == "rustlab:E001").unwrap();
        assert!(e004_pos < e001_pos, "findings out of order: {codes:?}");
    }
}
