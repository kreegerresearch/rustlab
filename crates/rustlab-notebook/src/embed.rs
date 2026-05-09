//! Obsidian-style file embeds (transclusion) for rustlab notebooks.
//!
//! Three forms recognised inside any markdown notebook:
//!
//!   - `![[Document]]`           — inline the body of `Document.md`
//!   - `![[Document#Heading]]`   — inline only the section under `## Heading`
//!   - `![[Document#^block-id]]` — inline the paragraph tagged `^block-id`
//!
//! Expansion runs as a textual pre-process pass: [`expand_embeds`] returns
//! a flat source string that the rest of the pipeline parses and executes
//! unchanged. Embedded `rustlab` code blocks therefore share the host
//! evaluator and can define variables for later host blocks.
//!
//! Loaded sources are cached per render invocation; one read per file
//! regardless of how many times it is embedded. Recursion is capped at
//! [`MAX_EMBED_DEPTH`] and self/cycle references emit inline error
//! callouts rather than aborting the render.

use crate::parse::extract_frontmatter;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Maximum depth of recursive embed expansion. A chain longer than this
/// (e.g. `A → B → C → D → E`) emits an inline error callout at the point
/// where the limit is reached.
pub const MAX_EMBED_DEPTH: usize = 4;

// ─────────────────────────── EmbedRef parser ───────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EmbedAnchor {
    /// `![[Doc]]` — whole document.
    None,
    /// `![[Doc#Heading]]` — case-insensitive heading match.
    Heading(String),
    /// `![[Doc#^id]]` — block reference.
    BlockId(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EmbedRef {
    pub target: String,
    pub anchor: EmbedAnchor,
}

impl EmbedRef {
    fn from_inner(inner: &str) -> Option<Self> {
        let inner = inner.trim();
        if inner.is_empty() {
            return None;
        }
        let (target, anchor) = match inner.find('#') {
            Some(idx) => (&inner[..idx], &inner[idx + 1..]),
            None => (inner, ""),
        };
        let target = target.trim().to_string();
        if target.is_empty() {
            return None;
        }
        let anchor = match anchor.trim() {
            "" => EmbedAnchor::None,
            a if a.starts_with('^') => {
                let id = a[1..].trim().to_string();
                if id.is_empty() {
                    return None;
                }
                EmbedAnchor::BlockId(id)
            }
            a => EmbedAnchor::Heading(a.to_string()),
        };
        Some(EmbedRef { target, anchor })
    }
}

/// Find every embed ref on a single line, returning them in source order.
/// Each entry is `(byte_start, byte_end_exclusive, ref)`. Skips occurrences
/// inside inline backtick code spans on that line.
pub(crate) fn find_embed_refs_in_line(line: &str) -> Vec<(usize, usize, EmbedRef)> {
    let bytes = line.as_bytes();
    let n = bytes.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let b = bytes[i];
        // Inline code span: skip a matched run of N backticks to the next
        // run of equal length (CommonMark inline-code rule, simplified).
        if b == b'`' {
            let run_start = i;
            while i < n && bytes[i] == b'`' {
                i += 1;
            }
            let open_len = i - run_start;
            let mut j = i;
            let mut closed = false;
            while j < n {
                if bytes[j] == b'`' {
                    let cs = j;
                    while j < n && bytes[j] == b'`' {
                        j += 1;
                    }
                    if j - cs == open_len {
                        i = j;
                        closed = true;
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            if !closed {
                // Unclosed: rest of line is literal — but we still want to
                // detect an embed ref later if the user typed a stray `.
                // In practice this is rare; fall through to per-char scan.
                continue;
            }
            continue;
        }
        // Embed opener: `!` followed by `[[`.
        if b == b'!' && i + 2 < n && bytes[i + 1] == b'[' && bytes[i + 2] == b'[' {
            // Find closing `]]`.
            let body_start = i + 3;
            let mut k = body_start;
            let mut closing: Option<usize> = None;
            while k + 1 < n {
                if bytes[k] == b']' && bytes[k + 1] == b']' {
                    closing = Some(k);
                    break;
                }
                k += 1;
            }
            if let Some(close) = closing {
                let inner = &line[body_start..close];
                if let Some(eref) = EmbedRef::from_inner(inner) {
                    out.push((i, close + 2, eref));
                }
                i = close + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

// ─────────────────────────── Path resolver ─────────────────────────────

#[derive(Debug)]
#[allow(dead_code)] // Variants other than NotFound exist for resolver-shape parity;
                    // expander surfaces these via embed_error_block instead.
pub(crate) enum EmbedError {
    NotFound { target: String },
    HeadingNotFound { target: String, heading: String },
    BlockIdNotFound { target: String, id: String },
    Cycle { chain: Vec<PathBuf> },
    DepthExceeded,
}

/// Resolve an embed target (with or without `.md` extension) against the
/// host directory first, then the notebook root, with case-insensitive
/// basename fallback in each.
pub(crate) fn resolve_target(
    target: &str,
    host_dir: &Path,
    root_dir: &Path,
) -> Result<PathBuf, EmbedError> {
    let with_ext = if Path::new(target).extension().is_some() {
        target.to_string()
    } else {
        format!("{target}.md")
    };
    // 1+2: exact-case, host then root.
    for dir in [host_dir, root_dir] {
        let candidate = dir.join(&with_ext);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    // 3+4: case-insensitive basename in each dir.
    let target_lc = with_ext.to_ascii_lowercase();
    for dir in [host_dir, root_dir] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    == target_lc
                    && entry.path().is_file()
                {
                    return Ok(entry.path());
                }
            }
        }
    }
    Err(EmbedError::NotFound {
        target: target.to_string(),
    })
}

// ─────────────────────────── Source loader ─────────────────────────────

/// Load a source file, stripping frontmatter and block-id markers. The
/// raw read is cached in `cache` keyed by canonical path.
pub(crate) fn load_source(
    path: &Path,
    cache: &mut HashMap<PathBuf, String>,
) -> std::io::Result<String> {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(s) = cache.get(&canonical) {
        return Ok(s.clone());
    }
    let raw = std::fs::read_to_string(&canonical)?;
    cache.insert(canonical.clone(), raw.clone());
    Ok(raw)
}

// ─────────────────────────── Section slicer ────────────────────────────

/// Extract the section of `src` whose heading matches `heading` (case-
/// insensitive, whitespace-trimmed). The slice runs from the matching
/// heading line through the line *before* the next heading at the same
/// or higher level (i.e. fewer or equal `#`s), or to EOF.
///
/// Returns `None` if no matching heading is found. Skips heading-shaped
/// lines that appear inside fenced code blocks.
pub(crate) fn slice_section<'a>(src: &'a str, heading: &str) -> Option<&'a str> {
    let target = heading.trim().to_ascii_lowercase();
    let mut _in_fence = false;
    let mut fence_marker: Option<char> = None;
    let mut start: Option<usize> = None;
    let mut start_level: usize = 0;
    let mut byte_pos: usize = 0;

    // First pass: find the start.
    for line in src.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(c) = fence_marker {
            if (c == '`' && trimmed.starts_with("```"))
                || (c == '~' && trimmed.starts_with("~~~"))
            {
                _in_fence = false;
                fence_marker = None;
            }
        } else if trimmed.starts_with("```") {
            _in_fence = true;
            fence_marker = Some('`');
        } else if trimmed.starts_with("~~~") {
            _in_fence = true;
            fence_marker = Some('~');
        } else {
            if let Some((level, text)) = parse_heading_line(line) {
                if text.trim().to_ascii_lowercase() == target {
                    start = Some(byte_pos);
                    start_level = level;
                    byte_pos += line.len();
                    break;
                }
            }
        }
        byte_pos += line.len();
    }

    let start = start?;

    // Second pass: from after the start line, find next heading at <= start_level.
    let mut end = src.len();
    let mut cursor = byte_pos;
    _in_fence = false;
    fence_marker = None;
    for line in src[cursor..].split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(c) = fence_marker {
            if (c == '`' && trimmed.starts_with("```"))
                || (c == '~' && trimmed.starts_with("~~~"))
            {
                _in_fence = false;
                fence_marker = None;
            }
        } else if trimmed.starts_with("```") {
            _in_fence = true;
            fence_marker = Some('`');
        } else if trimmed.starts_with("~~~") {
            _in_fence = true;
            fence_marker = Some('~');
        } else {
            if let Some((level, _)) = parse_heading_line(line) {
                if level <= start_level {
                    end = cursor;
                    break;
                }
            }
        }
        cursor += line.len();
    }

    Some(&src[start..end])
}

/// Recognise a markdown ATX heading line (`# `, `## `, ...). Returns
/// `(level, text_after_hashes)` where `text_after_hashes` excludes the
/// trailing newline if any. Only matches heading lines with up to 3
/// leading spaces and a space (or end-of-line) after the hashes —
/// matching CommonMark.
fn parse_heading_line(line: &str) -> Option<(usize, &str)> {
    let trimmed_start = line.trim_start_matches(' ');
    let leading_spaces = line.len() - trimmed_start.len();
    if leading_spaces > 3 {
        return None;
    }
    let mut hashes = 0;
    for ch in trimmed_start.chars() {
        if ch == '#' {
            hashes += 1;
        } else {
            break;
        }
    }
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let after = &trimmed_start[hashes..];
    // Heading marker must be followed by a space, newline, or be EOF.
    let next_byte = after.as_bytes().first().copied();
    match next_byte {
        Some(b' ') | Some(b'\n') | None => {}
        Some(b'\r') => {}
        _ => return None,
    }
    let text = after.trim_end_matches(['\n', '\r']).trim_start_matches(' ');
    Some((hashes, text))
}

// ─────────────────────────── Block-id slicer ───────────────────────────

/// Extract the paragraph or list item containing the `^id` marker (at end
/// of line). Returns the captured text with the marker stripped.
pub(crate) fn slice_block_id(src: &str, id: &str) -> Option<String> {
    let lines: Vec<&str> = src.lines().collect();
    let mut _in_fence = false;
    let mut fence_marker: Option<char> = None;
    let mut hit: Option<usize> = None;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if let Some(c) = fence_marker {
            if (c == '`' && trimmed.starts_with("```"))
                || (c == '~' && trimmed.starts_with("~~~"))
            {
                _in_fence = false;
                fence_marker = None;
            }
            continue;
        }
        if trimmed.starts_with("```") {
            _in_fence = true;
            fence_marker = Some('`');
            continue;
        }
        if trimmed.starts_with("~~~") {
            _in_fence = true;
            fence_marker = Some('~');
            continue;
        }
        if line_has_block_id(line, id) {
            hit = Some(idx);
            break;
        }
    }

    let hit = hit?;

    // List item: emit only that line, marker stripped.
    if is_list_item(lines[hit]) {
        return Some(strip_block_id_marker(lines[hit]));
    }

    // Paragraph: walk back and forward to the surrounding blank lines.
    let mut start = hit;
    while start > 0 {
        let prev = lines[start - 1];
        if prev.trim().is_empty() || is_block_boundary(prev) {
            break;
        }
        start -= 1;
    }
    let mut end = hit;
    while end + 1 < lines.len() {
        let next = lines[end + 1];
        if next.trim().is_empty() || is_block_boundary(next) {
            break;
        }
        end += 1;
    }

    let mut out = String::new();
    for i in start..=end {
        if i > start {
            out.push('\n');
        }
        if i == hit {
            out.push_str(&strip_block_id_marker(lines[i]));
        } else {
            out.push_str(lines[i]);
        }
    }
    Some(out)
}

