//! Light server-side highlighting for prose fences tagged `bash`, `python`,
//! `toml`, `yaml` / `yml`, and `text`.
//!
//! Token classes are the same roles rustlab cells already use
//! ([`HlKind`]): comments, strings, keywords, numbers, functions, and
//! operators. Those roles already clear WCAG AA on the code panel in
//! mocha, macchiato, frappé, and latte, so markup fences do not add
//! colors. `text` is one uncolored span so printed output does not look
//! like source. No client highlighter and no extra crate — the scanners
//! walk the source once.
//!
//! TOML and YAML fences are display-only. The notebook parser never
//! treats them as code, so nothing in them runs.

use pulldown_cmark::CodeBlockKind;
use rustlab_script::highlight::{HlKind, HlSpan};

/// A fenced block this renderer styles. The label is the canonical
/// lowercase tag, whatever capitalisation the author wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProseFence {
    Bash,
    Python,
    Toml,
    Yaml,
    Text,
}

impl ProseFence {
    /// First info-string word, case-insensitive. Only `bash`, `python`,
    /// `toml`, `yaml`, `yml`, and `text` — other tags stay on the plain
    /// fence path. `yml` is the same highlighter as `yaml`.
    pub fn parse_info(info: &str) -> Option<Self> {
        let tag = info.split_whitespace().next()?;
        let tag = tag.split('{').next().unwrap_or(tag);
        match tag.to_ascii_lowercase().as_str() {
            "bash" => Some(Self::Bash),
            "python" => Some(Self::Python),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            "text" => Some(Self::Text),
            _ => None,
        }
    }

    pub fn from_code_block(kind: &CodeBlockKind<'_>) -> Option<Self> {
        match kind {
            // An empty info string is "no language". That is the text
            // panel, same as an explicit ```text fence. A tag we do not
            // highlight (`javascript`, `sh`) stays None. `toml` / `yaml`
            // are highlighted; they are still not executed.
            CodeBlockKind::Fenced(info) if info.trim().is_empty() => Some(Self::Text),
            CodeBlockKind::Fenced(info) => Self::parse_info(info),
            // Indented code is not a fenced block.
            CodeBlockKind::Indented => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Python => "python",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
            Self::Text => "text",
        }
    }

    /// Spans cover every byte of `source`, in order.
    pub fn highlight(self, source: &str) -> Vec<HlSpan> {
        match self {
            Self::Text => {
                if source.is_empty() {
                    Vec::new()
                } else {
                    vec![HlSpan {
                        start: 0,
                        end: source.len(),
                        kind: HlKind::Text,
                    }]
                }
            }
            Self::Bash => scan(source, Lang::Bash),
            Self::Python => scan(source, Lang::Python),
            Self::Toml => scan_toml(source),
            Self::Yaml => scan_yaml(source),
        }
    }
}

/// Drop the single trailing newline CommonMark keeps on a fenced body,
/// so the panel does not end on a blank line. A blank line inside the
/// fence stays.
pub fn display_body(source: &str) -> &str {
    let s = source.strip_suffix('\n').unwrap_or(source);
    s.strip_suffix('\r').unwrap_or(s)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    Bash,
    Python,
}

struct Scan<'a> {
    src: &'a str,
    i: usize,
    spans: Vec<HlSpan>,
    lang: Lang,
}

fn scan(source: &str, lang: Lang) -> Vec<HlSpan> {
    let mut sc = Scan {
        src: source,
        i: 0,
        spans: Vec::new(),
        lang,
    };
    sc.run();
    sc.spans
}

impl<'a> Scan<'a> {
    fn run(&mut self) {
        while self.i < self.src.len() {
            let ch = self.peek().expect("i < len implies a char");
            if ch == ' ' || ch == '\t' || ch == '\r' || ch == '\n' {
                self.scan_ws();
            } else if self.lang == Lang::Bash && ch == '$' && self.dollar_quote() {
                let start = self.i;
                self.bump();
                self.scan_string(start);
            } else if ch == '"' || ch == '\'' {
                let start = self.i;
                self.scan_string(start);
            } else if ch == '#' {
                if self.lang == Lang::Bash && self.bash_hash_is_literal() {
                    let start = self.i;
                    self.bump();
                    self.push(start, HlKind::Text);
                } else {
                    self.scan_comment();
                }
            } else if ch.is_ascii_digit() || (self.lang == Lang::Python && self.dot_number()) {
                self.scan_number();
            } else if is_ident_char(ch) {
                self.scan_ident();
            } else {
                let start = self.i;
                self.bump();
                self.push(start, HlKind::Text);
            }
        }
    }

    fn rest(&self) -> &'a str {
        &self.src[self.i..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.i += ch.len_utf8();
        Some(ch)
    }

    fn prev_char(&self) -> Option<char> {
        self.src[..self.i].chars().next_back()
    }

    fn dollar_quote(&self) -> bool {
        let mut chars = self.rest().chars();
        chars.next();
        matches!(chars.next(), Some('\'' | '"'))
    }

    fn dot_number(&self) -> bool {
        let mut chars = self.rest().chars();
        chars.next() == Some('.') && matches!(chars.next(), Some(c) if c.is_ascii_digit())
    }

    /// `${#arr}` and `$#` are length forms, not comments. A `#` glued to
    /// a word (`foo#bar`) is part of that word in the shell.
    fn bash_hash_is_literal(&self) -> bool {
        matches!(
            self.prev_char(),
            Some(c) if is_ident_char(c) || c == '$' || c == '{'
        )
    }

    fn push(&mut self, start: usize, kind: HlKind) {
        let end = self.i;
        if start >= end {
            return;
        }
        if kind == HlKind::Text {
            if let Some(last) = self.spans.last_mut() {
                if last.kind == HlKind::Text && last.end == start {
                    last.end = end;
                    return;
                }
            }
        }
        self.spans.push(HlSpan { start, end, kind });
    }

    fn scan_ws(&mut self) {
        let start = self.i;
        while matches!(self.peek(), Some(' ' | '\t' | '\r' | '\n')) {
            self.bump();
        }
        self.push(start, HlKind::Text);
    }

    fn scan_comment(&mut self) {
        let start = self.i;
        while matches!(self.peek(), Some(c) if c != '\n') {
            self.bump();
        }
        self.push(start, HlKind::Comment);
    }

    fn scan_ident(&mut self) {
        let start = self.i;
        while matches!(self.peek(), Some(c) if is_ident_char(c)) {
            self.bump();
        }
        let word = &self.src[start..self.i];
        if self.lang == Lang::Python
            && is_py_prefix(word)
            && matches!(self.peek(), Some('\'' | '"'))
        {
            self.scan_string(start);
            return;
        }
        let kind = if is_keyword(self.lang, word) {
            HlKind::Keyword
        } else {
            HlKind::Text
        };
        self.push(start, kind);
    }

