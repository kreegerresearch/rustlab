pub mod cache;
pub mod check;
pub mod embed;
pub mod execute;
#[cfg(feature = "mermaid")]
pub mod mermaid;
pub mod parse;
pub mod render;
pub mod render_json;
pub mod render_latex;
pub mod render_markdown;
pub mod watch;

use rustlab_plot::theme::ThemeColors;
use std::path::{Path, PathBuf};

/// Remove every `<iframe src="…" width="100%" height="600" style="border:
/// 0;"></iframe>` tag from `source`, along with surrounding blank-line
/// padding. The signature is the exact format `--obsidian` used to emit
/// in single-dir in-place mode before iframe-auto-suppression landed;
/// older files had one fresh copy added per render. Hand-authored
/// iframes that don't match all three style attributes are untouched.
fn strip_legacy_iframes(source: &str) -> String {
    const PREFIX: &str = "<iframe src=\"";
    const TAIL: &str = "\" width=\"100%\" height=\"600\" style=\"border: 0;\"></iframe>";
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0usize;
    while let Some(rel_start) = source[cursor..].find(PREFIX) {
        let start = cursor + rel_start;
        let after_prefix = start + PREFIX.len();
        let Some(rel_end) = source[after_prefix..].find(TAIL) else {
            break;
        };
        // Sanity: the bytes between prefix and tail should be the href
        // value — bail if a newline sneaks in (means we straddled tags).
        if source[after_prefix..after_prefix + rel_end].contains('\n') {
            cursor = after_prefix;
            continue;
        }
        let end = after_prefix + rel_end + TAIL.len();
        let mut after = end;
        while source[after..].starts_with('\n') {
            after += 1;
        }
        let mut before = start;
        while before > cursor && source.as_bytes()[before - 1] == b'\n' {
            before -= 1;
        }
        out.push_str(&source[cursor..before]);
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push_str("\n\n");
        }
        cursor = after;
    }
    out.push_str(&source[cursor..]);
    out
}

/// Strip every `` ```text … ``` `` block that immediately follows a
/// `` ```rustlab … ``` `` fence — the exact shape legacy in-place
/// renders left behind as captured stdout. We only match the pair so
/// hand-authored ```text fences elsewhere in the document are
/// preserved. Operates on UTF-8 byte offsets.
fn strip_legacy_text_outputs(source: &str) -> String {
    const RUSTLAB_OPEN: &str = "```rustlab\n";
    const FENCE_CLOSE: &str = "\n```";
    const TEXT_OPEN: &str = "```text\n";
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0usize;
    while let Some(rel_open) = source[cursor..].find(RUSTLAB_OPEN) {
        let rl_open = cursor + rel_open;
        let after_open = rl_open + RUSTLAB_OPEN.len();
        // Find this rustlab block's closing fence.
        let Some(rel_close) = source[after_open..].find(FENCE_CLOSE) else {
            break;
        };
        let close_start = after_open + rel_close;
        let close_end = close_start + FENCE_CLOSE.len();
        // The closing fence might be followed by `\n` or end-of-string.
        let mut scan = close_end;
        while scan < bytes.len() && bytes[scan] == b'\n' {
            scan += 1;
        }
        // Bare-text-block-follows test: the gap between the closing
        // fence and the next non-newline byte must contain *only*
        // newlines — i.e. no sentinel comment in between.
        if source[scan..].starts_with(TEXT_OPEN) {
            let text_open_start = scan;
            let text_body_start = text_open_start + TEXT_OPEN.len();
            if let Some(rel_text_close) = source[text_body_start..].find(FENCE_CLOSE) {
                let text_close_end =
                    text_body_start + rel_text_close + FENCE_CLOSE.len();
                let mut after_text = text_close_end;
                while after_text < bytes.len() && bytes[after_text] == b'\n' {
                    after_text += 1;
                }
                // Emit everything up through the rustlab close, normalise
                // to one blank line, skip the bare ```text``` entirely.
                out.push_str(&source[cursor..close_end]);
                out.push_str("\n\n");
                cursor = after_text;
                continue;
            }
        }
        // No legacy bare-text block; pass through up to the close + gap.
        out.push_str(&source[cursor..scan]);
        cursor = scan;
    }
    out.push_str(&source[cursor..]);
    out
}

/// Best-effort directory equality: canonicalize both sides when possible
/// (collapses `./foo`, symlinks, `..` segments) and fall back to literal
/// equality otherwise. Used to decide whether a render is "in-place" so
/// we can suppress the trailing iframe in `--obsidian` mode.
/// Process-wide mutex serialising every `cmd_render*` call so the
/// cwd-mutation window doesn't race with itself.
///
/// `cmd_render` changes the process cwd via `set_current_dir`. Until
/// we thread the host directory explicitly through embed expansion
/// and script evaluation (deferred refactor), serialising the render
/// itself is the cleanest way to prevent concurrent renders from
/// stomping each other's cwd. Today there's no parallel render path,
/// so the lock is effectively free. When parallel rendering lands,
/// the right fix is to remove the cwd mutation entirely; the lock
/// then becomes either obsolete or trivial to drop.
///
/// Held by `CwdGuard` for its lifetime, which is the render's entire
/// scope. `Mutex::lock()`'s poison on panic is fine — we
/// `unwrap_or_else(|e| e.into_inner())` so the next caller still
/// gets to run.
static RENDER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Inner guard — captures the cwd on construction and restores it on
/// drop. Does **not** acquire the render lock; the caller is responsible
/// for that. Production code uses the outer `CwdGuard` which bundles
/// both; tests that need to bracket a sequence with their own lock use
/// `CwdRestoreGuard` directly to avoid deadlocking on a re-entrant lock.
struct CwdRestoreGuard {
    original: Option<PathBuf>,
}

impl CwdRestoreGuard {
    fn new() -> Self {
        Self {
            original: std::env::current_dir().ok(),
        }
    }
}

impl Drop for CwdRestoreGuard {
    fn drop(&mut self) {
        if let Some(dir) = self.original.take() {
            let _ = std::env::set_current_dir(dir);
        }
    }
}

