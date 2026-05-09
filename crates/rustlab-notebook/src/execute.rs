use crate::parse::{Block, CalloutKind, MermaidDirectives};
use rustlab_plot::{
    clear_notebook_animations, clear_notebook_figures, set_plot_context, take_notebook_animations,
    take_notebook_figures, FigureState, NotebookAnimation, PlotContext, FIGURE,
};
use rustlab_script::Evaluator;

/// A rendered block ready for HTML output.
#[derive(Debug)]
pub enum Rendered {
    /// Markdown prose (raw markdown text, not yet converted to HTML).
    Markdown(String),
    /// An executed code block with its results.
    Code {
        source: String,
        text_output: String,
        error: Option<String>,
        /// One FigureState per inline plot produced by the block.
        /// Each `savefig()` call captures a snapshot; if the block ends
        /// with unsaved plot state, a final snapshot is appended.
        figures: Vec<FigureState>,
        /// One captured animation per `saveanim()` call in the block.
        /// In notebook mode `saveanim()` does not write the user's path —
        /// the renderer decides where to put the standalone HTML and how
        /// to embed it (inline div for HTML output, link for Markdown).
        animations: Vec<NotebookAnimation>,
        /// If true, source code should be hidden in rendered output.
        hidden: bool,
        /// If set, wrap output in a collapsible disclosure widget.
        details: Option<String>,
        /// If set, tile image outputs N-across.
        grid_cols: Option<usize>,
    },
    /// A Mermaid diagram block. Renderers turn `source` into SVG.
    Mermaid {
        source: String,
        hidden: bool,
        details: Option<String>,
        caption: Option<String>,
    },
    /// A callout box. `title` overrides the default kind label when set.
    Callout {
        kind: CalloutKind,
        title: Option<String>,
        content: String,
    },
    /// Start of a numbered exercise.
    ExerciseStart { number: usize },
    /// Start of a solution (collapsed by default).
    SolutionStart,
}

/// Execute a parsed notebook, returning rendered blocks.
///
/// Code blocks run in sequence through a shared evaluator (variables
/// persist across blocks). After each code block, the current figure
/// is captured if it has any series data, and any text output (from
/// assignments, `disp()`, `print()`, etc.) is captured via the
/// evaluator's output buffer.
pub fn execute_notebook(blocks: &[Block]) -> Vec<Rendered> {
    // Suppress TUI plot rendering — notebook captures FigureState directly.
    // PlotContext::Notebook is sticky: figure() calls cannot override it.
    set_plot_context(PlotContext::Notebook);

    let mut ev = Evaluator::new();
    let mut rendered = Vec::with_capacity(blocks.len());
    let mut exercise_counter = 0usize;

    for block in blocks {
        match block {
            Block::Markdown(text) => {
                let interpolated = interpolate_markdown(text, &mut ev);
                rendered.push(Rendered::Markdown(interpolated));
            }
            Block::Code { source, directives } => {
                // Reset figure before each code block so we only capture
                // what this block produces — unless hold is on, in which
                // case we preserve the figure state for multi-block overlays.
                let hold_active = FIGURE.with(|fig| fig.borrow().hold);
                if !hold_active {
                    FIGURE.with(|fig| fig.borrow_mut().reset());
                }
                // Drop any stray savefig snapshots from a prior block.
                clear_notebook_figures();
                clear_notebook_animations();

                // Capture text output during execution
                rustlab_script::start_capture();
                let error = run_code_block(&mut ev, source);
                let text_output = rustlab_script::stop_capture();

                // Collect per-savefig snapshots; if none were taken but the
                // block left plot data in FIGURE, fall back to a final snapshot.
                let mut figures = take_notebook_figures();
                if figures.is_empty() {
                    FIGURE.with(|fig| {
                        let f = fig.borrow();
                        if f.subplots.iter().any(|s| {
                            !s.series.is_empty()
                                || s.heatmap.is_some()
                                || s.surface.is_some()
                                || !s.contours.is_empty()
                                || !s.quivers.is_empty()
                                || !s.streamlines.is_empty()
                        }) {
                            figures.push(f.clone());
                        }
                    });
                }

                let animations = take_notebook_animations();

                rendered.push(Rendered::Code {
                    source: source.clone(),
                    text_output,
                    error,
                    figures,
                    animations,
                    hidden: directives.hidden,
                    details: directives.details.clone(),
                    grid_cols: directives.grid_cols,
                });
            }
            Block::Mermaid { source, directives } => {
                let MermaidDirectives { hidden, details, caption } = directives.clone();
                rendered.push(Rendered::Mermaid {
                    source: source.clone(),
                    hidden,
                    details,
                    caption,
                });
            }
            Block::Callout {
                kind,
                title,
                content,
            } => {
                let interpolated = interpolate_markdown(content, &mut ev);
                rendered.push(Rendered::Callout {
                    kind: *kind,
                    title: title.clone(),
                    content: interpolated,
                });
            }
            Block::ExerciseStart => {
                exercise_counter += 1;
                rendered.push(Rendered::ExerciseStart {
                    number: exercise_counter,
                });
            }
            Block::SolutionStart => {
                rendered.push(Rendered::SolutionStart);
            }
        }
    }

    rendered
}