    fn scan_string(&mut self, start: usize) {
        let quote = self.bump().expect("caller saw a quote");
        let triple = self.lang == Lang::Python
            && self.rest().starts_with(quote)
            && self.rest()[quote.len_utf8()..].starts_with(quote);
        if triple {
            self.bump();
            self.bump();
        }
        let bash_single = self.lang == Lang::Bash && quote == '\'';
        loop {
            match self.peek() {
                None => break,
                Some('\n') if !triple && self.lang == Lang::Python => break,
                Some('\\') if !bash_single => {
                    self.bump();
                    self.bump();
                }
                Some(c) if c == quote => {
                    self.bump();
                    if triple {
                        if self.rest().starts_with(quote)
                            && self.rest()[quote.len_utf8()..].starts_with(quote)
                        {
                            self.bump();
                            self.bump();
                            break;
                        }
                    } else {
                        break;
                    }
                }
                Some(_) => {
                    self.bump();
                }
            }
        }
        self.push(start, HlKind::String);
    }

    fn scan_number(&mut self) {
        let start = self.i;
        if self.lang == Lang::Python && self.rest().len() >= 2 {
            let b = self.rest().as_bytes();
            if b[0] == b'0' && matches!(b[1], b'x' | b'X' | b'b' | b'B' | b'o' | b'O') {
                let base = b[1].to_ascii_lowercase();
                self.bump();
                self.bump();
                let digits_at = self.i;
                self.consume_based_digits(base);
                if self.i == digits_at {
                    self.i = start + 1;
                }
                self.push(start, HlKind::Number);
                return;
            }
        }
        if self.peek() == Some('.') {
            self.bump();
        }
        self.consume_digits();
        if self.peek() == Some('.') {
            let next = self.rest()[1..].chars().next();
            let take_dot = match next {
                Some(c) if c.is_ascii_digit() => true,
                Some(c) if is_ident_char(c) => false,
                _ => self.lang == Lang::Python,
            };
            if take_dot {
                self.bump();
                self.consume_digits();
            }
        }
        if self.lang == Lang::Python && matches!(self.peek(), Some('e' | 'E')) {
            let save = self.i;
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            let digits_at = self.i;
            self.consume_digits();
            if self.i == digits_at {
                self.i = save;
            }
        }
        if self.lang == Lang::Python && matches!(self.peek(), Some('j' | 'J')) {
            self.bump();
        }
        self.push(start, HlKind::Number);
    }

    fn consume_digits(&mut self) {
        self.consume_based_digits(b'd');
    }

    fn consume_based_digits(&mut self, base: u8) {
        while let Some(c) = self.peek() {
            let ok = c == '_'
                || match base {
                    b'x' => c.is_ascii_hexdigit(),
                    b'b' => c == '0' || c == '1',
                    b'o' => ('0'..='7').contains(&c),
                    _ => c.is_ascii_digit(),
                };
            if !ok {
                break;
            }
            self.bump();
        }
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_py_prefix(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "r" | "u" | "b" | "f" | "fr" | "rf" | "br" | "rb"
    )
}

fn is_keyword(lang: Lang, word: &str) -> bool {
    match lang {
        Lang::Bash => matches!(
            word,
            "if" | "then"
                | "else"
                | "elif"
                | "fi"
                | "for"
                | "while"
                | "until"
                | "do"
                | "done"
                | "case"
                | "esac"
                | "in"
                | "function"
                | "select"
                | "time"
                | "coproc"
        ),
        Lang::Python => matches!(
            word,
            "False"
                | "None"
                | "True"
                | "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "try"
                | "while"
                | "with"
                | "yield"
                | "match"
                | "case"
        ),
    }
}

// ── TOML / YAML ────────────────────────────────────────────────────────────
//
// Display scanners, not parsers. Every byte is covered. Roles reuse the
// rustlab token colors:
//   comments            → Comment
//   strings             → String
//   numbers and dates   → Number
//   booleans, null, table names, `---` / `...` → Keyword
//   bare keys           → Function
//   brackets, `=`, `:`, list dashes, anchors, block-scalar indicators → Operator

struct Spans {
    spans: Vec<HlSpan>,
}

impl Spans {
    fn push(&mut self, start: usize, end: usize, kind: HlKind) {
        if start >= end {
            return;
        }
        if kind == HlKind::Text {
            if let Some(last) = self.spans.last_mut() {
                if last.kind == HlKind::Text && last.end == start {
                    last.end = end;
                    return;
                }
            }
        }
        self.spans.push(HlSpan { start, end, kind });
    }
}

struct Cursor<'a> {
    src: &'a str,
    i: usize,
    spans: Spans,
}

