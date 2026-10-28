use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScriptError {
    #[error("lex error at line {line}: {msg}")]
    Lex { line: usize, msg: String },
    #[error("parse error at line {line}: {msg}")]
    Parse { line: usize, msg: String },
    #[error("line {line}: runtime error: {msg}")]
    Runtime { line: usize, msg: String },
    #[error("line {line}: type error: {msg}")]
    Type { line: usize, msg: String },
    #[error("line {line}: undefined variable '{name}'")]
    Undefined { line: usize, name: String },
    #[error("line {line}: undefined function '{name}'")]
    UndefinedFn { line: usize, name: String },
    #[error("line {line}: wrong number of arguments for '{name}': expected {expected}, got {got}")]
    ArgCount {
        line: usize,
        name: String,
        expected: usize,
        got: usize,
    },
    #[error(
        "line {line}: wrong number of arguments for '{name}': expected {min}..{max}, got {got}"
    )]
    ArgCountRange {
        line: usize,
        name: String,
        min: usize,
        max: usize,
        got: usize,
    },
    #[error(transparent)]
    Dsp(#[from] rustlab_dsp::error::DspError),
    #[error(transparent)]
    Core(#[from] rustlab_core::CoreError),
    #[error(transparent)]
    Plot(#[from] rustlab_plot::PlotError),
    /// Internal signal: `return` statement in a function body.  Never shown to users.
    #[error("return")]
    EarlyReturn,
    /// stdin closed while audio_read was waiting for a full frame.
    /// Treated as a clean exit by the CLI (exit code 0, no error message).
    #[error("stdin closed")]
    AudioEof,
    /// `input` reached end of stdin. The CLI treats this as a clean exit
    /// (exit code 0, no error line). `fgetl` does not use this variant —
    /// it returns the scalar `-1`.
    #[error("stdin closed")]
    InputEof,
    /// `dbquit` at a `keyboard` prompt. Ends the script. `rustlab run`
    /// exits 1. The REPL exits 1 as well.
    #[error("keyboard: dbquit")]
    DbQuit,
    /// User pressed Ctrl-C or 'q' while a live figure was active, or
    /// Ctrl-C during a long viewer plot send (or the terminal figure
    /// that is waiting for a key). The CLI exits 0 and prints no
    /// `error:` line. The viewer path prints `plot aborted` on stderr.
    #[error("interrupted")]
    Interrupted,
    /// Cooperative cancellation: an external coordinator set the
    /// evaluator's cancel flag (e.g. the notebook watch server preempting
    /// a slow render because a newer save arrived). Not a user error —
    /// the caller discards the partial result. See
    /// [`crate::Evaluator::with_cancel`]. Default-off, so the REPL and
    /// one-shot CLI never see it.
    #[error("cancelled")]
    Cancelled,
}

impl ScriptError {
    /// Create a runtime error (line 0 = unknown, filled in by evaluator).
    pub fn runtime(msg: String) -> Self {
        ScriptError::Runtime { line: 0, msg }
    }
    /// Create a type error (line 0 = unknown, filled in by evaluator).
    pub fn type_err(msg: String) -> Self {
        ScriptError::Type { line: 0, msg }
    }
    /// Create an undefined-variable error.
    pub fn undefined(name: String) -> Self {
        ScriptError::Undefined { line: 0, name }
    }
    /// Create an undefined-function error.
    pub fn undefined_fn(name: String) -> Self {
        ScriptError::UndefinedFn { line: 0, name }
    }
    /// Create an arg-count error.
    pub fn arg_count(name: String, expected: usize, got: usize) -> Self {
        ScriptError::ArgCount {
            line: 0,
            name,
            expected,
            got,
        }
    }
    /// Create an arg-count-range error.
    pub fn arg_count_range(name: String, min: usize, max: usize, got: usize) -> Self {
        ScriptError::ArgCountRange {
            line: 0,
            name,
            min,
            max,
            got,
        }
    }

    /// Attach a source line number to errors that don't already have one.
    /// Used by the evaluator to annotate runtime errors with statement location.
    pub fn with_line(self, line: usize) -> Self {
        if line == 0 {
            return self;
        }
        match self {
            ScriptError::Runtime { line: 0, msg } => ScriptError::Runtime { line, msg },
            ScriptError::Type { line: 0, msg } => ScriptError::Type { line, msg },
            ScriptError::Undefined { line: 0, name } => ScriptError::Undefined { line, name },
            ScriptError::UndefinedFn { line: 0, name } => ScriptError::UndefinedFn { line, name },
            ScriptError::ArgCount {
                line: 0,
                name,
                expected,
                got,
            } => ScriptError::ArgCount {
                line,
                name,
                expected,
                got,
            },
            ScriptError::ArgCountRange {
                line: 0,
                name,
                min,
                max,
                got,
            } => ScriptError::ArgCountRange {
                line,
                name,
                min,
                max,
                got,
            },
            other => other,
        }
    }
}