fn line_has_block_id(line: &str, id: &str) -> bool {
    let trimmed = line.trim_end();
    let needle = format!("^{id}");
    if let Some(rest) = trimmed.strip_suffix(&needle) {
        // Marker must be preceded by whitespace.
        return rest.is_empty() || rest.ends_with(|c: char| c.is_whitespace());
    }
    false
}

fn is_list_item(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return true;
    }
    // Ordered list: digits then `. `.
    let bytes = t.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    i > 0 && i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1] == b' '
}

fn is_block_boundary(line: &str) -> bool {
    // Headings, horizontal rules, and fence markers all end a paragraph.
    let t = line.trim_start();
    parse_heading_line(line).is_some()
        || matches!(line.trim(), "---" | "***" | "___")
        || t.starts_with("```")
        || t.starts_with("~~~")
}

// ─────────────────────────── Block-id strip ────────────────────────────

/// Strip `^block-id` markers from every non-code line in `src`. Applied
/// to host and embedded sources alike so the marker never leaks into
/// rendered output.
pub(crate) fn strip_block_ids(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut _in_fence = false;
    let mut fence_marker: Option<char> = None;

    for (idx, line) in src.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_start();
        if let Some(c) = fence_marker {
            out.push_str(line);
            if (c == '`' && trimmed.starts_with("```"))
                || (c == '~' && trimmed.starts_with("~~~"))
            {
                _in_fence = false;
                fence_marker = None;
            }
            continue;
        }
        if trimmed.starts_with("```") {
            _in_fence = true;
            fence_marker = Some('`');
            out.push_str(line);
            continue;
        }
        if trimmed.starts_with("~~~") {
            _in_fence = true;
            fence_marker = Some('~');
            out.push_str(line);
            continue;
        }
        out.push_str(&strip_block_id_marker(line));
    }
    out
}

