//! Line input for scripts: `input`, `fgetl`, `keyboard`.
//!
//! `input` and `fgetl` read with `std::io` (or a test queue). They do not
//! use a line-editor crate. `keyboard` either calls a host handler (the
//! REPL installs one that shares its editor) or reads lines itself when
//! stdin is a terminal.
//!
//! Evaluate-mode `input` accepts one pure expression. String mode and
//! `fgetl` return the characters and never parse them.

use super::output;
use super::parmap;
use super::Evaluator;
use super::Value;
use crate::ast::{BinOp, Expr, StmtKind};
use crate::error::ScriptError;
use crate::lexer::{Spanned, Token};
use crate::purity;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io::{BufRead, Write};

/// How a `keyboard` session ended. `dbquit` is an error, not a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardStatus {
    /// `return` or `dbcont`.
    Resume,
}

/// Classification of one keyboard line, before it is parsed.
///
/// `return` here is the resume word. It is not [`StmtKind::Return`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyboardLine {
    Resume,
    Quit,
    RejectExit,
    /// Any other line. The text itself is not stored: the session
    /// appends the original line, and this variant only says "not a
    /// debugger word".
    Code,
}

type Handler = Box<dyn FnMut(&mut Evaluator) -> Result<KeyboardStatus, ScriptError>>;
type LineReader = Box<dyn FnMut(&str) -> Result<Option<String>, ScriptError>>;

enum LineSource {
    Stdin,
    Queue(VecDeque<Option<String>>),
    Panic,
}

thread_local! {
    static LINE_SOURCE: RefCell<LineSource> = RefCell::new(LineSource::Stdin);
    static FORCE_TTY: Cell<Option<bool>> = const { Cell::new(None) };
    static KEYBOARD_DEPTH: Cell<u32> = const { Cell::new(0) };
    static HANDLER: RefCell<Option<Handler>> = RefCell::new(None);
    static LINE_READER: RefCell<Option<LineReader>> = RefCell::new(None);
}

/// Test-only count of lines pulled since the current [`IoGuard`].
/// Production builds do not keep the counter.
#[cfg(test)]
thread_local! {
    static LINE_READS: Cell<u32> = const { Cell::new(0) };
}

/// Restores the line source, TTY override, keyboard handler, and depth.
pub struct IoGuard;

impl IoGuard {
    fn arm() -> Self {
        reset_line_io();
        IoGuard
    }

    /// Later reads return these lines. `None` is end of input.
    /// An exhausted queue is also end of input.
    pub fn queue(lines: Vec<Option<String>>) -> Self {
        let g = Self::arm();
        LINE_SOURCE.with(|s| *s.borrow_mut() = LineSource::Queue(lines.into()));
        g
    }

    /// Any read panics. Used to prove a context never touches stdin.
    pub fn panic_reader() -> Self {
        let g = Self::arm();
        LINE_SOURCE.with(|s| *s.borrow_mut() = LineSource::Panic);
        g
    }

    pub fn tty(self, yes: bool) -> Self {
        FORCE_TTY.with(|c| c.set(Some(yes)));
        self
    }
}

impl Drop for IoGuard {
    fn drop(&mut self) {
        reset_line_io();
    }
}

fn reset_line_io() {
    LINE_SOURCE.with(|s| *s.borrow_mut() = LineSource::Stdin);
    #[cfg(test)]
    LINE_READS.with(|c| c.set(0));
    FORCE_TTY.with(|c| c.set(None));
    KEYBOARD_DEPTH.with(|c| c.set(0));
    HANDLER.with(|h| *h.borrow_mut() = None);
    LINE_READER.with(|r| *r.borrow_mut() = None);
}

/// How many lines have been pulled since the current [`IoGuard`].
#[cfg(test)]
fn line_read_count() -> u32 {
    LINE_READS.with(|c| c.get())
}

/// Install the REPL's keyboard session. The handler owns the loop.
/// Nested `keyboard` calls (depth > 1) use [`Evaluator::keyboard_session`]
/// and the optional line reader, so they do not re-enter this closure.
pub fn set_keyboard_handler<F>(f: F)
where
    F: FnMut(&mut Evaluator) -> Result<KeyboardStatus, ScriptError> + 'static,
{
    HANDLER.with(|h| *h.borrow_mut() = Some(Box::new(f)));
}

pub fn clear_keyboard_handler() {
    HANDLER.with(|h| *h.borrow_mut() = None);
}

pub fn clear_keyboard_line_reader() {
    LINE_READER.with(|r| *r.borrow_mut() = None);
}

/// Line source for [`Evaluator::keyboard_session`]. The prompt is the
/// argument. `Ok(None)` is end of input. The REPL passes rustyline here
/// so `K>> ` has editing and history. `input` / `fgetl` do not use it.
pub fn set_keyboard_line_reader<F>(f: F)
where
    F: FnMut(&str) -> Result<Option<String>, ScriptError> + 'static,
{
    LINE_READER.with(|r| *r.borrow_mut() = Some(Box::new(f)));
}

/// `true` when stdin is a terminal, unless a test forced the flag.
fn stdin_is_tty() -> bool {
    FORCE_TTY.with(|c| match c.get() {
        Some(v) => v,
        None => std::io::IsTerminal::is_terminal(&std::io::stdin()),
    })
}

