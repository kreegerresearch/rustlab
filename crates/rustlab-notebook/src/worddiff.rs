//! Word-level marks for the `watch` git diff.
//!
//! A modified paragraph (or heading, list item, code line) is compared
//! token by token. Prose tokens are words, whitespace, and punctuation,
//! with math (`$…$`, `$$…$$`), inline code, links, and escapes kept
//! whole so a change replaces them as a unit. The result is either byte
//! ranges to mark (code and source lines) or the new source with
//! private-use marker pairs around removed and added runs (prose), which
//! survive markdown rendering and become `<del>` / `<ins>` afterwards.

use std::ops::Range;

pub(crate) const INS_OPEN: char = '\u{E010}';
pub(crate) const INS_CLOSE: char = '\u{E011}';
pub(crate) const DEL_OPEN: char = '\u{E012}';
pub(crate) const DEL_CLOSE: char = '\u{E013}';

/// Pairs with fewer surviving tokens than this are shown as removed plus
/// added, not as a line of confetti.
pub(crate) const MIN_SURVIVAL: f64 = 0.5;

const MAX_TOKENS: usize = 1500;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    Prose,
    Code,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// End of the run that opens with `delim` at `at`, or `None`.
fn closing(s: &str, at: usize, delim: &str) -> Option<usize> {
    let body = at + delim.len();
    s.get(body..)?.find(delim).map(|i| body + i + delim.len())
}

/// End of a bracket group starting at `at` (`[` or `(`), honouring nesting.
fn bracket_end(s: &str, at: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s[at..].char_indices() {
        if c == '\n' && open == '(' {
            return None;
        }
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(at + i + c.len_utf8());
            }
        }
    }
    None
}

/// Token byte ranges covering all of `s`.
pub(crate) fn tokens(s: &str, mode: Mode) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < s.len() {
        let rest = &s[i..];
        let c = rest.chars().next().unwrap_or(' ');
        let end = if c.is_whitespace() {
            i + rest
                .char_indices()
                .find(|(_, c)| !c.is_whitespace())
                .map_or(rest.len(), |(k, _)| k)
        } else if is_word(c) {
            let mut e = i + rest
                .char_indices()
                .find(|(_, c)| !is_word(*c))
                .map_or(rest.len(), |(k, _)| k);
            // 2.5, 1,000 and don't stay one token.
            while let Some(sep) = s[e..].chars().next() {
                let after = s[e + sep.len_utf8()..].chars().next();
                let joins = match sep {
                    '.' | ',' => {
                        s[i..e].chars().all(|c| c.is_ascii_digit())
                            && after.is_some_and(|a| a.is_ascii_digit())
                    }
                    '\'' | '’' => mode == Mode::Prose && after.is_some_and(is_word),
                    _ => false,
                };
                if !joins {
                    break;
                }
                let tail = &s[e + sep.len_utf8()..];
                e += sep.len_utf8()
                    + tail
                        .char_indices()
                        .find(|(_, c)| !is_word(*c))
                        .map_or(tail.len(), |(k, _)| k);
            }
            e
        } else if mode == Mode::Code && c == '"' {
            rest[1..].find('"').map_or(i + 1, |k| i + 1 + k + 1)
        } else if mode == Mode::Prose {
            let whole = match c {
                '\\' => rest.chars().nth(1).map(|n| i + 1 + n.len_utf8()),
                '$' if rest.starts_with("$$") => closing(s, i, "$$"),
                '$' => closing(s, i, "$").filter(|&e| !s[i..e].contains('\n')),
                '`' => {
                    let run = rest.chars().take_while(|&c| c == '`').count();
                    closing(s, i, &"`".repeat(run))
                }
                '=' if rest.starts_with("==") => closing(s, i, "=="),
                '%' if rest.starts_with("%%") => closing(s, i, "%%"),
                '<' => rest
                    .find('>')
                    .filter(|&k| !rest[..k].contains('\n'))
                    .map(|k| i + k + 1),
                '[' => bracket_end(s, i, '[', ']').map(|e| {
                    match s[e..].chars().next() {
                        Some('(') => bracket_end(s, e, '(', ')').unwrap_or(e),
                        Some('[') => bracket_end(s, e, '[', ']').unwrap_or(e),
                        _ => e,
                    }
                }),
                _ => None,
            };
            whole.unwrap_or(i + c.len_utf8())
        } else {
            i + c.len_utf8()
        };
        out.push(i..end.max(i + c.len_utf8()));
        i = end.max(i + c.len_utf8());
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    Eq,
    Del,
    Ins,
}