/// Remove a trailing `\s\^[a-zA-Z0-9_-]+\s*$` marker from a single line.
/// Conservative: only strips the last whitespace-delimited token, and only
/// when it matches the block-id shape. Leaves math expressions like
/// `$x^2$` alone because their trailing token is `$x^2$`, not `^…`.
fn strip_block_id_marker(line: &str) -> String {
    let trailing_ws_len = line.len() - line.trim_end().len();
    let trailing_ws = &line[line.len() - trailing_ws_len..];
    let body = &line[..line.len() - trailing_ws_len];
    if let Some(idx) = body.rfind(char::is_whitespace) {
        let candidate = &body[idx + 1..];
        if is_block_id_token(candidate) {
            // Keep the line up through the whitespace before the marker —
            // but trim that whitespace too, so we don't leave a dangling
            // space at the end of the line.
            return format!("{}{}", body[..idx].trim_end(), trailing_ws);
        }
    }
    line.to_string()
}

fn is_block_id_token(s: &str) -> bool {
    if s.len() < 2 || !s.starts_with('^') {
        return false;
    }
    s[1..]
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ─────────────────────────── Heading demoter ───────────────────────────

/// Increase the level of every ATX heading in `src` by `levels`. Caps at
/// level 6 (HTML's max). Skips heading-shaped lines inside fenced code.
/// Heading attribute syntax `# Title {#id}` survives untouched because we
/// only modify the leading hashes.
pub(crate) fn demote_headings(src: &str, levels: usize) -> String {
    if levels == 0 {
        return src.to_string();
    }
    let mut out = String::with_capacity(src.len() + 16);
    let mut _in_fence = false;
    let mut fence_marker: Option<char> = None;
    for (idx, line) in src.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_start();
        if let Some(c) = fence_marker {
            out.push_str(line);
            if (c == '`' && trimmed.starts_with("```"))
                || (c == '~' && trimmed.starts_with("~~~"))
            {
                _in_fence = false;
                fence_marker = None;
            }
            continue;
        }
        if trimmed.starts_with("```") {
            _in_fence = true;
            fence_marker = Some('`');
            out.push_str(line);
            continue;
        }
        if trimmed.starts_with("~~~") {
            _in_fence = true;
            fence_marker = Some('~');
            out.push_str(line);
            continue;
        }
        if let Some((level, _)) = parse_heading_line(line) {
            let new_level = (level + levels).min(6);
            let extra = new_level - level;
            // Insert `extra` extra `#` characters at the start of the
            // hash run. Find the first `#` in the line.
            if extra > 0 {
                if let Some(hash_pos) = line.find('#') {
                    out.push_str(&line[..hash_pos]);
                    for _ in 0..extra {
                        out.push('#');
                    }
                    out.push_str(&line[hash_pos..]);
                    continue;
                }
            }
            out.push_str(line);
            continue;
        }
        out.push_str(line);
    }
    out
}