fn keyboard_depth() -> u32 {
    KEYBOARD_DEPTH.with(|c| c.get())
}

/// Prompt for a keyboard session. Depth 1 is `K>> `; deeper is `K2>> `.
fn keyboard_prompt(depth: u32) -> String {
    if depth <= 1 {
        "K>> ".to_string()
    } else {
        format!("K{depth}>> ")
    }
}

/// Resume words, matched on the trimmed line before any parse.
pub fn classify_keyboard_line(line: &str) -> KeyboardLine {
    match line.trim() {
        "return" | "dbcont" => KeyboardLine::Resume,
        "dbquit" => KeyboardLine::Quit,
        "exit" | "quit" => KeyboardLine::RejectExit,
        _ => KeyboardLine::Code,
    }
}

/// A submitted buffer is ready to parse.
///
/// Unfinished when a block is still open, the last line ends in `...`,
/// a string is unterminated, or `()`, `[]`, or `{}` is unbalanced.
/// A lex error that is not an open string is ready, so the parser reports
/// it instead of leaving the prompt at `.. `.
pub fn statement_ready(buf: &str) -> bool {
    if ends_with_ellipsis(buf) {
        return false;
    }
    match crate::lexer::tokenize(buf) {
        Err(ScriptError::Lex { msg, .. }) if msg.contains("unterminated") => false,
        Err(_) => true,
        Ok(tokens) => delimiters_and_blocks_closed(&tokens),
    }
}

fn ends_with_ellipsis(buf: &str) -> bool {
    buf.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim_end().ends_with("..."))
        .unwrap_or(false)
}

fn delimiters_and_blocks_closed(tokens: &[Spanned]) -> bool {
    let mut depth = 0i32;
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    for t in tokens {
        match &t.token {
            Token::Function | Token::If | Token::For | Token::While | Token::Switch => depth += 1,
            Token::End => depth -= 1,
            Token::LParen => paren += 1,
            Token::RParen => paren -= 1,
            Token::LBracket => bracket += 1,
            Token::RBracket => bracket -= 1,
            Token::LBrace => brace += 1,
            Token::RBrace => brace -= 1,
            _ => {}
        }
    }
    if paren < 0 || bracket < 0 || brace < 0 {
        return true;
    }
    depth <= 0 && paren == 0 && bracket == 0 && brace == 0
}

fn strip_newline(mut s: String) -> String {
    if s.ends_with('\n') {
        s.pop();
    }
    if s.ends_with('\r') {
        s.pop();
    }
    s
}

enum Pulled {
    Panic,
    Eof,
    Line(String),
    Stdin,
}

fn read_raw_line() -> Result<Option<String>, ScriptError> {
    let pulled = LINE_SOURCE.with(|src| {
        let mut src = src.borrow_mut();
        match &mut *src {
            LineSource::Panic => Pulled::Panic,
            LineSource::Queue(q) => match q.pop_front() {
                Some(Some(s)) => Pulled::Line(s),
                _ => Pulled::Eof,
            },
            LineSource::Stdin => Pulled::Stdin,
        }
    });
    match pulled {
        Pulled::Panic => panic!("stdin line reader called"),
        Pulled::Eof => counted(None),
        Pulled::Line(s) => counted(Some(s)),
        Pulled::Stdin => {
            let mut buf = String::new();
            let n = std::io::stdin()
                .lock()
                .read_line(&mut buf)
                .map_err(|e| ScriptError::runtime(format!("stdin: {e}")))?;
            if n == 0 {
                counted(None)
            } else {
                counted(Some(buf))
            }
        }
    }
}

fn counted(line: Option<String>) -> Result<Option<String>, ScriptError> {
    #[cfg(test)]
    LINE_READS.with(|c| c.set(c.get().saturating_add(1)));
    Ok(line)
}

fn flush_stdout() {
    if !output::capturing() {
        let _ = std::io::stdout().flush();
    }
}

fn refuse_notebook(name: &str) -> Result<(), ScriptError> {
    if rustlab_plot::plot_context() == rustlab_plot::PlotContext::Notebook {
        return Err(ScriptError::runtime(format!(
            "{name}: not available while rendering a notebook"
        )));
    }
    Ok(())
}

const INPUT_REJECT: &[&str] = &[
    "eval", "fclose", "feval", "fgetl", "fopen", "fread", "fwrite", "getenv", "input", "keyboard",
    "load", "run", "save", "setenv", "source", "system", "unix",
];

fn name_rejected(name: &str) -> bool {
    INPUT_REJECT.contains(&name) || purity::is_impure_builtin(name)
}

fn is_indexable(v: &Value) -> bool {
    matches!(
        v,
        Value::Vector(_)
            | Value::Matrix(_)
            | Value::Tensor3(_)
            | Value::SparseVector(_)
            | Value::SparseMatrix(_)
            | Value::Str(_)
            | Value::StringArray(_)
            | Value::Tuple(_)
            | Value::Table(_)
            | Value::IntArray { .. }
    )
}