impl<'a> Cursor<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            i: 0,
            spans: Spans { spans: Vec::new() },
        }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.i..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.i += ch.len_utf8();
        Some(ch)
    }

    fn rest(&self) -> &'a str {
        &self.src[self.i..]
    }

    fn paint(&mut self, n: usize, kind: HlKind) {
        let end = self.i + n;
        self.spans.push(self.i, end, kind);
        self.i = end;
    }

    fn mark_op(&mut self) {
        let start = self.i;
        self.bump();
        self.spans.push(start, self.i, HlKind::Operator);
    }

    fn eat_one_text(&mut self) {
        let start = self.i;
        self.bump();
        self.spans.push(start, self.i, HlKind::Text);
    }

    fn eat_inline_ws(&mut self) {
        let start = self.i;
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.bump();
        }
        self.spans.push(start, self.i, HlKind::Text);
    }

    fn eat_newline(&mut self) {
        let start = self.i;
        if self.peek() == Some('\r') {
            self.bump();
        }
        if self.peek() == Some('\n') {
            self.bump();
        }
        self.spans.push(start, self.i, HlKind::Text);
    }

    fn eat_comment(&mut self) {
        let start = self.i;
        while matches!(self.peek(), Some(c) if c != '\n' && c != '\r') {
            self.bump();
        }
        self.spans.push(start, self.i, HlKind::Comment);
    }

    fn prev_is_ws(&self) -> bool {
        if self.i == 0 {
            return true;
        }
        matches!(
            self.src[..self.i].chars().next_back(),
            Some(' ' | '\t' | '\n' | '\r')
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TFrame {
    Top,
    Array,
    Inline,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TExpect {
    /// Key or table at the top, key inside `{ }`, value inside `[ ]`.
    Start,
    Value,
    Tail,
}

struct Toml<'a> {
    cur: Cursor<'a>,
    frames: Vec<TFrame>,
    expect: TExpect,
}

fn scan_toml(source: &str) -> Vec<HlSpan> {
    let mut t = Toml {
        cur: Cursor::new(source),
        frames: vec![TFrame::Top],
        expect: TExpect::Start,
    };
    t.run();
    t.cur.spans.spans
}

impl<'a> Toml<'a> {
    fn run(&mut self) {
        while self.cur.i < self.cur.src.len() {
            let before = self.cur.i;
            self.step();
            if self.cur.i == before {
                self.cur.eat_one_text();
            }
        }
    }

    fn frame(&self) -> TFrame {
        self.frames.last().copied().unwrap_or(TFrame::Top)
    }

    fn step(&mut self) {
        match self.cur.peek() {
            Some(' ' | '\t') => self.cur.eat_inline_ws(),
            Some('\n' | '\r') => {
                self.cur.eat_newline();
                if self.frames.len() == 1 {
                    self.expect = TExpect::Start;
                }
            }
            Some('#') => self.cur.eat_comment(),
            _ => self.dispatch(),
        }
    }

    fn dispatch(&mut self) {
        if self.expect == TExpect::Tail {
            self.tail();
            return;
        }
        if self.expect == TExpect::Value || self.frame() == TFrame::Array {
            self.value();
            return;
        }
        if self.frame() == TFrame::Inline && self.cur.peek() == Some('}') {
            self.closer();
            return;
        }
        if self.frame() == TFrame::Top && self.cur.peek() == Some('[') {
            self.table();
            self.expect = TExpect::Tail;
            return;
        }
        self.key_eq();
    }

    fn tail(&mut self) {
        match self.cur.peek() {
            Some(',') => {
                self.cur.mark_op();
                self.expect = match self.frame() {
                    TFrame::Array => TExpect::Value,
                    TFrame::Inline | TFrame::Top => TExpect::Start,
                };
            }
            Some(']') if self.frame() == TFrame::Array => self.closer(),
            Some('}') if self.frame() == TFrame::Inline => self.closer(),
            _ => self.cur.eat_one_text(),
        }
    }

    fn closer(&mut self) {
        self.cur.mark_op();
        if self.frames.len() > 1 {
            self.frames.pop();
        }
        self.expect = TExpect::Tail;
    }

    fn value(&mut self) {
        match self.cur.peek() {
            Some(']') if self.frame() == TFrame::Array => self.closer(),
            Some('}') if self.frame() == TFrame::Inline => self.closer(),
            Some('"' | '\'') => {
                self.string();
                self.expect = TExpect::Tail;
            }
            Some('[') => {
                self.cur.mark_op();
                self.frames.push(TFrame::Array);
                self.expect = TExpect::Start;
            }
            Some('{') => {
                self.cur.mark_op();
                self.frames.push(TFrame::Inline);
                self.expect = TExpect::Start;
            }
            Some(_) if self.take_len(toml_bool_len, HlKind::Keyword) => {}
            Some(_) if self.take_len(toml_date_len, HlKind::Number) => {}
            Some(_) if self.take_len(toml_number_len, HlKind::Number) => {}
            Some(c) if is_toml_bare(c) => {
                let start = self.cur.i;
                while matches!(self.cur.peek(), Some(ch) if is_toml_bare(ch)) {
                    self.cur.bump();
                }
                self.cur.spans.push(start, self.cur.i, HlKind::Text);
                self.expect = TExpect::Tail;
            }
            _ => {
                self.cur.eat_one_text();
                self.expect = TExpect::Tail;
            }
        }
    }

    fn take_len(&mut self, len_of: fn(&str) -> Option<usize>, kind: HlKind) -> bool {
        let Some(n) = len_of(self.cur.rest()) else {
            return false;
        };
        self.cur.paint(n, kind);
        self.expect = TExpect::Tail;
        true
    }

    fn key_eq(&mut self) {
        if !self.take_key(HlKind::Function) {
            self.cur.eat_one_text();
            return;
        }
        self.cur.eat_inline_ws();
        if self.cur.peek() == Some('=') {
            self.cur.mark_op();
            self.expect = TExpect::Value;
        }
    }

    fn take_key(&mut self, kind: HlKind) -> bool {
        if !self.key_segment(kind) {
            return false;
        }
        loop {
            let after = self.index_after_inline_ws();
            if self.cur.src[after..].starts_with('.') {
                self.cur.spans.push(self.cur.i, after, HlKind::Text);
                self.cur.i = after;
                self.cur.mark_op();
                let after = self.index_after_inline_ws();
                self.cur.spans.push(self.cur.i, after, HlKind::Text);
                self.cur.i = after;
                if !self.key_segment(kind) {
                    break;
                }
            } else {
                break;
            }
        }
        true
    }

    fn index_after_inline_ws(&self) -> usize {
        let b = self.cur.src.as_bytes();
        let mut j = self.cur.i;
        while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
            j += 1;
        }
        j
    }

    fn key_segment(&mut self, kind: HlKind) -> bool {
        match self.cur.peek() {
            Some('"' | '\'') => {
                self.string();
                true
            }
            Some(c) if is_toml_bare(c) => {
                let start = self.cur.i;
                while matches!(self.cur.peek(), Some(ch) if is_toml_bare(ch)) {
                    self.cur.bump();
                }
                self.cur.spans.push(start, self.cur.i, kind);
                true
            }
            _ => false,
        }
    }

    fn table(&mut self) {
        let start = self.cur.i;
        self.cur.bump();
        if self.cur.peek() == Some('[') {
            self.cur.bump();
        }
        self.cur.spans.push(start, self.cur.i, HlKind::Operator);
        loop {
            self.cur.eat_inline_ws();
            match self.cur.peek() {
                None | Some('\n' | '\r') => break,
                Some(']') => {
                    let open = self.cur.i;
                    self.cur.bump();
                    if self.cur.peek() == Some(']') {
                        self.cur.bump();
                    }
                    self.cur.spans.push(open, self.cur.i, HlKind::Operator);
                    break;
                }
                Some('.') => self.cur.mark_op(),
                Some('"' | '\'') => self.string(),
                Some(c) if is_toml_bare(c) => {
                    let seg = self.cur.i;
                    while matches!(self.cur.peek(), Some(ch) if is_toml_bare(ch)) {
                        self.cur.bump();
                    }
                    self.cur.spans.push(seg, self.cur.i, HlKind::Keyword);
                }
                Some(_) => self.cur.eat_one_text(),
            }
        }
    }

    /// Basic, literal, and multiline TOML strings. The span includes the
    /// quotes. A newline ends a single-line string; multiline strings run
    /// until the closing triple quote.
    fn string(&mut self) {
        let start = self.cur.i;
        let quote = self.cur.bump().unwrap_or('"');
        let multiline = self.quote_run(quote) >= 2;
        if multiline {
            self.cur.bump();
            self.cur.bump();
        }
        let literal = quote == '\'';
        loop {
            match self.cur.peek() {
                None => break,
                Some('\n' | '\r') if !multiline => break,
                Some('\\') if !literal => {
                    self.cur.bump();
                    match self.cur.peek() {
                        Some('\r') if multiline => {
                            self.cur.bump();
                            if self.cur.peek() == Some('\n') {
                                self.cur.bump();
                            }
                        }
                        Some('\n') if multiline => {
                            self.cur.bump();
                        }
                        Some('\n' | '\r') => break,
                        Some(_) => {
                            self.cur.bump();
                        }
                        None => break,
                    }
                }
                Some(c) if c == quote => {
                    if multiline && self.quote_run(quote) >= 3 {
                        for _ in 0..3 {
                            self.cur.bump();
                        }
                        break;
                    }
                    self.cur.bump();
                    if !multiline {
                        break;
                    }
                }
                Some(_) => {
                    self.cur.bump();
                }
            }
        }
        self.cur.spans.push(start, self.cur.i, HlKind::String);
    }

    fn quote_run(&self, quote: char) -> usize {
        self.cur.rest().chars().take_while(|c| *c == quote).count()
    }
}

fn is_toml_bare(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn token_boundary(s: &str, n: usize) -> bool {
    match s.as_bytes().get(n) {
        None => true,
        Some(c) => !c.is_ascii_alphanumeric() && *c != b'_' && *c != b'-',
    }
}

fn toml_bool_len(s: &str) -> Option<usize> {
    for word in ["true", "false"] {
        if s.starts_with(word) && token_boundary(s, word.len()) {
            return Some(word.len());
        }
    }
    None
}

fn toml_date_len(s: &str) -> Option<usize> {
    let n = date_body_len(s)?;
    token_boundary(s, n).then_some(n)
}

fn toml_number_len(s: &str) -> Option<usize> {
    let n = toml_number_body(s)?;
    token_boundary(s, n).then_some(n)
}

/// `YYYY-MM-DD`, optional time, or `HH:MM:SS`. Shape only — highlighting
/// does not reject an out-of-range month.
fn date_body_len(s: &str) -> Option<usize> {
    if let Some(mut n) = full_date_len(s) {
        let b = s.as_bytes();
        if n < b.len() && matches!(b[n], b'T' | b't' | b' ') {
            if let Some(tn) = partial_time_len(&s[n + 1..]) {
                n += 1 + tn;
                if n < b.len() && matches!(b[n], b'Z' | b'z') {
                    n += 1;
                } else if let Some(off) = num_offset_len(s.get(n..)?) {
                    n += off;
                }
            }
        }
        return Some(n);
    }
    partial_time_len(s)
}

fn full_date_len(s: &str) -> Option<usize> {
    (has_digits(s, 0, 4)
        && byte_at(s, 4) == Some(b'-')
        && has_digits(s, 5, 2)
        && byte_at(s, 7) == Some(b'-')
        && has_digits(s, 8, 2))
    .then_some(10)
}

fn partial_time_len(s: &str) -> Option<usize> {
    if !(has_digits(s, 0, 2)
        && byte_at(s, 2) == Some(b':')
        && has_digits(s, 3, 2)
        && byte_at(s, 5) == Some(b':')
        && has_digits(s, 6, 2))
    {
        return None;
    }
    let mut n = 8;
    if byte_at(s, n) == Some(b'.') {
        let b = s.as_bytes();
        let mut k = n + 1;
        if k >= b.len() || !b[k].is_ascii_digit() {
            return Some(n);
        }
        while k < b.len() && (b[k].is_ascii_digit() || b[k] == b'_') {
            k += 1;
        }
        n = k;
    }
    Some(n)
}

fn num_offset_len(s: &str) -> Option<usize> {
    let sign = byte_at(s, 0)?;
    if sign != b'+' && sign != b'-' {
        return None;
    }
    (has_digits(s, 1, 2) && byte_at(s, 3) == Some(b':') && has_digits(s, 4, 2)).then_some(6)
}

fn has_digits(s: &str, i: usize, n: usize) -> bool {
    let b = s.as_bytes();
    i + n <= b.len() && b[i..i + n].iter().all(|c| c.is_ascii_digit())
}

fn byte_at(s: &str, i: usize) -> Option<u8> {
    s.as_bytes().get(i).copied()
}

fn toml_number_body(s: &str) -> Option<usize> {
    if let Some(n) = toml_inf_nan(s) {
        return Some(n);
    }
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut j = 0;
    if b[0] == b'+' || b[0] == b'-' {
        j = 1;
        if j >= b.len() {
            return None;
        }
    }
    if j + 1 < b.len() && b[j] == b'0' {
        let base = b[j + 1].to_ascii_lowercase();
        if matches!(base, b'x' | b'o' | b'b') {
            j += 2;
            let digits_at = j;
            while j < b.len() && (b[j] == b'_' || based_digit(base, b[j])) {
                j += 1;
            }
            return (j > digits_at).then_some(j);
        }
    }
    let mut saw_digit = false;
    while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'_') {
        if b[j].is_ascii_digit() {
            saw_digit = true;
        }
        j += 1;
    }
    if j < b.len() && b[j] == b'.' && j + 1 < b.len() && b[j + 1].is_ascii_digit() {
        saw_digit = true;
        j += 1;
        while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'_') {
            j += 1;
        }
    }
    let exp = take_exponent(s, j);
    if exp != j {
        saw_digit = true;
        j = exp;
    }
    saw_digit.then_some(j)
}