/// Run a code block through the evaluator. Returns `Some(error_message)` on failure.
fn run_code_block(ev: &mut Evaluator, source: &str) -> Option<String> {
    let tokens = match rustlab_script::lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => return Some(format!("{e}")),
    };
    let stmts = match rustlab_script::parser::parse(tokens) {
        Ok(s) => s,
        Err(e) => return Some(format!("{e}")),
    };
    for stmt in &stmts {
        if let Err(e) = ev.exec_stmt(stmt) {
            return Some(format!("{e}"));
        }
    }
    None
}

/// Interpolate `${expr}` and `${expr:format}` templates in markdown text.
///
/// - `${x}` evaluates expression `x` and inserts its Display representation
/// - `${x:%,.2f}` evaluates `x` and formats it with sprintf
/// - When `${expr}` is encountered inside an already-open `$...$` inline-math
///   span, the value is emitted plain (no extra `$`), so the surrounding
///   `$<math> ${expr}$` source becomes `$<math> <value>$`.
/// - When `${expr}$` appears in plain text (no math span open), the trailing
///   `$` is consumed and the value is wrapped: output `$<value>$`. This keeps
///   the source pattern Obsidian-compatible while producing valid math.
/// - `\${...}` produces literal `${...}` (drops the backslash, escapes interp).
/// - `\$` (any other follower) passes through verbatim and does *not* toggle
///   inline-math state — this is the standard markdown escape for currency.
/// - If the expression errors, inserts `<ERROR: message>`
fn interpolate_markdown(md: &str, ev: &mut Evaluator) -> String {
    // Fast path: no templates
    if !md.contains("${") {
        return md.to_string();
    }

    let chars: Vec<char> = md.chars().collect();
    let mut result = String::with_capacity(md.len());
    let mut i = 0;
    // Tracks open `$...$` inline-math spans so `${expr}$` can decide whether
    // to wrap (plain text) or stay bare (already inside math). Best-effort —
    // ignores `$` inside code spans, which is the rare edge case.
    let mut in_math = false;

    while i < chars.len() {
        match &chars[i..] {
            // `\${` → literal `${` (escapes interpolation). No math toggle.
            ['\\', '$', '{', ..] => {
                result.push_str("${");
                i += 3;
            }
            // `\$X` → verbatim `\$` (markdown's currency escape). No toggle.
            ['\\', '$', ..] => {
                result.push_str("\\$");
                i += 2;
            }
            // `${...}` interpolation, optionally with math-wrap shorthand.
            ['$', '{', ..] => match consume_interp(&chars, i, in_math, ev) {
                Some((text, next)) => {
                    result.push_str(&text);
                    i = next;
                }
                None => {
                    // Unmatched `}` — emit the rest verbatim and stop.
                    result.extend(&chars[i..]);
                    break;
                }
            },
            // `$$` display-math fences pass through without toggling.
            ['$', '$', ..] => {
                result.push_str("$$");
                i += 2;
            }
            // Plain `$` toggles inline-math state.
            ['$', ..] => {
                in_math = !in_math;
                result.push('$');
                i += 1;
            }
            [c, ..] => {
                result.push(*c);
                i += 1;
            }
            [] => break,
        }
    }
    result
}