fn pure_expr(ev: &Evaluator, expr: &Expr) -> Result<(), String> {
    match expr {
        Expr::Number(_) | Expr::Imag(_) | Expr::IntLit(_) | Expr::Str(_) | Expr::All => Ok(()),
        Expr::Var(_) => Ok(()),
        Expr::UnaryMinus(inner) | Expr::UnaryNot(inner) => pure_expr(ev, inner),
        Expr::Transpose(inner) | Expr::NonConjTranspose(inner) => pure_expr(ev, inner),
        Expr::BinOp { op, lhs, rhs } => {
            // Every operator is arithmetic, comparison, or `&&` / `||`.
            // Naming them keeps a new operator from slipping through.
            match op {
                BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Pow
                | BinOp::ElemMul
                | BinOp::ElemDiv
                | BinOp::ElemPow
                | BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge
                | BinOp::And
                | BinOp::Or => {}
            }
            pure_expr(ev, lhs)?;
            pure_expr(ev, rhs)
        }
        Expr::Call { name, args } => {
            if name_rejected(name) {
                return Err(format!("'{name}' is not allowed"));
            }
            if ev.user_fns.contains_key(name) {
                return Err(format!("user function '{name}' is not allowed"));
            }
            if let Some(v) = ev.env.get(name) {
                if matches!(v, Value::Lambda { .. } | Value::FuncHandle(_)) {
                    return Err(format!("'{name}' is not a pure builtin"));
                }
                if is_indexable(v) {
                    for a in args {
                        pure_expr(ev, a)?;
                    }
                    return Ok(());
                }
            }
            if ev.builtins.contains(name) {
                for a in args {
                    pure_expr(ev, a)?;
                }
                return Ok(());
            }
            Err(format!("'{name}' is not a pure builtin"))
        }
        Expr::Index { expr, args } => {
            pure_expr(ev, expr)?;
            for a in args {
                pure_expr(ev, a)?;
            }
            Ok(())
        }
        Expr::Matrix(_)
        | Expr::CellArray(_)
        | Expr::Range { .. }
        | Expr::Field { .. }
        | Expr::ContentIndex { .. }
        | Expr::Lambda { .. }
        | Expr::FuncHandle(_) => Err("only a pure expression is allowed".to_string()),
    }
}

struct DepthGuard;

