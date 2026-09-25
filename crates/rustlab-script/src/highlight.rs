//! Span-preserving syntax highlighter for rustlab source.
//!
//! Classification follows [`crate::lexer`] (keywords, `#`/`%` comments,
//! strings vs transpose, numbers, operators) but never fails: a lex error
//! in the interpreter becomes plain text here so a broken notebook cell
//! still highlights. Whitespace and comments are kept — [`crate::lexer::tokenize`]
//! drops both and records no byte offsets, so it cannot drive highlighting.
//!
//! `cache` is a soft keyword (an ordinary name unless the parser sees a
//! cache statement) and is not colored as a keyword.

/// Token class for one source span. [`HlKind::Text`] is uncolored
/// (identifiers that are not calls, whitespace, delimiters, and
/// characters the lexer would reject).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HlKind {
    Text,
    Keyword,
    /// Identifier immediately followed by `(`.
    Function,
    Number,
    String,
    Comment,
    Operator,
}

/// Half-open byte range `[start, end)` into the source passed to [`highlight`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HlSpan {
    pub start: usize,
    pub end: usize,
    pub kind: HlKind,
}

/// Keywords the lexer recognizes as tokens (not the soft keyword `cache`).
pub const KEYWORDS: &[&str] = &[
    "function",
    "end",
    "return",
    "if",
    "elseif",
    "else",
    "for",
    "while",
    "switch",
    "case",
    "otherwise",
    "run",
    "format",
    "hold",
    "grid",
    "viewer",
    "close",
];

/// Highlight `source`. Spans are ordered, non-overlapping, and cover every
/// byte. Adjacent [`HlKind::Text`] spans are merged.
pub fn highlight(source: &str) -> Vec<HlSpan> {
    let mut sc = Scanner {
        src: source,
        i: 0,
        spans: Vec::new(),
        transpose_base: false,
    };
    sc.run();
    sc.spans
}

struct Scanner<'a> {
    src: &'a str,
    i: usize,
    spans: Vec<HlSpan>,
    /// Previous real token is one the lexer treats as a transpose prefix
    /// (`)`, `]`, ident, number, imaginary, `'`, `.'`). Comments and the
    /// newline swallowed by `...` do not clear this; a normal newline does.
    transpose_base: bool,
}

