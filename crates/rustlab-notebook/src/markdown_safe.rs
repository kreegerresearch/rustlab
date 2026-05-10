//! GitHub-safe rewrites for markdown that contains math.
//!
//! ## The problem
//!
//! When a `.md` file is committed and viewed on GitHub, GitHub's renderer
//! runs CommonMark passes (emphasis, backslash-escapes) **before** its
//! KaTeX math pass. Several common LaTeX constructs survive every other
//! tool but break here:
//!
//! - `$\mathbf{x}^*$ ... $\mathbf{x}^*$` — the `*` chars in two separate
//!   math spans get paired as emphasis delimiters across the math
//!   boundaries. KaTeX then sees `\mathbf{x}^` (no superscript content)
//!   and reports `Missing open brace for superscript`.
//! - `\|` (norm bar) — CommonMark backslash-escape rule says `\X` for
//!   ASCII punctuation `X` is the literal character. So `\|` becomes
//!   `|`, and KaTeX gets a single bar instead of a double-bar norm.
//! - `\,` `\;` `\:` `\!` (LaTeX spacing commands) — same backslash-
//!   escape problem; the backslash is stripped before KaTeX runs.
//!
//! `--format html` is unaffected because rustlab's bundled KaTeX runs
//! on the raw output before any CommonMark pass touches it.
//!
//! ## The fix
//!
//! Inside every `$...$` and `$$...$$` math span, rewrite GitHub-hostile
//! tokens to KaTeX equivalents that survive CommonMark:
//!
//! - `^*`  → `^{\ast}`
//! - `_*`  → `_{\ast}`
//! - `\|`  → `\Vert`
//! - `\,`  → `\thinspace`
//! - `\;`  → `\thickspace`
//! - `\:`  → `\medspace`
//! - `\!`  → `\negthinspace`
//!
//! Each replacement renders identically to the original under KaTeX.
//!
//! Code spans (`` `...` ``) and fenced code blocks (```` ``` ```` /
//! `~~~`) are left untouched, even if they contain `$` or LaTeX-shaped
//! text — the `examples.md` notebook in this repo has plenty of those.

