//! Light server-side highlighting for prose fences tagged `bash`, `python`,
//! and `text`.
//!
//! Token classes are the same roles rustlab cells already use
//! ([`HlKind`]): comments, strings, keywords, and numbers. `text` is one
//! uncolored span so printed output does not look like source. No client
//! highlighter and no extra crate — the scanners walk the source once.

use pulldown_cmark::CodeBlockKind;
use rustlab_script::highlight::{HlKind, HlSpan};

/// A fenced block this renderer styles. The label is the canonical
/// lowercase tag, whatever capitalisation the author wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProseFence {
    Bash,
    Python,
    Text,
}

impl ProseFence {
    /// First info-string word, case-insensitive. Only `bash`, `python`,
    /// and `text` — other tags stay on the plain fence path.
    pub fn parse_info(info: &str) -> Option<Self> {
        let tag = info.split_whitespace().next()?;
        let tag = tag.split('{').next().unwrap_or(tag);
        match tag.to_ascii_lowercase().as_str() {
            "bash" => Some(Self::Bash),
            "python" => Some(Self::Python),
            "text" => Some(Self::Text),
            _ => None,
        }
    }

    pub fn from_code_block(kind: &CodeBlockKind<'_>) -> Option<Self> {
        match kind {
            CodeBlockKind::Fenced(info) => Self::parse_info(info),
            CodeBlockKind::Indented => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Python => "python",
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
}