fn toml_inf_nan(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut j = 0;
    if b[0] == b'+' || b[0] == b'-' {
        j = 1;
    }
    let rest = s.get(j..)?;
    if rest.starts_with("inf") {
        Some(j + 3)
    } else if rest.starts_with("nan") {
        Some(j + 3)
    } else {
        None
    }
}

fn based_digit(base: u8, c: u8) -> bool {
    match base {
        b'x' => c.is_ascii_hexdigit(),
        b'o' => (b'0'..=b'7').contains(&c),
        b'b' => c == b'0' || c == b'1',
        _ => c.is_ascii_digit(),
    }
}

fn take_exponent(s: &str, j: usize) -> usize {
    let b = s.as_bytes();
    if j >= b.len() || (b[j] != b'e' && b[j] != b'E') {
        return j;
    }
    let mut k = j + 1;
    if k < b.len() && (b[k] == b'+' || b[k] == b'-') {
        k += 1;
    }
    let digits_at = k;
    while k < b.len() && (b[k].is_ascii_digit() || b[k] == b'_') {
        k += 1;
    }
    if k == digits_at {
        j
    } else {
        k
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum YFlow {
    Map,
    Seq,
}

struct Yaml<'a> {
    cur: Cursor<'a>,
    flow: Vec<YFlow>,
    /// Indent of the line that opened a `|` / `>` scalar. Content lines
    /// are strictly deeper. `None` when not inside one.
    block_parent: Option<usize>,
    line_indent: usize,
    /// Still at the first token of a line (a list dash keeps this true).
    indicator: bool,
    need_line: bool,
}

fn scan_yaml(source: &str) -> Vec<HlSpan> {
    let mut y = Yaml {
        cur: Cursor::new(source),
        flow: Vec::new(),
        block_parent: None,
        line_indent: 0,
        indicator: true,
        need_line: true,
    };
    y.run();
    y.cur.spans.spans
}

impl<'a> Yaml<'a> {
    fn run(&mut self) {
        while self.cur.i < self.cur.src.len() {
            if self.need_line {
                if self.take_block_line() {
                    continue;
                }
                self.line_indent = self.take_indent();
                self.indicator = true;
                self.need_line = false;
                if self.cur.i >= self.cur.src.len() {
                    break;
                }
                if self.try_doc() {
                    continue;
                }
                if matches!(self.cur.peek(), Some('\n' | '\r')) {
                    self.cur.eat_newline();
                    self.need_line = true;
                    continue;
                }
            }
            let before = self.cur.i;
            self.step();
            if self.cur.i == before {
                self.cur.eat_one_text();
            }
        }
    }

    fn take_indent(&mut self) -> usize {
        let start = self.cur.i;
        let mut n = 0;
        while matches!(self.cur.peek(), Some(' ' | '\t')) {
            self.cur.bump();
            n += 1;
        }
        self.cur.spans.push(start, self.cur.i, HlKind::Text);
        n
    }

    fn take_block_line(&mut self) -> bool {
        let Some(parent) = self.block_parent else {
            return false;
        };
        let b = self.cur.rest().as_bytes();
        let mut indent = 0;
        while indent < b.len() && (b[indent] == b' ' || b[indent] == b'\t') {
            indent += 1;
        }
        let blank = indent >= b.len() || b[indent] == b'\n' || b[indent] == b'\r';
        if !blank && indent <= parent {
            self.block_parent = None;
            return false;
        }
        let start = self.cur.i;
        self.cur.i += indent;
        self.cur.spans.push(start, self.cur.i, HlKind::Text);
        let body = self.cur.i;
        while matches!(self.cur.peek(), Some(c) if c != '\n' && c != '\r') {
            self.cur.bump();
        }
        if self.cur.i > body {
            self.cur.spans.push(body, self.cur.i, HlKind::String);
        }
        self.cur.eat_newline();
        true
    }

    fn try_doc(&mut self) -> bool {
        if !self.flow.is_empty() {
            return false;
        }
        let rest = self.cur.rest();
        let marker = if rest.starts_with("---") || rest.starts_with("...") {
            3
        } else {
            return false;
        };
        let after = &rest.as_bytes()[marker..];
        let mut k = 0;
        while k < after.len() && (after[k] == b' ' || after[k] == b'\t') {
            k += 1;
        }
        let ok = k == after.len() || matches!(after[k], b'\n' | b'\r' | b'#');
        if !ok {
            return false;
        }
        let start = self.cur.i;
        self.cur.i += marker;
        self.cur.spans.push(start, self.cur.i, HlKind::Keyword);
        if k > 0 {
            let ws = self.cur.i;
            self.cur.i += k;
            self.cur.spans.push(ws, self.cur.i, HlKind::Text);
        }
        if self.cur.peek() == Some('#') {
            self.cur.eat_comment();
        }
        self.cur.eat_newline();
        self.need_line = true;
        true
    }

    fn step(&mut self) {
        match self.cur.peek() {
            Some(' ' | '\t') => self.cur.eat_inline_ws(),
            Some('\n' | '\r') => {
                self.cur.eat_newline();
                self.need_line = true;
            }
            Some('#') if self.cur.prev_is_ws() || self.indicator => self.cur.eat_comment(),
            Some('-') if self.indicator && self.flow.is_empty() && self.list_dash() => {
                self.cur.mark_op();
            }
            Some('&' | '*') if self.anchor_here() => self.take_anchor(),
            Some('!') if self.tag_here() => self.take_tag(),
            Some('\'' | '"') => {
                self.yaml_string();
                self.indicator = false;
            }
            Some('{') => {
                self.cur.mark_op();
                self.flow.push(YFlow::Map);
                self.indicator = false;
            }
            Some('[') => {
                self.cur.mark_op();
                self.flow.push(YFlow::Seq);
                self.indicator = false;
            }
            Some('}') if self.flow.last() == Some(&YFlow::Map) => {
                self.cur.mark_op();
                self.flow.pop();
                self.indicator = false;
            }
            Some(']') if self.flow.last() == Some(&YFlow::Seq) => {
                self.cur.mark_op();
                self.flow.pop();
                self.indicator = false;
            }
            Some(',') if !self.flow.is_empty() => {
                self.cur.mark_op();
                self.indicator = false;
            }
            Some(':') => {
                self.cur.mark_op();
                self.indicator = false;
            }
            _ => self.word_or_plain(),
        }
    }

    fn list_dash(&self) -> bool {
        let rest = &self.cur.src[self.cur.i + 1..];
        rest.is_empty() || rest.starts_with([' ', '\t', '\n', '\r', '#'])
    }

    fn anchor_here(&self) -> bool {
        let mut chars = self.cur.rest().chars();
        chars.next();
        matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '-')
    }

    fn take_anchor(&mut self) {
        let start = self.cur.i;
        self.cur.bump();
        while matches!(self.cur.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            self.cur.bump();
        }
        self.cur.spans.push(start, self.cur.i, HlKind::Operator);
        self.indicator = false;
    }

    fn tag_here(&self) -> bool {
        let mut chars = self.cur.rest().chars();
        if chars.next() != Some('!') {
            return false;
        }
        match chars.next() {
            Some('!' | '<') => true,
            Some(c) if c.is_ascii_alphanumeric() || c == '_' => true,
            _ => false,
        }
    }

    fn take_tag(&mut self) {
        let start = self.cur.i;
        self.cur.bump();
        if self.cur.peek() == Some('!') {
            self.cur.bump();
        }
        if self.cur.peek() == Some('<') {
            self.cur.bump();
            while matches!(self.cur.peek(), Some(c) if c != '>' && c != '\n' && c != '\r') {
                self.cur.bump();
            }
            if self.cur.peek() == Some('>') {
                self.cur.bump();
            }
        } else {
            while matches!(self.cur.peek(), Some(c) if yaml_tag_char(c)) {
                self.cur.bump();
            }
        }
        self.cur.spans.push(start, self.cur.i, HlKind::Operator);
        self.indicator = false;
    }

    fn word_or_plain(&mut self) {
        if self.flow.is_empty() && self.try_block() {
            return;
        }
        if self.try_key() {
            self.indicator = false;
            return;
        }
        if self.take_scalar(yaml_bool_len, HlKind::Keyword)
            || self.take_scalar(yaml_date_len, HlKind::Number)
            || self.take_scalar(yaml_number_len, HlKind::Number)
        {
            self.indicator = false;
            return;
        }
        self.plain();
        self.indicator = false;
    }

    fn take_scalar(&mut self, len_of: fn(&str) -> Option<usize>, kind: HlKind) -> bool {
        let Some(n) = len_of(self.cur.rest()) else {
            return false;
        };
        self.cur.paint(n, kind);
        true
    }

    fn try_block(&mut self) -> bool {
        let Some(n) = block_header_len(self.cur.rest()) else {
            return false;
        };
        let start = self.cur.i;
        self.cur.i += n;
        self.cur.spans.push(start, self.cur.i, HlKind::Operator);
        self.cur.eat_inline_ws();
        if self.cur.peek() == Some('#') {
            self.cur.eat_comment();
        }
        self.cur.eat_newline();
        self.block_parent = Some(self.line_indent);
        self.need_line = true;
        self.indicator = true;
        true
    }

    fn try_key(&mut self) -> bool {
        let rest = self.cur.rest();
        let b = rest.as_bytes();
        if b.is_empty() || !yaml_plain_start(b[0]) {
            return false;
        }
        let mut j = 0;
        while j < b.len() && yaml_key_byte(b[j]) {
            j += 1;
        }
        if j == 0 {
            return false;
        }
        let mut k = j;
        while k < b.len() && (b[k] == b' ' || b[k] == b'\t') {
            k += 1;
        }
        if k >= b.len() || b[k] != b':' {
            return false;
        }
        let after = k + 1;
        let boundary = after >= b.len()
            || matches!(
                b[after],
                b' ' | b'\t'
                    | b'\n'
                    | b'\r'
                    | b'#'
                    | b'{'
                    | b'}'
                    | b'['
                    | b']'
                    | b','
                    | b'|'
                    | b'>'
            );
        if !boundary {
            return false;
        }
        let start = self.cur.i;
        self.cur.i += j;
        self.cur.spans.push(start, self.cur.i, HlKind::Function);
        if k > j {
            let ws = self.cur.i;
            self.cur.i += k - j;
            self.cur.spans.push(ws, self.cur.i, HlKind::Text);
        }
        self.cur.mark_op();
        true
    }

    fn plain(&mut self) {
        let start = self.cur.i;
        while let Some(c) = self.cur.peek() {
            if matches!(c, '\n' | '\r' | ',' | '{' | '}' | '[' | ']') {
                break;
            }
            if c == '#' && self.cur.prev_is_ws() {
                break;
            }
            if c == ':' && self.colon_ends() {
                break;
            }
            self.cur.bump();
        }
        let mut end = self.cur.i;
        while end > start {
            let ch = self.cur.src[..end].chars().next_back().unwrap();
            if ch == ' ' || ch == '\t' {
                end -= ch.len_utf8();
            } else {
                break;
            }
        }
        if end > start {
            self.cur.spans.push(start, end, HlKind::String);
        }
        if end < self.cur.i {
            self.cur.spans.push(end, self.cur.i, HlKind::Text);
        }
        if self.cur.i == start {
            self.cur.eat_one_text();
        }
    }

    fn colon_ends(&self) -> bool {
        let rest = &self.cur.src[self.cur.i + 1..];
        rest.is_empty() || rest.starts_with([' ', '\t', '\n', '\r', '#', ',', '}', ']', '{', '['])
    }

    fn yaml_string(&mut self) {
        let start = self.cur.i;
        let quote = self.cur.bump().unwrap_or('"');
        loop {
            match self.cur.peek() {
                None => break,
                Some('\'') if quote == '\'' => {
                    self.cur.bump();
                    if self.cur.peek() == Some('\'') {
                        self.cur.bump();
                    } else {
                        break;
                    }
                }
                Some('"') if quote == '"' => {
                    self.cur.bump();
                    break;
                }
                Some('\\') if quote == '"' => {
                    self.cur.bump();
                    self.cur.bump();
                }
                Some(_) => {
                    self.cur.bump();
                }
            }
        }
        self.cur.spans.push(start, self.cur.i, HlKind::String);
    }
}