/// Rewrite a markdown source string so its math survives GitHub's
/// CommonMark renderer. Outside-math content (prose, code, headings,
/// lists, links) is preserved verbatim.
///
/// The algorithm is a small state machine over characters that tracks:
/// - Fenced code blocks (opening ``` `````` `` ` ` `` ` `` ``` or `~~~`).
/// - Inline code spans (`` `...` ``, including the multi-backtick form
///   `` `` ` `` `` etc.).
/// - Inline math `$...$` (single dollar, not preceded by backslash).
/// - Display math `$$...$$`.
///
/// We only apply rewrites when inside math. The state machine is line-
/// aware for fenced code blocks (CommonMark requires the opening fence
/// to start a line, modulo leading whitespace).
/// Length in bytes of the UTF-8 character starting at `b` (a leading byte).
#[inline]
fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xC0 {
        1 // continuation byte (shouldn't appear at boundary; treat as 1)
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

/// Append the single UTF-8 char starting at `i` to `out` and return the
/// new byte index. Used everywhere a char passes through verbatim, so
/// non-ASCII (em dashes, accents, etc.) survives the rewrite.
#[inline]
fn pass_one(out: &mut String, input: &str, i: usize) -> usize {
    let len = utf8_len(input.as_bytes()[i]);
    out.push_str(&input[i..i + len]);
    i + len
}

pub fn github_safe_math(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len() + input.len() / 16);
    let mut i = 0;
    let n = bytes.len();
    let mut in_fenced_code: Option<(u8, usize)> = None; // (fence char, fence length)
    let mut at_line_start = true;

    while i < n {
        // ── Fenced code block handling ──────────────────────────────
        if at_line_start {
            // Skip up to 3 leading spaces (CommonMark allows indented
            // fences up to 3 spaces; 4+ is an indented code block).
            let mut ws = 0usize;
            while i + ws < n && bytes[i + ws] == b' ' && ws < 4 {
                ws += 1;
            }
            if let Some((fence_char, fence_len)) = in_fenced_code {
                // Check for matching closing fence.
                let mut p = i + ws;
                let mut count = 0usize;
                while p < n && bytes[p] == fence_char {
                    count += 1;
                    p += 1;
                }
                if count >= fence_len && (p == n || bytes[p] == b'\n') {
                    // Close the fence — copy up to and including the newline.
                    while i < n && bytes[i] != b'\n' {
                        i = pass_one(&mut out, input, i);
                    }
                    if i < n {
                        out.push('\n');
                        i += 1;
                    }
                    at_line_start = true;
                    in_fenced_code = None;
                    continue;
                }
                // Still inside the fenced block: copy the line verbatim.
                while i < n && bytes[i] != b'\n' {
                    i = pass_one(&mut out, input, i);
                }
                if i < n {
                    out.push('\n');
                    i += 1;
                }
                at_line_start = true;
                continue;
            }
            // Not in a fenced block — check for an opening fence.
            let mut p = i + ws;
            let mut count = 0usize;
            let fc = if p < n && (bytes[p] == b'`' || bytes[p] == b'~') {
                bytes[p]
            } else {
                0
            };
            if fc != 0 {
                while p < n && bytes[p] == fc {
                    count += 1;
                    p += 1;
                }
                if count >= 3 {
                    in_fenced_code = Some((fc, count));
                    while i < n && bytes[i] != b'\n' {
                        i = pass_one(&mut out, input, i);
                    }
                    if i < n {
                        out.push('\n');
                        i += 1;
                    }
                    at_line_start = true;
                    continue;
                }
            }
            at_line_start = false;
        }

        // ── Inline code span ────────────────────────────────────────
        if bytes[i] == b'`' {
            let mut count = 0usize;
            while i + count < n && bytes[i + count] == b'`' {
                count += 1;
            }
            // Emit the opening run, then look for a matching closing run
            // of the same length.
            for _ in 0..count {
                out.push('`');
            }
            i += count;
            let mut closed = false;
            while i < n {
                if bytes[i] == b'`' {
                    let mut c = 0usize;
                    while i + c < n && bytes[i + c] == b'`' {
                        c += 1;
                    }
                    if c == count {
                        for _ in 0..count {
                            out.push('`');
                        }
                        i += count;
                        closed = true;
                        break;
                    }
                    for _ in 0..c {
                        out.push('`');
                    }
                    i += c;
                } else {
                    if bytes[i] == b'\n' {
                        at_line_start = true;
                    }
                    i = pass_one(&mut out, input, i);
                }
            }
            if !closed {
                // Unterminated code span — caller's input was already
                // malformed; leave the rest as-is.
            }
            continue;
        }

        // ── Math spans ──────────────────────────────────────────────
        if bytes[i] == b'$' {
            // Backslash-escaped dollar (`\$`) is a literal dollar in
            // CommonMark, not a math delimiter. The previous char
            // already emitted handles this — we just check what was
            // last pushed.
            if out.ends_with('\\') && !out.ends_with("\\\\") {
                out.push('$');
                i += 1;
                continue;
            }
            // Display math `$$...$$`?
            if i + 1 < n && bytes[i + 1] == b'$' {
                // Find the closing `$$`.
                let body_start = i + 2;
                let mut p = body_start;
                let mut close: Option<usize> = None;
                while p + 1 < n {
                    if bytes[p] == b'$'
                        && bytes[p + 1] == b'$'
                        && (p == body_start || bytes[p - 1] != b'\\')
                    {
                        close = Some(p);
                        break;
                    }
                    p += 1;
                }
                if let Some(end) = close {
                    out.push_str("$$");
                    let math = &input[body_start..end];
                    out.push_str(&rewrite_math_body(math));
                    out.push_str("$$");
                    i = end + 2;
                    continue;
                }
                // Unterminated display math: emit literal and bail.
                out.push_str("$$");
                i += 2;
                continue;
            }
            // Inline math `$...$`. Find the closing `$` on the same line
            // (CommonMark/GitHub math spans don't cross paragraphs).
            let body_start = i + 1;
            let mut p = body_start;
            let mut close: Option<usize> = None;
            while p < n && bytes[p] != b'\n' {
                if bytes[p] == b'$' && bytes[p - 1] != b'\\' {
                    close = Some(p);
                    break;
                }
                p += 1;
            }
            if let Some(end) = close {
                out.push('$');
                let math = &input[body_start..end];
                out.push_str(&rewrite_math_body(math));
                out.push('$');
                i = end + 1;
                continue;
            }
            // Unterminated inline math: emit literal `$` and continue.
            out.push('$');
            i += 1;
            continue;
        }

        // ── Default: copy verbatim, track newlines ──────────────────
        let c = bytes[i];
        if c == b'\n' {
            at_line_start = true;
        } else {
            at_line_start = false;
        }
        i = pass_one(&mut out, input, i);
    }
    out
}

