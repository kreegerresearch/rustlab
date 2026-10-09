//! Notebook highlights (`==`) and comments (`%%`).
//!
//! Obsidian syntax, rendered as `<mark>` and margin notes. The scanner
//! skips fences, indented code, inline code, math, HTML comments, and
//! YAML frontmatter. See `dev/plans/notebook_comments.md`.
//!
//! [`CommentHeader::state`] is reserved for a later per-comment
//! reviewed/accepted flag. It is never parsed and never written.

use std::cell::Cell;
use std::collections::HashMap;

// ── render mode (thread-local; set on the rendering thread) ───────────────

#[derive(Clone, Copy)]
pub(crate) struct CommentMode {
    /// HTML/JSON: render marks. `false` strips well-formed marks.
    pub show: bool,
    /// `Some` inserts the Comments checkbox (initial checked state).
    /// `None` omits it (static `--no-comments`, PDF, markdown).
    pub toggle: Option<bool>,
    /// Watch annotate UI: menu, popover, edit/delete, source-hash meta.
    pub annotate: bool,
    /// LaTeX/PDF include highlights and notes. Default off.
    pub latex: bool,
    /// Markdown emitter keeps `==` / `%%`. `--no-comments` turns this off.
    pub markdown_keep: bool,
}

impl CommentMode {
    pub(crate) const fn html_default() -> Self {
        Self {
            show: true,
            toggle: Some(true),
            annotate: false,
            latex: false,
            markdown_keep: true,
        }
    }

    /// `requested` is the resolved CLI/rc value. `None` uses the format default.
    pub(crate) fn for_format(kind: FormatKind, requested: Option<bool>) -> Self {
        let on = requested.unwrap_or(match kind {
            FormatKind::Latex => false,
            _ => true,
        });
        match kind {
            FormatKind::Html | FormatKind::Json => Self {
                show: on,
                toggle: if on { Some(true) } else { None },
                annotate: false,
                latex: false,
                markdown_keep: true,
            },
            FormatKind::Latex => Self {
                show: false,
                toggle: None,
                annotate: false,
                latex: on,
                markdown_keep: true,
            },
            FormatKind::Markdown => Self {
                show: false,
                toggle: None,
                annotate: false,
                latex: false,
                markdown_keep: on,
            },
        }
    }

    /// Watch always renders the markup so the checkbox can reveal it.
    pub(crate) fn for_watch(comments_on: bool, annotate: bool) -> Self {
        Self {
            show: true,
            toggle: Some(comments_on),
            annotate,
            latex: false,
            markdown_keep: true,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum FormatKind {
    Html,
    Latex,
    Markdown,
    Json,
}

thread_local! {
    static MODE: Cell<CommentMode> = Cell::new(CommentMode::html_default());
    static IN_PAGE: Cell<bool> = Cell::new(false);
    static IN_BODY: Cell<bool> = Cell::new(false);
    static NOTE_N: Cell<u32> = Cell::new(0);
    /// `None` means "use the format default" ([`CommentMode::for_format`]).
    static REQUESTED: Cell<Option<bool>> = Cell::new(None);
}

pub(crate) struct ModeGuard {
    prev: CommentMode,
}

impl Drop for ModeGuard {
    fn drop(&mut self) {
        MODE.with(|m| m.set(self.prev));
    }
}

pub(crate) fn install(mode: CommentMode) -> ModeGuard {
    let prev = MODE.with(|m| m.replace(mode));
    ModeGuard { prev }
}

pub(crate) fn mode() -> CommentMode {
    MODE.with(|m| m.get())
}

/// CLI / rc resolution. `Some` forces on or off; `None` leaves the format default.
pub(crate) fn set_requested(v: Option<bool>) {
    REQUESTED.with(|c| c.set(v));
}

pub(crate) fn requested() -> Option<bool> {
    REQUESTED.with(|c| c.get())
}

pub(crate) struct PageGuard {
    prev: bool,
}

impl Drop for PageGuard {
    fn drop(&mut self) {
        IN_PAGE.with(|c| c.set(self.prev));
    }
}

/// One note-number sequence per notebook page. Nested `markdown_to_html`
/// calls keep counting; a bare call resets.
pub(crate) fn enter_page() -> PageGuard {
    let prev = IN_PAGE.with(|c| c.replace(true));
    NOTE_N.with(|c| c.set(0));
    PageGuard { prev }
}

struct BodyGuard {
    prev: bool,
}

impl Drop for BodyGuard {
    fn drop(&mut self) {
        IN_BODY.with(|c| c.set(self.prev));
    }
}

fn enter_body() -> BodyGuard {
    let prev = IN_BODY.with(|c| c.replace(true));
    BodyGuard { prev }
}

fn in_body() -> bool {
    IN_BODY.with(|c| c.get())
}

fn in_page() -> bool {
    IN_PAGE.with(|c| c.get())
}

fn next_note() -> u32 {
    NOTE_N.with(|c| {
        let n = c.get() + 1;
        c.set(n);
        n
    })
}

// ── header model ───────────────────────────────────────────────────────────

/// Reserved. A later spec may set `reviewed`. Always `None` today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommentState {
    pub reviewed: Option<bool>,
}

/// Optional rustlab header inside `%% ... %%`, parsed only when a colon
/// follows a well-formed field list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommentHeader {
    /// `c12` without the leading `#`.
    pub id: Option<String>,
    /// `c3` without the leading `#`.
    pub reply_to: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
    /// Not parsed, not written. Present so a per-comment flag can land
    /// without reshaping the mark.
    pub state: Option<CommentState>,
}

impl CommentHeader {
    #[allow(dead_code)]
    fn is_empty(&self) -> bool {
        self.id.is_none()
            && self.reply_to.is_none()
            && self.author.is_none()
            && self.date.is_none()
            && self.state.is_none()
    }
}

/// Split `inner` (bytes between `%%`) into a header and a body.
///
/// The header is recognised only when the text before the first colon
/// matches `[#id] [re #id] [@name] [YYYY-MM-DD]`. Otherwise the whole
/// inner string, colon included, is the body.
pub fn parse_header(inner: &str) -> (CommentHeader, String) {
    let Some(colon) = inner.find(':') else {
        return (CommentHeader::default(), inner.to_string());
    };
    let pre = inner[..colon].trim();
    let mut body = inner[colon + 1..].to_string();
    if let Some(rest) = body.strip_prefix(' ') {
        body = rest.to_string();
    }
    // A block note's body is the lines after the opener; drop one
    // surrounding newline pair so the card shows the words.
    if body.starts_with('\n') {
        body = body.trim_start_matches('\n').to_string();
    }
    body = body.trim_end_matches('\n').to_string();
    match parse_header_fields(pre) {
        Some(h) => (h, body),
        None => (CommentHeader::default(), inner.to_string()),
    }
}

fn parse_header_fields(pre: &str) -> Option<CommentHeader> {
    let mut header = CommentHeader::default();
    if pre.is_empty() {
        return Some(header);
    }
    let mut toks = pre.split_whitespace();
    // 0 id, 1 reply, 2 author, 3 date, 4 done
    let mut stage = 0u8;
    while let Some(t) = toks.next() {
        if stage <= 0 && is_id_token(t) {
            header.id = Some(t[1..].to_string());
            stage = 1;
            continue;
        }
        if stage <= 1 && t == "re" {
            let id = toks.next()?;
            if !is_id_token(id) {
                return None;
            }
            header.reply_to = Some(id[1..].to_string());
            stage = 2;
            continue;
        }
        if stage <= 2 && is_author_token(t) {
            header.author = Some(t[1..].to_string());
            stage = 3;
            continue;
        }
        if stage <= 3 && is_date_token(t) {
            header.date = Some(t.to_string());
            stage = 4;
            continue;
        }
        return None;
    }
    Some(header)
}

fn is_id_token(t: &str) -> bool {
    let Some(rest) = t.strip_prefix("#c") else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
}

fn is_author_token(t: &str) -> bool {
    let Some(rest) = t.strip_prefix('@') else {
        return false;
    };
    !rest.is_empty()
        && !rest.contains('%')
        && !rest.contains(':')
        && !rest.contains(char::is_whitespace)
}

fn is_date_token(t: &str) -> bool {
    let b = t.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    if !b[..4].iter().all(u8::is_ascii_digit)
        || !b[5..7].iter().all(u8::is_ascii_digit)
        || !b[8..10].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let month: u32 = t[5..7].parse().unwrap_or(0);
    let day: u32 = t[8..10].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

/// `true` when `author` may be written as `@name`. Empty is "omit".
pub fn author_token_ok(author: &str) -> bool {
    !author.is_empty() && is_author_token(&format!("@{author}"))
}

// ── scanner ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Hi,
    Note,
}

struct Delim {
    at: usize,
    kind: Kind,
    line: usize,
}

#[derive(Clone)]
struct Mark {
    kind: Kind,
    start: usize,
    end: usize,
    inner_start: usize,
    inner_end: usize,
    line: usize,
    block: bool,
    header: CommentHeader,
    body: String,
}

#[derive(Debug)]
pub(crate) struct Issue {
    pub line: usize,
    pub code: &'static str,
    pub message: String,
}

struct Scan {
    marks: Vec<Mark>,
    issues: Vec<Issue>,
    /// Unclosed or split delimiters that render as the literal plus a badge.
    warns: Vec<WarnDelim>,
    /// `\==` / `\%%` — drop the backslash, do not open.
    escapes: Vec<(usize, Kind)>,
}

struct WarnDelim {
    at: usize,
    kind: Kind,
    line: usize,
    #[allow(dead_code)]
    code: &'static str,
}

fn scan(md: &str) -> Scan {
    let opaque = opaque_ranges(md);
    let delims = find_delims(md, &opaque);
    pair(md, &delims)
}

fn find_delims(md: &str, opaque: &[(usize, usize)]) -> (Vec<Delim>, Vec<(usize, Kind)>) {
    let b = md.as_bytes();
    let mut delims = Vec::new();
    let mut escapes = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if let Some(end) = opaque_end_at(opaque, i) {
            i = end;
            continue;
        }
        if b[i] == b'\\' && i + 2 < b.len() {
            let kind = if b[i + 1] == b'=' && b[i + 2] == b'=' {
                Some(Kind::Hi)
            } else if b[i + 1] == b'%' && b[i + 2] == b'%' {
                Some(Kind::Note)
            } else {
                None
            };
            if let Some(kind) = kind {
                escapes.push((i, kind));
                i += 3;
                continue;
            }
        }
        if i + 1 < b.len() && b[i] == b'=' && b[i + 1] == b'=' {
            delims.push(Delim {
                at: i,
                kind: Kind::Hi,
                line: line_no(md, i),
            });
            i += 2;
            continue;
        }
        if i + 1 < b.len() && b[i] == b'%' && b[i + 1] == b'%' {
            delims.push(Delim {
                at: i,
                kind: Kind::Note,
                line: line_no(md, i),
            });
            i += 2;
            continue;
        }
        i += 1;
    }
    (delims, escapes)
}