fn yaml_tag_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, ',' | '{' | '}' | '[' | ']' | ':' | '#' | '\'' | '"')
}

fn yaml_plain_start(b: u8) -> bool {
    yaml_key_byte(b)
}

fn yaml_key_byte(b: u8) -> bool {
    !matches!(
        b,
        b' ' | b'\t'
            | b'\n'
            | b'\r'
            | b':'
            | b'#'
            | b'{'
            | b'}'
            | b'['
            | b']'
            | b','
            | b'\''
            | b'"'
            | b'&'
            | b'*'
            | b'!'
            | b'|'
            | b'>'
    )
}

/// `|`, `|-`, `|+`, `|2`, `|2-`, `|-2`, and the `>` forms. The rest of
/// the line must be empty or a comment, so `a | b` stays a plain string.
fn block_header_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.is_empty() || (b[0] != b'|' && b[0] != b'>') {
        return None;
    }
    let mut j = 1;
    let mut saw_digit = false;
    let mut saw_chomp = false;
    while j < b.len() {
        if !saw_digit && (b'1'..=b'9').contains(&b[j]) {
            saw_digit = true;
            j += 1;
            continue;
        }
        if !saw_chomp && (b[j] == b'+' || b[j] == b'-') {
            saw_chomp = true;
            j += 1;
            continue;
        }
        break;
    }
    let mut k = j;
    while k < b.len() && (b[k] == b' ' || b[k] == b'\t') {
        k += 1;
    }
    if k == b.len() || matches!(b[k], b'\n' | b'\r' | b'#') {
        Some(j)
    } else {
        None
    }
}