/// Apply the math-only rewrites to a math span body. Replacements are
/// chosen so each renders identically to the original under KaTeX.
fn rewrite_math_body(body: &str) -> String {
    // Order matters — process longer / more specific patterns first to
    // avoid partial-match issues. A single linear scan with manual
    // pattern detection is simpler than chained `replace` calls and
    // also handles the `\|` rule correctly (don't rewrite `\\|`).
    let bytes = body.as_bytes();
    let mut out = String::with_capacity(body.len() + body.len() / 8);
    let mut i = 0usize;
    let n = bytes.len();
    while i < n {
        let c = bytes[i];
        if c == b'^' || c == b'_' {
            // Look for `^*` or `_*` — replace `*` with `{\ast}`.
            // Also `^**`, `_**` — replace with `{\ast\ast}`.
            if i + 1 < n && bytes[i + 1] == b'*' {
                let star_count = if i + 2 < n && bytes[i + 2] == b'*' { 2 } else { 1 };
                out.push(c as char);
                out.push('{');
                for _ in 0..star_count {
                    out.push_str(r"\ast");
                }
                out.push('}');
                i += 1 + star_count;
                continue;
            }
        }
        if c == b'\\' && i + 1 < n {
            // Backslash-followed-by-punctuation forms that CommonMark
            // would strip the backslash from. Pick the alpha-suffixed
            // KaTeX equivalent. Insert a separator (` ` or `{}`) when
            // the following char would otherwise extend the command
            // name and turn `\Vert x` into `\Vertx`.
            let next = bytes[i + 1];
            let replacement: Option<&'static str> = match next {
                b'|' => Some(r"\Vert"),
                b',' => Some(r"\thinspace"),
                b';' => Some(r"\thickspace"),
                b':' => Some(r"\medspace"),
                b'!' => Some(r"\negthinspace"),
                _ => None,
            };
            if let Some(repl) = replacement {
                out.push_str(repl);
                // Separator: empty group `{}` if the next char would
                // be merged into the alpha command name, otherwise no
                // separator needed (the next non-alpha byte already
                // terminates the command).
                let after = if i + 2 < n { Some(bytes[i + 2]) } else { None };
                let needs_sep = match after {
                    Some(b) => b.is_ascii_alphabetic(),
                    None => false,
                };
                if needs_sep {
                    out.push_str("{}");
                }
                i += 2;
                continue;
            }
            // Pass through `\\X` (escaped backslash followed by anything)
            // as a literal `\\` plus whatever comes next; the second
            // backslash will be re-evaluated against the rules above on
            // the next iteration.
            if next == b'\\' {
                out.push_str("\\\\");
                i += 2;
                continue;
            }
        }
        // Default: copy one UTF-8 char.
        let len = utf8_len(c);
        out.push_str(&body[i..i + len]);
        i += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_caret_star_in_inline_math() {
        // The headline bug from the issue report.
        let input = r"A **fixed point** $\mathbf{x}^*$ satisfies $\mathbf{f}(\mathbf{x}^*) = \mathbf{0}$ — derivatives are zero.";
        let out = github_safe_math(input);
        assert!(out.contains(r"$\mathbf{x}^{\ast}$"));
        assert!(out.contains(r"$\mathbf{f}(\mathbf{x}^{\ast}) = \mathbf{0}$"));
        // Crucially, no bare `^*` remains anywhere in math.
        assert!(!out.contains("^*"));
        // The surrounding emphasis / em-dash is preserved.
        assert!(out.contains("**fixed point**"));
        assert!(out.contains("—"));
    }

    #[test]
    fn rewrites_underscore_star() {
        let out = github_safe_math(r"$\mathbf{u}_*$");
        assert_eq!(out, r"$\mathbf{u}_{\ast}$");
    }

    #[test]
    fn rewrites_backslash_pipe_to_vert() {
        // `\|x` → `\Vert{}x` (the `{}` terminates the command name so
        // KaTeX doesn't parse `\Vertx` as a single token). The second
        // `\|` is followed by ` ` (space), no separator needed.
        let out = github_safe_math(r"$$\|x\| \leq 1$$");
        assert_eq!(out, r"$$\Vert{}x\Vert \leq 1$$");
    }

    #[test]
    fn rewrites_backslash_thinspace() {
        // From the bug repro's display equation.
        let out = github_safe_math(
            r"$$\frac{\partial \mathbf{f}}{\partial \mathbf{x}}\,\delta\mathbf{x}$$",
        );
        assert!(out.contains(r"\thinspace\delta"));
        assert!(!out.contains(r"}\,"));
    }

    #[test]
    fn rewrites_display_math_too() {
        let input = r"$$\dot{\mathbf{x}} = \mathbf{f}(\mathbf{x}^*) + \mathcal{O}(\|\delta\mathbf{x}\|^2).$$";
        let out = github_safe_math(input);
        assert!(out.contains(r"\mathbf{x}^{\ast}"));
        assert!(out.contains(r"\Vert\delta\mathbf{x}\Vert"));
    }

    #[test]
    fn does_not_rewrite_outside_math() {
        // Star outside math (CommonMark emphasis) is preserved.
        let input = "**bold** and *italic* and a bare * here.";
        let out = github_safe_math(input);
        assert_eq!(out, input);
    }

    #[test]
    fn does_not_rewrite_inside_inline_code() {
        let input = r"Use `$\mathbf{x}^*$` to write the math.";
        let out = github_safe_math(input);
        assert_eq!(out, input, "code spans must be untouched");
    }

    #[test]
    fn does_not_rewrite_inside_fenced_code() {
        let input = "```\n$\\mathbf{x}^*$ inside code\n```";
        let out = github_safe_math(input);
        assert_eq!(out, input, "fenced code blocks must be untouched");
    }

    #[test]
    fn does_not_rewrite_inside_tilde_fenced_code() {
        let input = "~~~\n$\\mathbf{x}^*$ inside tilde fence\n~~~";
        let out = github_safe_math(input);
        assert_eq!(out, input);
    }

    #[test]
    fn unrelated_star_inside_math_left_alone() {
        // `*` not adjacent to ^ or _ stays as-is. The bug is specifically
        // about `^*` and `_*`; bare `*` for multiplication is preserved.
        let out = github_safe_math(r"$a * b$");
        assert_eq!(out, r"$a * b$");
    }

    #[test]
    fn caret_double_star_protected() {
        let out = github_safe_math(r"$x^{**}$");
        // Already braced — leave as is.
        assert_eq!(out, r"$x^{**}$");
        // Unbraced `^**` — replace with braced `\ast\ast`.
        let out2 = github_safe_math(r"$x^**$");
        assert_eq!(out2, r"$x^{\ast\ast}$");
    }

    #[test]
    fn double_backslash_pipe_preserved() {
        // `\\|` is escaped backslash + bar — not the math norm bar.
        // We should not rewrite it. Our state machine pushes `\\` when
        // it sees backslash-backslash, so the second `|` is its own
        // thing.
        let out = github_safe_math(r"$a \\| b$");
        // The result should still contain a literal `|` (or whatever
        // KaTeX expects). The point is `\Vert` should NOT appear.
        assert!(!out.contains(r"\Vert"), "should not rewrite \\\\|");
    }

    #[test]
    fn full_repro_paragraph_renders_safely() {
        // The exact paragraph from the bug report.
        let input = r"A **fixed point** $\mathbf{x}^*$ satisfies $\mathbf{f}(\mathbf{x}^*) = \mathbf{0}$ — derivatives are zero. Define the deviation $\delta\mathbf{x} = \mathbf{x} - \mathbf{x}^*$ and Taylor-expand:

$$\dot{\mathbf{x}} = \mathbf{f}(\mathbf{x}^*) + \frac{\partial \mathbf{f}}{\partial \mathbf{x}}\bigg|_{\mathbf{x}^*}\,\delta\mathbf{x} + \mathcal{O}(\|\delta\mathbf{x}\|^2).$$";
        let out = github_safe_math(input);
        // No bare `^*` survives in any math span.
        // Note: `^*` could in principle appear as text in prose, but
        // the bug's repro paragraph has them only inside math, so a
        // global absence check is informative here.
        assert!(!out.contains("^*"), "found bare ^* in:\n{out}");
        // Bracketed `\,` and `\|` survive as their alpha equivalents.
        assert!(out.contains(r"\thinspace\delta"));
        assert!(out.contains(r"\Vert\delta"));
        // Prose-level emphasis and dashes are preserved.
        assert!(out.contains("**fixed point**"));
        assert!(out.contains("—"));
    }

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(github_safe_math(""), "");
    }

    #[test]
    fn unterminated_math_is_passed_through() {
        // Don't crash, don't rewrite.
        let out = github_safe_math(r"$x^* with no close");
        assert_eq!(out, r"$x^* with no close");
    }

    #[test]
    fn dollar_in_prose_left_alone() {
        // A lone `$` in prose (e.g., a price) shouldn't be misparsed.
        // CommonMark requires matching pairs; we follow the same rule.
        let out = github_safe_math("Cost is $5 today.");
        assert_eq!(out, "Cost is $5 today.");
    }
}