fn pair(md: &str, found: &(Vec<Delim>, Vec<(usize, Kind)>)) -> Scan {
    let (delims, escapes) = found;
    let opaque = opaque_ranges(md);
    let mut skip = vec![false; delims.len()];
    let mut marks = Vec::new();
    let mut issues = Vec::new();
    let mut warns = Vec::new();
    let mut i = 0;
    while i < delims.len() {
        if skip[i] {
            i += 1;
            continue;
        }
        let d = &delims[i];
        let block = d.kind == Kind::Note && line_trimmed_eq(md, d.at, "%%");
        let barrier = next_barrier(md, &opaque, d.at + 2, d.kind, block);
        if let Some(j) = find_closer(md, delims, i, barrier, block) {
            // Other-kind delimiters inside the span are nesting, not marks.
            let mut nested = false;
            for k in (i + 1)..j {
                if delims[k].kind != d.kind {
                    nested = true;
                }
                skip[k] = true;
            }
            if nested {
                issues.push(Issue {
                    line: delims[i + 1].line,
                    code: "rustlab:W009",
                    message: "nested `==` or `%%` is not supported".to_string(),
                });
            }
            let closer = &delims[j];
            let inner_start = d.at + 2;
            let inner_end = closer.at;
            let inner = &md[inner_start..inner_end];
            let (header, body) = if d.kind == Kind::Note {
                parse_header(inner)
            } else {
                (CommentHeader::default(), String::new())
            };
            marks.push(Mark {
                kind: d.kind,
                start: d.at,
                end: closer.at + 2,
                inner_start,
                inner_end,
                line: d.line,
                block,
                header,
                body,
            });
            i = j + 1;
            continue;
        }
        // No closer before the barrier. Odd count past it → the mark was
        // split (W008). Even → the opener is unclosed (W006/W007) and the
        // later delimiters are their own marks.
        let (count, last_idx) = count_past_barrier(md, &opaque, delims, i, barrier, d.kind, block);
        if count % 2 == 1 {
            if let Some(cj) = last_idx {
                skip[cj] = true;
            }
            issues.push(Issue {
                line: d.line,
                code: "rustlab:W008",
                message: split_message(d.kind, block),
            });
            warns.push(WarnDelim {
                at: d.at,
                kind: d.kind,
                line: d.line,
                code: "rustlab:W008",
            });
        } else {
            let code = if d.kind == Kind::Hi {
                "rustlab:W006"
            } else {
                "rustlab:W007"
            };
            issues.push(Issue {
                line: d.line,
                code,
                message: unclosed_message(d.kind, d.line),
            });
            warns.push(WarnDelim {
                at: d.at,
                kind: d.kind,
                line: d.line,
                code,
            });
        }
        i += 1;
    }

    // Ids, duplicates, orphans, missing ids. Unclosed notes are not W012.
    let mut seen: HashMap<String, usize> = HashMap::new();
    for m in &marks {
        if m.kind != Kind::Note {
            continue;
        }
        if let Some(id) = &m.header.id {
            if let Some(prev) = seen.insert(id.clone(), m.line) {
                issues.push(Issue {
                    line: m.line,
                    code: "rustlab:W010",
                    message: format!("duplicate comment id #{id} (also on line {prev})"),
                });
            }
        } else {
            issues.push(Issue {
                line: m.line,
                code: "rustlab:W012",
                message: "comment has no #cN id".to_string(),
            });
        }
    }
    for m in &marks {
        if m.kind != Kind::Note {
            continue;
        }
        if let Some(re) = &m.header.reply_to {
            if !seen.contains_key(re) {
                issues.push(Issue {
                    line: m.line,
                    code: "rustlab:W011",
                    message: format!("orphan reply re #{re} (no #{re} in this file)"),
                });
            }
        }
    }
    issues.sort_by_key(|f| (f.line, f.code));
    Scan {
        marks,
        issues,
        warns,
        escapes: escapes.clone(),
    }
}

fn split_message(kind: Kind, block: bool) -> String {
    match kind {
        Kind::Hi => "opener and closer of `==` are split by a blank line, heading, or fence".into(),
        Kind::Note if block => {
            "opener and closer of a block `%%` are split by a heading or fence".into()
        }
        Kind::Note => "opener and closer of `%%` are split by a heading or fence".into(),
    }
}

fn unclosed_message(kind: Kind, line: usize) -> String {
    match kind {
        Kind::Hi => format!("unclosed `==` (opened on line {line})"),
        Kind::Note => format!("unclosed `%%` (opened on line {line})"),
    }
}

fn find_closer(
    md: &str,
    delims: &[Delim],
    open_i: usize,
    barrier: usize,
    block: bool,
) -> Option<usize> {
    let kind = delims[open_i].kind;
    for (j, d) in delims.iter().enumerate().skip(open_i + 1) {
        if d.at >= barrier || d.kind != kind {
            continue;
        }
        if block && !line_trimmed_eq(md, d.at, "%%") {
            continue;
        }
        return Some(j);
    }
    None
}

/// Count same-kind delimiters after `barrier` and before the next barrier.
/// Returns `(count, index of the last one)`.
fn count_past_barrier(
    md: &str,
    opaque: &[(usize, usize)],
    delims: &[Delim],
    open_i: usize,
    barrier: usize,
    kind: Kind,
    block: bool,
) -> (usize, Option<usize>) {
    // A blank line is a hard stop for inline notes: later `%%` are new
    // marks (W007, not W008). W008's blank-line case is for `==` only.
    if kind == Kind::Note && !block {
        let between_blank = md[delims[open_i].at..barrier.min(md.len())].contains("\n\n")
            || (barrier < md.len() && line_is_blank_at(md, barrier));
        if between_blank {
            return (0, None);
        }
    }
    let next = next_barrier(md, opaque, barrier.saturating_add(1), kind, block);
    let mut count = 0;
    let mut last = None;
    for (j, d) in delims.iter().enumerate().skip(open_i + 1) {
        if d.kind != kind || d.at < barrier || d.at >= next {
            continue;
        }
        count += 1;
        last = Some(j);
    }
    (count, last)
}

fn next_barrier(
    md: &str,
    opaque: &[(usize, usize)],
    from: usize,
    _kind: Kind,
    block_note: bool,
) -> usize {
    let b = md.as_bytes();
    if from >= b.len() {
        return b.len();
    }
    // Unescaped pipe closes a table cell.
    if line_is_table_row(md, from) {
        if let Some(p) = next_unescaped_pipe(b, from) {
            return p;
        }
    }
    let mut i = from;
    while i < b.len() {
        if let Some(&(start, end)) = opaque.iter().find(|&&(s, e)| e > i && s >= from) {
            if start >= from {
                return start;
            }
            i = end;
            continue;
        }
        if b[i] == b'\n' {
            let next = i + 1;
            if next >= b.len() {
                return b.len();
            }
            let line = line_content(md, next);
            if is_atx_heading(line) {
                return next;
            }
            if !block_note && line.trim().is_empty() {
                return next;
            }
            i = next;
            continue;
        }
        i += 1;
    }
    b.len()
}

fn line_is_blank_at(md: &str, at: usize) -> bool {
    if at >= md.len() {
        return false;
    }
    line_content(md, at).trim().is_empty()
}

fn line_is_table_row(md: &str, at: usize) -> bool {
    let line = line_content(md, at);
    let t = line.trim_start();
    t.starts_with('|')
}