/// A bool, number, or date is only that token when it is the whole plain
/// scalar. `yes please` stays a string.
fn ends_scalar(s: &str, n: usize) -> bool {
    let b = s.as_bytes();
    if n > b.len() {
        return false;
    }
    if n == b.len() {
        return true;
    }
    match b[n] {
        b'\n' | b'\r' | b',' | b'}' | b']' | b'{' | b'[' | b'#' => true,
        b':' => {
            let m = n + 1;
            m >= b.len() || matches!(b.get(m), Some(b' ' | b'\t' | b'\n' | b'\r' | b'#'))
        }
        b' ' | b'\t' => {
            let mut k = n;
            while k < b.len() && (b[k] == b' ' || b[k] == b'\t') {
                k += 1;
            }
            k == b.len()
                || matches!(
                    b[k],
                    b'\n' | b'\r' | b'#' | b',' | b'}' | b']' | b'{' | b'['
                )
        }
        _ => false,
    }
}

fn yaml_bool_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.first() == Some(&b'~') && ends_scalar(s, 1) {
        return Some(1);
    }
    let mut j = 0;
    while j < b.len() && b[j].is_ascii_alphanumeric() {
        j += 1;
    }
    if j == 0 || !ends_scalar(s, j) {
        return None;
    }
    let word = s[..j].to_ascii_lowercase();
    matches!(
        word.as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "null"
    )
    .then_some(j)
}

fn yaml_date_len(s: &str) -> Option<usize> {
    let n = date_body_len(s)?;
    ends_scalar(s, n).then_some(n)
}

fn yaml_number_len(s: &str) -> Option<usize> {
    let n = yaml_number_body(s)?;
    ends_scalar(s, n).then_some(n)
}

fn yaml_number_body(s: &str) -> Option<usize> {
    if let Some(n) = yaml_special_len(s) {
        return Some(n);
    }
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut j = 0;
    if b[0] == b'+' || b[0] == b'-' {
        j = 1;
        if j >= b.len() {
            return None;
        }
    }
    if j + 1 < b.len() && b[j] == b'0' {
        let base = b[j + 1].to_ascii_lowercase();
        if matches!(base, b'x' | b'o') {
            j += 2;
            let digits_at = j;
            while j < b.len()
                && (b[j] == b'_'
                    || if base == b'x' {
                        b[j].is_ascii_hexdigit()
                    } else {
                        (b'0'..=b'7').contains(&b[j])
                    })
            {
                j += 1;
            }
            return (j > digits_at).then_some(j);
        }
    }
    if b[j] == b'.' {
        if j + 1 < b.len() && b[j + 1].is_ascii_digit() {
            j += 1;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'_') {
                j += 1;
            }
            return Some(take_exponent(s, j));
        }
        return None;
    }
    let mut saw = false;
    while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'_') {
        if b[j].is_ascii_digit() {
            saw = true;
        }
        j += 1;
    }
    if j < b.len() && b[j] == b'.' && j + 1 < b.len() && b[j + 1].is_ascii_digit() {
        saw = true;
        j += 1;
        while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'_') {
            j += 1;
        }
    }
    let exp = take_exponent(s, j);
    if exp != j {
        saw = true;
        j = exp;
    }
    saw.then_some(j)
}