impl<'a> Scanner<'a> {
    fn run(&mut self) {
        while self.i < self.src.len() {
            let ch = self.peek().expect("i < len implies a char");
            if ch == ' ' || ch == '\t' || ch == '\r' {
                self.scan_ws();
            } else if ch == '\n' {
                let start = self.i;
                self.bump();
                self.push(start, HlKind::Text);
                self.transpose_base = false;
            } else if ch == '#' || ch == '%' {
                self.scan_comment();
            } else if self.rest().starts_with("...") {
                self.scan_continuation();
            } else if ch == '"' {
                self.scan_string('"');
            } else if ch == '\'' {
                if self.transpose_base {
                    self.push_op(1);
                } else {
                    self.scan_string('\'');
                }
            } else if let Some(len) = self.match_op() {
                self.push_op(len);
            } else if ch.is_ascii_digit() || ch == '.' {
                self.scan_number();
            } else if ch.is_alphabetic() || ch == '_' {
                self.scan_ident();
            } else {
                let start = self.i;
                self.bump();
                self.push(start, HlKind::Text);
                self.transpose_base = false;
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
        while matches!(self.peek(), Some(' ' | '\t' | '\r')) {
            self.bump();
        }
        self.push(start, HlKind::Text);
    }

    fn scan_comment(&mut self) {
        let start = self.i;
        self.bump();
        while matches!(self.peek(), Some(c) if c != '\n') {
            self.bump();
        }
        self.push(start, HlKind::Comment);
        // Lexer discards the comment and leaves the previous token in place.
    }

    /// `...` skips the rest of the line *and* the newline, without emitting
    /// a newline token, so the transpose context survives.
    fn scan_continuation(&mut self) {
        let start = self.i;
        self.i += 3;
        while matches!(self.peek(), Some(c) if c != '\n') {
            self.bump();
        }
        self.push(start, HlKind::Comment);
        if self.peek() == Some('\n') {
            let nl = self.i;
            self.bump();
            self.push(nl, HlKind::Text);
        }
    }

    fn scan_string(&mut self, quote: char) {
        let start = self.i;
        self.bump();
        loop {
            match self.peek() {
                None | Some('\n') => break,
                Some(c) if c == quote => {
                    self.bump();
                    break;
                }
                Some(_) => {
                    self.bump();
                }
            }
        }
        self.push(start, HlKind::String);
        self.transpose_base = false;
    }

    fn push_op(&mut self, len: usize) {
        let start = self.i;
        let text = &self.src[start..start + len];
        self.i += len;
        self.push(start, HlKind::Operator);
        self.transpose_base = text == "'" || text == ".'";
    }

    /// Multi-char and single-char operators, matching lexer tokens.
    /// Lone `&`, `|`, `~`, and `\` are not operators.
    fn match_op(&self) -> Option<usize> {
        let r = self.rest();
        const TWO: &[&str] = &[
            "+=", "-=", "*=", "/=", "==", "!=", "<=", ">=", "&&", "||", ".*", "./", ".^", ".'",
        ];
        for op in TWO {
            if r.starts_with(op) {
                return Some(op.len());
            }
        }
        let ch = self.peek()?;
        match ch {
            '+' | '-' | '*' | '/' | '^' | ':' | '=' | '<' | '>' | '!' | '@' => Some(ch.len_utf8()),
            '.' => {
                let next = r.chars().nth(1)?;
                if next.is_alphabetic() || next == '_' {
                    Some(1)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn scan_number(&mut self) {
        if self.try_radix() {
            return;
        }
        let start = self.i;
        let bytes = self.src.as_bytes();
        let mut k = self.i;
        while k < bytes.len() {
            let c = bytes[k];
            if c.is_ascii_digit() || c == b'.' || c == b'_' {
                k += 1;
            } else {
                break;
            }
        }
        // Exponent matches the lexer: `e` / `E`, optional sign, then
        // digits. `1e` (no digits) is still consumed; the lexer would
        // error, and we color the whole span as a number.
        if k < bytes.len() && (bytes[k] == b'e' || bytes[k] == b'E') {
            k += 1;
            if k < bytes.len() && (bytes[k] == b'+' || bytes[k] == b'-') {
                k += 1;
            }
            while k < bytes.len() && (bytes[k].is_ascii_digit() || bytes[k] == b'_') {
                k += 1;
            }
        }
        let has_digit = self.src[start..k].chars().any(|c| c.is_ascii_digit());
        if !has_digit {
            // `.` / `..` are not numbers. Emit one plain character and
            // rescan so `..` does not swallow a following digit.
            self.bump();
            self.push(start, HlKind::Text);
            self.transpose_base = false;
            return;
        }
        let rest = &self.src[k..];
        let mut rest_chars = rest.chars();
        let imag = matches!(rest_chars.next(), Some('i' | 'j'))
            && rest_chars
                .next()
                .map_or(true, |c| !c.is_alphanumeric() && c != '_');
        if imag {
            k += 1;
        }
        self.i = k;
        self.push(start, HlKind::Number);
        // f64 numbers and imaginary literals are transpose bases. Radix
        // integers (IntLit) are not — handled in `try_radix`.
        self.transpose_base = true;
    }

    /// `0x` / `0b` / `0o` literals. Returns false when the prefix is not a
    /// radix form (or has no digits), so the caller scans a normal number.
    /// A typed suffix (`0xFFu8`) is left for the next token.
    fn try_radix(&mut self) -> bool {
        let bytes = self.src.as_bytes();
        let i = self.i;
        if i + 1 >= bytes.len() || bytes[i] != b'0' {
            return false;
        }
        let radix: u32 = match bytes[i + 1] {
            b'x' | b'X' => 16,
            b'b' | b'B' => 2,
            b'o' | b'O' => 8,
            _ => return false,
        };
        let mut k = i + 2;
        let mut digits = 0usize;
        while k < bytes.len() {
            let c = bytes[k] as char;
            if c == '_' {
                if k + 1 < bytes.len() && (bytes[k + 1] as char).is_digit(radix) {
                    k += 1;
                    continue;
                }
                break;
            }
            if c.is_digit(radix) {
                digits += 1;
                k += 1;
                continue;
            }
            break;
        }
        if digits == 0 {
            return false;
        }
        self.i = k;
        self.push(i, HlKind::Number);
        self.transpose_base = false;
        true
    }

    fn scan_ident(&mut self) {
        let start = self.i;
        while matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_') {
            self.bump();
        }
        let word = &self.src[start..self.i];
        if KEYWORDS.contains(&word) {
            self.push(start, HlKind::Keyword);
            self.transpose_base = false;
        } else if self.peek() == Some('(') {
            self.push(start, HlKind::Function);
            self.transpose_base = true;
        } else {
            self.push(start, HlKind::Text);
            self.transpose_base = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(HlKind, String)> {
        highlight(src)
            .into_iter()
            .map(|s| (s.kind, src[s.start..s.end].to_string()))
            .collect()
    }

    fn assert_covers(src: &str) {
        let spans = highlight(src);
        let mut at = 0;
        for s in &spans {
            assert_eq!(s.start, at, "gap or overlap in {src:?}: {spans:?}");
            assert!(s.end > s.start);
            assert!(src.is_char_boundary(s.start) && src.is_char_boundary(s.end));
            at = s.end;
        }
        assert_eq!(at, src.len(), "did not cover {src:?}");
    }

    #[test]
    fn empty_covers() {
        assert!(highlight("").is_empty());
    }

    #[test]
    fn hash_and_percent_comments() {
        assert_eq!(
            kinds("# comment"),
            vec![(HlKind::Comment, "# comment".into())]
        );
        assert_eq!(
            kinds("% comment"),
            vec![(HlKind::Comment, "% comment".into())]
        );
        let src = "# comment\nx = 1";
        assert_covers(src);
        let ks = kinds(src);
        assert_eq!(ks[0], (HlKind::Comment, "# comment".into()));
        assert!(!ks[0].1.contains('\n'));
        // The newline is uncolored text (merged with the following indent/name).
        assert!(ks
            .iter()
            .any(|(k, t)| *k == HlKind::Text && t.contains('\n')));
    }

    #[test]
    fn full_keyword_list_and_cache_is_plain() {
        for kw in KEYWORDS {
            assert_eq!(kinds(kw), vec![(HlKind::Keyword, (*kw).into())], "{kw}");
        }
        let ks = kinds("cache = 5");
        assert_eq!(ks[0].0, HlKind::Text);
        assert!(ks[0].1.starts_with("cache"), "{ks:?}");
        assert!(ks.iter().all(|(k, _)| *k != HlKind::Keyword));
        // Call-like still uses the function color, not a keyword color.
        assert_eq!(kinds("cache(")[0], (HlKind::Function, "cache".into()));
    }

    #[test]
    fn not_equal_is_bang_eq_not_tilde() {
        assert_eq!(kinds("!="), vec![(HlKind::Operator, "!=".into())]);
        assert_eq!(
            kinds("~="),
            vec![(HlKind::Text, "~".into()), (HlKind::Operator, "=".into())]
        );
        for op in [
            "+=", "-=", "*=", "/=", "==", "<=", ">=", "&&", "||", ".*", "./", ".^", ".'",
        ] {
            assert_eq!(kinds(op), vec![(HlKind::Operator, op.into())], "{op}");
        }
    }

    #[test]
    fn numbers_radix_underscore_imaginary() {
        assert_eq!(kinds("0xFF"), vec![(HlKind::Number, "0xFF".into())]);
        assert_eq!(kinds("0b1010"), vec![(HlKind::Number, "0b1010".into())]);
        assert_eq!(kinds("0o17"), vec![(HlKind::Number, "0o17".into())]);
        assert_eq!(
            kinds("1_000_000"),
            vec![(HlKind::Number, "1_000_000".into())]
        );
        assert_eq!(kinds("2.5j"), vec![(HlKind::Number, "2.5j".into())]);
        assert_eq!(kinds("1.5e-3"), vec![(HlKind::Number, "1.5e-3".into())]);
        assert_eq!(kinds(".5"), vec![(HlKind::Number, ".5".into())]);
        // Suffix is an identifier, not part of the literal (`2jx`).
        assert_eq!(
            kinds("2jx"),
            vec![(HlKind::Number, "2".into()), (HlKind::Text, "jx".into())]
        );
        // Empty radix prefix falls back to a normal `0`.
        assert_eq!(
            kinds("0x"),
            vec![(HlKind::Number, "0".into()), (HlKind::Text, "x".into())]
        );
        // Typed suffix stays outside the literal.
        assert_eq!(kinds("0xFFu8")[0], (HlKind::Number, "0xFF".into()));
    }

    #[test]
    fn transpose_versus_string() {
        assert_eq!(
            kinds("x'"),
            vec![(HlKind::Text, "x".into()), (HlKind::Operator, "'".into())]
        );
        // Whitespace does not break the transpose context.
        assert_eq!(
            kinds("x '"),
            vec![(HlKind::Text, "x ".into()), (HlKind::Operator, "'".into())]
        );
        assert!(kinds("'world'").iter().any(|(k, _)| *k == HlKind::String));
        assert_eq!(kinds("='hi'")[1].0, HlKind::String);
        // A real newline ends the previous token, so the quote starts a string.
        let after_nl = kinds("x\n'");
        assert_eq!(after_nl.last().unwrap().0, HlKind::String);
        // `...` swallows the newline and keeps the ident as the previous token.
        let after_cont = kinds("x ...\n'");
        assert_eq!(after_cont.last().unwrap().0, HlKind::Operator);
        // IntLit is not a transpose base.
        assert_eq!(kinds("0xFF'")[1].0, HlKind::String);
        // A float is.
        assert_eq!(kinds("1'")[1].0, HlKind::Operator);
    }

    #[test]
    fn call_like_is_function_keyword_is_not() {
        assert_eq!(kinds("plot(x)")[0], (HlKind::Function, "plot".into()));
        assert_eq!(kinds("if(1)")[0], (HlKind::Keyword, "if".into()));
        assert_eq!(kinds("hold on")[0], (HlKind::Keyword, "hold".into()));
    }

    #[test]
    fn bad_chars_and_unterminated_string_cover_the_buffer() {
        for src in [
            "foo \\ bar",
            "\"unterminated",
            "\"hello\nworld\"",
            "</span><script>alert(1)</script>",
            "<img onerror=\"x\">",
            "x_1 = 1 # comment % also",
            "π = 3",
            "",
        ] {
            assert_covers(src);
        }
        let s = kinds("\"unterminated");
        assert_eq!(s[0].0, HlKind::String);
        assert_eq!(s[0].1, "\"unterminated");
        // Backslash is plain, not an operator, and scanning continues.
        assert!(kinds("a \\ b")
            .iter()
            .any(|(k, t)| *k == HlKind::Text && t.contains('\\')));
        assert!(crate::lexer::tokenize("a \\ b").is_err());
    }

    #[test]
    fn spans_round_trip_to_source() {
        let src = "for k = 1:3\n  disp(k)\nend # tail\nm = [1, 2];\nz = m'";
        assert_covers(src);
        let back: String = highlight(src)
            .into_iter()
            .map(|s| src[s.start..s.end].to_string())
            .collect();
        assert_eq!(back, src);
    }
}