// ─────────────────────────── Error formatting ──────────────────────────

/// Render an embed error as a `[!CAUTION]` callout block. Blank lines on
/// either side ensure the block parser recognises it as standalone, even
/// when the original `![[...]]` link sat between prose lines.
fn embed_error_block(message: &str) -> String {
    let escaped = message.replace('\n', " ");
    format!("\n\n> [!CAUTION] Embed error\n> {escaped}\n\n")
}

fn format_chain(chain: &[PathBuf]) -> String {
    chain
        .iter()
        .map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string())
        })
        .collect::<Vec<_>>()
        .join(" → ")
}

fn is_markdown_target(target: &str) -> bool {
    match Path::new(target).extension() {
        None => true,
        Some(ext) => ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"),
    }
}

// ─────────────────────────── Recursive expander ────────────────────────

/// Public entry point. Resolve every `![[...]]` reference in `src`,
/// returning a flat assembled source. The host source is treated as a
/// markdown file in `host_dir`; embed targets resolve first against
/// `host_dir`, then against `root_dir`.
///
/// `host_dir == root_dir` is fine for single-file renders.
pub fn expand_embeds(src: &str, host_dir: &Path, root_dir: &Path) -> String {
    let (_, body) = extract_frontmatter(src);
    let mut visiting: HashSet<PathBuf> = HashSet::new();
    let mut chain: Vec<PathBuf> = Vec::new();
    let mut cache: HashMap<PathBuf, String> = HashMap::new();
    let expanded = expand_recursive(
        body,
        host_dir,
        root_dir,
        0,
        &mut visiting,
        &mut chain,
        &mut cache,
    );
    strip_block_ids(&expanded)
}

fn expand_recursive(
    src: &str,
    host_dir: &Path,
    root_dir: &Path,
    depth: usize,
    visiting: &mut HashSet<PathBuf>,
    chain: &mut Vec<PathBuf>,
    cache: &mut HashMap<PathBuf, String>,
) -> String {
    let mut out = String::with_capacity(src.len());
    let mut _in_fence = false;
    let mut fence_marker: Option<char> = None;

    for (idx, line) in src.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_start();
        if let Some(c) = fence_marker {
            out.push_str(line);
            if (c == '`' && trimmed.starts_with("```"))
                || (c == '~' && trimmed.starts_with("~~~"))
            {
                _in_fence = false;
                fence_marker = None;
            }
            continue;
        }
        if trimmed.starts_with("```") {
            _in_fence = true;
            fence_marker = Some('`');
            out.push_str(line);
            continue;
        }
        if trimmed.starts_with("~~~") {
            _in_fence = true;
            fence_marker = Some('~');
            out.push_str(line);
            continue;
        }

        let refs = find_embed_refs_in_line(line);
        if refs.is_empty() {
            out.push_str(line);
            continue;
        }

        // Substitute each ref with its expansion, in order.
        let mut cursor = 0;
        for (start, end, eref) in refs {
            out.push_str(&line[cursor..start]);
            let expansion =
                expand_one(&eref, host_dir, root_dir, depth, visiting, chain, cache);
            out.push_str(&expansion);
            cursor = end;
        }
        out.push_str(&line[cursor..]);
    }
    out
}