/// Consume `${expr}` (and an optional math-wrap trailing `$`) starting at
/// `i`, where `chars[i..i+2] == ['$', '{']`. Returns the substituted text and
/// the index just past what was consumed, or `None` if the closing `}` is
/// missing (caller should bail out).
fn consume_interp(
    chars: &[char],
    i: usize,
    in_math: bool,
    ev: &mut Evaluator,
) -> Option<(String, usize)> {
    let inner_start = i + 2;
    let close = find_close_brace(chars, inner_start)?;
    let inner: String = chars[inner_start..close].iter().collect();
    let mut next = close + 1;

    // Math-wrap when followed by a single `$` (not `$$` or `${...}`) and we
    // aren't already inside an open math span.
    let trailing_dollar = chars.get(next) == Some(&'$')
        && !matches!(chars.get(next + 1), Some('$' | '{'));
    let math_wrap = !in_math && trailing_dollar;

    let (expr_str, fmt_spec) = split_expr_format(&inner);
    let value = eval_template_expr(ev, expr_str, fmt_spec);
    let text = if math_wrap {
        next += 1; // consume trailing `$`
        format!("${value}$")
    } else {
        value
    };
    Some((text, next))
}

/// Find the matching `}` for a `${` opener, scanning from `start`. Tracks
/// nested `{` so embedded blocks (e.g. function bodies) don't close early.
fn find_close_brace(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    for (offset, &c) in chars[start..].iter().enumerate() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split `expr:format` into expression and optional format spec.
/// A colon inside parentheses is not treated as a separator.
fn split_expr_format(s: &str) -> (&str, Option<&str>) {
    let mut depth = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ':' if depth == 0 => {
                return (&s[..i], Some(&s[i + 1..]));
            }
            _ => {}
        }
    }
    (s, None)
}

