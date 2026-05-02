//! Embedded scripting language for rustlab.
//!
//! `rustlab-script` provides a small, scientific scripting language designed for
//! interactive signal-processing work. Scripts are stored in plain text files
//! with a `.rlab` extension and executed by passing their source text to [`run`].
//!
//! # Language overview
//!
//! ## Imaginary unit
//! The constant `j` represents the imaginary unit (√−1), so complex literals
//! are written as `3 + 2j` or `1.5j`.
//!
//! ## Range operator
//! Ranges generate uniformly-spaced vectors:
//! - `1:10` — integers 1, 2, …, 10 (inclusive).
//! - `0:0.5:2` — `start:step:stop`, producing 0, 0.5, 1.0, 1.5, 2.0.
//!
//! ## Indexing (1-based)
//! Vectors and matrices use **1-based** indexing, consistent with scientific computing convention:
//! - `v(3)` — third element.
//! - `v(2:5)` — elements 2 through 5.
//! - `v(end)` — last element; `end` resolves to the length of `v`.
//!
//! ## Output suppression
//! A statement terminated with a **semicolon** (`;`) evaluates normally but
//! suppresses printing of the result, keeping the REPL output clean.
//!
//! ## Entry point
//! Use [`run`] to execute a complete script from a source string. The function
//! lexes, parses, and evaluates the script in a fresh [`Evaluator`] context.

pub mod ast;
pub mod error;
pub mod eval;
pub mod lexer;
pub mod parser;

#[cfg(test)]
mod tests;

pub use error::ScriptError;
pub use eval::output::{capturing, start_capture, stop_capture};
pub use eval::Evaluator;
pub use eval::Value;

/// Execute a `.rlab` script from source text.
///
/// This is the primary entry point for the scripting subsystem. It runs the
/// full pipeline in sequence:
///
/// 1. **Lex** — converts `source` into a flat token stream.
/// 2. **Parse** — builds an abstract syntax tree from the tokens.
/// 3. **Evaluate** — walks the AST in a fresh [`Evaluator`] context,
///    printing results for statements that are not semicolon-terminated.
///
/// # Errors
/// Returns a [`ScriptError`] if lexing, parsing, or evaluation fails.
///
/// # Example
/// ```rust,no_run
/// use rustlab_script::run;
/// run("x = 1:5; y = x * 2").unwrap();
/// ```
pub fn run(source: &str) -> Result<(), ScriptError> {
    let tokens = lexer::tokenize(source)?;
    let stmts = parser::parse(tokens)?;
    let mut ev = Evaluator::new();
    ev.run_script(&stmts)
}

/// Execute a `.rlab` script with `--profile`-style tracking of all function calls.
/// Equivalent to calling `profile()` at the top of the script.
/// The profiling report is printed to stderr at the end.
pub fn run_profiled(source: &str) -> Result<(), ScriptError> {
    let tokens = lexer::tokenize(source)?;
    let stmts = parser::parse(tokens)?;
    let mut ev = Evaluator::new();
    ev.enable_profiling(None);
    ev.run_script(&stmts)
}