/// Resolve and expand a single embed ref into its inlined replacement
/// text. Returns either the (heading-demoted) embed body or an error
/// callout block when something goes wrong. Non-markdown targets pass
/// through unchanged so the existing wikilink-image transform downstream
/// (`render.rs`) handles them.
fn expand_one(
    eref: &EmbedRef,
    host_dir: &Path,
    root_dir: &Path,
    depth: usize,
    visiting: &mut HashSet<PathBuf>,
    chain: &mut Vec<PathBuf>,
    cache: &mut HashMap<PathBuf, String>,
) -> String {
    // Image embeds (`![[diagram.svg]]`, `![[paper.pdf]]`) are not our
    // problem — leave them for the wikilink-image transform downstream.
    if !is_markdown_target(&eref.target) {
        return reconstruct_literal(eref);
    }

    if depth + 1 > MAX_EMBED_DEPTH {
        return embed_error_block(&format!(
            "max embed depth ({MAX_EMBED_DEPTH}) exceeded at {}",
            eref.target
        ));
    }

    let path = match resolve_target(&eref.target, host_dir, root_dir) {
        Ok(p) => p,
        Err(_) => {
            eprintln!(
                "warning: embed error: target not found: {}",
                eref.target
            );
            return embed_error_block(&format!("target not found: {}", eref.target));
        }
    };

    let canonical = std::fs::canonicalize(&path).unwrap_or(path.clone());
    if visiting.contains(&canonical) {
        let mut display_chain = chain.clone();
        display_chain.push(canonical.clone());
        eprintln!(
            "warning: embed error: cycle detected: {}",
            format_chain(&display_chain)
        );
        return embed_error_block(&format!(
            "cycle detected: {}",
            format_chain(&display_chain)
        ));
    }

    let raw = match load_source(&canonical, cache) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("warning: embed error: cannot read {}: {e}", canonical.display());
            return embed_error_block(&format!(
                "cannot read {}: {e}",
                eref.target
            ));
        }
    };
    let (_, body) = extract_frontmatter(&raw);

    let sliced: String = match &eref.anchor {
        EmbedAnchor::None => body.to_string(),
        EmbedAnchor::Heading(h) => match slice_section(body, h) {
            Some(s) => s.to_string(),
            None => {
                eprintln!(
                    "warning: embed error: heading '{}' not found in {}",
                    h, eref.target
                );
                return embed_error_block(&format!(
                    "heading '{}' not found in {}",
                    h, eref.target
                ));
            }
        },
        EmbedAnchor::BlockId(id) => match slice_block_id(body, id) {
            Some(s) => s,
            None => {
                eprintln!(
                    "warning: embed error: block id '^{}' not found in {}",
                    id, eref.target
                );
                return embed_error_block(&format!(
                    "block id '^{}' not found in {}",
                    id, eref.target
                ));
            }
        },
    };

    visiting.insert(canonical.clone());
    chain.push(canonical.clone());
    let new_host_dir = canonical.parent().unwrap_or(host_dir);
    let expanded = expand_recursive(
        &sliced,
        new_host_dir,
        root_dir,
        depth + 1,
        visiting,
        chain,
        cache,
    );
    chain.pop();
    visiting.remove(&canonical);

    demote_headings(&expanded, 1)
}

fn reconstruct_literal(eref: &EmbedRef) -> String {
    let anchor = match &eref.anchor {
        EmbedAnchor::None => String::new(),
        EmbedAnchor::Heading(h) => format!("#{h}"),
        EmbedAnchor::BlockId(id) => format!("#^{id}"),
    };
    format!("![[{}{anchor}]]", eref.target)
}