/// The token comparison of one pair.
pub(crate) struct WordDiff {
    /// Removed runs in the old text, byte ranges.
    pub old_marks: Vec<Range<usize>>,
    /// Added runs in the new text, byte ranges.
    pub new_marks: Vec<Range<usize>>,
    /// Share of non-space tokens that are unchanged.
    pub survival: f64,
    /// Text of every removed or added token.
    pub changed: Vec<String>,
    /// The new text with marker pairs around removed and added runs.
    pub merged: String,
}

/// `(op, old index, new index)`; the index that does not apply is unused.
fn lcs_ops(a: &[&str], b: &[&str]) -> Vec<(Op, usize, usize)> {
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut ops = Vec::with_capacity(n + m);
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push((Op::Eq, i, j));
            i += 1;
            j += 1;
        } else if i < n && (j == m || dp[i + 1][j] >= dp[i][j + 1]) {
            ops.push((Op::Del, i, j));
            i += 1;
        } else {
            ops.push((Op::Ins, i, j));
            j += 1;
        }
    }
    ops
}

/// Compare two texts token by token. `None` when either side is too long
/// to compare this way.
pub(crate) fn diff(old: &str, new: &str, mode: Mode) -> Option<WordDiff> {
    let ta = tokens(old, mode);
    let tb = tokens(new, mode);
    if ta.len() > MAX_TOKENS || tb.len() > MAX_TOKENS {
        return None;
    }
    let a: Vec<&str> = ta.iter().map(|r| &old[r.clone()]).collect();
    let b: Vec<&str> = tb.iter().map(|r| &new[r.clone()]).collect();
    let mut ops = lcs_ops(&a, &b);
    // A space kept between two changes joins them, so `40 dB` → `53 dB`
    // stays two marks but `a b c` → `x y z` is one run, not five.
    let mut k = 1;
    while k + 1 < ops.len() {
        let (op, ai, bj) = ops[k];
        if op == Op::Eq
            && a[ai].trim().is_empty()
            && ops[k - 1].0 != Op::Eq
            && ops[k + 1].0 != Op::Eq
        {
            ops.splice(k..=k, [(Op::Del, ai, bj), (Op::Ins, ai, bj)]);
            k += 2;
        } else {
            k += 1;
        }
    }
    fn solid(s: &str) -> bool {
        !s.trim().is_empty()
    }
    let kept = ops
        .iter()
        .filter(|(op, i, _)| *op == Op::Eq && solid(a[*i]))
        .count();
    let total = a
        .iter()
        .filter(|s| solid(s))
        .count()
        .max(b.iter().filter(|s| solid(s)).count());
    let survival = if total == 0 {
        1.0
    } else {
        kept as f64 / total as f64
    };
    let mut out = WordDiff {
        old_marks: Vec::new(),
        new_marks: Vec::new(),
        survival,
        changed: Vec::new(),
        merged: String::with_capacity(new.len() + 16),
    };
    let mut k = 0;
    while k < ops.len() {
        if ops[k].0 == Op::Eq {
            out.merged.push_str(b[ops[k].2]);
            k += 1;
            continue;
        }
        let mut dels = Vec::new();
        let mut ins = Vec::new();
        while k < ops.len() && ops[k].0 != Op::Eq {
            match ops[k].0 {
                Op::Del => dels.push(ops[k].1),
                _ => ins.push(ops[k].2),
            }
            k += 1;
        }
        if let (Some(&f), Some(&l)) = (dels.first(), dels.last()) {
            out.old_marks.push(ta[f].start..ta[l].end);
            out.merged.push(DEL_OPEN);
            for &d in &dels {
                out.merged.push_str(a[d]);
                out.changed.push(a[d].to_string());
            }
            out.merged.push(DEL_CLOSE);
        }
        if let (Some(&f), Some(&l)) = (ins.first(), ins.last()) {
            out.new_marks.push(tb[f].start..tb[l].end);
            out.merged.push(INS_OPEN);
            for &n in &ins {
                out.merged.push_str(b[n]);
                out.changed.push(b[n].to_string());
            }
            out.merged.push(INS_CLOSE);
        }
    }
    Some(out)
}