impl DepthGuard {
    fn enter() -> Self {
        KEYBOARD_DEPTH.with(|d| d.set(d.get().saturating_add(1)));
        DepthGuard
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        KEYBOARD_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

struct HandlerRestore(Option<Handler>);

impl HandlerRestore {
    fn take_if(yes: bool) -> Self {
        if yes {
            HandlerRestore(HANDLER.with(|h| h.borrow_mut().take()))
        } else {
            HandlerRestore(None)
        }
    }

    fn as_mut(&mut self) -> Option<&mut Handler> {
        self.0.as_mut()
    }
}

impl Drop for HandlerRestore {
    fn drop(&mut self) {
        if let Some(h) = self.0.take() {
            HANDLER.with(|slot| {
                if slot.borrow().is_none() {
                    *slot.borrow_mut() = Some(h);
                }
            });
        }
    }
}

impl Evaluator {
    pub(crate) fn dispatch_line_io(
        &mut self,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, ScriptError> {
        match name {
            "input" => self.input_call(args),
            "fgetl" => self.fgetl_call(args),
            "keyboard" => self.keyboard_call(args),
            "dbcont" | "dbquit" => self.debugger_word(name),
            _ => Err(ScriptError::runtime(format!(
                "{name}: internal line-io dispatch"
            ))),
        }
    }

    pub(crate) fn is_line_io(name: &str) -> bool {
        matches!(name, "input" | "fgetl" | "keyboard" | "dbcont" | "dbquit")
    }

    fn input_call(&mut self, args: Vec<Value>) -> Result<Value, ScriptError> {
        refuse_notebook("input")?;
        parmap::require_pure_context("input")?;
        let (prompt, string_mode) = match args.as_slice() {
            [] => (String::new(), false),
            [p] => (prompt_string(p)?, false),
            [p, mode] => {
                let flag = mode.to_str().map_err(|_| {
                    ScriptError::type_err("input: second argument must be \"s\"".to_string())
                })?;
                if flag != "s" {
                    return Err(ScriptError::type_err(
                        "input: second argument must be \"s\"".to_string(),
                    ));
                }
                (prompt_string(p)?, true)
            }
            _ => {
                return Err(ScriptError::arg_count_range(
                    "input".to_string(),
                    0,
                    2,
                    args.len(),
                ))
            }
        };
        if !prompt.is_empty() {
            output::script_print(&prompt);
            flush_stdout();
        }
        let line = match read_raw_line()? {
            None => return Err(ScriptError::InputEof),
            Some(s) => strip_newline(s),
        };
        if string_mode || line.trim().is_empty() {
            return Ok(Value::Str(line));
        }
        self.eval_pure_input(&line)
    }

    fn eval_pure_input(&mut self, line: &str) -> Result<Value, ScriptError> {
        // `input_call` has already stripped the trailing newline.
        let src = format!("{line}\n");
        let tokens = crate::lexer::tokenize(&src)?;
        let stmts = crate::parser::parse(tokens)?;
        if stmts.len() != 1 {
            return Err(ScriptError::runtime(
                "input: only a single pure expression is allowed".to_string(),
            ));
        }
        let expr = match &stmts[0].kind {
            StmtKind::Expr(expr, _) => expr.clone(),
            _ => {
                return Err(ScriptError::runtime(
                    "input: only a single pure expression is allowed".to_string(),
                ))
            }
        };
        pure_expr(self, &expr)
            .map_err(|why| ScriptError::runtime(format!("input: rejected expression ({why})")))?;
        self.eval_expr(&expr)
    }

    fn fgetl_call(&mut self, args: Vec<Value>) -> Result<Value, ScriptError> {
        refuse_notebook("fgetl")?;
        parmap::require_pure_context("fgetl")?;
        if args.len() != 1 {
            return Err(ScriptError::runtime(
                "fgetl: expected fgetl(stdin)".to_string(),
            ));
        }
        if !matches!(args[0], Value::Stdin) {
            return Err(ScriptError::type_err(
                "fgetl: file handles are not available; pass stdin".to_string(),
            ));
        }
        match read_raw_line()? {
            None => Ok(Value::Scalar(-1.0)),
            Some(s) => Ok(Value::Str(strip_newline(s))),
        }
    }

    fn keyboard_call(&mut self, args: Vec<Value>) -> Result<Value, ScriptError> {
        if !args.is_empty() {
            return Err(ScriptError::arg_count(
                "keyboard".to_string(),
                0,
                args.len(),
            ));
        }
        refuse_notebook("keyboard")?;
        parmap::require_pure_context("keyboard")?;
        if !stdin_is_tty() {
            return Err(ScriptError::runtime(
                "keyboard: no interactive terminal".to_string(),
            ));
        }
        let _depth = DepthGuard::enter();
        let depth = keyboard_depth();
        let mut restore = HandlerRestore::take_if(depth == 1);
        let result = if depth == 1 {
            if let Some(h) = restore.as_mut() {
                h(self)
            } else {
                self.keyboard_session()
            }
        } else {
            self.keyboard_session()
        };
        result.map(|_| Value::None)
    }

    /// Read and run statements until `return` / `dbcont`. Shared by the
    /// std fallback and by a host handler that wants the same rules.
    pub fn keyboard_session(&mut self) -> Result<KeyboardStatus, ScriptError> {
        let mut buf = String::new();
        loop {
            let prompt = if buf.is_empty() {
                keyboard_prompt(keyboard_depth())
            } else {
                ".. ".to_string()
            };
            let line = self.read_keyboard_line(&prompt)?;
            let Some(line) = line else {
                return Err(ScriptError::runtime("keyboard: stdin closed".to_string()));
            };
            if buf.is_empty() {
                match classify_keyboard_line(&line) {
                    KeyboardLine::Resume => return Ok(KeyboardStatus::Resume),
                    KeyboardLine::Quit => return Err(ScriptError::DbQuit),
                    KeyboardLine::RejectExit => {
                        output::script_println(
                            "keyboard: type return to resume (exit stays in this session)",
                        );
                        continue;
                    }
                    KeyboardLine::Code => {}
                }
            }
            buf.push_str(&line);
            buf.push('\n');
            if !statement_ready(&buf) {
                continue;
            }
            if buf.trim().is_empty() {
                buf.clear();
                continue;
            }
            match crate::lexer::tokenize(&buf).and_then(|t| crate::parser::parse(t)) {
                Ok(stmts) => {
                    for s in &stmts {
                        self.exec_stmt(s)?;
                    }
                }
                Err(e) => output::script_println(&format!("error: {e}")),
            }
            buf.clear();
        }
    }

    fn read_keyboard_line(&mut self, prompt: &str) -> Result<Option<String>, ScriptError> {
        // Take the reader out before calling it. A nested `keyboard`
        // must not see this closure still installed, and the slot is
        // empty only when no host reader was set.
        if let Some(mut reader) = LINE_READER.with(|r| r.borrow_mut().take()) {
            let result = reader(prompt);
            LINE_READER.with(|r| {
                if r.borrow().is_none() {
                    *r.borrow_mut() = Some(reader);
                }
            });
            return result;
        }
        output::script_print(prompt);
        flush_stdout();
        read_raw_line().map(|l| l.map(strip_newline))
    }

    pub(crate) fn debugger_word(&mut self, name: &str) -> Result<Value, ScriptError> {
        refuse_notebook(name)?;
        parmap::require_pure_context(name)?;
        if !stdin_is_tty() {
            return Err(ScriptError::runtime(
                "keyboard: no interactive terminal".to_string(),
            ));
        }
        if keyboard_depth() == 0 {
            return Err(ScriptError::runtime(format!(
                "keyboard: {name} is not inside a keyboard session"
            )));
        }
        Err(ScriptError::runtime(format!(
            "keyboard: type {name} on its own line"
        )))
    }

    /// Run a script file. When the path jail is unset, the process
    /// directory moves to the script's folder for the call and is
    /// restored afterwards, including on error.
    pub fn exec_run_path(&mut self, path: &str) -> Result<(), ScriptError> {
        let safe = crate::path_jail::check_path(path).map_err(ScriptError::runtime)?;
        // Absolute before the directory change below: a relative path read
        // after moving into its own parent would resolve twice
        // (`../../lib/x.rlab` from inside `../../lib`).
        let safe = std::path::absolute(&safe)
            .map_err(|e| ScriptError::runtime(format!("run: {path}: {e}")))?;
        let _cwd = if crate::path_jail::path_jail().is_none() {
            let prev = std::env::current_dir().ok();
            if let Some(dir) = safe.parent() {
                if !dir.as_os_str().is_empty() {
                    std::env::set_current_dir(dir).map_err(|e| {
                        ScriptError::runtime(format!("run: {}: {e}", dir.display()))
                    })?;
                }
            }
            CwdGuard(prev)
        } else {
            CwdGuard(None)
        };
        let source = std::fs::read_to_string(&safe)
            .map_err(|e| ScriptError::runtime(format!("run: {path}: {e}")))?;
        let tokens = crate::lexer::tokenize(&source)?;
        let stmts = crate::parser::parse(tokens)?;
        for s in &stmts {
            self.exec_stmt(s)?;
        }
        Ok(())
    }
}

fn prompt_string(v: &Value) -> Result<String, ScriptError> {
    v.to_str()
        .map_err(|_| ScriptError::type_err("input: prompt must be a string".to_string()))
}

struct CwdGuard(Option<std::path::PathBuf>);

impl Drop for CwdGuard {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::env::set_current_dir(p);
        }
    }
}

/// Built-in constant names, including `stdin`.
pub(crate) const BUILTIN_CONSTS: &[&str] =
    &["i", "j", "pi", "e", "Inf", "NaN", "true", "false", "stdin"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::output::{start_capture, stop_capture};
    use crate::eval::Evaluator;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::ScriptError;
    use std::sync::Mutex;

    static CWD_LOCK: Mutex<()> = Mutex::new(());
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn exec(ev: &mut Evaluator, src: &str) -> Result<(), ScriptError> {
        let stmts = parse(tokenize(src)?)?;
        ev.run(&stmts)
    }

    fn scalar(ev: &Evaluator, name: &str) -> f64 {
        match ev.get(name) {
            Some(Value::Scalar(n)) => *n,
            other => panic!("{name} = {other:?}"),
        }
    }

    fn string_val(ev: &Evaluator, name: &str) -> String {
        match ev.get(name) {
            Some(Value::Str(s)) => s.clone(),
            other => panic!("{name} = {other:?}"),
        }
    }

    struct PlotGuard(rustlab_plot::PlotContext);
    impl Drop for PlotGuard {
        fn drop(&mut self) {
            rustlab_plot::set_plot_context(self.0);
        }
    }

    struct CwdRestore(std::path::PathBuf);
    impl Drop for CwdRestore {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    #[test]
    fn continuation_tracks_blocks_brackets_and_ellipsis() {
        assert!(!statement_ready("while false\n"));
        assert!(!statement_ready("for i = 1:3\n"));
        assert!(!statement_ready("if x > 0\n"));
        assert!(!statement_ready("switch x\n"));
        assert!(!statement_ready("while false\n  for i = 1:2\n  end\n"));
        assert!(statement_ready("while false\n  for i = 1:2\n  end\nend\n"));
        assert!(statement_ready("if 1, x = 1; end\n"));
        assert!(statement_ready("for i = 1:2\n  x = i\nend\n"));
        assert!(statement_ready("switch 1\n  case 1\n    x = 1\nend\n"));
        assert!(!statement_ready("x = 1 + ...\n"));
        assert!(statement_ready("x = 1 + ...\n  2\n"));
        assert!(!statement_ready("x = [\n"));
        assert!(statement_ready("x = \"[\"\n"));
        assert!(statement_ready("x = \"a]b\"\n"));
        assert!(statement_ready("whilelist = 1\n"));
        assert!(statement_ready("x = 1\n"));
        assert!(!statement_ready("x = \"hi\n"));
        let paste = "while false\n  y = 1\nend\n";
        assert!(statement_ready(paste));
    }

    #[test]
    fn return_at_keyboard_is_not_a_return_statement() {
        assert_eq!(classify_keyboard_line("return"), KeyboardLine::Resume);
        assert_eq!(classify_keyboard_line("  dbcont "), KeyboardLine::Resume);
        assert_eq!(classify_keyboard_line("dbquit"), KeyboardLine::Quit);
        assert_eq!(classify_keyboard_line("exit"), KeyboardLine::RejectExit);
        let stmts = parse(tokenize("return\n").unwrap()).unwrap();
        assert!(matches!(stmts[0].kind, crate::ast::StmtKind::Return));
    }

    #[test]
    fn bare_expression_binds_ans_and_clear_removes_it() {
        let mut ev = Evaluator::new();
        exec(&mut ev, "1 + 2\n").unwrap();
        assert_eq!(scalar(&ev, "ans"), 3.0);
        exec(&mut ev, "x = 4\n").unwrap();
        assert_eq!(scalar(&ev, "ans"), 3.0);
        exec(&mut ev, "9;\n").unwrap();
        assert_eq!(scalar(&ev, "ans"), 3.0);
        exec(&mut ev, "5\n").unwrap();
        assert_eq!(scalar(&ev, "ans"), 5.0);
        exec(&mut ev, "function y = f()\n  7\n  y = 1\nend\ny = f();\n").unwrap();
        assert_eq!(scalar(&ev, "ans"), 5.0);
        assert_eq!(scalar(&ev, "y"), 1.0);
        exec(&mut ev, "clear\n").unwrap();
        assert!(ev.get("ans").is_none());
        assert!(matches!(ev.get("stdin"), Some(Value::Stdin)));
    }

    #[test]
    fn stdin_is_a_handle_and_clear_restores_it() {
        assert_eq!(format!("{}", Value::Stdin), "stdin");
        let mut ev = Evaluator::new();
        let err = exec(&mut ev, "stdin + 1\n").unwrap_err();
        assert!(err.to_string().contains("type error"), "{err}");
        exec(&mut ev, "stdin = 1\nclear\n").unwrap();
        assert!(matches!(ev.get("stdin"), Some(Value::Stdin)));
    }

    #[test]
    fn input_string_mode_and_expression_mode_and_fgetl() {
        let _io = IoGuard::queue(vec![
            Some("1+2".into()),
            Some("".into()),
            Some("save(\"k.npy\", x)".into()),
            Some("k+1".into()),
            Some("2*pi".into()),
            None,
            Some("hello".into()),
            None,
        ]);
        let mut ev = Evaluator::new();
        exec(&mut ev, "k = 2\nx = input(\"p\")\n").unwrap();
        assert_eq!(scalar(&ev, "x"), 3.0);
        exec(&mut ev, "blank = input(\"p\")\n").unwrap();
        assert_eq!(string_val(&ev, "blank"), "");
        exec(&mut ev, "raw = input(\"p\", \"s\")\n").unwrap();
        assert_eq!(string_val(&ev, "raw"), "save(\"k.npy\", x)");
        assert!(!std::path::Path::new("k.npy").exists());
        exec(&mut ev, "y = input(\"\")\n").unwrap();
        assert_eq!(scalar(&ev, "y"), 3.0);
        exec(&mut ev, "z = input(\"\")\n").unwrap();
        assert!((scalar(&ev, "z") - 2.0 * std::f64::consts::PI).abs() < 1e-12);
        let err = exec(&mut ev, "input(\"p\", \"s\")\n").unwrap_err();
        assert!(matches!(err, ScriptError::InputEof));
        exec(&mut ev, "line = fgetl(stdin)\n").unwrap();
        assert_eq!(string_val(&ev, "line"), "hello");
        exec(&mut ev, "eof = fgetl(stdin)\n").unwrap();
        assert_eq!(scalar(&ev, "eof"), -1.0);
        exec(&mut ev, "a = ischar(\"-1\")\nb = ischar(-1)\n").unwrap();
        assert!(matches!(ev.get("a"), Some(Value::Bool(true))));
        assert!(matches!(ev.get("b"), Some(Value::Bool(false))));
        let err = exec(&mut ev, "fgetl()\n").unwrap_err();
        assert!(err.to_string().contains("fgetl(stdin)"), "{err}");
        let err = exec(&mut ev, "fgetl(\"stdin\")\n").unwrap_err();
        assert!(err.to_string().contains("pass stdin"), "{err}");
        let err = exec(&mut ev, "input(1, 2, 3)\n").unwrap_err();
        assert!(err.to_string().contains("input"), "{err}");
    }

    #[test]
    fn expression_input_rejects_statements_and_impure_calls() {
        let dir = tempfile::tempdir().unwrap();
        let npy = dir.path().join("k.npy");
        let npy_s = npy.display().to_string();
        let lines = vec![
            Some("2*pi".into()),
            Some("sin(k)".into()),
            Some("x = 1".into()),
            Some("v(1) = 2".into()),
            Some("function y = f(x)\n  y = x\nend".into()),
            Some("run \"t.rlab\"".into()),
            Some(format!("feval(\"save\", \"{npy_s}\", x)")),
            Some("eval(\"1\")".into()),
            Some("source(\"t.rlab\")".into()),
            Some(format!("save(\"{npy_s}\", x)")),
            Some(format!("load(\"{npy_s}\")")),
            Some("fprintf(\"%s\\n\", \"no\")".into()),
            Some("system(\"true\")".into()),
            Some("add1(1)".into()),
        ];
        let _io = IoGuard::queue(lines);
        let mut ev = Evaluator::new();
        exec(
            &mut ev,
            "k = 0.0\nfunction y = add1(x)\n  y = x + 1\nend\nmarker = 1\n",
        )
        .unwrap();
        exec(&mut ev, "good = input(\"\")\n").unwrap();
        assert!((scalar(&ev, "good") - 2.0 * std::f64::consts::PI).abs() < 1e-9);
        exec(&mut ev, "s = input(\"\")\n").unwrap();
        assert!(scalar(&ev, "s").abs() < 1e-12);
        for _ in 0..12 {
            let err = exec(&mut ev, "input(\"\")\n").unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("input:"), "{msg}");
        }
        assert_eq!(scalar(&ev, "marker"), 1.0);
        assert!(ev.get("x").is_none());
        assert!(!npy.exists());
        drop(_io);
        let _io = IoGuard::queue(vec![
            Some(format!("save(\"{npy_s}\", x)")),
            Some(format!("save(\"{npy_s}\", x)")),
        ]);
        exec(&mut ev, "raw = input(\"p\", \"s\")\n").unwrap();
        assert!(string_val(&ev, "raw").contains("save"));
        exec(&mut ev, "raw2 = fgetl(stdin)\n").unwrap();
        assert!(string_val(&ev, "raw2").contains("save"));
        assert!(!npy.exists());
    }

    #[test]
    fn no_io_builtin_prints_the_environment() {
        let _lock = ENV_LOCK.lock().unwrap();
        let key = "RUSTLAB_TEST_SECRET";
        let secret = "rustlab-secret-7f3a9c-do-not-print";
        let previous = std::env::var(key).ok();
        std::env::set_var(key, secret);
        let mut ev = Evaluator::new();
        start_capture();
        exec(
            &mut ev,
            "disp(1)\nprint(2)\nfprintf(\"%d\\n\", 3)\ns = sprintf(\"%d\", 4)\n",
        )
        .unwrap();
        let out = stop_capture();
        match previous {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        assert!(!out.contains(secret), "{out}");
        assert!(!ev.builtins.contains("getenv"));
    }

    #[test]
    fn notebook_context_refuses_stdin_reads() {
        let _plot = PlotGuard(rustlab_plot::plot_context());
        rustlab_plot::set_plot_context(rustlab_plot::PlotContext::Notebook);
        let _io = IoGuard::panic_reader();
        let mut ev = Evaluator::new();
        start_capture();
        for src in ["input(\"you: \", \"s\")\n", "fgetl(stdin)\n", "keyboard\n"] {
            let err = exec(&mut ev, src).unwrap_err();
            assert!(
                err.to_string()
                    .contains("not available while rendering a notebook"),
                "{err}"
            );
        }
        let out = stop_capture();
        assert!(!out.contains("you:"), "{out}");
        assert_eq!(line_read_count(), 0);
    }

    #[test]
    fn headless_context_still_reads_lines() {
        let _plot = PlotGuard(rustlab_plot::plot_context());
        rustlab_plot::set_plot_context(rustlab_plot::PlotContext::Headless);
        let _io = IoGuard::queue(vec![Some("hi".into()), Some("there".into())]);
        let mut ev = Evaluator::new();
        exec(&mut ev, "a = input(\"p\", \"s\")\nb = fgetl(stdin)\n").unwrap();
        assert_eq!(string_val(&ev, "a"), "hi");
        assert_eq!(string_val(&ev, "b"), "there");
    }

    #[test]
    fn parmap_refuses_line_input() {
        let mut ev = Evaluator::new();
        for (name, src) in [
            ("input", "f = @(x) input(\"p\")\nparmap(f, [1])\n"),
            ("fgetl", "f = @(x) fgetl(stdin)\nparmap(f, [1])\n"),
            ("keyboard", "f = @(x) keyboard\nparmap(f, [1])\n"),
        ] {
            let err = exec(&mut ev, src).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains(&format!("parmap: cannot {name}")),
                "{name}: {msg}"
            );
        }
    }

    #[test]
    fn keyboard_session_resume_quit_and_exit() {
        let _io = IoGuard::queue(vec![]).tty(true);
        set_keyboard_handler(|ev| {
            ev.set("z", Value::Scalar(5.0));
            Ok(KeyboardStatus::Resume)
        });
        let mut ev = Evaluator::new();
        exec(
            &mut ev,
            "function y = f()\n  keyboard\n  y = z\nend\ny = f();\n",
        )
        .unwrap();
        assert_eq!(scalar(&ev, "y"), 5.0);
        drop(_io);

        let _io = IoGuard::queue(vec![Some("dbquit".into())]).tty(true);
        let err = exec(&mut ev, "keyboard\n").unwrap_err();
        assert!(matches!(err, ScriptError::DbQuit), "{err}");
        drop(_io);

        let _io = IoGuard::queue(vec![
            Some("exit".into()),
            Some("a = 1".into()),
            Some("return".into()),
        ])
        .tty(true);
        start_capture();
        exec(&mut ev, "keyboard\n").unwrap();
        let out = stop_capture();
        assert!(out.contains("return"), "{out}");
        assert_eq!(scalar(&ev, "a"), 1.0);
        drop(_io);

        let _io = IoGuard::queue(vec![
            Some("keyboard".into()),
            Some("return".into()),
            Some("return".into()),
        ])
        .tty(true);
        start_capture();
        exec(&mut ev, "keyboard\n").unwrap();
        let out = stop_capture();
        assert!(out.contains("K>> "), "{out}");
        assert!(out.contains("K2>> "), "{out}");
    }

    #[test]
    fn keyboard_without_a_tty_does_not_call_the_handler() {
        let _io = IoGuard::panic_reader().tty(false);
        set_keyboard_handler(|_| panic!("handler called"));
        let mut ev = Evaluator::new();
        for src in [
            "keyboard\n",
            "dbcont\n",
            "dbquit\n",
            "dbcont()\n",
            "dbquit()\n",
        ] {
            let err = exec(&mut ev, src).unwrap_err();
            assert!(
                err.to_string()
                    .contains("keyboard: no interactive terminal"),
                "{src}: {err}"
            );
        }
        assert_eq!(line_read_count(), 0);
    }

    #[test]
    fn debugger_words_outside_a_session_error_on_a_tty() {
        let _io = IoGuard::queue(vec![]).tty(true);
        let mut ev = Evaluator::new();
        let err = exec(&mut ev, "dbcont\n").unwrap_err();
        assert!(
            err.to_string().contains("not inside a keyboard session"),
            "{err}"
        );
    }

    #[test]
    fn run_chdir_follows_the_script_and_the_jail_does_not() {
        let _lock = CWD_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script_dir = dir.path().join("proj");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir(&script_dir).unwrap();
        std::fs::create_dir(&elsewhere).unwrap();
        std::fs::write(script_dir.join("weights.toml"), "answer = 7\n").unwrap();
        std::fs::write(
            script_dir.join("main.rlab"),
            "cfg = load(\"weights.toml\")\ngot = cfg.answer\n",
        )
        .unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(&elsewhere).unwrap();
        let _restore = CwdRestore(orig);
        let mut ev = Evaluator::new();
        ev.exec_run_path(script_dir.join("main.rlab").to_str().unwrap())
            .unwrap();
        assert_eq!(scalar(&ev, "got"), 7.0);
        assert_eq!(std::env::current_dir().unwrap(), elsewhere);

        let root = dir.path().join("jail");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret.rlab"), "leaked = 1\n").unwrap();
        std::env::set_current_dir(&root).unwrap();
        let _jail = crate::PathJailGuard::with_base(Some(root.clone()), Some(root.clone()));
        let err = ev.exec_run_path("../outside/secret.rlab").unwrap_err();
        assert!(err.to_string().contains("escapes"), "{err}");
        assert_eq!(std::env::current_dir().unwrap(), root);
        assert!(ev.get("leaked").is_none());
    }

    #[test]
    fn run_resolves_relative_paths_from_the_calling_directory() {
        let _lock = CWD_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let caller = root.join("rt/a/b");
        let lib = root.join("rt/lib");
        std::fs::create_dir_all(&caller).unwrap();
        std::fs::create_dir_all(caller.join("sub")).unwrap();
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(caller.join("y.rlab"), "got = 1\n").unwrap();
        std::fs::write(caller.join("sub/x.rlab"), "got = 2\n").unwrap();
        std::fs::write(root.join("rt/a/lib.rlab"), "got = 3\n").unwrap();
        std::fs::write(lib.join("x.rlab"), "got = 4\n").unwrap();
        // A nested run resolves against the directory of the script that
        // contains it, and loads a file next to itself.
        std::fs::write(lib.join("w.toml"), "answer = 6\n").unwrap();
        std::fs::write(lib.join("outer.rlab"), "run inner.rlab\n").unwrap();
        std::fs::write(
            lib.join("inner.rlab"),
            "cfg = load(\"w.toml\")\ngot = cfg.answer\n",
        )
        .unwrap();
        std::fs::write(root.join("abs.rlab"), "got = 5\n").unwrap();

        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(&caller).unwrap();
        let _restore = CwdRestore(orig);
        let abs = root.join("abs.rlab");
        let cases: [(&str, f64); 7] = [
            ("y.rlab", 1.0),
            ("sub/x.rlab", 2.0),
            ("../lib.rlab", 3.0),
            ("../../lib/x.rlab", 4.0),
            ("sub/../../../lib/x.rlab", 4.0),
            ("../../lib/outer.rlab", 6.0),
            (abs.to_str().unwrap(), 5.0),
        ];
        for (path, want) in cases {
            let mut ev = Evaluator::new();
            ev.exec_run_path(path)
                .unwrap_or_else(|e| panic!("run {path}: {e}"));
            assert_eq!(scalar(&ev, "got"), want, "run {path}");
            assert_eq!(std::env::current_dir().unwrap(), caller, "after {path}");
        }
    }

    #[test]
    fn chat_loop_example_is_string_mode_and_echoes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/language/chat_loop.rlab");
        let src = std::fs::read_to_string(&path).unwrap();
        assert!(!src.contains("getenv"), "{src}");
        for line in src.lines() {
            if let Some(rest) = line.split("input(").nth(1) {
                assert!(
                    rest.contains("\"s\""),
                    "input( must be the string form: {line}"
                );
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let npy = dir.path().join("k.npy");
        let payload = format!("save(\"{}\", x)", npy.display());
        let _io = IoGuard::queue(vec![
            Some("hello".into()),
            Some(payload.clone()),
            Some(String::new()),
        ]);
        let mut ev = Evaluator::new();
        start_capture();
        exec(&mut ev, &src).unwrap();
        let out = stop_capture();
        assert!(out.contains("echo: hello"), "{out}");
        assert!(out.contains(&format!("echo: {payload}")), "{out}");
        assert!(!npy.exists());
    }
}