/// RAII guard that captures the process's current working directory
/// on construction and restores it on drop, *and* serialises every
/// concurrent render via `RENDER_LOCK`.
///
/// The notebook renderer changes the process cwd via
/// `std::env::set_current_dir` so that the embed expander and script
/// evaluator resolve relative paths against the notebook's parent
/// directory. `cwd` is **process-global** on Unix (no per-thread
/// cwd), so without restoring it the change leaks to anything that
/// runs after the render — including the caller, sibling commands in
/// the same `rustlab` process, and any future parallel renderer.
///
/// Usage: bind the guard at the top of a function that may call
/// `set_current_dir`. The cwd is restored when the guard drops,
/// including on early returns and panics — both render success and
/// failure paths get the same treatment.
struct CwdGuard {
    _restore: CwdRestoreGuard,
    // Holding the lock guard ties its lifetime to ours, so the lock
    // is released exactly when the cwd is restored.
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl CwdGuard {
    fn new() -> Self {
        let lock = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        Self {
            _lock: lock,
            _restore: CwdRestoreGuard::new(),
        }
    }
}

pub(crate) fn paths_equal(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// Cross-notebook navigation context passed to `render::render_html` when a
/// notebook is rendered as part of a multi-notebook directory build. All
/// fields are relative hrefs (e.g. `index.html`, `analysis.html`).
#[derive(Debug, Clone, Default)]
pub struct NotebookNav {
    pub index_href: Option<String>,
    pub prev: Option<(String, String)>,
    pub next: Option<(String, String)>,
}

/// Comment line `render_markdown` prepends to every rendered .md file.
/// Stripping it from the source before parsing keeps the markdown output
/// byte-stable across re-renders, which is what lets `notebook watch`
/// running in-place (out_dir == src_dir) converge instead of accumulating
/// one extra header line per pass.
pub(crate) const GENERATED_HEADER: &str =
    "<!-- Generated by rustlab-notebook — do not edit directly. -->";

/// Sentinel comments that wrap every rustlab code block's output region
/// (text, error, plots, animations) in markdown output. Re-rendering
/// strips these regions on the way in so we don't accumulate one extra
/// copy of every block's output per pass.
pub(crate) const OUTPUT_BLOCK_START: &str = "<!-- rustlab:output-start -->";
pub(crate) const OUTPUT_BLOCK_END: &str = "<!-- rustlab:output-end -->";

/// Strip rustlab's rendered decorations (header + all output-block
/// regions) from `source` so the parser sees the user's authored
/// content as if it had never been rendered. This is what makes
/// re-rendering byte-stable: pass 1's output, fed back as pass 2's
/// input, decomposes to the same parsed shape as the original source.
///
/// Both header iteration and output-block stripping are repeated to
/// cope with legacy files that prior buggy renders accumulated extras
/// into.
pub fn strip_render_artifacts(source: &str) -> String {
    // Header: drop every occurrence of the canonical `HEADER + "\n\n"`
    // emit, regardless of position. `--obsidian` prepends a YAML
    // frontmatter block before the header, so a prefix-only strip would
    // miss it on every pass. Replacing all occurrences also cleans up
    // legacy files that prior buggy loops accumulated multiple headers
    // into.
    let header_emit = format!("{GENERATED_HEADER}\n\n");
    let s = source.replace(&header_emit, "");
    // Cleanup pass for legacy unwrapped iframes: earlier in-place renders
    // emitted the trailing `<iframe>` without sentinel wrapping, so
    // already-loop-corrupted files have N copies of it. Match the exact
    // emission signature (width/height/style triplet) so a user-authored
    // iframe with different attributes is left alone.
    let s = strip_legacy_iframes(&s);
    // Cleanup pass for legacy text-output blocks: when the previous
    // emitter ran without sentinels, a `\`\`\`text` block followed every
    // rustlab block as its captured stdout. Re-rendering treats that as
    // ordinary prose, so a fresh sentinel-wrapped copy ends up duplicated
    // next to it. Strip the bare-text-after-rustlab pattern so legacy
    // files clean up on first load.
    let s = strip_legacy_text_outputs(&s);
    // Output blocks: strip every `OUTPUT_BLOCK_START … OUTPUT_BLOCK_END`
    // pair plus the surrounding blank-line padding the emitter inserts.
    // Unmatched start sentinels (truncated file) are left alone so we
    // don't quietly destroy user content.
    let mut out = String::with_capacity(s.len());
    let mut cursor = 0usize;
    while let Some(rel_start) = s[cursor..].find(OUTPUT_BLOCK_START) {
        let start = cursor + rel_start;
        let after_start = start + OUTPUT_BLOCK_START.len();
        match s[after_start..].find(OUTPUT_BLOCK_END) {
            Some(rel_end) => {
                let end = after_start + rel_end + OUTPUT_BLOCK_END.len();
                // Pull in trailing newlines so a stripped region collapses
                // back to a single blank-line separator (matching how the
                // emitter formats around the sentinels).
                let mut after = end;
                while s[after..].starts_with('\n') {
                    after += 1;
                }
                // And pull leading newlines before the start sentinel so
                // we don't leave a blank-line surplus. Clamp to `cursor`
                // so two adjacent regions don't eat past the previous
                // strip's tail (the panic would be `begin > end` on the
                // slice below).
                let mut before = start;
                while before > cursor && s.as_bytes()[before - 1] == b'\n' {
                    before -= 1;
                }
                out.push_str(&s[cursor..before]);
                out.push_str("\n\n");
                cursor = after;
            }
            None => break,
        }
    }
    out.push_str(&s[cursor..]);
    out
}

/// Render a single notebook file to the chosen format.
/// Strip rendered artifacts (generated-by header, output sentinels and
/// their wrapped content, legacy unwrapped iframes, legacy bare text
/// blocks left over from pre-sentinel renders) from one or more `.md`
/// files. Hand-authored prose, code fences, and unrelated HTML / iframes
/// are preserved.
///
/// `input` may be a single file or a directory; directories are walked
/// non-recursively for `.md` files (mirrors `cmd_render_dir`).
///
/// When `output` is `None`, files are cleaned in place. When `output` is
/// `Some(out)`:
///   - single file in → single file out at `out`
///   - directory in → directory out: cleaned copies written under `out`
///     using the same filenames.
///
/// In `check` mode, no files are written. The function returns the count
/// of files that would change; callers (e.g. CI) use this for an exit
/// code without modifying the tree.
/// Result of a `notebook check` run: enough for a caller to print and
/// set a process exit code.
pub struct CheckOutcome {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    pub files_scanned: usize,
    pub files_fixed: usize,
}

impl CheckOutcome {
    /// Process exit code matching the linter contract:
    ///   - 0 = clean (no findings, or only info)
    ///   - 1 = warnings (or any info under `--strict`)
    ///   - 2 = any error
    pub fn exit_code(&self, strict: bool) -> i32 {
        if self.errors > 0 {
            2
        } else if self.warnings > 0 || (strict && self.infos > 0) {
            1
        } else {
            0
        }
    }
}

/// Lint one or more notebook `.md` files for rustlab-shaped failures.
/// See `check.rs` for the catalogue of lints. When `fix` is true,
/// invokes `cmd_clean` on files whose findings include `auto_fixable`
/// entries, then re-checks them so the final report reflects what
/// remained after the fix.
pub fn cmd_check(input: PathBuf, fix: bool, strict: bool) -> CheckOutcome {
    use crate::check;
    let files = if input.is_dir() {
        list_md_files_recursive(&input)
    } else {
        vec![input.clone()]
    };

    let mut outcome = CheckOutcome {
        errors: 0,
        warnings: 0,
        infos: 0,
        files_scanned: files.len(),
        files_fixed: 0,
    };

    for file in &files {
        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: cannot read {}: {e}", file.display());
                outcome.errors += 1;
                continue;
            }
        };
        let host_dir = file.parent().unwrap_or_else(|| Path::new("."));
        // For the embed expander, treat the file's parent as the vault
        // root when scanning a single file. When scanning a directory,
        // use the directory itself.
        let root_dir = if input.is_dir() { input.as_path() } else { host_dir };
        let mut findings = check::check_source(&source, file, host_dir, root_dir);

        if fix && findings.iter().any(|f| f.auto_fixable) {
            let _ = cmd_clean(file.clone(), None, false);
            outcome.files_fixed += 1;
            // Re-lint to surface anything `cmd_clean` couldn't repair.
            if let Ok(after) = std::fs::read_to_string(file) {
                findings = check::check_source(&after, file, host_dir, root_dir);
            }
        }

        for finding in &findings {
            let loc = match finding.line {
                Some(l) => format!("{}:{}", file.display(), l),
                None => format!("{}", file.display()),
            };
            println!(
                "{loc} [{}] {}: {}",
                finding.code,
                finding.severity.as_str(),
                finding.message,
            );
            match finding.severity {
                check::Severity::Error => outcome.errors += 1,
                check::Severity::Warning => outcome.warnings += 1,
                check::Severity::Info => outcome.infos += 1,
            }
        }
    }

    println!();
    let total = outcome.errors + outcome.warnings + outcome.infos;
    if total == 0 {
        println!("✓ {} file(s) clean", outcome.files_scanned);
    } else {
        let mut parts: Vec<String> = Vec::new();
        if outcome.errors > 0 {
            parts.push(format!("{} error(s)", outcome.errors));
        }
        if outcome.warnings > 0 {
            parts.push(format!("{} warning(s)", outcome.warnings));
        }
        if outcome.infos > 0 {
            parts.push(format!("{} info", outcome.infos));
        }
        println!(
            "{} across {} file(s)",
            parts.join(", "),
            outcome.files_scanned,
        );
        if outcome.files_fixed > 0 {
            println!("({} file(s) auto-fixed via --fix)", outcome.files_fixed);
        }
        let _ = strict;
    }
    outcome
}

pub fn cmd_clean(input: PathBuf, output: Option<PathBuf>, check: bool) -> usize {
    let mut changed = 0usize;
    let files = if input.is_dir() {
        list_md_files_recursive(&input)
    } else {
        vec![input.clone()]
    };

    for src in &files {
        let original = match std::fs::read_to_string(src) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("warning: cannot read {}: {e}", src.display());
                continue;
            }
        };
        let cleaned = strip_render_artifacts(&original);
        if cleaned == original {
            continue;
        }
        changed += 1;
        if check {
            println!("would clean: {}", src.display());
            continue;
        }
        let dest = match (&output, input.is_dir()) {
            (None, _) => src.clone(),
            (Some(out_path), false) => out_path.clone(),
            (Some(out_dir), true) => {
                let rel = src.strip_prefix(&input).unwrap_or(src);
                out_dir.join(rel)
            }
        };
        write_output(&dest, cleaned.as_bytes());
        println!("cleaned: {}", dest.display());
    }
    if check {
        println!(
            "{} of {} file{} would be cleaned",
            changed,
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        );
    }
    changed
}

/// Recursively walk `dir`, collecting every `.md` file. Skips `README.md`
/// to match the renderer's "project metadata, not a notebook" rule.
fn list_md_files_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = match std::fs::read_dir(&d) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().map_or(false, |e| e == "md")
                && p.file_name().map_or(true, |n| n != "README.md")
            {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Per-render summary the watcher uses for telemetry: how many
/// executable blocks were served from cache vs total. Returned by
/// `cmd_render_cached`.
pub struct CachedRenderSummary {
    pub cached_blocks: usize,
    pub total_blocks: usize,
}

impl CachedRenderSummary {
    pub fn cache_hit(&self) -> bool {
        self.total_blocks > 0 && self.cached_blocks == self.total_blocks
    }
    pub fn cache_partial(&self) -> bool {
        self.cached_blocks > 0 && self.cached_blocks < self.total_blocks
    }
}

/// Render a single notebook file using a watcher-owned cache. Returns
/// a summary the watcher uses to log "code blocks unchanged" /
/// "N of M code blocks cached" lines so the user can see the cache
/// working.
pub fn cmd_render_cached(
    input: PathBuf,
    output: Option<PathBuf>,
    format: Format,
    theme: &ThemeColors,
    cache: &mut cache::NotebookCache,
) -> CachedRenderSummary {
    let _cwd_guard = CwdGuard::new();
    let source = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", input.display());
            return CachedRenderSummary { cached_blocks: 0, total_blocks: 0 };
        }
    };
    let source = strip_render_artifacts(&source);

    let host_dir_owned = input
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    if let Some(dir) = input.parent() {
        if !dir.as_os_str().is_empty() {
            let _ = std::env::set_current_dir(dir);
        }
    }

    let title = extract_title(&source, &input);
    let expanded = embed::expand_embeds(&source, &host_dir_owned, &host_dir_owned);
    let blocks = parse::parse_notebook(&expanded);
    let outcome = execute::execute_notebook_with_cache(&blocks, Some(cache));

    let ext = format.extension();
    let out_path = output.unwrap_or_else(|| {
        let stem = input.file_stem().unwrap_or_default();
        PathBuf::from(format!("{}.{ext}", stem.to_string_lossy()))
    });

    render_output(
        &out_path,
        &format,
        &title,
        &outcome.rendered,
        theme,
        None,
        Some(&source),
        Some(&input),
    );
    print_summary(&input, &out_path, &outcome.rendered);
    CachedRenderSummary {
        cached_blocks: outcome.cached_blocks,
        total_blocks: outcome.total_blocks,
    }
}