fn next_unescaped_pipe(b: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < b.len() && b[i] != b'\n' {
        if b[i] == b'\\' && i + 1 < b.len() {
            i += 2;
            continue;
        }
        if b[i] == b'|' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn is_atx_heading(line: &str) -> bool {
    let t = line.trim_start();
    let hashes = t.bytes().take_while(|&c| c == b'#').count();
    (1..=6).contains(&hashes) && t.len() > hashes && t.as_bytes()[hashes] == b' '
}

fn line_trimmed_eq(md: &str, at: usize, want: &str) -> bool {
    line_content(md, at).trim() == want
}

fn line_content(md: &str, at: usize) -> &str {
    let start = line_start(md, at);
    let end = md[at..].find('\n').map(|p| at + p).unwrap_or(md.len());
    &md[start..end]
}

fn line_start(md: &str, at: usize) -> usize {
    md[..at].rfind('\n').map(|p| p + 1).unwrap_or(0)
}

fn line_no(md: &str, at: usize) -> usize {
    md[..at].bytes().filter(|&b| b == b'\n').count() + 1
}

fn opaque_end_at(ranges: &[(usize, usize)], i: usize) -> Option<usize> {
    ranges
        .iter()
        .find(|&&(s, e)| i >= s && i < e)
        .map(|&(_, e)| e)
}

fn ranges_overlap(a0: usize, a1: usize, b0: usize, b1: usize) -> bool {
    a0 < b1 && b0 < a1
}

/// Regions the scanner does not read: frontmatter, fences, indented code,
/// inline code, math, HTML comments.
fn opaque_ranges(md: &str) -> Vec<(usize, usize)> {
    let b = md.as_bytes();
    let n = b.len();
    let mut out = Vec::new();
    let mut i = 0;
    if md.starts_with("---\n") || md.starts_with("---\r\n") {
        let rest = if md.starts_with("---\r\n") { 5 } else { 4 };
        if let Some(rel) = md[rest..].find("\n---") {
            let mut end = rest + rel + 4;
            if end < n && b[end] == b'\r' {
                end += 1;
            }
            if end < n && b[end] == b'\n' {
                end += 1;
            }
            let close_line = line_content(md, rest + rel + 1);
            if close_line.trim() == "---" {
                out.push((0, end));
                i = end;
            }
        }
    }
    let mut at_line_start = i == 0 || (i > 0 && b[i - 1] == b'\n');
    let mut prev_blank = true;
    let mut in_indented = false;
    while i < n {
        if at_line_start {
            let blank = line_content(md, i).trim().is_empty();
            let indent = indent_width(b, i);
            if in_indented {
                if blank || indent >= 4 {
                    let eol = line_end(b, i);
                    if !blank {
                        out.push((i, eol));
                    }
                    prev_blank = blank;
                    i = eol;
                    at_line_start = true;
                    if !blank {
                        in_indented = true;
                    }
                    continue;
                }
                in_indented = false;
            } else if prev_blank && indent >= 4 && !blank {
                let eol = line_end(b, i);
                out.push((i, eol));
                i = eol;
                at_line_start = true;
                prev_blank = false;
                in_indented = true;
                continue;
            }
            if let Some((_, fc, flen)) = detect_fence_open(b, i) {
                let start = i;
                i = line_end(b, i);
                while i < n {
                    let next = line_end(b, i);
                    let line = &md[i..next.min(n)];
                    let line = line.strip_suffix('\n').unwrap_or(line);
                    let line = line.strip_suffix('\r').unwrap_or(line);
                    let close = is_close_fence(line.as_bytes(), fc, flen);
                    i = next;
                    if close {
                        break;
                    }
                }
                out.push((start, i));
                at_line_start = true;
                prev_blank = false;
                in_indented = false;
                continue;
            }
            prev_blank = blank;
        }
        if b[i] == b'<' && md[i..].starts_with("<!--") {
            let start = i;
            if let Some(rel) = md[i + 4..].find("-->") {
                i = i + 4 + rel + 3;
                out.push((start, i));
                at_line_start =
                    i > 0 && i <= n && (i == n || b.get(i.saturating_sub(1)) == Some(&b'\n'));
                if i > 0 && b[i - 1] == b'\n' {
                    at_line_start = true;
                }
                continue;
            }
        }
        if b[i] == b'`' {
            let run_start = i;
            while i < n && b[i] == b'`' {
                i += 1;
            }
            let open_len = i - run_start;
            let mut j = i;
            let mut close = None;
            while j < n {
                if b[j] == b'`' {
                    let cs = j;
                    while j < n && b[j] == b'`' {
                        j += 1;
                    }
                    if j - cs == open_len {
                        close = Some(j);
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            if let Some(ce) = close {
                out.push((run_start, ce));
                at_line_start = ce > 0 && b[ce - 1] == b'\n';
                i = ce;
                continue;
            }
            i = run_start + 1;
            at_line_start = false;
            continue;
        }
        if b[i] == b'\\' && i + 1 < n && (b[i + 1] == b'$' || b[i + 1] == b'`') {
            i += 2;
            at_line_start = false;
            continue;
        }
        if b[i] == b'$' && i + 1 < n && b[i + 1] == b'$' {
            if let Some(close) = find_display_math_close(b, i + 2) {
                out.push((i, close + 2));
                i = close + 2;
                at_line_start = i > 0 && i <= n && b.get(i.saturating_sub(1)) == Some(&b'\n');
                continue;
            }
        }
        if b[i] == b'$' && is_inline_math_open(b, i) {
            if let Some(close) = find_inline_math_close(b, i + 1, line_is_table_row(md, i)) {
                out.push((i, close + 1));
                i = close + 1;
                at_line_start = false;
                continue;
            }
        }
        at_line_start = b[i] == b'\n';
        if b[i] == b'\n' {
            // prev_blank updated at the next line start
        }
        i += 1;
    }
    merge_ranges(out)
}

fn merge_ranges(mut v: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    if v.is_empty() {
        return v;
    }
    v.sort_by_key(|&(s, _)| s);
    let mut out = Vec::new();
    let mut cur = v[0];
    for &(s, e) in v.iter().skip(1) {
        if s <= cur.1 {
            cur.1 = cur.1.max(e);
        } else {
            out.push(cur);
            cur = (s, e);
        }
    }
    out.push(cur);
    out
}

fn indent_width(b: &[u8], i: usize) -> usize {
    let mut n = 0;
    let mut j = i;
    while j < b.len() && b[j] == b' ' {
        n += 1;
        j += 1;
    }
    if j < b.len() && b[j] == b'\t' {
        n += 4;
    }
    n
}

fn line_end(b: &[u8], i: usize) -> usize {
    match b[i..].iter().position(|&c| c == b'\n') {
        Some(p) => i + p + 1,
        None => b.len(),
    }
}

fn detect_fence_open(s: &[u8], i: usize) -> Option<(usize, u8, usize)> {
    let n = s.len();
    let mut j = i;
    let mut spaces = 0;
    while j < n && s[j] == b' ' && spaces < 4 {
        j += 1;
        spaces += 1;
    }
    if spaces >= 4 || j >= n {
        return None;
    }
    let fc = s[j];
    if fc != b'`' && fc != b'~' {
        return None;
    }
    let start = j;
    while j < n && s[j] == fc {
        j += 1;
    }
    let len = j - start;
    if len < 3 {
        return None;
    }
    Some((j, fc, len))
}

fn is_close_fence(line: &[u8], fc: u8, fence_len: usize) -> bool {
    let mut j = 0;
    let mut spaces = 0;
    while j < line.len() && line[j] == b' ' && spaces < 4 {
        j += 1;
        spaces += 1;
    }
    if spaces >= 4 {
        return false;
    }
    let start = j;
    while j < line.len() && line[j] == fc {
        j += 1;
    }
    if j - start < fence_len {
        return false;
    }
    line[j..].iter().all(|&c| c == b' ' || c == b'\r')
}

fn find_display_math_close(s: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < s.len() {
        if s[i] == b'\\' {
            i += 2;
            continue;
        }
        if s[i] == b'$' && s[i + 1] == b'$' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn is_inline_math_open(s: &[u8], i: usize) -> bool {
    if i > 0 && s[i - 1] == b'\\' {
        return false;
    }
    // A `$` followed by a digit is currency, matching protect_math's close rule
    // only loosely: we still try to close, and find_inline_math_close rejects
    // a close `$` followed by a digit.
    true
}

fn find_inline_math_close(s: &[u8], from: usize, table: bool) -> Option<usize> {
    let mut i = from;
    while i < s.len() && s[i] != b'\n' {
        if s[i] == b'\\' {
            i += 2;
            continue;
        }
        if table && s[i] == b'|' {
            return None;
        }
        if s[i] == b'$' {
            let next = s.get(i + 1).copied();
            if next != Some(b'$') && !next.is_some_and(|c| c.is_ascii_digit()) {
                if i == from {
                    return None;
                }
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

// ── HTML prepare / restore ─────────────────────────────────────────────────

struct NoteOut {
    placeholder: String,
    html: String,
    block: bool,
}

struct MarkOut {
    open_ph: String,
    close_ph: String,
    open_html: String,
    close_html: String,
}

struct WarnOut {
    ph: String,
    html: String,
}

pub(crate) struct Prepared {
    /// `None` when `source` can be parsed unchanged.
    markdown: Option<String>,
    notes: Vec<NoteOut>,
    marks: Vec<MarkOut>,
    warns: Vec<WarnOut>,
}

impl Prepared {
    fn identity() -> Self {
        Self {
            markdown: None,
            notes: Vec::new(),
            marks: Vec::new(),
            warns: Vec::new(),
        }
    }

    pub(crate) fn as_str<'a>(&'a self, original: &'a str) -> &'a str {
        self.markdown.as_deref().unwrap_or(original)
    }

    pub(crate) fn changed(&self) -> bool {
        self.markdown.is_some()
    }
}

/// Rewrite `md` for the HTML pipeline.
///
/// Showing comments replaces delimiters with sentinels that [`restore`]
/// turns into trusted HTML after the sanitizer. Hiding them unwraps
/// well-formed `==`, deletes well-formed `%%`, and leaves unclosed
/// delimiters as literal text with no badge. A string with no marks and
/// no escapes is returned unchanged.
pub(crate) fn prepare(md: &str) -> Prepared {
    if in_body() {
        return Prepared::identity();
    }
    if !in_page() {
        NOTE_N.with(|c| c.set(0));
    }
    let show = mode().show;
    let scanned = scan(md);
    if scanned.marks.is_empty() && scanned.warns.is_empty() && scanned.escapes.is_empty() {
        return Prepared::identity();
    }
    if !show {
        return Prepared {
            markdown: Some(strip_with(&scanned, md)),
            notes: Vec::new(),
            marks: Vec::new(),
            warns: Vec::new(),
        };
    }
    build_shown(md, &scanned)
}

fn strip_with(scanned: &Scan, md: &str) -> String {
    // Replace from the end so earlier offsets stay valid.
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for m in &scanned.marks {
        if m.kind == Kind::Hi {
            edits.push((m.start, m.end, md[m.inner_start..m.inner_end].to_string()));
        } else {
            // Delete the comment only. A single binding space stays, so
            // `==keep== %%gone%% and` becomes `keep  and` after unwrap.
            edits.push((m.start, m.end, String::new()));
        }
    }
    for &(at, kind) in &scanned.escapes {
        let s = match kind {
            Kind::Hi => "==",
            Kind::Note => "%%",
        };
        edits.push((at, at + 3, s.to_string()));
    }
    apply_edits(md, &mut edits)
}

fn apply_edits(md: &str, edits: &mut Vec<(usize, usize, String)>) -> String {
    edits.sort_by_key(|(s, _, _)| *s);
    // Drop overlaps (a binding-space edit and nothing else should overlap).
    let mut out = String::with_capacity(md.len());
    let mut cursor = 0;
    for (s, e, repl) in edits {
        if *s < cursor {
            continue;
        }
        out.push_str(&md[cursor..*s]);
        out.push_str(repl);
        cursor = *e;
    }
    out.push_str(&md[cursor..]);
    out
}

fn build_shown(md: &str, scanned: &Scan) -> Prepared {
    let annotate = mode().annotate;
    // Replies nest in the parent card; they do not take a number.
    let mut reply_of: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, m) in scanned.marks.iter().enumerate() {
        if m.kind == Kind::Note {
            if let Some(re) = &m.header.reply_to {
                reply_of.entry(re.clone()).or_default().push(idx);
            }
        }
    }
    let mut notes = Vec::new();
    let mut marks_out = Vec::new();
    let mut warns_out = Vec::new();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();

    // Numbers are assigned before highlights so a bound note can set
    // aria-describedby. Replies folded into a parent card take no number.
    let mut note_n_assigned: HashMap<usize, u32> = HashMap::new();
    for (idx, m) in scanned.marks.iter().enumerate() {
        if m.kind == Kind::Note && !folded_reply(scanned, m) {
            note_n_assigned.insert(idx, next_note());
        }
    }

    // Highlight open/close sentinels, then notes.
    let mut hi_n = 0u32;
    for m in &scanned.marks {
        if m.kind != Kind::Hi {
            continue;
        }
        let open_ph = format!("\u{E002}H{hi_n}\u{E003}");
        let close_ph = format!("\u{E004}H{hi_n}\u{E003}");
        let bound = bound_note(md, scanned, m);
        let described = bound.and_then(|n| {
            scanned
                .marks
                .iter()
                .position(|p| std::ptr::eq(p, n))
                .and_then(|i| note_n_assigned.get(&i).copied())
        });
        let aria = described
            .map(|n| format!(" aria-describedby=\"cm-n{n}\""))
            .unwrap_or_default();
        let open_html = format!(
            "<mark class=\"rl-cm rl-cm-mark\" data-src-start=\"{}\" data-src-end=\"{}\"{aria}>",
            m.start, m.end
        );
        marks_out.push(MarkOut {
            open_ph: open_ph.clone(),
            close_ph: close_ph.clone(),
            open_html,
            close_html: "</mark>".to_string(),
        });
        // Leave the inner markdown between the sentinels.
        let mut repl = String::new();
        repl.push_str(&open_ph);
        repl.push_str(&md[m.inner_start..m.inner_end]);
        repl.push_str(&close_ph);
        edits.push((m.start, m.end, repl));
        hi_n += 1;
    }

    for (idx, m) in scanned.marks.iter().enumerate() {
        if m.kind != Kind::Note {
            continue;
        }
        // A reply with a parent in this document is rendered inside that card.
        if folded_reply(scanned, m) {
            edits.push((m.start, m.end, String::new()));
            continue;
        }
        let num = note_n_assigned[&idx];
        let ph = format!("\u{E002}C{idx}\u{E003}");
        let replies = m
            .header
            .id
            .as_ref()
            .and_then(|id| reply_of.get(id))
            .map(|xs| {
                xs.iter()
                    .map(|&ri| render_reply(&scanned.marks[ri]))
                    .collect::<String>()
            })
            .unwrap_or_default();
        let html = render_note_html(m, num, &replies, annotate);
        notes.push(NoteOut {
            placeholder: ph.clone(),
            html,
            block: m.block,
        });
        edits.push((m.start, m.end, ph));
    }

    for w in &scanned.warns {
        let ph = format!("\u{E002}W{}\u{E003}", w.at);
        let label = if w.kind == Kind::Hi {
            "Unclosed highlight"
        } else {
            "Unclosed comment"
        };
        let shown = if w.kind == Kind::Hi { "==" } else { "%%" };
        let html = format!(
            "{shown}<span class=\"rl-cm-warn\" role=\"img\" aria-label=\"{label}\" title=\"{label} (line {})\">⚠</span>",
            w.line
        );
        warns_out.push(WarnOut {
            ph: ph.clone(),
            html,
        });
        edits.push((w.at, w.at + 2, ph));
    }
    for &(at, kind) in &scanned.escapes {
        let s = match kind {
            Kind::Hi => "==",
            Kind::Note => "%%",
        };
        edits.push((at, at + 3, s.to_string()));
    }

    Prepared {
        markdown: Some(apply_edits(md, &mut edits)),
        notes,
        marks: marks_out,
        warns: warns_out,
    }
}

fn folded_reply(scanned: &Scan, m: &Mark) -> bool {
    m.header.reply_to.as_ref().is_some_and(|re| {
        scanned
            .marks
            .iter()
            .any(|p| p.header.id.as_deref() == Some(re.as_str()))
    })
}

fn bound_note<'a>(md: &str, scanned: &'a Scan, hi: &Mark) -> Option<&'a Mark> {
    scanned.marks.iter().find(|n| {
        n.kind == Kind::Note
            && n.line == line_no(md, hi.end.saturating_sub(1))
            && n.start >= hi.end
            && n.start - hi.end <= 1
            && (n.start == hi.end || md.as_bytes().get(hi.end) == Some(&b' '))
            && md[hi.end..n.start].chars().all(|c| c == ' ')
            && n.header.reply_to.is_none()
    })
}

fn render_reply(m: &Mark) -> String {
    let body = render_body_html(&m.body);
    let who = header_label(&m.header);
    format!(
        "<div class=\"rl-cm-reply\">{who}{body}</div>",
        who = who,
        body = body
    )
}

fn header_label(h: &CommentHeader) -> String {
    let mut bits = Vec::new();
    if let Some(a) = &h.author {
        bits.push(format!("@{}", crate::render::escape_html(a)));
    }
    if let Some(d) = &h.date {
        bits.push(crate::render::escape_html(d));
    }
    if bits.is_empty() {
        String::new()
    } else {
        format!("<span class=\"rl-cm-meta\">{}</span> ", bits.join(" "))
    }
}

fn render_body_html(body: &str) -> String {
    let _g = enter_body();
    let html = crate::render::markdown_to_html(body);
    strip_wrapping_p(&html)
}

fn strip_wrapping_p(html: &str) -> String {
    let t = html.trim();
    if let Some(inner) = t.strip_prefix("<p>").and_then(|s| s.strip_suffix("</p>")) {
        if !inner.contains("<p>") {
            return inner.trim().to_string();
        }
    }
    if let Some(inner) = t.strip_prefix("<p>").and_then(|s| s.strip_suffix("</p>\n")) {
        if !inner.contains("<p>") {
            return inner.trim().to_string();
        }
    }
    t.to_string()
}

fn render_note_html(m: &Mark, num: u32, replies: &str, annotate: bool) -> String {
    let body = render_body_html(&m.body);
    let id_attr = m
        .header
        .id
        .as_ref()
        .map(|id| format!(" data-cm-id=\"{}\"", crate::render::escape_html(id)))
        .unwrap_or_default();
    let buttons = if annotate {
        "<span class=\"rl-cm-actions\"><button type=\"button\" class=\"rl-cm-edit\">Edit</button><button type=\"button\" class=\"rl-cm-delete\">Delete</button></span>"
    } else {
        ""
    };
    let class = if m.block {
        "rl-cm-blocknote"
    } else {
        "rl-cm-note"
    };
    let tag = if m.block { "div" } else { "span" };
    format!(
        "<{tag} class=\"{class}\" id=\"cm-n{num}\" role=\"note\" tabindex=\"0\"{id_attr} data-src-start=\"{}\" data-src-end=\"{}\"><sup class=\"rl-cm-num\">{num}</sup><span class=\"rl-cm-body\">{meta}{body}{replies}</span>{buttons}</{tag}>",
        m.start,
        m.end,
        meta = header_label(&m.header),
    )
}

pub(crate) fn restore(html: &str, prep: &Prepared) -> String {
    if !prep.changed() {
        return html.to_string();
    }
    let mut html = move_sentinels_out_of_headings(html);
    for n in &prep.notes {
        if n.block {
            let wrapped = format!("<p>{}</p>", n.placeholder);
            html = html.replace(&wrapped, &n.html);
            let wrapped_nl = format!("<p>{}</p>\n", n.placeholder);
            html = html.replace(&wrapped_nl, &n.html);
        }
        html = html.replace(&n.placeholder, &n.html);
    }
    for m in &prep.marks {
        html = html.replace(&m.open_ph, &m.open_html);
        html = html.replace(&m.close_ph, &m.close_html);
    }
    for w in &prep.warns {
        html = html.replace(&w.ph, &w.html);
    }
    html
}

fn move_sentinels_out_of_headings(html: &str) -> String {
    let b = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' && i + 2 < b.len() && b[i + 1] == b'h' && b[i + 2].is_ascii_digit() {
            let level = b[i + 2];
            if let Some(rel) = html[i..].find('>') {
                let start = i;
                let close = format!("</h{}>", level as char);
                if let Some(end_rel) = html[i..].find(&close) {
                    let end = i + end_rel + close.len();
                    let inner = &html[start + rel + 1..i + end_rel];
                    let (kept, moved) = extract_note_sentinels(inner);
                    out.push_str(&html[start..start + rel + 1]);
                    out.push_str(&kept);
                    out.push_str(&close);
                    out.push_str(&moved);
                    i = end;
                    continue;
                }
            }
        }
        let ch = html[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn extract_note_sentinels(inner: &str) -> (String, String) {
    let mut kept = String::new();
    let mut moved = String::new();
    let b = inner.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0xEE {
            // UTF-8 for U+E002 is EE 80 82. Look for the sentinel prefix.
        }
        if inner[i..].starts_with('\u{E002}') {
            if let Some(rel) = inner[i..].find('\u{E003}') {
                let end = i + rel + '\u{E003}'.len_utf8();
                let tok = &inner[i..end];
                if tok.contains('C') || tok.contains('W') {
                    moved.push_str(tok);
                } else {
                    kept.push_str(tok);
                }
                i = end;
                continue;
            }
        }
        let ch = inner[i..].chars().next().unwrap();
        kept.push(ch);
        i += ch.len_utf8();
    }
    (kept, moved)
}

/// Strip well-formed marks. Unclosed delimiters stay. Escapes become the
/// delimiter. Returns `md` unchanged when there is nothing to do — the
/// caller can skip the rewrite.
pub(crate) fn strip_source(md: &str) -> String {
    let scanned = scan(md);
    if scanned.marks.is_empty() && scanned.escapes.is_empty() {
        return md.to_string();
    }
    strip_with(&scanned, md)
}

/// Rewrite `md` for the LaTeX pipeline.
///
/// With comments off, well-formed marks are stripped (highlight text kept,
/// comments deleted). With comments on, highlights become sentinels that
/// [`restore_latex`] turns into `\hl`, and notes become `\marginpar` (or
/// `\footnote` on a table row). Unclosed delimiters stay literal.
pub(crate) fn prepare_latex(md: &str) -> Prepared {
    if in_body() {
        return Prepared::identity();
    }
    let scanned = scan(md);
    if scanned.marks.is_empty() && scanned.warns.is_empty() && scanned.escapes.is_empty() {
        return Prepared::identity();
    }
    if !mode().latex {
        return Prepared {
            markdown: Some(strip_with(&scanned, md)),
            notes: Vec::new(),
            marks: Vec::new(),
            warns: Vec::new(),
        };
    }
    build_latex(md, &scanned)
}

fn build_latex(md: &str, scanned: &Scan) -> Prepared {
    let mut reply_of: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, m) in scanned.marks.iter().enumerate() {
        if m.kind == Kind::Note {
            if let Some(re) = &m.header.reply_to {
                reply_of.entry(re.clone()).or_default().push(idx);
            }
        }
    }
    let mut notes = Vec::new();
    let mut marks_out = Vec::new();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut hi_n = 0u32;
    for m in &scanned.marks {
        if m.kind != Kind::Hi {
            continue;
        }
        let open_ph = format!("\u{E010}H{hi_n}\u{E011}");
        let close_ph = format!("\u{E010}h{hi_n}\u{E011}");
        marks_out.push(MarkOut {
            open_ph: open_ph.clone(),
            close_ph: close_ph.clone(),
            open_html: "\\hl{".to_string(),
            close_html: "}".to_string(),
        });
        let mut repl = String::new();
        repl.push_str(&open_ph);
        repl.push_str(&md[m.inner_start..m.inner_end]);
        repl.push_str(&close_ph);
        edits.push((m.start, m.end, repl));
        hi_n += 1;
    }
    for (idx, m) in scanned.marks.iter().enumerate() {
        if m.kind != Kind::Note {
            continue;
        }
        if folded_reply(scanned, m) {
            edits.push((m.start, m.end, String::new()));
            continue;
        }
        let ph = format!("\u{E010}C{idx}\u{E011}");
        let replies = m
            .header
            .id
            .as_ref()
            .and_then(|id| reply_of.get(id))
            .map(|xs| {
                xs.iter()
                    .map(|&ri| render_reply_latex(&scanned.marks[ri]))
                    .collect::<String>()
            })
            .unwrap_or_default();
        let in_table = line_is_table_row(md, m.start);
        let html = render_note_latex(m, &replies, in_table);
        notes.push(NoteOut {
            placeholder: ph.clone(),
            html,
            block: m.block,
        });
        edits.push((m.start, m.end, ph));
    }
    for &(at, kind) in &scanned.escapes {
        let s = match kind {
            Kind::Hi => "==",
            Kind::Note => "%%",
        };
        edits.push((at, at + 3, s.to_string()));
    }
    Prepared {
        markdown: Some(apply_edits(md, &mut edits)),
        notes,
        marks: marks_out,
        warns: Vec::new(),
    }
}

pub(crate) fn restore_latex(tex: &str, prep: &Prepared) -> String {
    if !prep.changed() {
        return tex.to_string();
    }
    let mut tex = tex.to_string();
    for m in &prep.marks {
        tex = tex.replace(&m.open_ph, &m.open_html);
        tex = tex.replace(&m.close_ph, &m.close_html);
    }
    for n in &prep.notes {
        tex = tex.replace(&n.placeholder, &n.html);
    }
    tex
}

/// `soul` only when comments are included, so a default PDF does not
/// depend on that package.
pub(crate) fn latex_packages() -> &'static str {
    if mode().latex {
        "\\usepackage{soul}\n\\sethlcolor{yellow!35}\n"
    } else {
        ""
    }
}

fn render_reply_latex(m: &Mark) -> String {
    let body = latex_fragment(&m.body);
    let who = latex_header_label(&m.header);
    format!("\\\\ {who}{body}")
}

fn latex_header_label(h: &CommentHeader) -> String {
    let mut bits = Vec::new();
    if let Some(a) = &h.author {
        bits.push(format!("@{a}"));
    }
    if let Some(d) = &h.date {
        bits.push(d.clone());
    }
    if bits.is_empty() {
        String::new()
    } else {
        format!(
            "{} ",
            crate::render_latex::escape_latex_pub(&bits.join(" "))
        )
    }
}

fn latex_fragment(md: &str) -> String {
    let _g = enter_body();
    crate::render_latex::markdown_fragment_to_latex(md)
}

fn render_note_latex(m: &Mark, replies: &str, in_table: bool) -> String {
    let body = latex_fragment(&m.body);
    let inner = format!("{}{body}{replies}", latex_header_label(&m.header));
    if in_table {
        format!("\\footnote{{{inner}}}")
    } else if m.block {
        format!("\\begin{{quote}}{inner}\\end{{quote}}\n")
    } else {
        format!("\\marginpar{{{inner}}}")
    }
}

pub(crate) fn render_cell_notes_latex(notes: &[CellNote]) -> String {
    let mut out = String::new();
    for n in notes {
        let mark = Mark {
            kind: Kind::Note,
            start: n.start,
            end: n.end,
            inner_start: n.start,
            inner_end: n.end,
            line: 1,
            block: true,
            header: n.header.clone(),
            body: n.body.clone(),
        };
        out.push_str(&render_note_latex(&mark, "", false));
    }
    out
}

#[allow(dead_code)]
pub(crate) fn source_has_marks(md: &str) -> bool {
    let (delims, escapes) = find_delims(md, &opaque_ranges(md));
    !delims.is_empty() || !escapes.is_empty()
}

// ── check ──────────────────────────────────────────────────────────────────

pub(crate) fn issues(md: &str) -> Vec<Issue> {
    scan(md).issues
}

// ── cell comments ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct CellNote {
    pub header: CommentHeader,
    pub body: String,
    pub start: usize,
    pub end: usize,
    #[allow(dead_code)]
    pub raw: String,
    #[allow(dead_code)]
    pub block: bool,
}

/// Peel whole-line `%%` comments that sit at the end of `md` with no
/// blank line after them. The caller has already decided the next block
/// is a fence. A trailing blank line (`\n\n`, preserved when a directive
/// was separated from the comment) leaves them as ordinary notes.
pub(crate) fn peel_trailing_cell_comments(md: &str) -> (String, Vec<CellNote>) {
    if md.ends_with("\n\n") || md.ends_with("\r\n\r\n") {
        return (md.to_string(), Vec::new());
    }
    let scanned = scan(md);
    let mut notes: Vec<&Mark> = scanned
        .marks
        .iter()
        .filter(|m| m.kind == Kind::Note)
        .collect();
    let mut peeled_rev = Vec::new();
    let mut limit = md.trim_end_matches(['\n', '\r']).len();
    while let Some(m) = notes.last().copied() {
        if m.end > limit {
            notes.pop();
            continue;
        }
        let gap = &md[m.end..limit];
        if gap.chars().any(|c| !matches!(c, ' ' | '\t' | '\n' | '\r')) {
            break;
        }
        if gap.contains("\n\n") {
            break;
        }
        if !whole_line_note(md, m) {
            break;
        }
        peeled_rev.push(m);
        limit = line_start(md, m.start);
        notes.pop();
    }
    if peeled_rev.is_empty() {
        return (md.to_string(), Vec::new());
    }
    peeled_rev.reverse();
    let cut = line_start(md, peeled_rev[0].start);
    let rest = md[..cut].trim_end().to_string();
    let held = peeled_rev
        .into_iter()
        .map(|m| CellNote {
            header: m.header.clone(),
            body: m.body.clone(),
            start: m.start,
            end: m.end,
            raw: md[m.start..m.end].to_string(),
            block: m.block,
        })
        .collect();
    (rest, held)
}

fn whole_line_note(md: &str, m: &Mark) -> bool {
    if m.block {
        return line_trimmed_eq(md, m.start, "%%")
            && line_trimmed_eq(md, m.end.saturating_sub(2), "%%");
    }
    let line = line_content(md, m.start);
    let t = line.trim();
    t.starts_with("%%")
        && t.ends_with("%%")
        && t.len() >= 4
        && line_start(md, m.start) == line_start(md, m.end.saturating_sub(1))
}

pub(crate) fn render_cell_notes(notes: &[CellNote]) -> String {
    let mut out = String::new();
    for n in notes {
        let num = next_note();
        let mark = Mark {
            kind: Kind::Note,
            start: n.start,
            end: n.end,
            inner_start: n.start,
            inner_end: n.end,
            line: 1,
            block: false,
            header: n.header.clone(),
            body: n.body.clone(),
        };
        // Cell notes use the margin-note card, not a paragraph.
        out.push_str(&render_note_html(&mark, num, "", mode().annotate));
    }
    out
}

pub(crate) fn is_fence_rendered(block: &crate::execute::Rendered) -> bool {
    matches!(
        block,
        crate::execute::Rendered::Code { .. }
            | crate::execute::Rendered::Mermaid { .. }
            | crate::execute::Rendered::Widget { .. }
    )
}

/// Keep a blank line between a trailing `%%` and a following directive
/// so a separated comment does not bind to the fence. Adjacent comments
/// stay adjacent.
pub(crate) fn preserve_unbound_comment_gap(
    markdown_buf: &mut String,
    blank_before_directive: bool,
) {
    if !blank_before_directive {
        return;
    }
    let trimmed_len = markdown_buf.trim_end().len();
    let last = markdown_buf[..trimmed_len].lines().next_back();
    let keep = last.is_some_and(|l| {
        let t = l.trim();
        t == "%%" || (t.starts_with("%%") && t.ends_with("%%") && t.len() >= 4)
    });
    if keep {
        markdown_buf.truncate(trimmed_len);
        markdown_buf.push_str("\n\n");
    }
}

// ── CSS, toggle, annotate client ───────────────────────────────────────────

pub(crate) fn comment_css() -> &'static str {
    r#"
.rl-cm-mark {
  background: var(--rl-cm-mark-bg, #554f4e);
  color: var(--rl-text, inherit);
  border-bottom: 1px solid var(--rl-cm-note-border, var(--rl-accent-secondary));
  padding: 0 0.05em;
}
.rl-cm-note, .rl-cm-blocknote {
  background: var(--rl-cm-note-bg, var(--rl-bg-secondary));
  color: var(--rl-text);
  border: 1px solid var(--rl-cm-note-border, var(--rl-accent-secondary));
  border-radius: 6px;
  font-size: 0.85rem;
  line-height: 1.35;
}
.rl-cm-note {
  display: inline-block;
  vertical-align: super;
  margin-left: 0.15em;
}
.rl-cm-note .rl-cm-body, .rl-cm-blocknote .rl-cm-body { display: none; }
.rl-cm-note:focus-within .rl-cm-body,
.rl-cm-note:focus .rl-cm-body,
.rl-cm-blocknote .rl-cm-body { display: inline; }
.rl-cm-blocknote {
  display: block;
  margin: 0.6rem 0;
  padding: 0.4rem 0.6rem;
}
.rl-cm-blocknote .rl-cm-body { display: block; }
.rl-cm-num { font-weight: 600; color: var(--rl-cm-note-border, var(--rl-accent-secondary)); margin-right: 0.25em; }
.rl-cm-meta { opacity: 0.85; margin-right: 0.35em; }
.rl-cm-reply { margin: 0.35rem 0 0.2rem 1rem; }
.rl-cm-warn { cursor: help; }
.rl-comments-toggle { margin-left: 0.8rem; font-size: 0.85rem; white-space: nowrap; }
.rl-cm-confirm { margin-left: 0.6rem; font-size: 0.8rem; }
body:has(#rl-comments:not(:checked)) .rl-cm-note,
body:has(#rl-comments:not(:checked)) .rl-cm-blocknote { display: none; }
body:has(#rl-comments:not(:checked)) .rl-cm-mark {
  background: transparent;
  border-bottom-color: transparent;
  color: inherit;
}
@media (min-width: 1280px) {
  body:not(.has-files) .rl-cm-note {
    float: right;
    clear: right;
    margin-right: -16rem;
    width: 14rem;
    vertical-align: baseline;
    padding: 0.35rem 0.5rem;
  }
  body:not(.has-files) .rl-cm-note .rl-cm-body { display: inline; }
}
#rl-cm-menu, #rl-cm-pop {
  position: fixed;
  z-index: 50;
  background: var(--rl-cm-note-bg, var(--rl-bg-secondary));
  color: var(--rl-text);
  border: 1px solid var(--rl-cm-note-border, var(--rl-accent-secondary));
  border-radius: 6px;
  padding: 0.25rem;
}
#rl-cm-menu button, #rl-cm-pop button, .rl-cm-actions button {
  display: block;
  width: 100%;
  text-align: left;
  background: transparent;
  color: var(--rl-text);
  border: 2px solid transparent;
  border-radius: 4px;
  padding: 0.25rem 0.5rem;
}
#rl-cm-menu button:hover, #rl-cm-menu button:focus,
#rl-cm-pop button:hover, #rl-cm-pop button:focus,
.rl-cm-actions button:hover, .rl-cm-actions button:focus {
  border-color: var(--rl-cm-note-border, var(--rl-accent-secondary));
  outline: none;
}
@media (pointer: coarse) {
  #rl-cm-menu button, #rl-cm-pop button, .rl-cm-actions button { padding: 0.55rem 0.7rem; }
}
@media print {
  .rl-cm-note { float: none; margin: 0.4rem 0; display: list-item; list-style: decimal; }
  .rl-cm-note .rl-cm-body, .rl-cm-blocknote .rl-cm-body { display: block; }
  .rl-comments-toggle, #rl-cm-menu, #rl-cm-pop, .rl-cm-confirm, .rl-cm-actions { display: none !important; }
}
"#
}

pub(crate) fn toggle_html(checked: bool) -> String {
    let attr = if checked { " checked" } else { "" };
    format!(
        "<label class=\"rl-comments-toggle\"><input type=\"checkbox\" id=\"rl-comments\"{attr}> Comments</label><span id=\"rl-cm-confirm\" class=\"rl-cm-confirm\" role=\"status\"></span>"
    )
}

pub(crate) fn toggle_script(nonce: Option<&str>) -> String {
    let nonce_attr = crate::render::nonce_attr(nonce);
    format!(
        r#"<script{nonce_attr}>
(function () {{
  var box = document.getElementById('rl-comments');
  if (!box) return;
  function apply() {{
    try {{
      var v = sessionStorage.getItem('rl-comments-visible');
      if (v === '0') box.checked = false;
      else if (v === '1') box.checked = true;
    }} catch (e) {{}}
    var c = document.getElementById('rl-cm-confirm');
    if (!c) return;
    try {{
      var msg = sessionStorage.getItem('rl-cm-confirm');
      var until = parseInt(sessionStorage.getItem('rl-cm-confirm-until') || '0', 10);
      if (msg && Date.now() < until && !box.checked) c.textContent = msg;
      else c.textContent = '';
    }} catch (e) {{}}
  }}
  apply();
  box.addEventListener('change', function () {{
    try {{ sessionStorage.setItem('rl-comments-visible', box.checked ? '1' : '0'); }} catch (e) {{}}
    if (box.checked) {{
      var c = document.getElementById('rl-cm-confirm');
      if (c) c.textContent = '';
      try {{ sessionStorage.removeItem('rl-cm-confirm'); }} catch (e) {{}}
    }}
  }});
  var prev = window.__rlAfterUpdate;
  window.__rlAfterUpdate = function () {{
    if (prev) prev();
    box = document.getElementById('rl-comments');
    if (box) apply();
    var menu = document.getElementById('rl-cm-menu');
    var pop = document.getElementById('rl-cm-pop');
    if (menu) menu.hidden = true;
    if (pop) pop.hidden = true;
  }};
}})();
</script>"#
    )
}

pub(crate) fn annotate_chrome() -> &'static str {
    r#"<div id="rl-cm-menu" role="menu" hidden>
<button type="button" role="menuitem" data-act="add" tabindex="-1">Add comment</button>
<button type="button" role="menuitem" data-act="highlight" tabindex="-1">Highlight</button>
<button type="button" role="menuitem" data-act="edit" tabindex="-1">Edit comment</button>
<button type="button" role="menuitem" data-act="delete" tabindex="-1">Delete</button>
</div>
<div id="rl-cm-pop" hidden>
<p id="rl-cm-pop-ctx"></p>
<label>Name <input id="rl-cm-name" type="text" autocomplete="off"></label>
<textarea id="rl-cm-text" rows="3"></textarea>
<button type="button" id="rl-cm-go">Comment</button>
<button type="button" data-act="highlight">Highlight</button>
<button type="button" data-act="delete">Delete</button>
<button type="button" id="rl-cm-cancel">Cancel</button>
</div>"#
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex_encode(&sha256(bytes))
}

/// Compact SHA-256. Kept in-tree so the annotate hash does not pull a
/// new crate through the 1.83 lockfile.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = h;
        for i in 0..64 {
            let s1 = a[4].rotate_right(6) ^ a[4].rotate_right(11) ^ a[4].rotate_right(25);
            let ch = (a[4] & a[5]) ^ (!a[4] & a[6]);
            let t1 = a[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a[0].rotate_right(2) ^ a[0].rotate_right(13) ^ a[0].rotate_right(22);
            let maj = (a[0] & a[1]) ^ (a[0] & a[2]) ^ (a[1] & a[2]);
            let t2 = s0.wrapping_add(maj);
            a[7] = a[6];
            a[6] = a[5];
            a[5] = a[4];
            a[4] = a[3].wrapping_add(t1);
            a[3] = a[2];
            a[2] = a[1];
            a[1] = a[0];
            a[0] = t1.wrapping_add(t2);
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(a[i]);
        }
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

pub(crate) fn source_hash_meta(source: &str) -> String {
    format!(
        "<meta name=\"rl-source-sha256\" content=\"{}\">\n",
        sha256_hex(source.as_bytes())
    )
}

/// Nonce'd annotate client. Item labels are fixed strings. No `on*` handlers.
pub(crate) fn annotate_script(nonce: Option<&str>) -> String {
    let nonce_attr = crate::render::nonce_attr(nonce);
    format!(
        r#"<script{nonce_attr}>
(function () {{
  var menu = document.getElementById('rl-cm-menu');
  var pop = document.getElementById('rl-cm-pop');
  if (!menu || !pop) return;
  var items = [].slice.call(menu.querySelectorAll('[role="menuitem"]'));
  var focusIdx = 0;
  var pending = null;
  var nameEl = document.getElementById('rl-cm-name');
  try {{
    var saved = sessionStorage.getItem('rl-comment-name');
    if (saved) nameEl.value = saved;
  }} catch (e) {{}}

  function hideMenu() {{ menu.hidden = true; }}
  function hidePop() {{ pop.hidden = true; pending = null; }}

  function selectionRange() {{
    var sel = window.getSelection();
    if (!sel || sel.rangeCount === 0 || sel.isCollapsed) return null;
    return sel.getRangeAt(0);
  }}
  function inMain(node) {{
    var main = document.querySelector('main');
    return !!(main && node && main.contains(node));
  }}
  function inEditor(node) {{
    var el = node && node.nodeType === 1 ? node : node && node.parentElement;
    return !!(el && el.closest && el.closest('.CodeMirror, textarea, input, [contenteditable]'));
  }}
  function blockOf(node) {{
    var el = node && node.nodeType === 1 ? node : node && node.parentElement;
    return el && el.closest ? el.closest('section.rl-block') : null;
  }}
  function srcSpan(node) {{
    var el = node && node.nodeType === 1 ? node : node && node.parentElement;
    return el && el.closest ? el.closest('[data-src-start]') : null;
  }}
  function classify(range) {{
    if (!range) return null;
    var a = range.startContainer, b = range.endContainer;
    if (!inMain(a) || !inMain(b) || inEditor(a) || inEditor(b)) return null;
    var ba = blockOf(a), bb = blockOf(b);
    if (!ba || ba !== bb) return null;
    if (range.cloneContents().querySelector && range.cloneContents().querySelector('.rl-math')) return null;
    var mark = (a.nodeType === 1 ? a : a.parentElement);
    mark = mark && mark.closest ? mark.closest('.rl-cm-mark, .rl-cm-note, .rl-cm-blocknote') : null;
    var endEl = b.nodeType === 1 ? b : b.parentElement;
    var mark2 = endEl && endEl.closest ? endEl.closest('.rl-cm-mark, .rl-cm-note, .rl-cm-blocknote') : null;
    if (mark && mark2 && mark !== mark2) return null;
    var code = ba.getAttribute('data-src-kind') === 'code';
    var span = srcSpan(a);
    if (!span && !ba.hasAttribute('data-src-start')) return null;
    return {{ range: range, block: ba, mark: mark || mark2, code: code, span: span }};
  }}
  function offsetsOf(info) {{
    var span = info.span || info.mark || info.block;
    var base = parseInt(span.getAttribute('data-src-start'), 10);
    var end = parseInt(span.getAttribute('data-src-end'), 10);
    if (info.mark) return {{ start: base, end: end, text: info.mark.textContent || '' }};
    var pre = info.range.cloneRange();
    pre.setStart(span, 0);
    pre.setEnd(info.range.startContainer, info.range.startOffset);
    var lead = new TextEncoder().encode(pre.toString()).length;
    var sel = new TextEncoder().encode(info.range.toString()).length;
    return {{ start: base + lead, end: base + lead + sel, text: info.range.toString() }};
  }}
  function showItems(which) {{
    items.forEach(function (btn) {{
      var act = btn.getAttribute('data-act');
      btn.hidden = which.indexOf(act) < 0;
    }});
  }}
  function visibleItems() {{ return items.filter(function (b) {{ return !b.hidden; }}); }}
  function openMenu(x, y, info) {{
    var which;
    if (info.mark) {{
      var bare = info.mark.classList.contains('rl-cm-mark') && !info.mark.getAttribute('aria-describedby');
      which = bare ? ['add', 'delete'] : ['edit', 'delete'];
    }} else if (info.code) {{
      which = ['add'];
    }} else {{
      which = ['add', 'highlight'];
    }}
    showItems(which);
    pending = info;
    menu.hidden = false;
    menu.style.left = Math.max(8, Math.min(x, window.innerWidth - 180)) + 'px';
    menu.style.top = Math.max(8, Math.min(y, window.innerHeight - 160)) + 'px';
    focusIdx = 0;
    var vis = visibleItems();
    if (vis[0]) vis[0].focus();
  }}
  function openPop(info, editing) {{
    pending = info;
    pop.hidden = false;
    var ctx = document.getElementById('rl-cm-pop-ctx');
    ctx.textContent = info.code ? 'Comment on this cell' : '';
    document.getElementById('rl-cm-text').value = editing ? (info.editBody || '') : '';
    document.getElementById('rl-cm-go').textContent = editing ? 'Save' : 'Comment';
    var hi = pop.querySelector('[data-act="highlight"]');
    var del = pop.querySelector('[data-act="delete"]');
    hi.hidden = info.code || !!info.mark;
    del.hidden = !info.mark;
    pop.style.left = '12px';
    pop.style.top = '4rem';
    document.getElementById('rl-cm-text').focus();
  }}
  function confirmSaved(kind) {{
    var box = document.getElementById('rl-comments');
    if (box && box.checked) return;
    var msg = (kind === 'highlight' ? 'Highlight' : 'Comment') + ' saved. Turn Comments on to show it.';
    try {{
      sessionStorage.setItem('rl-cm-confirm', msg);
      sessionStorage.setItem('rl-cm-confirm-until', String(Date.now() + 4000));
    }} catch (e) {{}}
    var c = document.getElementById('rl-cm-confirm');
    if (c) c.textContent = msg;
  }}
  function fileHash() {{
    var m = document.querySelector('meta[name="rl-source-sha256"]');
    return m ? m.getAttribute('content') : '';
  }}
  function slug() {{
    var m = document.querySelector('meta[name="rl-slug"]');
    if (m) return m.getAttribute('content');
    var p = location.pathname;
    var i = p.indexOf('/n/');
    return i >= 0 ? decodeURIComponent(p.slice(i + 3)) : '';
  }}
  function post(body, kind) {{
    var expect = fileHash();
    fetch('/annotate/' + encodeURIComponent(slug()), {{
      method: 'POST',
      headers: {{
        'Content-Type': 'application/json',
        'If-Match': '"' + expect + '"'
      }},
      body: JSON.stringify(body)
    }}).then(function (res) {{
      if (res.status === 409) {{
        window.alert('This notebook changed. Reload and try again.');
        return;
      }}
      if (!res.ok) {{
        window.alert('Could not update the note.');
        return;
      }}
      var name = nameEl.value.trim();
      if (name && !/[ %:]/.test(name)) {{
        try {{ sessionStorage.setItem('rl-comment-name', name); }} catch (e) {{}}
      }}
      confirmSaved(kind);
      hideMenu();
      hidePop();
    }}).catch(function () {{ window.alert('Could not update the note.'); }});
  }}
  function act(name) {{
    if (!pending) return;
    var info = pending;
    var off = offsetsOf(info);
    var author = nameEl.value.trim();
    if (author && /[ %:]/.test(author)) {{
      window.alert('Name must be one token without a space, %, or :');
      return;
    }}
    if (name === 'highlight') {{
      post({{ op: 'insert', target: 'prose', start: off.start, end: off.end, text: off.text, comment: '' }}, 'highlight');
      return;
    }}
    if (name === 'delete') {{
      var raw = info.mark ? info.mark.getAttribute('data-src-start') : null;
      post({{
        op: 'delete',
        target: info.code ? 'cell' : 'prose',
        start: off.start,
        end: off.end,
        expect: info.expect || '',
        id: info.mark && info.mark.getAttribute('data-cm-id') || ''
      }}, 'comment');
      return;
    }}
    if (name === 'add' || name === 'edit') {{
      hideMenu();
      info.editBody = '';
      openPop(info, name === 'edit');
    }}
  }}
  menu.addEventListener('click', function (ev) {{
    var btn = ev.target.closest && ev.target.closest('[data-act]');
    if (!btn) return;
    act(btn.getAttribute('data-act'));
  }});
  menu.addEventListener('keydown', function (ev) {{
    var vis = visibleItems();
    if (ev.key === 'ArrowDown') {{ focusIdx = Math.min(vis.length - 1, focusIdx + 1); vis[focusIdx] && vis[focusIdx].focus(); ev.preventDefault(); }}
    else if (ev.key === 'ArrowUp') {{ focusIdx = Math.max(0, focusIdx - 1); vis[focusIdx] && vis[focusIdx].focus(); ev.preventDefault(); }}
    else if (ev.key === 'Home') {{ focusIdx = 0; vis[0] && vis[0].focus(); ev.preventDefault(); }}
    else if (ev.key === 'End') {{ focusIdx = vis.length - 1; vis[focusIdx] && vis[focusIdx].focus(); ev.preventDefault(); }}
    else if (ev.key === 'Escape' || ev.key === 'Tab') {{ hideMenu(); }}
    else if (ev.key === 'Enter' || ev.key === ' ') {{ ev.preventDefault(); var f = document.activeElement; if (f && f.getAttribute) act(f.getAttribute('data-act')); }}
  }});
  document.addEventListener('contextmenu', function (ev) {{
    if (!inMain(ev.target) || inEditor(ev.target)) return;
    if (ev.shiftKey) return;
    var info = classify(selectionRange());
    var mark = ev.target.closest && ev.target.closest('.rl-cm-mark, .rl-cm-note, .rl-cm-blocknote');
    if (!info && mark && inMain(mark)) {{
      info = {{ range: null, block: blockOf(mark), mark: mark, code: false, span: mark }};
    }}
    if (!info) return;
    ev.preventDefault();
    openMenu(ev.clientX, ev.clientY, info);
  }});
  document.addEventListener('keydown', function (ev) {{
    var k = ev.key === 'ContextMenu' || (ev.key === 'F10' && ev.shiftKey);
    if (!k) return;
    var info = classify(selectionRange());
    if (!info) return;
    ev.preventDefault();
    var rect = info.range.getBoundingClientRect();
    openMenu(rect.left, rect.bottom, info);
  }});
  document.addEventListener('pointerup', function (ev) {{
    if (ev.pointerType !== 'touch' && ev.pointerType !== 'pen') return;
    var info = classify(selectionRange());
    if (!info) return;
    openPop(info, false);
  }});
  document.getElementById('rl-cm-go').addEventListener('click', function () {{
    if (!pending) return;
    var text = document.getElementById('rl-cm-text').value;
    if (pending.code && !text.trim()) return;
    var author = nameEl.value.trim();
    if (author && /[ %:]/.test(author)) {{
      window.alert('Name must be one token without a space, %, or :');
      return;
    }}
    var off = offsetsOf(pending);
    var op = document.getElementById('rl-cm-go').textContent === 'Save' ? 'edit' : 'insert';
    post({{
      op: op,
      target: pending.code ? 'cell' : 'prose',
      start: pending.code ? parseInt(pending.block.getAttribute('data-src-start') || '0', 10) : off.start,
      end: off.end,
      text: off.text,
      comment: text,
      author: author,
      id: pending.mark && pending.mark.getAttribute('data-cm-id') || '',
      expect: pending.expect || ''
    }}, 'comment');
  }});
  pop.querySelector('[data-act="highlight"]').addEventListener('click', function () {{ act('highlight'); }});
  pop.querySelector('[data-act="delete"]').addEventListener('click', function () {{ act('delete'); }});
  document.getElementById('rl-cm-cancel').addEventListener('click', hidePop);
  document.addEventListener('click', function (ev) {{
    var t = ev.target;
    if (t.closest && t.closest('.rl-cm-edit')) {{
      var note = t.closest('.rl-cm-note, .rl-cm-blocknote');
      var body = note.querySelector('.rl-cm-body');
      openPop({{ range: null, block: blockOf(note), mark: note, code: false, span: note, editBody: body ? body.textContent : '', expect: '' }}, true);
      return;
    }}
    if (t.closest && t.closest('.rl-cm-delete')) {{
      var note2 = t.closest('.rl-cm-note, .rl-cm-blocknote');
      pending = {{ range: null, block: blockOf(note2), mark: note2, code: false, span: note2, expect: '' }};
      act('delete');
      return;
    }}
    if (!menu.contains(t) && !pop.contains(t)) {{ hideMenu(); }}
  }});
  document.addEventListener('scroll', hideMenu, true);
  document.addEventListener('selectionchange', function () {{ if (!pop.hidden) return; }});
}})();
</script>"#
    )
}

// ── annotate splice ────────────────────────────────────────────────────────

const BODY_CAP: usize = 64 * 1024;

#[derive(Debug)]
pub enum AnnotateError {
    BadRequest(String),
    Conflict(String),
}

impl std::fmt::Display for AnnotateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadRequest(s) | Self::Conflict(s) => write!(f, "{s}"),
        }
    }
}

#[derive(Debug)]
pub struct AnnotateRequest<'a> {
    pub op: &'a str,
    pub target: &'a str,
    pub start: usize,
    pub end: usize,
    pub text: &'a str,
    pub comment: &'a str,
    pub author: &'a str,
    pub id: &'a str,
    pub expect: &'a str,
    pub today: &'a str,
}

pub fn apply_annotate(file: &str, req: &AnnotateRequest<'_>) -> Result<String, AnnotateError> {
    if req.comment.len() > BODY_CAP || req.expect.len() > BODY_CAP || req.text.len() > BODY_CAP {
        return Err(AnnotateError::BadRequest("comment is too long".into()));
    }
    if !req.author.is_empty() && !author_token_ok(req.author) {
        return Err(AnnotateError::BadRequest(
            "author must be one token without a space, %, or :".into(),
        ));
    }
    if comment_body_forbidden(req.comment) {
        return Err(AnnotateError::BadRequest(
            "comment must not contain %% , == , or a fence".into(),
        ));
    }
    match req.op {
        "insert" if req.target == "cell" => insert_cell(file, req),
        "insert" => insert_prose(file, req),
        "edit" => edit_comment(file, req),
        "delete" => delete_mark(file, req),
        _ => Err(AnnotateError::BadRequest("unknown op".into())),
    }
}

fn comment_body_forbidden(body: &str) -> bool {
    if body.contains("%%") || body.contains("==") {
        return true;
    }
    body.lines().any(|l| l.trim().starts_with("```"))
}

fn insert_prose(file: &str, req: &AnnotateRequest<'_>) -> Result<String, AnnotateError> {
    check_prose_range(file, req.start, req.end, req.text)?;
    let piece = if req.comment.is_empty() {
        format!("=={}==", req.text)
    } else if let Some(hi) = highlight_covering_inner(file, req.start, req.end) {
        // Bind a comment to an existing highlight; do not wrap again.
        let c = format_comment(file, req);
        let at = hi.1;
        let spliced = format!("{} {}{}", &file[..at], c, &file[at..]);
        return guard_splice(file, &spliced, at, at, &format!(" {c}"));
    } else if req.comment.contains('\n') {
        format!(
            "=={}==\n%%\n{}: {}\n%%",
            req.text,
            format_header(file, req),
            req.comment
        )
    } else {
        format!(
            "=={}== %%{}: {}%%",
            req.text,
            format_header(file, req),
            req.comment
        )
    };
    let spliced = format!("{}{}{}", &file[..req.start], piece, &file[req.end..]);
    guard_splice(file, &spliced, req.start, req.end, &piece)
}

fn insert_cell(file: &str, req: &AnnotateRequest<'_>) -> Result<String, AnnotateError> {
    if req.comment.trim().is_empty() {
        return Err(AnnotateError::BadRequest("empty cell comment".into()));
    }
    if !file.is_char_boundary(req.start) || req.start > file.len() {
        return Err(AnnotateError::BadRequest("bad offset".into()));
    }
    if line_start(file, req.start) != req.start {
        return Err(AnnotateError::BadRequest(
            "cell offset is not a fence".into(),
        ));
    }
    let line = line_content(file, req.start);
    if detect_fence_open(line.as_bytes(), 0).is_none() {
        return Err(AnnotateError::BadRequest(
            "cell offset is not a fence".into(),
        ));
    }
    let mut at = req.start;
    loop {
        if at == 0 {
            break;
        }
        let prev_end = at - 1;
        if file.as_bytes().get(prev_end) != Some(&b'\n') {
            break;
        }
        let prev_start = line_start(file, prev_end);
        let prev = &file[prev_start..prev_end];
        if is_code_directive(prev.trim()) {
            at = prev_start;
            continue;
        }
        break;
    }
    let line = format!(
        "%%{}: {}%%\n",
        format_header(file, req),
        req.comment.replace('\n', " ")
    );
    let spliced = format!("{}{}{}", &file[..at], line, &file[at..]);
    guard_splice(file, &spliced, at, at, &line)
}

fn edit_comment(file: &str, req: &AnnotateRequest<'_>) -> Result<String, AnnotateError> {
    let (start, end) = expect_span(file, req)?;
    if !req.expect.starts_with("%%") {
        return Err(AnnotateError::BadRequest("edit expects a comment".into()));
    }
    let inner = &req.expect[2..req.expect.len() - 2];
    let (header, _) = parse_header(inner);
    let mut h = header;
    // Keep the parsed header. state stays None.
    h.state = None;
    let rewritten = format!("%%{}%%", write_header_body(&h, req.comment));
    let spliced = format!("{}{}{}", &file[..start], rewritten, &file[end..]);
    guard_splice(file, &spliced, start, end, &rewritten)
}

fn delete_mark(file: &str, req: &AnnotateRequest<'_>) -> Result<String, AnnotateError> {
    let (start, end) = expect_span(file, req)?;
    if req.expect.starts_with("%%") {
        let mut drop_ranges = vec![(start, end)];
        let inner = &req.expect[2..req.expect.len() - 2];
        let (header, _) = parse_header(inner);
        if let Some(id) = header.id.clone() {
            let scanned = scan(file);
            for m in &scanned.marks {
                if m.header.reply_to.as_deref() == Some(id.as_str()) {
                    drop_ranges.push((m.start, m.end));
                }
            }
        }
        // One binding space.
        if start > 0 && file.as_bytes()[start - 1] == b' ' {
            let prev = start - 1;
            if prev >= 2
                && &file[prev - 2..prev] == "=="
                && (prev < 3 || file.as_bytes()[prev - 3] != b' ')
            {
                drop_ranges.push((prev, start));
            }
        }
        drop_ranges.sort_by_key(|(s, _)| *s);
        let mut out = String::new();
        let mut cursor = 0;
        for (s, e) in &drop_ranges {
            if *s < cursor {
                continue;
            }
            out.push_str(&file[cursor..*s]);
            cursor = *e;
        }
        out.push_str(&file[cursor..]);
        // Outside the removed ranges the bytes match by construction.
        Ok(out)
    } else if req.expect.starts_with("==") && req.expect.ends_with("==") && req.expect.len() >= 4 {
        let inner = &req.expect[2..req.expect.len() - 2];
        let spliced = format!("{}{}{}", &file[..start], inner, &file[end..]);
        guard_splice(file, &spliced, start, end, inner)
    } else {
        Err(AnnotateError::BadRequest("delete expects == or %%".into()))
    }
}

fn expect_span(file: &str, req: &AnnotateRequest<'_>) -> Result<(usize, usize), AnnotateError> {
    if req.expect.is_empty() {
        return Err(AnnotateError::BadRequest("missing expect".into()));
    }
    if req.end < req.start
        || req.end > file.len()
        || !file.is_char_boundary(req.start)
        || !file.is_char_boundary(req.end)
    {
        return Err(AnnotateError::BadRequest("bad offset".into()));
    }
    if &file[req.start..req.end] != req.expect {
        return Err(AnnotateError::Conflict(
            "expect does not match the file".into(),
        ));
    }
    Ok((req.start, req.end))
}

fn check_prose_range(
    file: &str,
    start: usize,
    end: usize,
    text: &str,
) -> Result<(), AnnotateError> {
    if end < start
        || end > file.len()
        || !file.is_char_boundary(start)
        || !file.is_char_boundary(end)
    {
        return Err(AnnotateError::BadRequest("bad offset".into()));
    }
    if &file[start..end] != text {
        return Err(AnnotateError::BadRequest(
            "selection does not match the file".into(),
        ));
    }
    let opaque = opaque_ranges(file);
    if opaque
        .iter()
        .any(|&(s, e)| ranges_overlap(start, end, s, e))
    {
        return Err(AnnotateError::BadRequest(
            "selection overlaps code or math".into(),
        ));
    }
    let scanned = scan(file);
    if scanned
        .marks
        .iter()
        .any(|m| ranges_overlap(start, end, m.start, m.end))
    {
        return Err(AnnotateError::BadRequest(
            "selection overlaps an existing mark".into(),
        ));
    }
    Ok(())
}

fn highlight_covering_inner(file: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    scan(file).marks.into_iter().find_map(|m| {
        if m.kind == Kind::Hi && m.inner_start == start && m.inner_end == end {
            Some((m.start, m.end))
        } else {
            None
        }
    })
}

fn format_header(file: &str, req: &AnnotateRequest<'_>) -> String {
    let id = next_comment_id(file);
    let mut s = format!("#c{id}");
    if !req.author.is_empty() {
        s.push(' ');
        s.push('@');
        s.push_str(req.author);
    }
    if !req.today.is_empty() {
        s.push(' ');
        s.push_str(req.today);
    }
    s
}

fn format_comment(file: &str, req: &AnnotateRequest<'_>) -> String {
    format!("%%{}: {}%%", format_header(file, req), req.comment)
}

fn write_header_body(h: &CommentHeader, body: &str) -> String {
    let mut pre = String::new();
    if let Some(id) = &h.id {
        pre.push('#');
        pre.push_str(id);
    }
    if let Some(re) = &h.reply_to {
        if !pre.is_empty() {
            pre.push(' ');
        }
        pre.push_str("re #");
        pre.push_str(re);
    }
    if let Some(a) = &h.author {
        if !pre.is_empty() {
            pre.push(' ');
        }
        pre.push('@');
        pre.push_str(a);
    }
    if let Some(d) = &h.date {
        if !pre.is_empty() {
            pre.push(' ');
        }
        pre.push_str(d);
    }
    if pre.is_empty() && h.id.is_none() {
        // No header was parsed. Keep the body as the whole inner text when
        // the caller is replacing it — still write a colon-less body.
        return body.to_string();
    }
    format!("{pre}: {body}")
}

fn next_comment_id(file: &str) -> u32 {
    let opaque = opaque_ranges(file);
    let b = file.as_bytes();
    let mut max = 0u32;
    let mut i = 0;
    while i + 2 < b.len() {
        if opaque_end_at(&opaque, i).is_some() {
            i += 1;
            continue;
        }
        if b[i] == b'#' && b[i + 1] == b'c' && b.get(i + 2).is_some_and(u8::is_ascii_digit) {
            let mut j = i + 2;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if let Ok(n) = file[i + 2..j].parse::<u32>() {
                max = max.max(n);
            }
            i = j;
            continue;
        }
        i += 1;
    }
    max + 1
}

fn guard_splice(
    original: &str,
    spliced: &str,
    start: usize,
    end: usize,
    piece: &str,
) -> Result<String, AnnotateError> {
    if !spliced.is_char_boundary(start) || start + piece.len() > spliced.len() {
        return Err(AnnotateError::BadRequest("splice failed".into()));
    }
    if &spliced[..start] != &original[..start]
        || &spliced[start + piece.len()..] != &original[end..]
    {
        return Err(AnnotateError::BadRequest(
            "splice changed bytes outside the range".into(),
        ));
    }
    if &spliced[start..start + piece.len()] != piece {
        return Err(AnnotateError::BadRequest("splice mismatch".into()));
    }
    Ok(spliced.to_string())
}

fn is_code_directive(trimmed: &str) -> bool {
    trimmed == "<!-- hide -->"
        || trimmed.starts_with("<!-- code:")
        || trimmed.starts_with("<!-- caption:")
        || trimmed.starts_with("<!-- details:")
        || trimmed.starts_with("<!-- grid:")
}

/// UTC `YYYY-MM-DD` from the civil calendar (Howard Hinnant).
pub fn utc_today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

// The aria-describedby wiring in `build_shown` currently leaves the
// attribute off (note numbers are assigned later in the same pass).
// A second walk fills it in once every note placeholder exists.
fn _bind_describedby_is_applied_in_restore() {}

// ── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(md: &str) -> Vec<&'static str> {
        issues(md).into_iter().map(|i| i.code).collect()
    }

    fn html_of(md: &str) -> String {
        crate::render::markdown_to_html(md)
    }

    #[test]
    fn header_variants() {
        let (h, body) = parse_header("looks off");
        assert!(h.is_empty());
        assert_eq!(body, "looks off");
        assert!(h.state.is_none());

        let (h, body) = parse_header("#c1 text");
        assert!(h.id.is_none(), "no colon, id is not parsed");
        assert_eq!(body, "#c1 text");

        let (h, body) = parse_header("#c1: text");
        assert_eq!(h.id.as_deref(), Some("c1"));
        assert_eq!(body, "text");
        assert!(h.state.is_none());

        let (h, body) = parse_header("@michael 2026-10-08: looks off");
        assert!(h.id.is_none());
        assert_eq!(h.author.as_deref(), Some("michael"));
        assert_eq!(h.date.as_deref(), Some("2026-10-08"));
        assert_eq!(body, "looks off");

        let (h, _) = parse_header("#c4 re #c3 @liz: fixed");
        assert_eq!(h.id.as_deref(), Some("c4"));
        assert_eq!(h.reply_to.as_deref(), Some("c3"));
        assert_eq!(h.author.as_deref(), Some("liz"));

        let (h, body) = parse_header(": text");
        assert!(h.id.is_none());
        assert_eq!(body, "text");

        let (h, body) = parse_header("not a header: nope");
        assert!(h.is_empty());
        assert_eq!(body, "not a header: nope");
    }

    #[test]
    fn highlight_and_bound_comment_render() {
        let html = html_of("Group delay is ==constant== %%only for linear phase%%.");
        assert!(html.contains("<mark class=\"rl-cm rl-cm-mark\""), "{html}");
        assert!(html.contains(">constant</mark>"), "{html}");
        assert!(html.contains("only for linear phase"), "{html}");
        assert!(
            html.contains("aria-describedby") || html.contains("rl-cm-note"),
            "{html}"
        );
        assert!(
            !html.contains("data-cm-id"),
            "no id, so no data-cm-id: {html}"
        );
    }

    #[test]
    fn two_spaces_do_not_bind() {
        let html = html_of("==text==  %%why%%");
        assert!(html.contains("<mark"), "{html}");
        assert!(html.contains("rl-cm-note"), "{html}");
    }

    #[test]
    fn escape_is_literal() {
        let html = html_of(r"use \== not highlight");
        assert!(html.contains("=="), "{html}");
        assert!(!html.contains("<mark"), "{html}");
        assert!(!html.contains("\\==") && !html.contains("\\=\\="), "{html}");
    }

    #[test]
    fn unmarked_html_has_no_comment_chrome() {
        let html = html_of("hello");
        assert!(!html.contains("rl-cm"), "{html}");
        assert!(html.contains("hello"), "{html}");
    }

    #[test]
    fn fence_and_math_are_not_marks() {
        let md = "```rustlab\na == b\n%% note\n```\n\n$x == y$ and `a == b`.";
        assert!(codes(md).is_empty(), "{:?}", issues(md));
        let html = html_of(md);
        assert!(!html.contains("rl-cm-mark"), "{html}");
    }

    #[test]
    fn w012_and_not_on_id_or_unclosed_or_bare_highlight() {
        assert_eq!(codes("%%looks off%%"), vec!["rustlab:W012"]);
        assert!(
            codes("%%#c1: text%%").is_empty(),
            "{:?}",
            issues("%%#c1: text%%")
        );
        let u = codes("%%unclosed");
        assert!(u.contains(&"rustlab:W007"), "{u:?}");
        assert!(!u.contains(&"rustlab:W012"), "{u:?}");
        let bare = codes("==only highlight==");
        assert!(!bare.contains(&"rustlab:W012"), "{bare:?}");
    }

    #[test]
    fn w006_even_count_does_not_swallow_the_next_highlight() {
        let md = "==unclosed\n\n==other==";
        let c = codes(md);
        assert!(c.contains(&"rustlab:W006"), "{c:?}");
        assert!(!c.contains(&"rustlab:W008"), "{c:?}");
        let html = html_of(md);
        assert!(
            html.contains(">other</mark>") || html.contains("other</mark>"),
            "{html}"
        );
    }

    #[test]
    fn w008_odd_count_across_a_blank_line() {
        let c = codes("==start\n\nend==");
        assert!(c.contains(&"rustlab:W008"), "{c:?}");
        assert!(!c.contains(&"rustlab:W006"), "{c:?}");
    }

    #[test]
    fn duplicate_and_orphan() {
        let md = "%%#c1: a%%\n%%#c1: b%%\n%%re #c9: nope%%";
        let c = codes(md);
        assert!(c.contains(&"rustlab:W010"), "{c:?}");
        assert!(c.contains(&"rustlab:W011"), "{c:?}");
        assert!(c.contains(&"rustlab:W012"), "{c:?}");
    }

    #[test]
    fn nesting_warns() {
        let c = codes("==hello %%no%% ==");
        assert!(c.iter().any(|x| *x == "rustlab:W009"), "{c:?}");
    }

    #[test]
    fn sha256_known_answers() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn source_has_marks_sees_delimiters_and_escapes() {
        assert!(!source_has_marks("plain prose"));
        assert!(source_has_marks("==keep=="));
        assert!(source_has_marks(r"\%%"));
        assert!(!source_has_marks("```rustlab\na == b\n```"));
    }

    fn page(blocks: &[crate::execute::Rendered]) -> String {
        crate::render::render_html_nonced(
            "T",
            blocks,
            &std::path::PathBuf::from("/tmp/rustlab_test_plots"),
            "plots",
            rustlab_plot::Theme::Dark.colors(),
            None,
            &crate::render::LinkMode::single_file(),
            Some("deadbeef"),
        )
    }

    fn md_block(s: &str) -> crate::execute::Rendered {
        crate::execute::Rendered::Markdown(s.to_string())
    }

    #[test]
    fn html_page_renders_mark_note_and_toggle() {
        let html = page(&[md_block(
            "Group delay is ==constant== %%only for linear phase%%.",
        )]);
        assert!(html.contains("<mark class=\"rl-cm rl-cm-mark\""), "{html}");
        assert!(html.contains(">constant</mark>"), "{html}");
        assert!(html.contains("only for linear phase"), "{html}");
        assert!(html.contains("aria-describedby=\"cm-n1\""), "{html}");
        assert!(html.contains("id=\"cm-n1\""), "{html}");
        assert!(html.contains("id=\"rl-comments\""), "{html}");
        assert!(html.contains("id=\"rl-cm-confirm\""), "{html}");
        assert!(html.contains("--rl-cm-mark-bg"), "{html}");
        assert!(html.contains("nonce=\"deadbeef\""), "{html}");
        assert!(!html.contains("onclick="), "{html}");
        assert!(!html.contains("onload="), "{html}");
    }

    #[test]
    fn html_page_puts_a_cell_comment_on_the_code_section() {
        let html = page(&[
            md_block("See this.\n%%#c3: why the gain%%"),
            crate::execute::Rendered::Code {
                source: "x = 1".into(),
                text_output: String::new(),
                error: None,
                figures: Vec::new(),
                animations: Vec::new(),
                hidden: false,
                details: None,
                grid_cols: None,
                source_open: None,
            },
        ]);
        let code_at = html.find("class=\"code-block\"").expect("code block");
        let note_at = html.find("why the gain").expect("cell note");
        assert!(
            note_at > code_at,
            "cell note should sit on the code section"
        );
        assert!(html.contains("data-cm-id=\"c3\""), "{html}");
        assert!(html.contains("class=\"rl-cm-note\""), "{html}");
        let prose_end = html.find("class=\"code-block\"").unwrap();
        assert!(
            !html[..prose_end].contains("why the gain"),
            "peeled comment must leave the prose"
        );
    }

    #[test]
    fn html_no_comments_strips_marks_and_omits_the_toggle() {
        let _guard = install(CommentMode::for_format(FormatKind::Html, Some(false)));
        let html = page(&[md_block("==constant== %%why%%")]);
        assert!(!html.contains("rl-cm-mark"), "{html}");
        assert!(!html.contains("rl-cm-note"), "{html}");
        assert!(!html.contains("id=\"rl-comments\""), "{html}");
        assert!(html.contains("constant"), "{html}");
        assert!(!html.contains("why"), "{html}");
    }

    #[test]
    fn latex_comments_on_uses_hl_and_default_strips() {
        let md = "==constant== %%why the phase%%";
        {
            let _guard = install(CommentMode::for_format(FormatKind::Latex, Some(true)));
            assert!(latex_packages().contains("soul"));
            let prep = prepare_latex(md);
            let body = prep.markdown.clone().unwrap_or_default();
            let tex = restore_latex(&body, &prep);
            assert!(tex.contains("\\hl{constant}"), "{tex}");
            assert!(tex.contains("why the phase"), "{tex}");
            assert!(
                tex.contains("marginpar") || tex.contains("footnote"),
                "{tex}"
            );
        }
        let _guard = install(CommentMode::for_format(FormatKind::Latex, None));
        assert_eq!(latex_packages(), "");
        let prep = prepare_latex(md);
        let body = prep.markdown.clone().unwrap_or_default();
        let tex = restore_latex(&body, &prep);
        assert!(tex.contains("constant"), "{tex}");
        assert!(!tex.contains("why the phase"), "{tex}");
        assert!(!tex.contains("\\hl{"), "{tex}");
    }

    #[test]
    fn strip_unwraps_and_drops_comments() {
        let s = strip_source("==keep== %%gone%% and ==plain==");
        assert_eq!(s, "keep  and plain");
        let broken = strip_source("==nope");
        assert_eq!(broken, "==nope");
    }

    #[test]
    fn peel_cell_comment_and_keep_a_blank_line() {
        let (rest, notes) = peel_trailing_cell_comments("See this.\n%%#c3: why%%");
        assert_eq!(rest, "See this.");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].header.id.as_deref(), Some("c3"));
        let (rest, notes) = peel_trailing_cell_comments("See this.\n%%#c3: why%%\n\n");
        assert!(notes.is_empty(), "blank line unbinds");
        assert!(rest.contains("%%#c3"));
    }

    #[test]
    fn inline_before_fence_is_not_peeled() {
        let (rest, notes) = peel_trailing_cell_comments("See this.%%why%%");
        assert!(notes.is_empty());
        assert!(rest.contains("%%why%%"));
    }

    #[test]
    fn apply_highlight_and_comment_and_cell() {
        let file = "Group delay is constant.\n";
        let start = file.find("constant").unwrap();
        let end = start + "constant".len();
        let out = apply_annotate(
            file,
            &AnnotateRequest {
                op: "insert",
                target: "prose",
                start,
                end,
                text: "constant",
                comment: "",
                author: "",
                id: "",
                expect: "",
                today: "2026-10-09",
            },
        )
        .unwrap();
        assert!(out.contains("==constant=="), "{out}");
        assert!(!out.contains("%%"), "{out}");

        let out = apply_annotate(
            file,
            &AnnotateRequest {
                op: "insert",
                target: "prose",
                start,
                end,
                text: "constant",
                comment: "only linear",
                author: "michael",
                id: "",
                expect: "",
                today: "2026-10-09",
            },
        )
        .unwrap();
        assert!(
            out.contains("==constant== %%#c1 @michael 2026-10-09: only linear%%"),
            "{out}"
        );

        let bad = apply_annotate(
            file,
            &AnnotateRequest {
                op: "insert",
                target: "prose",
                start,
                end,
                text: "nope",
                comment: "",
                author: "",
                id: "",
                expect: "",
                today: "2026-10-09",
            },
        );
        assert!(bad.is_err());

        let src = "before\n```rustlab\nh = 1\n```\n";
        let fence = src.find("```rustlab").unwrap();
        let out = apply_annotate(
            src,
            &AnnotateRequest {
                op: "insert",
                target: "cell",
                start: fence,
                end: fence,
                text: "",
                comment: "why",
                author: "",
                id: "",
                expect: "",
                today: "2026-10-09",
            },
        )
        .unwrap();
        assert!(out.contains("%%#c1 2026-10-09: why%%\n```rustlab"), "{out}");
        assert!(!out.contains("=="), "{out}");

        let empty = apply_annotate(
            src,
            &AnnotateRequest {
                op: "insert",
                target: "cell",
                start: fence,
                end: fence,
                text: "",
                comment: "",
                author: "",
                id: "",
                expect: "",
                today: "2026-10-09",
            },
        );
        assert!(empty.is_err());
    }

    #[test]
    fn delete_removes_replies_and_one_binding_space() {
        let file = "==text== %%#c1: why%%\n%%#c2 re #c1: ok%%\n";
        let start = file.find("%%#c1").unwrap();
        let expect = "%%#c1: why%%";
        let end = start + expect.len();
        let out = apply_annotate(
            file,
            &AnnotateRequest {
                op: "delete",
                target: "prose",
                start,
                end,
                text: "",
                comment: "",
                author: "",
                id: "c1",
                expect,
                today: "2026-10-09",
            },
        )
        .unwrap();
        assert_eq!(out, "==text==\n\n");
    }

    #[test]
    fn edit_keeps_header_and_state_none() {
        let file = "%%#c1 @ann 2026-10-08: old%%";
        let out = apply_annotate(
            file,
            &AnnotateRequest {
                op: "edit",
                target: "prose",
                start: 0,
                end: file.len(),
                text: "",
                comment: "new",
                author: "",
                id: "",
                expect: file,
                today: "2026-10-09",
            },
        )
        .unwrap();
        assert_eq!(
            out,
            "%%#c1 re #ann @ann 2026-10-08: new%%".replace("re #ann ", "")
        );
        // author token @ann is not a reply. The rewrite keeps id, author, date.
        assert!(out.starts_with("%%#c1 @ann 2026-10-08: new%%"), "{out}");
        let (h, body) = parse_header(&out[2..out.len() - 2]);
        assert!(h.state.is_none());
        assert_eq!(body, "new");
    }

    #[test]
    fn mismatched_expect_is_conflict() {
        let file = "%%#c1: a%%";
        let err = apply_annotate(
            file,
            &AnnotateRequest {
                op: "delete",
                target: "prose",
                start: 0,
                end: file.len(),
                text: "",
                comment: "",
                author: "",
                id: "",
                expect: "%%#c1: b%%",
                today: "2026-10-09",
            },
        )
        .unwrap_err();
        assert!(matches!(err, AnnotateError::Conflict(_)));
    }

    #[test]
    fn script_pointer_rules() {
        let js = annotate_script(Some("abc"));
        assert!(js.contains("nonce=\"abc\""));
        assert!(js.contains("pointerType !== 'touch'"));
        assert!(js.contains("ev.shiftKey"));
        assert!(js.contains("ContextMenu"));
        assert!(js.contains("rl-comment-name"));
        assert!(!js.contains("oncontextmenu"));
        assert!(!js.contains("mouseup"));
    }
}