// ─────────────────────────── Tests (Phase 1) ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── EmbedRef parser ──

    #[test]
    fn parse_embed_ref_simple() {
        let refs = find_embed_refs_in_line("![[Doc]]");
        assert_eq!(refs.len(), 1);
        let (s, e, r) = &refs[0];
        assert_eq!(*s, 0);
        assert_eq!(*e, 8);
        assert_eq!(r.target, "Doc");
        assert_eq!(r.anchor, EmbedAnchor::None);
    }

    #[test]
    fn parse_embed_ref_heading() {
        let refs = find_embed_refs_in_line("See ![[Doc#Foo Bar]] inline.");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].2.target, "Doc");
        assert_eq!(refs[0].2.anchor, EmbedAnchor::Heading("Foo Bar".to_string()));
    }

    #[test]
    fn parse_embed_ref_block_id() {
        let refs = find_embed_refs_in_line("![[Doc#^my-id]]");
        assert_eq!(refs[0].2.anchor, EmbedAnchor::BlockId("my-id".to_string()));
    }

    #[test]
    fn parse_embed_ref_skipped_in_inline_code() {
        let refs = find_embed_refs_in_line("Literal `![[X]]` and a real ![[Y]].");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].2.target, "Y");
    }

    #[test]
    fn parse_embed_ref_multiple_on_line() {
        let refs = find_embed_refs_in_line("![[A]] then ![[B#sec]].");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].2.target, "A");
        assert_eq!(refs[1].2.target, "B");
        assert_eq!(refs[1].2.anchor, EmbedAnchor::Heading("sec".to_string()));
    }

    #[test]
    fn parse_embed_ref_empty_target_rejected() {
        let refs = find_embed_refs_in_line("![[]] and ![[#orphan]]");
        assert!(refs.is_empty());
    }

    // ── Path resolver ──

    #[test]
    fn resolve_host_dir_first() {
        let host = TempDir::new().unwrap();
        let root = TempDir::new().unwrap();
        fs::write(host.path().join("setup.md"), "host").unwrap();
        fs::write(root.path().join("setup.md"), "root").unwrap();
        let resolved = resolve_target("setup", host.path(), root.path()).unwrap();
        assert_eq!(fs::read_to_string(resolved).unwrap(), "host");
    }

    #[test]
    fn resolve_root_dir_fallback() {
        let host = TempDir::new().unwrap();
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("setup.md"), "root").unwrap();
        let resolved = resolve_target("setup", host.path(), root.path()).unwrap();
        assert_eq!(fs::read_to_string(resolved).unwrap(), "root");
    }

    #[test]
    fn resolve_case_insensitive_fallback() {
        let host = TempDir::new().unwrap();
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("Setup.md"), "actual").unwrap();
        let resolved = resolve_target("setup", host.path(), root.path()).unwrap();
        assert_eq!(fs::read_to_string(resolved).unwrap(), "actual");
    }

    #[test]
    fn resolve_missing_returns_error() {
        let host = TempDir::new().unwrap();
        let root = TempDir::new().unwrap();
        let err = resolve_target("nope", host.path(), root.path()).unwrap_err();
        assert!(matches!(err, EmbedError::NotFound { .. }));
    }

    #[test]
    fn resolve_explicit_extension_used_as_is() {
        let host = TempDir::new().unwrap();
        let root = TempDir::new().unwrap();
        fs::write(host.path().join("note.txt"), "txt").unwrap();
        let resolved = resolve_target("note.txt", host.path(), root.path()).unwrap();
        assert!(resolved.to_string_lossy().ends_with("note.txt"));
    }

    // ── Section slicer ──

    #[test]
    fn slice_section_basic() {
        let src = "intro\n\n## Foo\nbody\n\n## Bar\nrest\n";
        let s = slice_section(src, "Foo").unwrap();
        assert!(s.starts_with("## Foo"));
        assert!(s.contains("body"));
        assert!(!s.contains("## Bar"));
    }

    #[test]
    fn slice_section_until_sibling_heading() {
        let src = "## A\na1\n## B\nb1\n";
        let s = slice_section(src, "A").unwrap();
        assert!(s.contains("a1"));
        assert!(!s.contains("b1"));
    }

    #[test]
    fn slice_section_includes_nested_subheadings() {
        let src = "## Foo\nfoo body\n### Sub\nsub body\n## Bar\n";
        let s = slice_section(src, "Foo").unwrap();
        assert!(s.contains("### Sub"));
        assert!(s.contains("sub body"));
        assert!(!s.contains("## Bar"));
    }

    #[test]
    fn slice_section_case_insensitive_match() {
        let src = "## Frequency Response\nbody\n";
        assert!(slice_section(src, "frequency response").is_some());
    }

    #[test]
    fn slice_section_first_collision_wins() {
        let src = "## Examples\nfirst\n## Other\n## Examples\nsecond\n";
        let s = slice_section(src, "Examples").unwrap();
        assert!(s.contains("first"));
        assert!(!s.contains("second"));
    }

    #[test]
    fn slice_section_skips_headings_in_code_fences() {
        let src = "intro\n```\n## Fake\n```\n## Real\nbody\n";
        let s = slice_section(src, "Real").unwrap();
        assert!(s.contains("body"));
    }

    #[test]
    fn slice_section_missing_returns_none() {
        let src = "## A\nbody\n";
        assert!(slice_section(src, "Z").is_none());
    }

    #[test]
    fn slice_section_to_eof_when_no_sibling() {
        let src = "## Only\nthis is all";
        let s = slice_section(src, "Only").unwrap();
        assert!(s.contains("this is all"));
    }

    // ── Block-id slicer ──

    #[test]
    fn slice_block_id_paragraph() {
        let src = "intro\n\nThe Nyquist rate is twice the highest frequency. ^nyq\n\nafter\n";
        let s = slice_block_id(src, "nyq").unwrap();
        assert_eq!(s, "The Nyquist rate is twice the highest frequency.");
    }

    #[test]
    fn slice_block_id_multiline_paragraph() {
        let src = "first line\nsecond line ^id\nthird line\n";
        let s = slice_block_id(src, "id").unwrap();
        assert!(s.contains("first line"));
        assert!(s.contains("second line"));
        assert!(s.contains("third line"));
        assert!(!s.contains("^id"));
    }

    #[test]
    fn slice_block_id_list_item_only() {
        let src = "- alpha\n- beta with id ^foo\n- gamma\n";
        let s = slice_block_id(src, "foo").unwrap();
        assert_eq!(s.trim(), "- beta with id");
    }

    #[test]
    fn slice_block_id_missing_returns_none() {
        let src = "no marker here\n";
        assert!(slice_block_id(src, "foo").is_none());
    }

    #[test]
    fn slice_block_id_skipped_in_fence() {
        let src = "```\nstuff ^foo\n```\nreal ^foo\n";
        let s = slice_block_id(src, "foo").unwrap();
        assert!(s.contains("real"));
        assert!(!s.contains("stuff"));
    }

    // ── Block-id strip pass ──

    #[test]
    fn strip_block_ids_paragraph() {
        let s = strip_block_ids("hello world ^foo\nnext line\n");
        assert_eq!(s, "hello world\nnext line\n");
    }

    #[test]
    fn strip_block_ids_preserves_code_fences() {
        let src = "```\ndo not strip ^bar\n```\nstrip this ^baz\n";
        let s = strip_block_ids(src);
        assert!(s.contains("do not strip ^bar"));
        assert!(s.contains("strip this\n"));
    }

    #[test]
    fn strip_block_ids_preserves_inline_code_marker() {
        // Conservative: a line ending in `` `^foo` `` has the trailing
        // token `` `^foo` `` (with backticks), which is not a block-id
        // shape and therefore not stripped.
        let s = strip_block_ids("see `^foo` here\n");
        assert_eq!(s, "see `^foo` here\n");
    }

    #[test]
    fn strip_block_ids_preserves_math_with_caret() {
        let s = strip_block_ids("the value is $x^2$\n");
        assert_eq!(s, "the value is $x^2$\n");
    }

    #[test]
    fn strip_block_ids_preserves_lines_without_marker() {
        let src = "regular text\n## heading\n- bullet\n";
        assert_eq!(strip_block_ids(src), src);
    }

    #[test]
    fn strip_block_ids_handles_trailing_whitespace() {
        let s = strip_block_ids("text ^id   \n");
        assert_eq!(s, "text   \n");
    }

    // ── Source loader cache ──

    #[test]
    fn load_source_caches_by_canonical_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "first").unwrap();
        let mut cache = HashMap::new();
        let s1 = load_source(&path, &mut cache).unwrap();
        // Mutate underlying file; cached read should still return original.
        fs::write(&path, "second").unwrap();
        let s2 = load_source(&path, &mut cache).unwrap();
        assert_eq!(s1, "first");
        assert_eq!(s2, "first");
        assert_eq!(cache.len(), 1);
    }

    // ── Heading demoter ──

    #[test]
    fn demote_headings_increments_each_level() {
        let src = "# H1\n## H2\n### H3\n";
        let s = demote_headings(src, 1);
        assert_eq!(s, "## H1\n### H2\n#### H3\n");
    }

    #[test]
    fn demote_headings_caps_at_h6() {
        let src = "###### H6\n##### H5\n";
        let s = demote_headings(src, 2);
        // H6 stays H6; H5 + 2 = 7 → capped to 6.
        assert_eq!(s, "###### H6\n###### H5\n");
    }

    #[test]
    fn demote_headings_skips_fenced_code() {
        let src = "# Real\n```\n## Fake\n```\n# AlsoReal\n";
        let s = demote_headings(src, 1);
        assert!(s.contains("## Real"));
        assert!(s.contains("## Fake") == false || s.contains("## Fake\n"));
        // The fake heading inside the fence should remain `## Fake` (one #).
        // Verify by counting `### ` (which would only appear if demoted).
        assert!(!s.contains("### Fake"));
        assert!(s.contains("## AlsoReal"));
    }

    #[test]
    fn demote_headings_preserves_attributes() {
        let src = "# Title {#id}\n";
        let s = demote_headings(src, 1);
        assert_eq!(s, "## Title {#id}\n");
    }

    #[test]
    fn demote_headings_zero_levels_is_identity() {
        let src = "# A\n## B\nbody\n";
        assert_eq!(demote_headings(src, 0), src);
    }

    // ── Recursive expander ──

    fn write_file(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn expand_simple_full_file_inlines_content() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "setup.md", "Hello from setup.\n");
        let host = "before\n\n![[setup]]\n\nafter\n";
        let out = expand_embeds(host, dir.path(), dir.path());
        assert!(out.contains("Hello from setup."));
        assert!(out.contains("before"));
        assert!(out.contains("after"));
    }

    #[test]
    fn expand_anchored_section_only_emits_subtree() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "doc.md",
            "## Foo\nfoo body\n### Sub\nsub body\n## Bar\nbar body\n",
        );
        let host = "![[doc#Foo]]\n";
        let out = expand_embeds(host, dir.path(), dir.path());
        assert!(out.contains("foo body"));
        assert!(out.contains("sub body"));
        assert!(!out.contains("bar body"));
    }

    #[test]
    fn expand_block_id_emits_paragraph_marker_stripped() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "gloss.md",
            "intro\n\nThe Nyquist rate matters. ^nyq\n\nafter\n",
        );
        let host = "![[gloss#^nyq]]\n";
        let out = expand_embeds(host, dir.path(), dir.path());
        assert!(out.contains("The Nyquist rate matters."));
        assert!(!out.contains("^nyq"));
        assert!(!out.contains("intro"));
        assert!(!out.contains("after"));
    }

    #[test]
    fn expand_recursive_demotes_headings_per_level() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "c.md", "# H1 in C\n");
        write_file(dir.path(), "b.md", "![[c]]\n");
        let host = "![[b]]\n";
        let out = expand_embeds(host, dir.path(), dir.path());
        // Host depth 0; B at depth 1 (demote +1); C at depth 2 (demote +2).
        // H1 in C ends up as H3.
        assert!(out.contains("### H1 in C"));
    }

    #[test]
    fn expand_cycle_emits_caution_callout() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "a.md", "self-embed:\n\n![[a]]\n");
        let host = "![[a]]\n";
        let out = expand_embeds(host, dir.path(), dir.path());
        assert!(out.contains("[!CAUTION]"));
        assert!(out.contains("cycle detected"));
    }

    #[test]
    fn expand_indirect_cycle_emits_caution_callout() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "a.md", "A\n\n![[b]]\n");
        write_file(dir.path(), "b.md", "B\n\n![[a]]\n");
        let host = "![[a]]\n";
        let out = expand_embeds(host, dir.path(), dir.path());
        assert!(out.contains("[!CAUTION]"));
        assert!(out.contains("cycle"));
    }

    #[test]
    fn expand_max_depth_emits_caution_callout() {
        let dir = TempDir::new().unwrap();
        // Linear chain longer than MAX_EMBED_DEPTH = 4.
        write_file(dir.path(), "f.md", "F\n");
        write_file(dir.path(), "e.md", "E\n\n![[f]]\n");
        write_file(dir.path(), "d.md", "D\n\n![[e]]\n");
        write_file(dir.path(), "c.md", "C\n\n![[d]]\n");
        write_file(dir.path(), "b.md", "B\n\n![[c]]\n");
        let host = "![[b]]\n";
        let out = expand_embeds(host, dir.path(), dir.path());
        assert!(out.contains("[!CAUTION]"));
        assert!(out.contains("max embed depth"));
    }

    #[test]
    fn expand_unresolved_target_emits_caution_callout() {
        let dir = TempDir::new().unwrap();
        let host = "![[ghost]]\n";
        let out = expand_embeds(host, dir.path(), dir.path());
        assert!(out.contains("[!CAUTION]"));
        assert!(out.contains("target not found: ghost"));
    }

    #[test]
    fn expand_unresolved_heading_emits_caution_callout() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "doc.md", "## Real\nbody\n");
        let host = "![[doc#Missing]]\n";
        let out = expand_embeds(host, dir.path(), dir.path());
        assert!(out.contains("[!CAUTION]"));
        assert!(out.contains("heading 'Missing' not found in doc"));
    }

    #[test]
    fn expand_unresolved_block_id_emits_caution_callout() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "doc.md", "no markers\n");
        let host = "![[doc#^missing]]\n";
        let out = expand_embeds(host, dir.path(), dir.path());
        assert!(out.contains("[!CAUTION]"));
        assert!(out.contains("block id '^missing' not found"));
    }

    #[test]
    fn expand_cache_loads_each_source_once() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "setup.md", "shared\n");
        let host = "![[setup]]\n\nbetween\n\n![[setup]]\n";
        // Indirect: rely on the cache having one entry after expansion.
        let (_, body) = extract_frontmatter(host);
        let mut visiting = HashSet::new();
        let mut chain = Vec::new();
        let mut cache: HashMap<PathBuf, String> = HashMap::new();
        let _ = expand_recursive(
            body,
            dir.path(),
            dir.path(),
            0,
            &mut visiting,
            &mut chain,
            &mut cache,
        );
        assert_eq!(cache.len(), 1, "shared source should be loaded only once");
    }

    #[test]
    fn expand_strips_block_ids_from_host_source() {
        let dir = TempDir::new().unwrap();
        let host = "intro paragraph ^my-id\n";
        let out = expand_embeds(host, dir.path(), dir.path());
        assert!(!out.contains("^my-id"));
        assert!(out.contains("intro paragraph"));
    }

    #[test]
    fn expand_strips_frontmatter_from_embed_source() {
        let dir = TempDir::new().unwrap();
        write_file(
            dir.path(),
            "doc.md",
            "---\ntitle: Hidden\n---\nVisible body.\n",
        );
        let host = "![[doc]]\n";
        let out = expand_embeds(host, dir.path(), dir.path());
        assert!(out.contains("Visible body."));
        assert!(!out.contains("title: Hidden"));
    }

    #[test]
    fn expand_leaves_image_embeds_alone() {
        let dir = TempDir::new().unwrap();
        write_file(dir.path(), "diagram.svg", "<svg/>");
        let host = "![[diagram.svg]]\n";
        let out = expand_embeds(host, dir.path(), dir.path());
        // Pass-through: the wikilink-image transform downstream handles it.
        assert!(out.contains("![[diagram.svg]]"));
    }
}