/// Evaluate a template expression and format the result.
fn eval_template_expr(ev: &mut Evaluator, expr: &str, fmt: Option<&str>) -> String {
    let expr = expr.trim();
    if expr.is_empty() {
        return "<ERROR: empty expression>".to_string();
    }

    // Wrap as assignment: __nb_interp__ = (expr);
    let code = format!("__nb_interp__ = ({expr});");
    if let Some(err_msg) = run_code_block(ev, &code) {
        return format!("<ERROR: {err_msg}>");
    }

    let value = match ev.get("__nb_interp__") {
        Some(v) => v.clone(),
        None => return "<ERROR: expression produced no value>".to_string(),
    };
    ev.remove("__nb_interp__");

    match fmt {
        None => format!("{value}"),
        Some(spec) => {
            // Use sprintf via the evaluator for format specs
            let fmt_code = format!("__nb_interp__ = sprintf(\"{spec}\", __nb_fmt_val__);");
            // Temporarily insert the value
            ev.set("__nb_fmt_val__", value.clone());
            if let Some(err_msg) = run_code_block(ev, &fmt_code) {
                ev.remove("__nb_fmt_val__");
                ev.remove("__nb_interp__");
                return format!("<ERROR: format: {err_msg}>");
            }
            let result = match ev.get("__nb_interp__") {
                Some(rustlab_script::Value::Str(s)) => s.clone(),
                Some(v) => format!("{v}"),
                None => "<ERROR: format produced no value>".to_string(),
            };
            ev.remove("__nb_interp__");
            ev.remove("__nb_fmt_val__");
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustlab_script::Evaluator;

    fn make_ev(code: &str) -> Evaluator {
        let mut ev = Evaluator::new();
        if !code.is_empty() {
            let tokens = rustlab_script::lexer::tokenize(code).unwrap();
            let stmts = rustlab_script::parser::parse(tokens).unwrap();
            for stmt in &stmts {
                ev.exec_stmt(stmt).unwrap();
            }
        }
        ev
    }

    #[test]
    fn interp_basic_var() {
        let mut ev = make_ev("x = 42;");
        let result = interpolate_markdown("The answer is ${x}.", &mut ev);
        assert_eq!(result, "The answer is 42.");
    }

    #[test]
    fn interp_expression() {
        let mut ev = make_ev("");
        let result = interpolate_markdown("Sum: ${1 + 2}.", &mut ev);
        assert_eq!(result, "Sum: 3.");
    }

    #[test]
    fn interp_format_spec() {
        let mut ev = make_ev("total = 1234567.89;");
        let result = interpolate_markdown("Total: ${total:%,.2f}", &mut ev);
        assert_eq!(result, "Total: 1,234,567.89");
    }

    #[test]
    fn interp_escape() {
        let mut ev = make_ev("");
        let result = interpolate_markdown("Price: \\${100}", &mut ev);
        assert_eq!(result, "Price: ${100}");
    }

    #[test]
    fn interp_undefined_var() {
        let mut ev = make_ev("");
        let result = interpolate_markdown("Value: ${undefined_var}", &mut ev);
        assert!(result.contains("<ERROR:"));
    }

    #[test]
    fn interp_multiple() {
        let mut ev = make_ev("a = 1; b = 2;");
        let result = interpolate_markdown("${a} and ${b}", &mut ev);
        assert_eq!(result, "1 and 2");
    }

    #[test]
    fn interp_no_templates() {
        let mut ev = make_ev("");
        let input = "Just plain markdown with no templates.";
        let result = interpolate_markdown(input, &mut ev);
        assert_eq!(result, input);
    }

    #[test]
    fn interp_string_value() {
        let mut ev = make_ev("name = 'world';");
        let result = interpolate_markdown("Hello ${name}!", &mut ev);
        assert_eq!(result, "Hello world!");
    }

    #[test]
    fn interp_empty_expr() {
        let mut ev = make_ev("");
        let result = interpolate_markdown("Bad: ${}", &mut ev);
        assert!(result.contains("<ERROR:"));
    }

    #[test]
    fn interp_math_wrap_consumes_trailing_dollar() {
        let mut ev = make_ev("v = 0.839;");
        let result = interpolate_markdown("gets ${v:%.3f}$ of the mass", &mut ev);
        assert_eq!(result, "gets $0.839$ of the mass");
    }

    #[test]
    fn interp_math_wrap_skipped_for_double_dollar() {
        let mut ev = make_ev("x = 1;");
        // `${x}$$` — display-math `$$` follows; do not steal the first `$`.
        let result = interpolate_markdown("${x}$$y$$", &mut ev);
        assert_eq!(result, "1$$y$$");
    }

    #[test]
    fn interp_math_wrap_skipped_for_adjacent_interpolation() {
        let mut ev = make_ev("a = 1; b = 2;");
        // `${a}${b}$` — adjacent interpolation; only the second one math-wraps.
        let result = interpolate_markdown("${a}${b}$", &mut ev);
        assert_eq!(result, "1$2$");
    }

    #[test]
    fn interp_inside_open_math_does_not_double_wrap() {
        // `$X = ${v}$` — the value is inside an already-open math span;
        // emit bare and let the trailing `$` close the span: `$X = 0.500$`.
        let mut ev = make_ev("v = 0.5;");
        let result = interpolate_markdown("$X = ${v:%.3f}$ next", &mut ev);
        assert_eq!(result, "$X = 0.500$ next");
    }

    #[test]
    fn interp_after_closed_math_wraps() {
        // After `$X$` math closes, a later `${v}$` is in plain text and wraps.
        let mut ev = make_ev("v = 7;");
        let result = interpolate_markdown("$X$ then ${v}$ done", &mut ev);
        assert_eq!(result, "$X$ then $7$ done");
    }

    // Regression cases lifted from rustlab_llm/notebooks/. Each previously
    // produced a broken `$<value>$` or stray `$` somewhere in the output;
    // they pin the interpolator's behavior so the bugs can't quietly return.

    #[test]
    fn interp_regression_geq_value_dash() {
        // `is now $\geq ${v}$ — no more` (lesson 05). The `\geq` requires the
        // value to stay inside the math span; trailing `$` closes it.
        let mut ev = make_ev("v = 0.333;");
        let result = interpolate_markdown(
            r"is now $\geq ${v:%.3f}$ — no more",
            &mut ev,
        );
        assert_eq!(result, r"is now $\geq 0.333$ — no more");
    }

    #[test]
    fn interp_regression_alternating_math_spans() {
        // `$H(a) = ${a}$ bits, $H(b) = ${b}$ bits` (lesson 05). Each
        // `$...${expr}$` is its own balanced math span.
        let mut ev = make_ev("a = 0.0; b = 1.0;");
        let result = interpolate_markdown(
            "$H(a) = ${a:%.3f}$ bits, $H(b) = ${b:%.3f}$ bits",
            &mut ev,
        );
        assert_eq!(result, "$H(a) = 0.000$ bits, $H(b) = 1.000$ bits");
    }

    #[test]
    fn interp_regression_multiple_values_one_math_span() {
        // `$3 \cdot ${a} \cdot ${b} = ${c}$` (lesson 08, post-fix). All
        // substitutions stay inside one math span — no extra `$` per value.
        let mut ev = make_ev("a = 6; b = 4; c = 72;");
        let result = interpolate_markdown(
            r"$3 \cdot ${a} \cdot ${b} = ${c}$",
            &mut ev,
        );
        assert_eq!(result, r"$3 \cdot 6 \cdot 4 = 72$");
    }

    #[test]
    fn interp_regression_two_wraps_in_one_sentence() {
        // `gets ${a}$ ... only ${b}$ —` (lesson 02). Two independent
        // plain-text math-wraps in one sentence, both must produce `$v$`.
        let mut ev = make_ev("a = 0.839; b = 0.423;");
        let result = interpolate_markdown(
            "gets ${a:%.3f}$ of the mass; only ${b:%.3f}$ — flat",
            &mut ev,
        );
        assert_eq!(result, "gets $0.839$ of the mass; only $0.423$ — flat");
    }

    // ─── Currency / `\$` escape ─────────────────────────────────────────

    #[test]
    fn interp_escaped_dollar_passthrough() {
        // `\$5` is the markdown-standard escape for a literal `$`. It must
        // pass through verbatim so the downstream renderer (and GitHub /
        // Obsidian) see the escape and emit a literal `$`.
        let mut ev = make_ev("");
        let result = interpolate_markdown(r"costs \$5 today", &mut ev);
        assert_eq!(result, r"costs \$5 today");
    }

    #[test]
    fn interp_escaped_dollar_does_not_toggle_math() {
        // `\$5 plus ${tax}$ tip` — the `\$5` must NOT flip `in_math`,
        // otherwise the later `${tax}$` would think it's inside an open
        // math span and skip the wrap.
        let mut ev = make_ev("tax = 1;");
        let result = interpolate_markdown(r"costs \$5 plus ${tax}$ tip", &mut ev);
        assert_eq!(result, r"costs \$5 plus $1$ tip");
    }

    #[test]
    fn interp_currency_without_escape_passthrough() {
        // Bare `$5` (no escape) in plain text — the fast path returns
        // unchanged when there's no `${`. We rely on GitHub / Obsidian's
        // own currency heuristics to render it as literal.
        let mut ev = make_ev("");
        let result = interpolate_markdown("paid $5 yesterday", &mut ev);
        assert_eq!(result, "paid $5 yesterday");
    }

    #[test]
    fn interp_escaped_dollar_brace_still_escapes_interpolation() {
        // The pre-existing escape `\${...}` → literal `${...}` keeps working.
        let mut ev = make_ev("x = 99;");
        let result = interpolate_markdown(r"price tag \${100} not ${x}", &mut ev);
        assert_eq!(result, "price tag ${100} not 99");
    }

    #[test]
    fn interp_balanced_dollars_invariant() {
        // For every regression pattern, the count of `$` in the output must
        // be even — i.e. all math spans paired. This catches the whole class
        // of "stray `$`" bugs even if a new bad pattern appears.
        let mut ev = make_ev("a = 1; b = 2; c = 3; v = 0.5;");
        let inputs = [
            "gets ${v:%.3f}$ of mass",
            r"$\geq ${v:%.3f}$ — text",
            "$H(a) = ${a}$ bits, $H(b) = ${b}$ bits",
            r"$3 \cdot ${a} \cdot ${b} = ${c}$",
            "${a}, ${b}, ${c}.",        // plain-text list, no `$`
            "no templates here",
            r"$X = ${v}$. Next $Y = ${b}$.",
            "${a}$ and ${b}$",
            // Currency cases — `\$` escapes for literal `$`, mixed with interp.
            r"costs \$5 plus ${a}$ tax",
            r"\$${a} bill",                   // \$ then interp at start
            r"prices range \$10–\$20 always", // pure currency, no interp
        ];
        for input in inputs {
            let out = interpolate_markdown(input, &mut ev);
            // Count only un-escaped `$` — those are math delimiters. `\$`
            // is a literal currency symbol and shouldn't affect balance.
            let dollars = count_unescaped_dollars(&out);
            assert!(
                dollars % 2 == 0,
                "unbalanced `$` in output of {input:?}: {out:?} ({dollars} dollars)"
            );
        }
    }

    fn count_unescaped_dollars(s: &str) -> usize {
        let bytes = s.as_bytes();
        let mut n = 0;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'$' {
                i += 2;
            } else {
                if bytes[i] == b'$' {
                    n += 1;
                }
                i += 1;
            }
        }
        n
    }

    #[test]
    fn interp_regression_value_inside_math_followed_by_period() {
        // `: $\max| ... | = ${v:%.2e}$. Next sentence.` — value lives inside
        // an open math span and the trailing `$` closes it before a period.
        let mut ev = make_ev("v = 0.01;");
        let result = interpolate_markdown(
            r"diff: $\max|x| = ${v:%.2e}$. Next.",
            &mut ev,
        );
        assert_eq!(result, r"diff: $\max|x| = 1.00e-02$. Next.");
    }

    // ─── Notebook figure capture ──────────────────────────────────────────

    fn tmp_path(tag: &str) -> String {
        let mut p = std::env::temp_dir();
        p.push(format!("nb_figs_{}_{}.svg", std::process::id(), tag));
        p.to_str().unwrap().to_string()
    }

    /// Multiple `savefig()` calls in a single block produce separate snapshots.
    #[test]
    fn notebook_captures_every_savefig_in_block() {
        let a = tmp_path("a");
        let b = tmp_path("b");
        let src =
            format!("x = 0:10; plot(x, sin(x)); savefig('{a}'); plot(x, cos(x)); savefig('{b}');");
        let blocks = vec![Block::Code {
            source: src,
            directives: crate::parse::CodeDirectives::default(),
        }];
        let rendered = execute_notebook(&blocks);
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
        match &rendered[0] {
            Rendered::Code { figures, error, .. } => {
                assert!(error.is_none(), "unexpected error: {error:?}");
                assert_eq!(
                    figures.len(),
                    2,
                    "expected two snapshots, got {}",
                    figures.len()
                );
            }
            _ => panic!("expected Code block"),
        }
    }

    /// A block that plots but never calls savefig still yields exactly one
    /// figure (the final state) — the pre-fix behavior for unsaved plots.
    #[test]
    fn notebook_captures_final_figure_without_savefig() {
        let src = "x = 0:5; plot(x, x);".to_string();
        let blocks = vec![Block::Code {
            source: src,
            directives: crate::parse::CodeDirectives::default(),
        }];
        let rendered = execute_notebook(&blocks);
        match &rendered[0] {
            Rendered::Code { figures, error, .. } => {
                assert!(error.is_none(), "unexpected error: {error:?}");
                assert_eq!(figures.len(), 1);
            }
            _ => panic!("expected Code block"),
        }
    }

    /// Notebook mode suppresses assignment echo; only `print()` and bare
    /// expressions contribute to text output.
    #[test]
    fn notebook_suppresses_assignment_echo() {
        let blocks = vec![Block::Code {
            source: "x = 42\ny = [1, 2, 3]\nprint('hello')\n".to_string(),
            directives: crate::parse::CodeDirectives::default(),
        }];
        let rendered = execute_notebook(&blocks);
        match &rendered[0] {
            Rendered::Code {
                text_output, error, ..
            } => {
                assert!(error.is_none(), "unexpected error: {error:?}");
                assert!(
                    !text_output.contains("x ="),
                    "assignment echo leaked: {text_output:?}"
                );
                assert!(
                    !text_output.contains("y ="),
                    "assignment echo leaked: {text_output:?}"
                );
                assert!(
                    text_output.contains("hello"),
                    "print output missing: {text_output:?}"
                );
            }
            _ => panic!("expected Code block"),
        }
    }

    /// A bare expression (no `=`) still produces visible output in notebook mode.
    #[test]
    fn notebook_shows_bare_expression_output() {
        let blocks = vec![Block::Code {
            source: "1 + 2\n".to_string(),
            directives: crate::parse::CodeDirectives::default(),
        }];
        let rendered = execute_notebook(&blocks);
        match &rendered[0] {
            Rendered::Code { text_output, .. } => {
                assert!(
                    text_output.contains('3'),
                    "bare expression not shown: {text_output:?}"
                );
            }
            _ => panic!("expected Code block"),
        }
    }

    /// A block with no plotting and no savefig produces zero figures.
    #[test]
    fn notebook_no_plot_yields_no_figures() {
        let blocks = vec![Block::Code {
            source: "x = 42;".to_string(),
            directives: crate::parse::CodeDirectives::default(),
        }];
        let rendered = execute_notebook(&blocks);
        match &rendered[0] {
            Rendered::Code { figures, .. } => assert!(figures.is_empty()),
            _ => panic!("expected Code block"),
        }
    }

    /// Regression: `figure()` with no args used to call `default_new_output()`,
    /// which returned `Terminal` and overwrote the notebook's `Html`
    /// suppression — every subsequent plot then entered the ratatui alt
    /// screen and blocked on a keypress. `PlotContext::Notebook` makes the
    /// suppression sticky so `figure()` cannot break it.
    #[test]
    fn notebook_figure_call_does_not_override_suppression() {
        let blocks = vec![Block::Code {
            source: "x = 0:10; figure(); plot(x, sin(x));".to_string(),
            directives: crate::parse::CodeDirectives::default(),
        }];
        let rendered = execute_notebook(&blocks);
        match &rendered[0] {
            Rendered::Code { figures, error, .. } => {
                assert!(error.is_none(), "unexpected error: {error:?}");
                assert_eq!(figures.len(), 1);
            }
            _ => panic!("expected Code block"),
        }
        assert_eq!(
            rustlab_plot::plot_context(),
            PlotContext::Notebook,
            "PlotContext must remain Notebook after figure()"
        );
        assert!(
            matches!(
                rustlab_plot::current_figure_output(),
                rustlab_plot::FigureOutput::Html(_)
            ),
            "current figure output must be Html(_), got {:?}",
            rustlab_plot::current_figure_output()
        );
    }

    /// Regression: `imagesc()` in a notebook block must not enter the
    /// ratatui alternate screen or block on keypress. If the early-return
    /// guard in `render_heatmap_tui` regresses, this test hangs the suite.
    #[test]
    fn notebook_imagesc_does_not_block() {
        let blocks = vec![Block::Code {
            source: "A = [1 2 3; 4 5 6; 7 8 9]; imagesc(A);".to_string(),
            directives: crate::parse::CodeDirectives::default(),
        }];
        let rendered = execute_notebook(&blocks);
        match &rendered[0] {
            Rendered::Code { figures, error, .. } => {
                assert!(error.is_none(), "unexpected error: {error:?}");
                assert_eq!(figures.len(), 1, "expected one heatmap snapshot");
            }
            _ => panic!("expected Code block"),
        }
    }

    /// Mermaid blocks pass through to `Rendered::Mermaid` with no
    /// evaluator interaction.
    #[test]
    fn notebook_mermaid_passthrough() {
        use crate::parse::MermaidDirectives;
        let blocks = vec![Block::Mermaid {
            source: "flowchart LR\nA-->B".to_string(),
            directives: MermaidDirectives {
                hidden: false,
                details: Some("Arch".into()),
                caption: Some("Overview".into()),
            },
        }];
        let rendered = execute_notebook(&blocks);
        assert_eq!(rendered.len(), 1);
        match &rendered[0] {
            Rendered::Mermaid {
                source,
                hidden,
                details,
                caption,
            } => {
                assert_eq!(source, "flowchart LR\nA-->B");
                assert!(!hidden);
                assert_eq!(details.as_deref(), Some("Arch"));
                assert_eq!(caption.as_deref(), Some("Overview"));
            }
            _ => panic!("expected Mermaid"),
        }
    }

    /// Regression: multiple `figure()` calls in one block must not break
    /// suppression. Without explicit `savefig()` between them only the
    /// final figure is captured (existing contract), but the block must
    /// complete cleanly with no terminal leak — if suppression broke,
    /// the second `plot()` would block on a keypress.
    #[test]
    fn notebook_multiple_figures_in_block() {
        let a = tmp_path("multi_a");
        let b = tmp_path("multi_b");
        let src = format!(
            "figure(); plot(1:5); savefig('{a}'); figure(); plot(1:5, (1:5).^2); savefig('{b}');"
        );
        let blocks = vec![Block::Code {
            source: src,
            directives: crate::parse::CodeDirectives::default(),
        }];
        let rendered = execute_notebook(&blocks);
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
        match &rendered[0] {
            Rendered::Code {
                figures,
                text_output,
                error,
                ..
            } => {
                assert!(error.is_none(), "unexpected error: {error:?}");
                assert_eq!(
                    figures.len(),
                    2,
                    "expected two figure snapshots, got {}",
                    figures.len()
                );
                assert!(
                    text_output.trim().is_empty(),
                    "no terminal output should leak: {text_output:?}"
                );
            }
            _ => panic!("expected Code block"),
        }
    }
}