pub fn cmd_render(input: PathBuf, output: Option<PathBuf>, format: Format, theme: &ThemeColors) {
    let _cwd_guard = CwdGuard::new();
    let source = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", input.display());
            std::process::exit(1);
        }
    };
    let source = strip_render_artifacts(&source);

    // Canonicalize the host directory before any chdir so the embed
    // expander resolves siblings against the correct absolute location.
    let host_dir_owned = input
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    if let Some(dir) = input.parent() {
        if !dir.as_os_str().is_empty() {
            let _ = std::env::set_current_dir(dir);
        }
    }

    let title = extract_title(&source, &input);
    // Single-file render has no sibling notebook tree, so host_dir == root_dir.
    let expanded = embed::expand_embeds(&source, &host_dir_owned, &host_dir_owned);
    let blocks = parse::parse_notebook(&expanded);
    let rendered = execute::execute_notebook(&blocks);

    let ext = format.extension();
    let out_path = output.unwrap_or_else(|| {
        let stem = input.file_stem().unwrap_or_default();
        PathBuf::from(format!("{}.{ext}", stem.to_string_lossy()))
    });

    render_output(
        &out_path,
        &format,
        &title,
        &rendered,
        theme,
        None,
        Some(&source),
        Some(&input),
    );
    print_summary(&input, &out_path, &rendered);
}

/// Render a single notebook to JSON on stdout — the Phase-1 surface
/// consumed by the Obsidian community plugin and any other downstream
/// tool that wants the executed-block tree without HTML/Markdown framing.
///
/// `input` is a path (read from disk) or `None` (read from stdin).
/// `cwd` overrides the directory used to resolve relative paths
/// (embeds, frontmatter resolution); when `None`, defaults to the input
/// file's parent, or the current working directory for stdin input.
/// `pretty` controls JSON formatting — compact for piping, indented
/// for human inspection / golden tests.
///
/// Schema is documented in `render_json::Document`; the top-level
/// `version: 1` is the stability contract.
pub fn cmd_render_json(
    input: Option<PathBuf>,
    cwd: Option<PathBuf>,
    theme: &ThemeColors,
    pretty: bool,
) {
    let _cwd_guard = CwdGuard::new();

    let (source, title_path) = match &input {
        Some(path) => {
            let s = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: cannot read {}: {e}", path.display());
                    std::process::exit(1);
                }
            };
            (s, path.clone())
        }
        None => {
            use std::io::Read;
            let mut s = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut s) {
                eprintln!("error: cannot read stdin: {e}");
                std::process::exit(1);
            }
            (s, PathBuf::from("stdin.md"))
        }
    };
    let source = strip_render_artifacts(&source);

    let host_dir_owned: PathBuf = match (&cwd, &input) {
        (Some(d), _) => std::fs::canonicalize(d).unwrap_or_else(|_| d.clone()),
        (None, Some(path)) => path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        (None, None) => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let _ = std::env::set_current_dir(&host_dir_owned);

    let title = extract_title(&source, &title_path);
    let expanded = embed::expand_embeds(&source, &host_dir_owned, &host_dir_owned);
    let blocks = parse::parse_notebook(&expanded);
    let rendered = execute::execute_notebook(&blocks);

    let doc = render_json::render_json(&title, &rendered, theme);
    let json = if pretty {
        serde_json::to_string_pretty(&doc)
    } else {
        serde_json::to_string(&doc)
    }
    .expect("Document serialises (Serialize derive is infallible for our schema)");
    println!("{json}");
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
    let _cwd_guard = CwdGuard::new();
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
        let source = strip_render_artifacts(&source);
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
        let host_dir = p.md_path.parent().unwrap_or(&dir);
        let expanded = embed::expand_embeds(&p.source, host_dir, &dir);
        let blocks = parse::parse_notebook(&expanded);
        let rendered = execute::execute_notebook(&blocks);
        render_output(
            &p.out_file,
            &format,
            &p.title,
            &rendered,
            theme,
            nav.as_ref(),
            Some(&p.source),
            Some(&p.md_path),
        );
        print_summary(&p.md_path, &p.out_file, &rendered);
    }

    if matches!(format, Format::Html) {
        // Resolve the index title: CLI flag > index.md title > dir name.
        let (index_body_html, index_md_title) = match index_md_path.as_ref() {
            Some(p) => read_and_render_index_md(p, &dir, theme),
            None => (String::new(), None),
        };
        let resolved_title = index_title.clone().or(index_md_title).unwrap_or_else(|| {
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

    // Obsidian-flavored markdown gets a vault home page so users land on
    // a useful note instead of an empty file pane. If the source dir
    // already provides `index.md`, render it through the normal pipeline
    // (so it picks up frontmatter merging and embed expansion) instead
    // of overwriting it with the autogenerated wikilink list.
    if let Format::Markdown { obsidian: Some(_) } = &format {
        let index_path = out_dir.join("index.md");
        match index_md_path.as_ref() {
            Some(src_index) => {
                let source = match std::fs::read_to_string(src_index) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("warning: cannot read {}: {e}", src_index.display());
                        return;
                    }
                };
                let host_dir = src_index.parent().unwrap_or(&dir);
                let _ = std::env::set_current_dir(&dir);
                let expanded = embed::expand_embeds(&source, host_dir, &dir);
                let blocks = parse::parse_notebook(&expanded);
                let rendered = execute::execute_notebook(&blocks);
                let title = extract_title(&source, src_index);
                render_output(
                    &index_path,
                    &format,
                    &title,
                    &rendered,
                    theme,
                    None,
                    Some(&source),
                    Some(src_index),
                );
                print_summary(src_index, &index_path, &rendered);
            }
            None => {
                let title = index_title.clone().unwrap_or_else(|| {
                    dir.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                });
                let entries: Vec<(String, String)> = pending
                    .iter()
                    .map(|p| (p.title.clone(), p.filename.clone()))
                    .collect();
                let body = generate_obsidian_index_md(&title, &entries);
                let frontmatter = merge_obsidian_frontmatter("");
                let final_md = format!("{frontmatter}{body}");
                write_output(&index_path, final_md.as_bytes());
                println!(
                    "Generated {} ({} notebooks)",
                    index_path.display(),
                    pending.len()
                );
            }
        }
    }
}

/// Build the body of an autogenerated `index.md` for vault mode: an H1
/// title and a wikilink list of notebooks in render-order. Each entry
/// is `- [[<stem>|<title>]]` so Obsidian shows the human title, links
/// to the rendered note, and registers the link in the graph view.
fn generate_obsidian_index_md(title: &str, entries: &[(String, String)]) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {title}\n\n"));
    for (entry_title, filename) in entries {
        let stem = std::path::Path::new(filename)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| filename.clone());
        if entry_title == &stem {
            out.push_str(&format!("- [[{stem}]]\n"));
        } else {
            out.push_str(&format!("- [[{stem}|{entry_title}]]\n"));
        }
    }
    out.push('\n');
    out
}

