use crate::parse::{Block, CalloutKind};
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
    /// A callout box (note, tip, warning).
    Callout { kind: CalloutKind, content: String },
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
            Block::Callout { kind, content } => {
                let interpolated = interpolate_markdown(content, &mut ev);
                rendered.push(Rendered::Callout {
                    kind: *kind,
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
/// - `\${...}` is an escape — produces literal `${...}`
/// - If the expression errors, inserts `<ERROR: message>`
fn interpolate_markdown(md: &str, ev: &mut Evaluator) -> String {
    // Fast path: no templates
    if !md.contains("${") {
        return md.to_string();
    }

    let mut result = String::with_capacity(md.len());
    let chars: Vec<char> = md.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Check for escaped \${
        if chars[i] == '\\'
            && i + 1 < len
            && chars[i + 1] == '$'
            && i + 2 < len
            && chars[i + 2] == '{'
        {
            result.push('$');
            result.push('{');
            i += 3;
            continue;
        }

        // Check for ${
        if chars[i] == '$' && i + 1 < len && chars[i + 1] == '{' {
            i += 2; // skip ${

            // Find matching }
            let mut depth = 1;
            let start = i;
            while i < len && depth > 0 {
                match chars[i] {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                if depth > 0 {
                    i += 1;
                }
            }

            if depth != 0 {
                // Unmatched brace — pass through literally
                result.push_str("${");
                result.push_str(&chars[start..].iter().collect::<String>());
                break;
            }

            let inner: String = chars[start..i].iter().collect();
            i += 1; // skip closing }

            // Split on first ':' for format spec (but not '::' or inside parens)
            let (expr_str, fmt_spec) = split_expr_format(&inner);

            // Evaluate the expression
            let replacement = eval_template_expr(ev, expr_str, fmt_spec);
            result.push_str(&replacement);
            continue;
        }

        result.push(chars[i]);
        i += 1;
    }

    result
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
}