fn yaml_special_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut j = 0;
    if b[0] == b'+' || b[0] == b'-' {
        j = 1;
    }
    let rest = s.get(j..)?;
    if rest.len() >= 4 {
        let head = rest[..4].to_ascii_lowercase();
        if head == ".inf" || head == ".nan" {
            return Some(j + 4);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colored(lang: ProseFence, source: &str) -> Vec<(String, HlKind)> {
        let spans = lang.highlight(source);
        let mut at = 0;
        for s in &spans {
            assert_eq!(s.start, at, "gap or overlap in {source:?}");
            assert!(s.end <= source.len());
            assert!(source.is_char_boundary(s.start) && source.is_char_boundary(s.end));
            at = s.end;
        }
        assert_eq!(at, source.len(), "spans do not cover {source:?}");
        spans
            .into_iter()
            .filter(|s| s.kind != HlKind::Text)
            .map(|s| (source[s.start..s.end].to_string(), s.kind))
            .collect()
    }

    #[test]
    fn info_string_is_the_first_word() {
        assert_eq!(ProseFence::parse_info("python"), Some(ProseFence::Python));
        assert_eq!(ProseFence::parse_info("Bash"), Some(ProseFence::Bash));
        assert_eq!(ProseFence::parse_info("TEXT extra"), Some(ProseFence::Text));
        assert_eq!(
            ProseFence::parse_info("python {.python}"),
            Some(ProseFence::Python)
        );
        assert_eq!(ProseFence::parse_info("sh"), None);
        assert_eq!(ProseFence::parse_info("py"), None);
        assert_eq!(ProseFence::parse_info("javascript"), None);
        assert_eq!(ProseFence::parse_info(""), None);
    }

    #[test]
    fn untagged_fence_is_text_and_indented_is_not() {
        assert_eq!(
            ProseFence::from_code_block(&CodeBlockKind::Fenced("".into())),
            Some(ProseFence::Text)
        );
        assert_eq!(
            ProseFence::from_code_block(&CodeBlockKind::Fenced("   ".into())),
            Some(ProseFence::Text)
        );
        assert_eq!(ProseFence::from_code_block(&CodeBlockKind::Indented), None);
        assert_eq!(
            ProseFence::from_code_block(&CodeBlockKind::Fenced("javascript".into())),
            None
        );
        assert_eq!(
            ProseFence::from_code_block(&CodeBlockKind::Fenced("sh".into())),
            None
        );
        assert_eq!(
            ProseFence::from_code_block(&CodeBlockKind::Fenced("text".into())),
            Some(ProseFence::Text)
        );
    }

    #[test]
    fn bash_colors_comments_strings_keywords_and_numbers() {
        let src = "if true; then\n  echo \"hi # there\" 'x' 3\nfi # done\n";
        let got = colored(ProseFence::Bash, src);
        assert!(got.contains(&("if".into(), HlKind::Keyword)));
        assert!(got.contains(&("then".into(), HlKind::Keyword)));
        assert!(got.contains(&("fi".into(), HlKind::Keyword)));
        assert!(got.contains(&("\"hi # there\"".into(), HlKind::String)));
        assert!(got.contains(&("'x'".into(), HlKind::String)));
        assert!(got.contains(&("3".into(), HlKind::Number)));
        assert!(got.contains(&("# done".into(), HlKind::Comment)));
        assert!(!got
            .iter()
            .any(|(t, k)| *k == HlKind::Comment && t.contains("hi")));
    }

    #[test]
    fn hash_inside_quotes_stays_in_the_string() {
        // A `#` inside quotes is string text. The comment is only the
        // `#` that sits outside the quotes.
        let cases = [
            ("\"hi # there\" # real\n", "\"hi # there\"", "# real"),
            ("'hi # there' # real\n", "'hi # there'", "# real"),
        ];
        for lang in [ProseFence::Bash, ProseFence::Python] {
            for (src, string, comment) in cases {
                let got = colored(lang, src);
                assert!(
                    got.contains(&(string.into(), HlKind::String)),
                    "{lang:?} {src:?} -> {got:?}"
                );
                assert!(
                    got.contains(&(comment.into(), HlKind::Comment)),
                    "{lang:?} {src:?} -> {got:?}"
                );
                assert!(
                    !got.iter()
                        .any(|(t, k)| *k == HlKind::Comment && t.contains("hi")),
                    "{lang:?} {src:?} -> {got:?}"
                );
            }
        }
    }

    #[test]
    fn bash_length_forms_are_not_comments() {
        let got = colored(ProseFence::Bash, "echo ${#arr} $#\n");
        assert!(got.iter().all(|(_, k)| *k != HlKind::Comment));
    }

    #[test]
    fn bash_dollar_quote_is_a_string() {
        let got = colored(ProseFence::Bash, "echo $'a\\nb'\n");
        assert!(got
            .iter()
            .any(|(t, k)| *k == HlKind::String && t.starts_with("$'")));
    }

    #[test]
    fn python_colors_the_usual_tokens() {
        let src = "def f(n):\n    return 1_000 + 0x1F + 3.5e-2  # count\n";
        let got = colored(ProseFence::Python, src);
        assert!(got.contains(&("def".into(), HlKind::Keyword)));
        assert!(got.contains(&("return".into(), HlKind::Keyword)));
        assert!(got.contains(&("1_000".into(), HlKind::Number)));
        assert!(got.contains(&("0x1F".into(), HlKind::Number)));
        assert!(got.contains(&("3.5e-2".into(), HlKind::Number)));
        assert!(got.contains(&("# count".into(), HlKind::Comment)));
        assert!(!got.iter().any(|(t, k)| t == "f" && *k == HlKind::Keyword));
    }

    #[test]
    fn python_strings_include_prefixes_and_triples() {
        let got = colored(ProseFence::Python, "s = r'a\\nb'\nt = \"\"\"x\ny\"\"\"\n");
        assert!(got
            .iter()
            .any(|(t, k)| *k == HlKind::String && t.starts_with("r'")));
        assert!(got
            .iter()
            .any(|(t, k)| *k == HlKind::String && t.contains('\n')));
        assert!(!got.iter().any(|(_, k)| *k == HlKind::Comment));
    }

    #[test]
    fn python_keyword_inside_a_string_stays_a_string() {
        let got = colored(ProseFence::Python, "s = \"def return\"\n");
        assert_eq!(got, vec![("\"def return\"".into(), HlKind::String)]);
    }

    #[test]
    fn text_fence_is_uncolored() {
        let src = "def f():\n    # not code\n    42\n";
        let spans = ProseFence::Text.highlight(src);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].kind, HlKind::Text);
        assert_eq!(spans[0].end, src.len());
    }

    #[test]
    fn display_body_drops_one_trailing_newline() {
        assert_eq!(display_body("a\n"), "a");
        assert_eq!(display_body("a\r\n"), "a");
        assert_eq!(display_body("a\n\n"), "a\n");
        assert_eq!(display_body("a"), "a");
    }

    /// `needle` is the exact text of one span. A date or anchor that was
    /// split into pieces fails this check.
    fn assert_span(lang: ProseFence, source: &str, needle: &str, kind: HlKind) {
        let _ = colored(lang, source);
        let spans = lang.highlight(source);
        assert!(
            spans
                .iter()
                .any(|s| s.kind == kind && &source[s.start..s.end] == needle),
            "no {kind:?} span {needle:?}\n{spans:?}"
        );
    }

    #[test]
    fn toml_and_yaml_tags_parse_and_other_tags_do_not() {
        assert_eq!(ProseFence::parse_info("toml"), Some(ProseFence::Toml));
        assert_eq!(ProseFence::parse_info("TOML"), Some(ProseFence::Toml));
        assert_eq!(ProseFence::parse_info("yaml"), Some(ProseFence::Yaml));
        assert_eq!(ProseFence::parse_info("YAML"), Some(ProseFence::Yaml));
        assert_eq!(ProseFence::parse_info("yml"), Some(ProseFence::Yaml));
        assert_eq!(ProseFence::parse_info("Yml extra"), Some(ProseFence::Yaml));
        assert_eq!(ProseFence::Toml.label(), "toml");
        assert_eq!(ProseFence::Yaml.label(), "yaml");
        assert_eq!(ProseFence::parse_info("sh"), None);
        assert_eq!(ProseFence::parse_info("javascript"), None);
        assert_eq!(
            ProseFence::from_code_block(&CodeBlockKind::Fenced("toml".into())),
            Some(ProseFence::Toml)
        );
        assert_eq!(
            ProseFence::from_code_block(&CodeBlockKind::Fenced("".into())),
            Some(ProseFence::Text)
        );
    }

    #[test]
    fn toml_colors_comments_tables_keys_strings_numbers_bools_and_dates() {
        let src = "\
# server
title = \"rustlab\"
host = 'localhost'

[server]
port = 8080
debug = false

[[users]]
name = \"Ada\"
id = 1_000

[limits]
max = 5e+22
hex = 0xDEAD
oct = 0o755
bin = 0b1101
special = inf
missing = -nan
started = 1979-05-27T07:32:00Z
local = 1979-05-27T07:32:00.999999
day = 1979-05-27
clock = 07:32:00
offset = 1979-05-27T00:32:00-07:00
note = \"hi # there\" # real
text = \"\"\"
alpha
beta
\"\"\"
lit = '''keep'''
pair = { a = 1, b = \"x\" }
nums = [1, true, \"z\"]
dotted.child = 2
";
        let k = |needle: &str, kind| assert_span(ProseFence::Toml, src, needle, kind);
        k("# server", HlKind::Comment);
        k("# real", HlKind::Comment);
        k("\"hi # there\"", HlKind::String);
        k("title", HlKind::Function);
        k("\"rustlab\"", HlKind::String);
        k("'localhost'", HlKind::String);
        k("[", HlKind::Operator);
        k("server", HlKind::Keyword);
        k("8080", HlKind::Number);
        k("false", HlKind::Keyword);
        k("[[", HlKind::Operator);
        k("users", HlKind::Keyword);
        k("1_000", HlKind::Number);
        k("5e+22", HlKind::Number);
        k("0xDEAD", HlKind::Number);
        k("0o755", HlKind::Number);
        k("0b1101", HlKind::Number);
        k("inf", HlKind::Number);
        k("-nan", HlKind::Number);
        k("1979-05-27T07:32:00Z", HlKind::Number);
        k("1979-05-27T07:32:00.999999", HlKind::Number);
        k("1979-05-27", HlKind::Number);
        k("07:32:00", HlKind::Number);
        k("1979-05-27T00:32:00-07:00", HlKind::Number);
        k("\"\"\"\nalpha\nbeta\n\"\"\"", HlKind::String);
        k("'''keep'''", HlKind::String);
        k("dotted", HlKind::Function);
        k("child", HlKind::Function);
        k("true", HlKind::Keyword);
        let got = colored(ProseFence::Toml, src);
        assert!(!got
            .iter()
            .any(|(t, kind)| *kind == HlKind::Comment && t.contains("hi")));
        assert!(got
            .iter()
            .any(|(t, kind)| *kind == HlKind::String && t == "\"x\""));
    }

    #[test]
    fn toml_keyword_inside_a_string_stays_a_string() {
        let src = "s = \"true false\"\n";
        assert_eq!(
            colored(ProseFence::Toml, src),
            vec![
                ("s".into(), HlKind::Function),
                ("=".into(), HlKind::Operator),
                ("\"true false\"".into(), HlKind::String),
            ]
        );
    }

    #[test]
    fn yaml_colors_comments_keys_strings_numbers_bools_anchors_lists_docs_and_blocks() {
        let src = "\
# pipeline
---
name: \"demo\"
count: 3
enabled: true
empty: null
also: ~
yesno: YES
ratio: 1.5e-2
hex: 0x10
special: .inf
when: 2001-12-15T02:59:43.1Z
anchor: &id 10
alias: *id
items:
  - one
  - two
description: |
  block scalar
  # not a comment
folded: >-
  kept
flow: { a: 1, b: \"x\" }
seq: [2, false, null]
url: http://example.com
phrase: yes please
...
";
        let k = |needle: &str, kind| assert_span(ProseFence::Yaml, src, needle, kind);
        k("# pipeline", HlKind::Comment);
        k("---", HlKind::Keyword);
        k("...", HlKind::Keyword);
        k("name", HlKind::Function);
        k("\"demo\"", HlKind::String);
        k("3", HlKind::Number);
        k("true", HlKind::Keyword);
        k("null", HlKind::Keyword);
        k("~", HlKind::Keyword);
        k("YES", HlKind::Keyword);
        k("1.5e-2", HlKind::Number);
        k("0x10", HlKind::Number);
        k(".inf", HlKind::Number);
        k("2001-12-15T02:59:43.1Z", HlKind::Number);
        k("&id", HlKind::Operator);
        k("*id", HlKind::Operator);
        k("-", HlKind::Operator);
        k("one", HlKind::String);
        k("|", HlKind::Operator);
        k("block scalar", HlKind::String);
        k("# not a comment", HlKind::String);
        k(">-", HlKind::Operator);
        k("kept", HlKind::String);
        k("a", HlKind::Function);
        k("\"x\"", HlKind::String);
        k("false", HlKind::Keyword);
        k("http://example.com", HlKind::String);
        k("yes please", HlKind::String);
        assert_eq!(
            ProseFence::parse_info("yml").unwrap().highlight(src),
            ProseFence::Yaml.highlight(src)
        );
    }

    #[test]
    fn yaml_hash_inside_quotes_is_not_a_comment() {
        let src = "s: \"a # b\"  # real\n";
        assert_span(ProseFence::Yaml, src, "\"a # b\"", HlKind::String);
        assert_span(ProseFence::Yaml, src, "# real", HlKind::Comment);
        let src = "s: 'a # b'\n";
        assert_span(ProseFence::Yaml, src, "'a # b'", HlKind::String);
        assert!(colored(ProseFence::Yaml, src)
            .iter()
            .all(|(_, k)| *k != HlKind::Comment));
    }

    #[test]
    fn markup_scanners_cover_unclosed_strings() {
        let _ = colored(ProseFence::Toml, "a = \"oops");
        let _ = colored(ProseFence::Yaml, "a: \"oops");
        let _ = colored(ProseFence::Toml, "");
        let _ = colored(ProseFence::Yaml, "");
    }
}