/// Read `index.md`, render its markdown body to HTML, and return the body
/// plus the title used for the page. Code fences inside `index.md` are
/// rendered as plain markdown (not executed) to keep the landing page
/// lightweight — put executable content in regular notebooks and link to
/// them from `index.md`.
fn read_and_render_index_md(
    path: &PathBuf,
    dir: &PathBuf,
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
    let host_dir = path.parent().unwrap_or(dir);
    let expanded = embed::expand_embeds(&source, host_dir, dir);
    // expand_embeds already strips frontmatter at the host level.
    // Strip the first H1: it becomes the page title.
    let body_without_h1 = strip_leading_h1(&expanded).to_string();
    let mut opts = pulldown_cmark::Options::empty();
    opts.insert(pulldown_cmark::Options::ENABLE_TABLES);
    opts.insert(pulldown_cmark::Options::ENABLE_STRIKETHROUGH);
    let parser = pulldown_cmark::Parser::new_ext(&body_without_h1, opts);
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

/// Per-render Obsidian options. `Some(_)` selects vault-native markdown
/// emission: cross-notebook links become `[[wikilinks]]`, plots route
/// to `attachments_dir`, frontmatter is merged with vault metadata,
/// and (when `iframe` is true) a trailing iframe to the sibling `.html`
/// is appended.
///
/// Default: `_attachments` for plots, iframe enabled.
#[derive(Clone, Debug)]
pub struct ObsidianOpts {
    /// Subdirectory (relative to the notebook output dir) for plot SVGs.
    /// Default `"_attachments"`. The leading `_` keeps Obsidian's file
    /// pane grouped at the top, away from authored notes.
    pub attachments_dir: String,
    /// Append a trailing `<iframe src="<stem>.html">` so Obsidian's
    /// Reading view can show the interactive Plotly version inline.
    /// GitHub strips iframes during sanitization, so the same `.md` is
    /// safe to commit either way.
    pub iframe: bool,
}

impl Default for ObsidianOpts {
    fn default() -> Self {
        Self {
            attachments_dir: "_attachments".to_string(),
            iframe: true,
        }
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
    /// `obsidian: Some(_)` switches to vault-native emission (wikilinks,
    /// `_attachments/`, frontmatter injection, iframe). `None` produces
    /// the default GitHub-friendly form.
    Markdown { obsidian: Option<ObsidianOpts> },
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
    source_md: Option<&str>,
    input: Option<&Path>,
) {
    match format {
        Format::Html => {
            let (plot_dir, href_prefix) = plot_layout_for(out_path);
            let html =
                render::render_html(title, rendered, &plot_dir, &href_prefix, theme, nav);
            write_output(out_path, html.as_bytes());
        }
        Format::Markdown { obsidian } => {
            let (plot_dir, href_prefix) = match obsidian {
                Some(opts) => attachments_layout_for(out_path, &opts.attachments_dir),
                None => plot_layout_for(out_path),
            };
            // Auto-suppress the trailing iframe when rendering in place
            // (out_dir == src_dir). The iframe points at `<stem>.html`
            // sibling, which only exists in the two-dir flow where the
            // user also runs an .html render. In single-dir in-place
            // mode the sibling is missing and Obsidian's editor crashes
            // trying to resolve the URL ("Cannot read properties of
            // undefined (reading 'origin')"). Two-dir users still get
            // the iframe; `--no-iframe` still works as an explicit opt-
            // out.
            let in_place = match (input, out_path.parent()) {
                (Some(inp), Some(out_dir)) => inp
                    .parent()
                    .map(|src_dir| paths_equal(src_dir, out_dir))
                    .unwrap_or(false),
                _ => false,
            };
            let iframe_href = obsidian
                .as_ref()
                .filter(|o| o.iframe && !in_place)
                .map(|_| {
                    let stem = out_path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    format!("{stem}.html")
                });
            let link_style = match obsidian {
                Some(_) => render_markdown::LinkStyle::Wiki,
                None => render_markdown::LinkStyle::Standard,
            };
            // Suppress the `<!-- Generated -->` header on single-file
            // in-place renders: when the source IS the rendered output
            // (Obsidian editing the same .md it views in Reading mode),
            // a "do not edit directly" warning is misleading — the user
            // edits this file by design. Two-dir renders still emit it
            // so committed gallery output keeps the provenance line.
            let body = render_markdown::render_markdown(
                title,
                rendered,
                &plot_dir,
                &href_prefix,
                theme,
                iframe_href.as_deref(),
                link_style,
                !in_place,
            );
            let final_md = match obsidian {
                Some(_) => {
                    let frontmatter = merge_obsidian_frontmatter(source_md.unwrap_or(""));
                    format!("{frontmatter}{body}")
                }
                None => body,
            };
            write_output(out_path, final_md.as_bytes());
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
/// Resolve the on-disk plot directory and the relative href used by
/// the rendered document, for any [`Format`] + output path. Used by
/// the watch loop to garbage-collect stale plot files between
/// renders. Returns `None` for formats that don't produce sidecar
/// plot files (HTML and PDF are self-contained).
pub fn plot_dir_for_format(out_path: &PathBuf, format: &Format) -> Option<PathBuf> {
    match format {
        Format::Html | Format::Pdf => None,
        Format::Markdown { obsidian: Some(opts) } => {
            Some(attachments_layout_for(out_path, &opts.attachments_dir).0)
        }
        Format::Markdown { obsidian: None } | Format::Latex => {
            Some(plot_layout_for(out_path).0)
        }
    }
}

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

/// Build the merged Obsidian frontmatter block (including the leading
/// and trailing `---` lines) by reading the source's YAML frontmatter
/// and adding `tags: [rustlab]` / `cssclasses: [rustlab-notebook]` only
/// if those keys are absent. Existing keys are preserved untouched.
///
/// Returns the assembled frontmatter (always trailing `\n\n`) ready to
/// prepend to the rendered markdown body. Returns the empty string only
/// if `source` is empty *and* no obsidian additions need to be made —
/// in practice it always returns a non-empty block since we always
/// inject the two obsidian keys when they are missing.
fn merge_obsidian_frontmatter(source: &str) -> String {
    // Try to peel an existing frontmatter block off the source.
    let (existing_yaml, _body) = split_source_frontmatter(source);
    let mut lines: Vec<String> = existing_yaml
        .lines()
        .map(|l| l.to_string())
        .collect();

    let has_key = |key: &str, lines: &[String]| -> bool {
        lines
            .iter()
            .any(|l| l.trim_start().starts_with(&format!("{key}:")))
    };

    if !has_key("tags", &lines) {
        lines.push("tags: [rustlab]".to_string());
    }
    if !has_key("cssclasses", &lines) {
        lines.push("cssclasses: [rustlab-notebook]".to_string());
    }

    let mut out = String::from("---\n");
    for line in &lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("---\n\n");
    out
}

/// Peel a leading `---`-delimited YAML frontmatter block off `source`.
/// Returns `(yaml_body_without_delimiters, rest)`. If no frontmatter
/// is present, returns `("", source)`.
fn split_source_frontmatter(source: &str) -> (&str, &str) {
    let trimmed = source.trim_start_matches('\n');
    if !trimmed.starts_with("---") {
        return ("", source);
    }
    let after_open = &trimmed[3..];
    let first_nl = match after_open.find('\n') {
        Some(i) => i,
        None => return ("", source),
    };
    if !after_open[..first_nl].trim().is_empty() {
        return ("", source);
    }
    let rest = &after_open[first_nl + 1..];
    let mut consumed = 0usize;
    for line in rest.lines() {
        if line.trim() == "---" {
            let body_start = consumed + line.len();
            let body = rest.get(body_start..).unwrap_or("");
            let body = body.strip_prefix('\n').unwrap_or(body);
            let yaml = &rest[..consumed];
            // Trim trailing newline from yaml so callers don't need to.
            let yaml = yaml.strip_suffix('\n').unwrap_or(yaml);
            return (yaml, body);
        }
        consumed += line.len() + 1;
    }
    ("", source)
}

/// Vault-mode plot layout: writes plots under `<attachments_dir>/<stem>/`
/// instead of `plots/<stem>/`. `attachments_dir` is taken verbatim from
/// `ObsidianOpts` so users can override the default `_attachments`.
fn attachments_layout_for(out_path: &PathBuf, attachments_dir: &str) -> (PathBuf, String) {
    let stem = out_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let parent = out_path.parent().unwrap_or(std::path::Path::new("."));
    let plot_dir = parent.join(attachments_dir).join(&stem);
    let href_prefix = format!("{attachments_dir}/{stem}");
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

/// Process-wide cache of the hash of the last bytes `write_output`
/// successfully wrote to each path. Lets us skip the disk-read on the
/// fast path of the watcher loop: the renderer produced identical
/// bytes to last pass → we already know what's on disk → no need to
/// `std::fs::read` the file just to confirm.
///
/// Memory footprint: O(distinct paths written this process) × (path
/// length + 8 bytes). Negligible — bounded by the number of notebooks
/// in the watched vault.
///
/// Safety on external writes: if something edits the file between our
/// writes, the new render's bytes will (almost certainly) differ from
/// the cached hash anyway — the rendered output reflects the source,
/// and we re-render whenever the source changes. The narrow window
/// where this matters is the user hand-editing the rendered output
/// AND no source change AND no render-time non-determinism — at which
/// point the user is racing the watcher and is going to lose. We
/// accept that.
static WRITE_OUTPUT_HASHES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, u64>>,
> = std::sync::OnceLock::new();

fn write_output_hashes() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, u64>> {
    WRITE_OUTPUT_HASHES.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Stable-within-process hash of a byte slice. `DefaultHasher` is not
/// portable across Rust versions, but the caches that use it
/// (`write_output_hashes`, `watch::self_writes`) live only for the
/// current process — no cross-version concern.
pub(crate) fn hash_bytes(bytes: &[u8]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
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
    // Skip the write when the on-disk bytes already match `data`. Two
    // benefits: (1) `notebook watch` rendering in-place (out_dir == src_dir)
    // converges after one pass instead of self-triggering forever — the
    // second render produces byte-identical output, the write becomes a
    // no-op, no fs event fires, the loop dies; (2) untouched notebooks
    // don't churn mtimes, which keeps `git status` quiet and avoids
    // editors flagging files as externally modified.
    //
    // Fast path: we cache the hash of the bytes we last successfully
    // wrote to each path. On a repeat render that produces identical
    // bytes, the cached hash equals `hash_bytes(data)` and we can
    // return without ever touching disk — no `std::fs::read` of the
    // (potentially multi-MB) rendered output. The watcher loop is the
    // hot caller; one-shot `cmd_render` takes the slow path on first
    // write either way.
    let new_hash = hash_bytes(data);
    {
        let cache = write_output_hashes().lock().unwrap_or_else(|e| e.into_inner());
        if cache.get(path) == Some(&new_hash) {
            return;
        }
    }
    // Slow path: no cache entry, or the cached hash differs. Either we
    // haven't written this path yet in this process (cmd_render single
    // shot, or first watch iteration) or the content really changed.
    // Read the file as a defensive cross-check before deciding to
    // overwrite — handles the case where the cache is empty but the
    // on-disk file already matches what we'd write.
    if let Ok(existing) = std::fs::read(path) {
        if existing == data {
            // Memoise so the *next* repeat is a pure in-memory hit.
            let mut cache = write_output_hashes().lock().unwrap_or_else(|e| e.into_inner());
            cache.insert(path.clone(), new_hash);
            return;
        }
    }
    if let Err(e) = std::fs::write(path, data) {
        eprintln!("error: cannot write {}: {e}", path.display());
        std::process::exit(1);
    }
    let mut cache = write_output_hashes().lock().unwrap_or_else(|e| e.into_inner());
    cache.insert(path.clone(), new_hash);
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

    // write_output is the single sink for rendered .md/.html/.tex output.
    // Skipping the actual write when content is unchanged is what lets
    // `notebook watch dir/` work as an in-place Obsidian setup: the second
    // render in any potential self-trigger loop produces byte-identical
    // bytes, the write becomes a no-op, no fs event fires, and the loop
    // dies after one pass.
    #[test]
    fn strip_render_artifacts_removes_single_leading_header() {
        let src = format!("{}\n\n# Title\n\nbody\n", GENERATED_HEADER);
        let stripped = strip_render_artifacts(&src);
        assert_eq!(stripped, "# Title\n\nbody\n");
    }

    #[test]
    fn strip_render_artifacts_removes_stacked_headers_from_legacy_loops() {
        // Earlier buggy in-place renders accumulated one extra header per
        // pass. Strip cleans them all up so the next emit produces the
        // canonical single-header shape.
        let src = format!(
            "{h}\n\n{h}\n\n{h}\n\n# Title\n",
            h = GENERATED_HEADER
        );
        let stripped = strip_render_artifacts(&src);
        assert_eq!(stripped, "# Title\n");
    }

    #[test]
    fn strip_render_artifacts_leaves_user_authored_source_alone() {
        let src = "# Title\n\nbody with `<!-- something -->` inline.\n";
        let stripped = strip_render_artifacts(src);
        assert_eq!(stripped, src, "must not touch user content");
    }

    #[test]
    fn strip_render_artifacts_removes_output_block_region() {
        let src = format!(
            "# Demo\n\n```rustlab\nprint(1)\n```\n\n{s}\n```text\n1\n```\n\n{e}\n\nMore.\n",
            s = OUTPUT_BLOCK_START,
            e = OUTPUT_BLOCK_END,
        );
        let stripped = strip_render_artifacts(&src);
        assert_eq!(
            stripped,
            "# Demo\n\n```rustlab\nprint(1)\n```\n\nMore.\n",
            "output region between sentinels must be removed",
        );
    }

    #[test]
    fn strip_render_artifacts_handles_multiple_output_regions() {
        let src = format!(
            "a\n\n{s}\nx\n{e}\n\nb\n\n{s}\ny\n{e}\n\nc\n",
            s = OUTPUT_BLOCK_START,
            e = OUTPUT_BLOCK_END,
        );
        let stripped = strip_render_artifacts(&src);
        assert_eq!(stripped, "a\n\nb\n\nc\n");
    }

    #[test]
    fn strip_render_artifacts_removes_legacy_unwrapped_iframes() {
        // Regression: an Obsidian vault that ran the watcher before
        // iframe-auto-suppression accumulated one extra unwrapped iframe
        // per render. Opening the file with the current binary must
        // clean them up so the next save converges.
        let src = "# Demo\n\n\
<iframe src=\"note.html\" width=\"100%\" height=\"600\" style=\"border: 0;\"></iframe>\n\n\
<iframe src=\"note.html\" width=\"100%\" height=\"600\" style=\"border: 0;\"></iframe>\n\n\
<iframe src=\"note.html\" width=\"100%\" height=\"600\" style=\"border: 0;\"></iframe>\n\n\
More.\n";
        let stripped = strip_render_artifacts(src);
        assert_eq!(stripped.matches("<iframe").count(), 0, "all legacy iframes removed: {stripped:?}");
        assert!(stripped.contains("# Demo"));
        assert!(stripped.contains("More."));
    }

    #[test]
    fn strip_render_artifacts_leaves_user_authored_iframe_alone() {
        // A hand-authored iframe with different attributes (e.g. embedded
        // YouTube) must not be touched — only rustlab's exact signature
        // qualifies for removal.
        let src = "# Demo\n\n\
<iframe src=\"https://www.youtube.com/embed/abc\" width=\"560\" height=\"315\" frameborder=\"0\"></iframe>\n\n\
More.\n";
        let stripped = strip_render_artifacts(src);
        assert_eq!(stripped, src, "user iframe with non-rustlab attrs preserved");
    }

    #[test]
    fn strip_render_artifacts_removes_legacy_bare_text_output_after_rustlab() {
        let src = "# Demo\n\n\
```rustlab\nprint(1)\n```\n\n\
```text\n1\n```\n\n\
More.\n";
        let stripped = strip_render_artifacts(src);
        assert_eq!(
            stripped,
            "# Demo\n\n```rustlab\nprint(1)\n```\n\nMore.\n",
            "legacy bare text block after rustlab fence must be removed",
        );
    }

    #[test]
    fn strip_render_artifacts_preserves_standalone_text_block() {
        // A ```text``` block not adjacent to a rustlab fence is user
        // content (e.g. illustrating expected output prose) — leave it.
        let src = "# Demo\n\nHere's a sample log:\n\n```text\nINFO foo\n```\n\nMore.\n";
        let stripped = strip_render_artifacts(src);
        assert_eq!(stripped, src);
    }

    #[test]
    fn strip_render_artifacts_preserves_text_block_after_sentinel_wrapped_output() {
        // If the output block is already sentinel-wrapped (current emit
        // shape), a *user-authored* trailing ```text``` block must stay.
        let src = "# Demo\n\n\
```rustlab\nprint(1)\n```\n\n\
<!-- rustlab:output-start -->\n```text\n1\n```\n\n<!-- rustlab:output-end -->\n\n\
```text\nuser note\n```\n";
        let stripped = strip_render_artifacts(src);
        assert!(
            stripped.contains("user note"),
            "user-authored ```text``` after sentinel block must be preserved: {stripped:?}",
        );
    }

    #[test]
    fn strip_render_artifacts_preserves_unmatched_start_sentinel() {
        // A truncated file with a start but no end sentinel should not
        // silently delete everything past the start — better to leave it
        // and let the user see it.
        let src = format!("a\n\n{}\nx\nno end here\n", OUTPUT_BLOCK_START);
        let stripped = strip_render_artifacts(&src);
        assert!(stripped.contains("no end here"), "truncated region preserved: {stripped:?}");
    }

    // ── Obsidian end-to-end pipeline coverage ─────────────────────────────
    //
    // These tests drive `cmd_render` through the full Obsidian path
    // (frontmatter merge, attachments routing, iframe emission, cache-
    // bust on plots) rather than the lower-level helpers. They catch
    // regressions in any of the parts wiring together — the kind of
    // issue that wouldn't surface in unit tests of `merge_obsidian_
    // frontmatter` or `attachments_layout_for` in isolation.

    /// Build a tiny notebook source that produces one plot, suitable for
    /// driving obsidian-mode renders through `cmd_render`.
    #[cfg(test)]
    fn obsidian_test_source() -> &'static str {
        "# Demo\n\n```rustlab\nplot(1:10)\n```\n"
    }

    #[test]
    fn obsidian_two_dir_routes_plots_to_attachments_directory() {
        let src_dir = tempfile::TempDir::new().unwrap();
        let out_dir = tempfile::TempDir::new().unwrap();
        let src = src_dir.path().join("note.md");
        let out = out_dir.path().join("note.md");
        std::fs::write(&src, obsidian_test_source()).unwrap();

        cmd_render(
            src,
            Some(out.clone()),
            Format::Markdown {
                obsidian: Some(ObsidianOpts::default()),
            },
            Theme::Dark.colors(),
        );

        let md = std::fs::read_to_string(&out).unwrap();
        assert!(
            md.contains("![plot 1](_attachments/note/plot-1-"),
            "obsidian render must route plot URLs to _attachments/<stem>/ with hashed filename: {md}",
        );
        let attachments = out_dir.path().join("_attachments/note");
        let entries: Vec<_> = std::fs::read_dir(&attachments)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        assert!(
            entries.iter().any(|n| n.starts_with("plot-1-") && n.ends_with(".svg")),
            "plot SVG should be written under _attachments/<stem>/ with hashed name; got {entries:?}",
        );
    }

    #[test]
    fn obsidian_custom_attachments_dir_overrides_default() {
        let src_dir = tempfile::TempDir::new().unwrap();
        let out_dir = tempfile::TempDir::new().unwrap();
        let src = src_dir.path().join("note.md");
        let out = out_dir.path().join("note.md");
        std::fs::write(&src, obsidian_test_source()).unwrap();

        let opts = ObsidianOpts {
            attachments_dir: "media".to_string(),
            ..ObsidianOpts::default()
        };
        cmd_render(
            src,
            Some(out.clone()),
            Format::Markdown { obsidian: Some(opts) },
            Theme::Dark.colors(),
        );

        let md = std::fs::read_to_string(&out).unwrap();
        assert!(
            md.contains("![plot 1](media/note/plot-1-"),
            "custom attachments_dir must replace `_attachments` in plot URLs: {md}",
        );
        let custom_dir = out_dir.path().join("media/note");
        let entries: Vec<_> = std::fs::read_dir(&custom_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        assert!(
            entries.iter().any(|n| n.starts_with("plot-1-") && n.ends_with(".svg")),
            "hashed plot file must exist under custom attachments dir; got {entries:?}",
        );
        assert!(
            !out_dir.path().join("_attachments/note").exists(),
            "default _attachments dir must not be created when overridden",
        );
    }

    #[test]
    fn obsidian_no_iframe_option_suppresses_trailing_iframe() {
        let src_dir = tempfile::TempDir::new().unwrap();
        let out_dir = tempfile::TempDir::new().unwrap();
        let src = src_dir.path().join("note.md");
        let out = out_dir.path().join("note.md");
        std::fs::write(&src, "# Demo\n\nbody\n").unwrap();

        let opts = ObsidianOpts {
            iframe: false,
            ..ObsidianOpts::default()
        };
        cmd_render(
            src,
            Some(out.clone()),
            Format::Markdown { obsidian: Some(opts) },
            Theme::Dark.colors(),
        );

        let md = std::fs::read_to_string(&out).unwrap();
        assert!(
            !md.contains("<iframe "),
            "ObsidianOpts.iframe = false must suppress the trailing iframe: {md}",
        );
    }

    #[test]
    fn obsidian_plot_url_has_hashed_filename_not_query_string() {
        let src_dir = tempfile::TempDir::new().unwrap();
        let out_dir = tempfile::TempDir::new().unwrap();
        let src = src_dir.path().join("note.md");
        let out = out_dir.path().join("note.md");
        std::fs::write(&src, obsidian_test_source()).unwrap();

        cmd_render(
            src,
            Some(out.clone()),
            Format::Markdown {
                obsidian: Some(ObsidianOpts::default()),
            },
            Theme::Dark.colors(),
        );

        let md = std::fs::read_to_string(&out).unwrap();
        // Regression: an earlier fix used `plot-1.svg?v=hash` which works
        // in browsers (HTTP server strips query) but Obsidian's local-file
        // renderer treats the URL as a literal filesystem path → "file
        // not found" → broken image. Hash must be in the filename itself.
        assert!(
            !md.contains("?v="),
            "obsidian plot URL must not use a `?v=` query string (broken in Obsidian): {md}",
        );
        assert!(
            md.contains("plot-1-"),
            "obsidian plot URL must put the cache-bust hash in the filename: {md}",
        );
        // And the file on disk has the same hashed name (so Obsidian's
        // literal path lookup actually finds it).
        let attachments = out_dir.path().join("_attachments/note");
        let entries: Vec<_> = std::fs::read_dir(&attachments)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        assert!(
            entries.iter().any(|n| n.starts_with("plot-1-") && n.ends_with(".svg")),
            "file on disk must have the hashed name the .md points at; got {entries:?}",
        );
    }

    #[test]
    fn obsidian_preserves_existing_frontmatter_through_render() {
        let src_dir = tempfile::TempDir::new().unwrap();
        let out_dir = tempfile::TempDir::new().unwrap();
        let src = src_dir.path().join("note.md");
        let out = out_dir.path().join("note.md");
        std::fs::write(
            &src,
            "---\ntitle: Custom\ntags: [user, custom]\n---\n\n# Demo\n",
        )
        .unwrap();

        cmd_render(
            src,
            Some(out.clone()),
            Format::Markdown {
                obsidian: Some(ObsidianOpts::default()),
            },
            Theme::Dark.colors(),
        );

        let md = std::fs::read_to_string(&out).unwrap();
        // User-authored title and the user's existing tags entries must be
        // intact; rustlab only appends `tags:` / `cssclasses:` when missing.
        assert!(md.contains("title: Custom"), "user title lost: {md}");
        assert!(md.contains("user"), "existing tag entry lost: {md}");
        assert!(md.contains("custom"), "existing tag entry lost: {md}");
        // Output must still have a frontmatter block, not bare body.
        assert!(md.starts_with("---\n"), "frontmatter delimiter missing: {md}");
    }

    #[test]
    fn cmd_clean_strips_obsidian_iframe_through_sentinel_region() {
        // Obsidian mode wraps the trailing <iframe> in OUTPUT_BLOCK
        // sentinels. `clean` must remove that whole region — not just
        // the iframe attribute string, which is what the legacy iframe
        // strip would catch.
        let src_dir = tempfile::TempDir::new().unwrap();
        let out_dir = tempfile::TempDir::new().unwrap();
        let src = src_dir.path().join("note.md");
        let out = out_dir.path().join("note.md");
        std::fs::write(&src, "# Demo\n\nbody\n").unwrap();

        cmd_render(
            src,
            Some(out.clone()),
            Format::Markdown {
                obsidian: Some(ObsidianOpts::default()),
            },
            Theme::Dark.colors(),
        );
        let rendered = std::fs::read_to_string(&out).unwrap();
        assert!(rendered.contains("<iframe "), "test pre-condition: render emitted an iframe");

        cmd_clean(out.clone(), None, false);
        let cleaned = std::fs::read_to_string(&out).unwrap();
        assert!(
            !cleaned.contains("<iframe "),
            "clean must strip the sentinel-wrapped iframe: {cleaned}",
        );
        assert!(
            !cleaned.contains(OUTPUT_BLOCK_START),
            "clean must remove the output sentinel region itself",
        );
    }

    #[test]
    fn obsidian_render_then_clean_returns_to_pristine_source() {
        // The round-trip property: render an obsidian copy, then clean
        // it; the result should be byte-identical to what `clean` would
        // produce from the original source after frontmatter merge.
        // (Frontmatter is merged on render and stays on clean — that's
        // by design, since the merged keys are part of the "source as
        // a vault note" identity.)
        let src_dir = tempfile::TempDir::new().unwrap();
        let out_dir = tempfile::TempDir::new().unwrap();
        let src = src_dir.path().join("note.md");
        let out = out_dir.path().join("note.md");
        let original = "# Demo\n\nProse line.\n\n```rustlab\nprint(7)\n```\n";
        std::fs::write(&src, original).unwrap();

        cmd_render(
            src,
            Some(out.clone()),
            Format::Markdown {
                obsidian: Some(ObsidianOpts::default()),
            },
            Theme::Dark.colors(),
        );
        cmd_clean(out.clone(), None, false);
        let cleaned = std::fs::read_to_string(&out).unwrap();

        // Body content is back to the user-authored shape (after the
        // injected frontmatter).
        assert!(cleaned.contains("# Demo"), "heading lost: {cleaned}");
        assert!(
            cleaned.contains("```rustlab\nprint(7)\n```"),
            "rustlab code fence lost: {cleaned}",
        );
        assert!(cleaned.contains("Prose line."), "prose lost: {cleaned}");
        assert!(!cleaned.contains(GENERATED_HEADER));
        assert!(!cleaned.contains(OUTPUT_BLOCK_START));
        assert!(!cleaned.contains("<iframe "));
        assert!(!cleaned.contains("```text"), "captured stdout block leaked: {cleaned}");
    }

    // Bug regression: `--obsidian` mode prepends a YAML frontmatter
    // block before the header, which pushed `GENERATED_HEADER` past the
    // start of the file. A prefix-only strip missed it, so every pass
    // added a fresh header AND a fresh trailing iframe — the file grew
    // without bound. Fixed by (a) replacing every `HEADER + "\n\n"`
    // occurrence position-agnostically and (b) wrapping the iframe in
    // the same OUTPUT_BLOCK sentinels the per-block output uses.
    // ── paths_equal coverage (gates watcher auto-clean) ───────────────────

    #[test]
    fn paths_equal_true_for_literal_match() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(paths_equal(dir.path(), dir.path()));
    }

    #[test]
    fn paths_equal_true_through_dot_normalisation() {
        let dir = tempfile::TempDir::new().unwrap();
        let with_dot = dir.path().join(".").join(".");
        // Canonicalisation collapses `./.` → dir; literal equality wouldn't.
        assert!(paths_equal(dir.path(), &with_dot));
    }

    #[test]
    fn paths_equal_false_for_distinct_directories() {
        let a = tempfile::TempDir::new().unwrap();
        let b = tempfile::TempDir::new().unwrap();
        assert!(!paths_equal(a.path(), b.path()));
    }

    // ── End-to-end watcher startup: two-dir auto-clean ────────────────────
    // `cmd_watch` blocks indefinitely, so we test its startup sequence by
    // invoking the same logic it does — `paths_equal` decides in-place vs.
    // two-dir; `cmd_clean` strips artifacts in two-dir mode. This pins the
    // behaviour that turns a source dir containing rendered artifacts into
    // a clean source dir before the initial render fires.
    #[test]
    fn watch_startup_two_dir_strips_artifacts_from_source() {
        let src = tempfile::TempDir::new().unwrap();
        let out = tempfile::TempDir::new().unwrap();

        // Source has a header + sentinel-wrapped output region left over
        // from a prior single-dir render that someone moved into the
        // source dir of a new two-dir setup.
        let dirty = format!(
            "{h}\n\n# Demo\n\n```rustlab\nprint(1)\n```\n\n{s}\n```text\n1\n```\n{e}\n",
            h = GENERATED_HEADER,
            s = OUTPUT_BLOCK_START,
            e = OUTPUT_BLOCK_END,
        );
        std::fs::write(src.path().join("note.md"), &dirty).unwrap();

        // Run the same gate the watcher uses to decide auto-clean.
        let is_in_place = paths_equal(src.path(), out.path());
        assert!(!is_in_place, "different dirs must be detected as two-dir");

        let changed = cmd_clean(src.path().to_path_buf(), None, false);
        assert_eq!(changed, 1, "the single dirty file must be cleaned");

        let cleaned = std::fs::read_to_string(src.path().join("note.md")).unwrap();
        assert!(!cleaned.contains(GENERATED_HEADER));
        assert!(!cleaned.contains(OUTPUT_BLOCK_START));
        assert!(cleaned.contains("```rustlab\nprint(1)\n```"), "code fence preserved: {cleaned}");
    }

    #[test]
    fn watch_startup_in_place_does_not_strip_artifacts() {
        // Single-dir in-place: dir == out_dir. Auto-clean must NOT fire,
        // because the artifacts in the source ARE the rendered output
        // we're displaying in Obsidian's Reading view — stripping them
        // would break the user's display until the next render.
        let dir = tempfile::TempDir::new().unwrap();

        // Pre-rendered file with the sentinel-wrapped output present.
        let path = dir.path().join("note.md");
        let body = format!(
            "# Demo\n\n```rustlab\nprint(1)\n```\n\n{s}\n```text\n1\n```\n{e}\n",
            s = OUTPUT_BLOCK_START,
            e = OUTPUT_BLOCK_END,
        );
        std::fs::write(&path, &body).unwrap();

        let is_in_place = paths_equal(dir.path(), dir.path());
        assert!(is_in_place, "same path on both sides must register as in-place");

        // Don't call cmd_clean in this branch (mirrors what cmd_watch does).
        let preserved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(preserved, body, "in-place mode must leave the source bytes untouched");
    }

    #[test]
    fn cmd_render_markdown_obsidian_in_place_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(
            &path,
            "# Demo\n\n```rustlab\nx = 1 + 2;\nprint(x)\n```\n",
        )
        .unwrap();

        let format = Format::Markdown {
            obsidian: Some(ObsidianOpts::default()),
        };
        cmd_render(path.clone(), Some(path.clone()), format.clone(), Theme::Dark.colors());
        let after_first = std::fs::read_to_string(&path).unwrap();

        cmd_render(path.clone(), Some(path.clone()), format, Theme::Dark.colors());
        let after_second = std::fs::read_to_string(&path).unwrap();

        assert_eq!(
            after_first, after_second,
            "obsidian-mode in-place render must be byte-stable on pass 2",
        );
        // Single-file in-place renders suppress the generated-by
        // header — the source IS the rendered output, the "do not edit"
        // warning would just be misleading.
        assert_eq!(
            after_second.matches(GENERATED_HEADER).count(),
            0,
            "in-place obsidian render must not emit the generated-by header",
        );
        // In-place renders auto-suppress the iframe — it would point at a
        // missing sibling `.html` and Obsidian's editor crashes parsing
        // the bad URL. `--no-iframe` and `-o <other-dir>` both turn the
        // iframe back on; this test specifically pins the in-place case.
        assert_eq!(
            after_second.matches("<iframe ").count(),
            0,
            "single-dir in-place render must not emit an iframe",
        );
    }

    // Two-dir flow (out_dir != src_dir): the iframe is still emitted, so
    // users running `notebook render src/ -o vault/ --obsidian` continue
    // to get the Plotly view they expect.
    #[test]
    fn cmd_render_markdown_obsidian_two_dir_keeps_iframe() {
        let src_dir = tempfile::TempDir::new().unwrap();
        let out_dir = tempfile::TempDir::new().unwrap();
        let src_path = src_dir.path().join("note.md");
        let out_path = out_dir.path().join("note.md");
        std::fs::write(&src_path, "# Demo\n\nhi\n").unwrap();

        cmd_render(
            src_path.clone(),
            Some(out_path.clone()),
            Format::Markdown {
                obsidian: Some(ObsidianOpts::default()),
            },
            Theme::Dark.colors(),
        );

        let rendered = std::fs::read_to_string(&out_path).unwrap();
        assert_eq!(
            rendered.matches("<iframe ").count(),
            1,
            "two-dir obsidian render keeps the trailing iframe",
        );
    }

    // The single guarantee that makes single-dir in-place `notebook watch`
    // converge: rendering twice in a row over the same .md must produce
    // byte-identical output on the second pass. Combined with
    // `write_output_skips_when_content_unchanged`, the second pass becomes
    // a no-op write → no fs event → loop dies.
    #[test]
    fn cmd_render_markdown_in_place_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(
            &path,
            "# Demo\n\nProse.\n\n```rustlab\nx = 1 + 2;\nprint(x)\n```\n\nMore prose.\n",
        )
        .unwrap();

        // First pass: input == output (in-place).
        cmd_render(
            path.clone(),
            Some(path.clone()),
            Format::Markdown { obsidian: None },
            Theme::Dark.colors(),
        );
        let after_first = std::fs::read_to_string(&path).unwrap();

        // Second pass: re-read the rendered output and render again.
        cmd_render(
            path.clone(),
            Some(path.clone()),
            Format::Markdown { obsidian: None },
            Theme::Dark.colors(),
        );
        let after_second = std::fs::read_to_string(&path).unwrap();

        assert_eq!(
            after_first, after_second,
            "second in-place render must produce byte-identical output (otherwise notebook watch will loop forever)",
        );
        // Sanity: in-place renders suppress the generated-by header
        // (the source IS the rendered output, so "do not edit" makes
        // no sense). Two-dir renders keep the header — verified
        // elsewhere.
        assert_eq!(
            after_second.matches(GENERATED_HEADER).count(),
            0,
            "in-place render must not emit the generated-by header",
        );
    }

    #[test]
    fn cmd_render_markdown_two_dir_keeps_generated_header() {
        let src_dir = tempfile::TempDir::new().unwrap();
        let out_dir = tempfile::TempDir::new().unwrap();
        let src_path = src_dir.path().join("note.md");
        let out_path = out_dir.path().join("note.md");
        std::fs::write(&src_path, "# Demo\n\nhi\n").unwrap();

        cmd_render(
            src_path,
            Some(out_path.clone()),
            Format::Markdown { obsidian: None },
            Theme::Dark.colors(),
        );

        let rendered = std::fs::read_to_string(&out_path).unwrap();
        assert_eq!(
            rendered.matches(GENERATED_HEADER).count(),
            1,
            "two-dir render keeps the header so committed gallery output keeps provenance",
        );
    }

    // Regression for B1 (notebook_followups_2026_05_16.md): the
    // notebook renderer changes the process cwd via `set_current_dir`
    // so the embed expander and script evaluator can resolve relative
    // paths against the notebook's parent directory. `cwd` is process-
    // global on Unix; without a RAII guard restoring it on exit, the
    // change leaks to anything that runs next — a future parallel
    // renderer, a parmap inside a notebook, the calling CLI process,
    // etc.
    //
    // Production wraps every `cmd_render*` entry point with `CwdGuard`,
    // which is `CwdRestoreGuard` (capture + restore on drop) bundled
    // with a hold on `RENDER_LOCK` so concurrent renders serialise on
    // cwd. We test the inner `CwdRestoreGuard` directly while holding
    // `RENDER_LOCK` from the test, which guarantees no concurrent
    // render mutates cwd during the test window. Testing through the
    // outer `cmd_render*` calls would deadlock — the outer guard tries
    // to take `RENDER_LOCK` itself, which the test would already hold.

    /// Returns the canonical absolute path of the cwd, so comparisons
    /// aren't tripped by symlinks (`/var` vs `/private/var`).
    fn cwd_canon() -> std::path::PathBuf {
        let here = std::env::current_dir().unwrap();
        std::fs::canonicalize(&here).unwrap_or(here)
    }

    #[test]
    fn cwd_restore_guard_restores_cwd_on_drop() {
        let _lock = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let temp_a = tempfile::TempDir::new().unwrap();
        let temp_b = tempfile::TempDir::new().unwrap();
        std::env::set_current_dir(temp_a.path()).unwrap();
        let baseline = cwd_canon();
        {
            let _g = CwdRestoreGuard::new();
            std::env::set_current_dir(temp_b.path()).unwrap();
            assert_ne!(
                cwd_canon(),
                baseline,
                "test setup: inner cwd should differ from baseline",
            );
        } // `_g` drops here → restores cwd
        assert_eq!(
            cwd_canon(),
            baseline,
            "CwdRestoreGuard must restore the cwd it captured at construction",
        );
    }

    #[test]
    fn cwd_restore_guard_restores_after_panic_unwind() {
        // Mirrors cmd_render's failure-path behaviour: if execution
        // bails part-way through, the guard's Drop still runs and the
        // cwd is restored. catch_unwind simulates the unwind.
        let _lock = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let temp_a = tempfile::TempDir::new().unwrap();
        let temp_b = tempfile::TempDir::new().unwrap();
        std::env::set_current_dir(temp_a.path()).unwrap();
        let baseline = cwd_canon();
        let temp_b_path = temp_b.path().to_path_buf();
        let _ = std::panic::catch_unwind(|| {
            let _g = CwdRestoreGuard::new();
            std::env::set_current_dir(&temp_b_path).unwrap();
            panic!("simulated render failure mid-flight");
        });
        assert_eq!(
            cwd_canon(),
            baseline,
            "CwdRestoreGuard must restore the cwd even when its scope unwinds via panic",
        );
    }

    #[test]
    fn write_output_skips_when_content_unchanged() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, b"hello\n").unwrap();
        let original_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        // Sleep just over filesystem mtime granularity (HFS+/APFS = 1 s,
        // ext4 typically ns but ms-rounded). 1100 ms guarantees a fresh
        // mtime would be detectable if a write happened.
        std::thread::sleep(std::time::Duration::from_millis(1100));

        write_output(&path.to_path_buf(), b"hello\n");

        let post_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(
            original_mtime, post_mtime,
            "identical content must not trigger a write (mtime should be unchanged)",
        );
    }

    #[test]
    fn write_output_writes_when_content_differs() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, b"hello\n").unwrap();

        write_output(&path.to_path_buf(), b"hello world\n");

        assert_eq!(std::fs::read(&path).unwrap(), b"hello world\n");
    }

    #[test]
    fn write_output_creates_new_file_when_absent() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("subdir/new.md");
        assert!(!path.exists());

        write_output(&path.to_path_buf(), b"fresh\n");

        assert_eq!(std::fs::read(&path).unwrap(), b"fresh\n");
    }

    // B3 (notebook_followups_2026_05_16.md): once we've written a
    // file, a repeat write of the same bytes must short-circuit on
    // the in-memory hash cache without reading the file. The
    // observable proxy is that even if we *truncate the file on
    // disk*, `write_output` still won't write — because the cache
    // says "we already wrote this hash" and it skips both the
    // `std::fs::read` defensive check and the `std::fs::write`.
    //
    // That's exactly the behaviour B3 trades for: O(1) memory check
    // beats O(filesize) disk read on the watcher hot loop, at the
    // cost of trusting our own in-process record over what's on
    // disk. (External tampering between renders is out of scope —
    // see `WRITE_OUTPUT_HASHES`'s safety note.)
    #[test]
    fn write_output_in_memory_hash_skips_disk_read_on_repeat() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("note.md");

        // First write: nothing on disk; we should hit the slow path,
        // write, and populate the in-memory hash.
        write_output(&path.to_path_buf(), b"first\n");
        assert_eq!(std::fs::read(&path).unwrap(), b"first\n");

        // Externally truncate the file. A pure disk-read check would
        // see "current bytes empty != data" and overwrite. With the
        // in-memory cache, we trust our last-written hash and skip.
        std::fs::write(&path, b"").unwrap();
        write_output(&path.to_path_buf(), b"first\n");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"",
            "in-memory hash cache should have suppressed the rewrite — \
             confirming the fast path skipped both the read and the write",
        );

        // Sanity: a *different* hash on the next call must reach the
        // write branch and overwrite, so we haven't broken the
        // common case.
        write_output(&path.to_path_buf(), b"second\n");
        assert_eq!(std::fs::read(&path).unwrap(), b"second\n");
    }

    // Defensive cross-check: when the in-memory cache is cold (e.g.
    // first invocation in a one-shot `cmd_render` process) and the
    // on-disk bytes already match what we'd write, the slow path
    // recognises the match and skips the write while populating the
    // cache for next time.
    #[test]
    fn write_output_slow_path_skips_when_disk_already_matches() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("preexisting.md");
        std::fs::write(&path, b"identical\n").unwrap();
        let mtime_before = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));

        // No cache entry yet → falls through to disk read → equal →
        // skip write. mtime stays put.
        write_output(&path.to_path_buf(), b"identical\n");
        let mtime_after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(
            mtime_before, mtime_after,
            "cold-cache slow path must still recognise identical bytes and skip the write",
        );
    }

    // Regression for the "edit plot(1:10) → plot(1:100), plot didn't change"
    // user report. Root cause: Obsidian (and browser image cache generally)
    // keys cached images by URL — same `plot-1.svg` URL → cached bytes
    // shown, even if the file on disk changed. Fix is to put a content
    // hash in the filename itself: `plot-1-<hash>.svg`. This test pins:
    //   (a) the filename contains the hash suffix,
    //   (b) the hash differs when the plot's input changes,
    //   (c) the hash is stable on a re-render of identical input (so the
    //       `.md` stays byte-stable for the watcher's no-op skip).
    #[test]
    fn plot_filename_hash_changes_when_plot_input_changes() {
        let dir_a = tempfile::TempDir::new().unwrap();
        let dir_b = tempfile::TempDir::new().unwrap();
        let src_a = dir_a.path().join("note.md");
        let src_b = dir_b.path().join("note.md");

        std::fs::write(&src_a, "```rustlab\nplot(1:10)\n```\n").unwrap();
        std::fs::write(&src_b, "```rustlab\nplot(1:100)\n```\n").unwrap();

        cmd_render(
            src_a.clone(),
            Some(src_a.clone()),
            Format::Markdown { obsidian: None },
            Theme::Dark.colors(),
        );
        cmd_render(
            src_b.clone(),
            Some(src_b.clone()),
            Format::Markdown { obsidian: None },
            Theme::Dark.colors(),
        );

        let md_a = std::fs::read_to_string(&src_a).unwrap();
        let md_b = std::fs::read_to_string(&src_b).unwrap();

        let hash_a = extract_plot_hash(&md_a);
        let hash_b = extract_plot_hash(&md_b);
        assert_ne!(
            hash_a, hash_b,
            "plot filename hash must differ between 10-point and 100-point plot \
             (a: {hash_a}, b: {hash_b})",
        );
    }

    #[test]
    fn plot_filename_hash_stable_across_identical_renders() {
        let d1 = tempfile::TempDir::new().unwrap();
        let d2 = tempfile::TempDir::new().unwrap();
        let s1 = d1.path().join("note.md");
        let s2 = d2.path().join("note.md");
        std::fs::write(&s1, "```rustlab\nplot(1:50)\n```\n").unwrap();
        std::fs::write(&s2, "```rustlab\nplot(1:50)\n```\n").unwrap();

        cmd_render(
            s1.clone(),
            Some(s1.clone()),
            Format::Markdown { obsidian: None },
            Theme::Dark.colors(),
        );
        cmd_render(
            s2.clone(),
            Some(s2.clone()),
            Format::Markdown { obsidian: None },
            Theme::Dark.colors(),
        );

        let md1 = std::fs::read_to_string(&s1).unwrap();
        let md2 = std::fs::read_to_string(&s2).unwrap();
        assert_eq!(
            extract_plot_hash(&md1),
            extract_plot_hash(&md2),
            "identical plot input must produce identical filename hash (idempotency)",
        );
    }

    /// Pull the hex hash suffix out of a `![..](.../plot-N-<hex>.svg)` URL.
    fn extract_plot_hash(md: &str) -> String {
        // Find a substring like `plot-1-` … `.svg`; the bit between is the hash.
        let start = md.find("plot-1-").expect("no plot-1- in markdown");
        let after_prefix = &md[start + "plot-1-".len()..];
        let end = after_prefix.find(".svg").expect("no .svg after plot-1-");
        after_prefix[..end].to_string()
    }

    #[test]
    fn cmd_clean_strips_in_place_when_no_output_specified() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("note.md");
        let dirty = format!(
            "{h}\n\n# Demo\n\n```rustlab\nprint(1)\n```\n\n{s}\n```text\n1\n```\n{e}\n",
            h = GENERATED_HEADER,
            s = OUTPUT_BLOCK_START,
            e = OUTPUT_BLOCK_END,
        );
        std::fs::write(&path, &dirty).unwrap();

        let changed = cmd_clean(path.clone(), None, false);
        assert_eq!(changed, 1, "one file should be reported as cleaned");

        let cleaned = std::fs::read_to_string(&path).unwrap();
        assert!(!cleaned.contains(GENERATED_HEADER));
        assert!(!cleaned.contains(OUTPUT_BLOCK_START));
        assert!(cleaned.contains("```rustlab\nprint(1)\n```"));
    }

    #[test]
    fn cmd_clean_no_op_on_already_clean_source() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("note.md");
        let source = "# Demo\n\n```rustlab\nprint(1)\n```\n";
        std::fs::write(&path, source).unwrap();
        let original_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));

        let changed = cmd_clean(path.clone(), None, false);
        assert_eq!(changed, 0, "already-clean file should not count as changed");

        let post_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(
            original_mtime, post_mtime,
            "no-op clean must not touch the file",
        );
    }

    #[test]
    fn cmd_clean_check_mode_does_not_write() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("note.md");
        let dirty = format!("{}\n\n# Demo\n", GENERATED_HEADER);
        std::fs::write(&path, &dirty).unwrap();

        let changed = cmd_clean(path.clone(), None, true);
        assert_eq!(changed, 1);

        // File contents must be byte-identical to the dirty input.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), dirty);
    }

    #[test]
    fn cmd_clean_directory_walks_recursively() {
        let dir = tempfile::TempDir::new().unwrap();
        let nested = dir.path().join("sub/deep");
        std::fs::create_dir_all(&nested).unwrap();
        let a = dir.path().join("a.md");
        let b = nested.join("b.md");
        let header_then_demo = format!("{}\n\n# X\n", GENERATED_HEADER);
        std::fs::write(&a, &header_then_demo).unwrap();
        std::fs::write(&b, &header_then_demo).unwrap();
        // README.md must be excluded (matches list_notebooks behaviour).
        std::fs::write(dir.path().join("README.md"), &header_then_demo).unwrap();

        let changed = cmd_clean(dir.path().to_path_buf(), None, false);
        assert_eq!(changed, 2, "two .md under the tree, README excluded");

        assert!(!std::fs::read_to_string(&a).unwrap().contains(GENERATED_HEADER));
        assert!(!std::fs::read_to_string(&b).unwrap().contains(GENERATED_HEADER));
        // README untouched.
        assert!(std::fs::read_to_string(dir.path().join("README.md"))
            .unwrap()
            .contains(GENERATED_HEADER));
    }

    #[test]
    fn cmd_clean_with_output_writes_copy_leaving_source_untouched() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("note.md");
        let dst = dir.path().join("cleaned.md");
        let dirty = format!("{}\n\n# Demo\n", GENERATED_HEADER);
        std::fs::write(&src, &dirty).unwrap();

        cmd_clean(src.clone(), Some(dst.clone()), false);

        assert_eq!(std::fs::read_to_string(&src).unwrap(), dirty, "source untouched");
        assert!(!std::fs::read_to_string(&dst).unwrap().contains(GENERATED_HEADER));
    }

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

    // ── Obsidian frontmatter merge ──

    #[test]
    fn obsidian_injects_minimal_frontmatter_when_absent() {
        let fm = merge_obsidian_frontmatter("# Just a heading\n");
        assert!(fm.starts_with("---\n"));
        assert!(fm.contains("tags: [rustlab]"));
        assert!(fm.contains("cssclasses: [rustlab-notebook]"));
        assert!(fm.ends_with("---\n\n"));
    }

    #[test]
    fn obsidian_merges_frontmatter_preserving_existing_keys() {
        let source = "---\ntitle: My Notebook\norder: 5\n---\n\n# Body\n";
        let fm = merge_obsidian_frontmatter(source);
        assert!(fm.contains("title: My Notebook"), "lost title: {fm}");
        assert!(fm.contains("order: 5"), "lost order: {fm}");
        assert!(fm.contains("tags: [rustlab]"), "missing tags: {fm}");
        assert!(fm.contains("cssclasses: [rustlab-notebook]"), "missing cssclasses: {fm}");
    }

    #[test]
    fn obsidian_does_not_overwrite_existing_tags() {
        let source = "---\ntags: [physics, optics]\n---\n";
        let fm = merge_obsidian_frontmatter(source);
        assert!(fm.contains("tags: [physics, optics]"), "tags overwritten: {fm}");
        // Should NOT add a second `tags:` line.
        let occurrences = fm.matches("tags:").count();
        assert_eq!(occurrences, 1, "extra tags line: {fm}");
        // cssclasses still injected.
        assert!(fm.contains("cssclasses: [rustlab-notebook]"));
    }

    #[test]
    fn obsidian_does_not_overwrite_existing_cssclasses() {
        let source = "---\ncssclasses: [my-theme]\n---\n";
        let fm = merge_obsidian_frontmatter(source);
        assert!(fm.contains("cssclasses: [my-theme]"));
        let occurrences = fm.matches("cssclasses:").count();
        assert_eq!(occurrences, 1);
        assert!(fm.contains("tags: [rustlab]"));
    }

    // ── Vault index.md generator ──

    #[test]
    fn obsidian_index_emits_wikilink_per_notebook() {
        let entries = vec![
            ("Filter Analysis".to_string(), "filter.md".to_string()),
            ("Quick Look".to_string(), "quick.md".to_string()),
        ];
        let body = generate_obsidian_index_md("Lab Notes", &entries);
        assert!(body.starts_with("# Lab Notes\n"));
        assert!(body.contains("- [[filter|Filter Analysis]]"));
        assert!(body.contains("- [[quick|Quick Look]]"));
    }

    #[test]
    fn obsidian_index_drops_alias_when_title_matches_stem() {
        let entries = vec![("foo".to_string(), "foo.md".to_string())];
        let body = generate_obsidian_index_md("T", &entries);
        assert!(body.contains("- [[foo]]"));
        assert!(!body.contains("[[foo|foo]]"));
    }

    // ── Attachments layout ──

    #[test]
    fn attachments_layout_uses_configured_dir() {
        let (dir, href) = attachments_layout_for(
            &PathBuf::from("/out/notebook.md"),
            "_attachments",
        );
        assert!(dir.ends_with("_attachments/notebook"));
        assert_eq!(href, "_attachments/notebook");
    }

    #[test]
    fn attachments_layout_respects_custom_dir() {
        let (dir, href) = attachments_layout_for(&PathBuf::from("/out/lesson.md"), "media");
        assert!(dir.ends_with("media/lesson"));
        assert_eq!(href, "media/lesson");
    }
}