/// Turn the marker pairs in rendered HTML into `<del>` / `<ins>`. `None`
/// when they do not pair up (markdown moved one across an element edge).
pub(crate) fn markers_to_html(html: &str) -> Option<String> {
    let mut out = String::with_capacity(html.len() + 64);
    let mut open: Option<char> = None;
    for c in html.chars() {
        match c {
            INS_OPEN | DEL_OPEN => {
                if open.is_some() {
                    return None;
                }
                open = Some(c);
                out.push_str(if c == INS_OPEN {
                    "<ins class=\"rl-w\">"
                } else {
                    "<del class=\"rl-w\">"
                });
            }
            INS_CLOSE | DEL_CLOSE => {
                let want = if c == INS_CLOSE { INS_OPEN } else { DEL_OPEN };
                if open != Some(want) {
                    return None;
                }
                open = None;
                out.push_str(if c == INS_CLOSE { "</ins>" } else { "</del>" });
            }
            _ => out.push(c),
        }
    }
    open.is_none().then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(s: &str, mode: Mode) -> Vec<&str> {
        tokens(s, mode).into_iter().map(|r| &s[r]).collect()
    }

    #[test]
    fn prose_tokens_keep_math_code_and_links_whole() {
        let s = "About $x_1 + 2$ and `a b` see [the doc](a.md#x) 2.5 don't.";
        let t = texts(s, Mode::Prose);
        assert!(t.contains(&"$x_1 + 2$"), "{t:?}");
        assert!(t.contains(&"`a b`"), "{t:?}");
        assert!(t.contains(&"[the doc](a.md#x)"), "{t:?}");
        assert!(t.contains(&"2.5"), "{t:?}");
        assert!(t.contains(&"don't"), "{t:?}");
        assert_eq!(t.concat(), s);
        assert_eq!(texts("$$a\\\\b$$ x", Mode::Prose)[0], "$$a\\\\b$$");
        // A lone dollar is punctuation, not a span to the end.
        assert_eq!(texts("costs $5", Mode::Prose).concat(), "costs $5");
    }

    #[test]
    fn code_tokens_keep_strings_whole() {
        let t = texts("w = \"hann\";", Mode::Code);
        assert_eq!(t, vec!["w", " ", "=", " ", "\"hann\"", ";"]);
        let t = texts("h = fir(31, 2.5)", Mode::Code);
        assert!(t.contains(&"2.5") && t.contains(&"31"), "{t:?}");
    }

    #[test]
    fn numbers_change_as_two_marks() {
        let d = diff(
            "About 40 dB for 31 taps.",
            "About 53 dB for 63 taps.",
            Mode::Prose,
        )
        .unwrap();
        assert_eq!(d.changed, vec!["40", "53", "31", "63"]);
        assert_eq!(d.new_marks.len(), 2);
        assert!(d.survival >= MIN_SURVIVAL);
        let html = markers_to_html(&d.merged).unwrap();
        assert_eq!(
            html,
            "About <del class=\"rl-w\">40</del><ins class=\"rl-w\">53</ins> dB for <del class=\"rl-w\">31</del><ins class=\"rl-w\">63</ins> taps."
        );
    }

    #[test]
    fn a_rewrite_has_low_survival_and_one_run() {
        let d = diff("one two three", "four five six", Mode::Prose).unwrap();
        assert!(d.survival < MIN_SURVIVAL);
        assert_eq!(d.new_marks.len(), 1, "joined across kept spaces");
    }

    #[test]
    fn math_is_replaced_whole() {
        let d = diff("Let $x$ be.", "Let $y$ be.", Mode::Prose).unwrap();
        assert_eq!(d.changed, vec!["$x$", "$y$"]);
        assert!(d.merged.contains(&format!("{DEL_OPEN}$x${DEL_CLOSE}{INS_OPEN}$y${INS_CLOSE}")));
    }

    #[test]
    fn unbalanced_markers_are_refused() {
        assert!(markers_to_html(&format!("a{INS_OPEN}b")).is_none());
        assert!(markers_to_html(&format!("a{INS_OPEN}b{DEL_CLOSE}")).is_none());
        assert_eq!(markers_to_html("plain").as_deref(), Some("plain"));
    }
}
