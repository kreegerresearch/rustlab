use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: usize, got: usize },
    #[error("singular matrix")]
    SingularMatrix,
    #[error("invalid parameter: {0}")]
    InvalidParameter(String),
    #[error("not implemented: {0}")]
    NotImplemented(String),
    /// The element count or the byte length is not a legal variable.
    /// Callers attach the function name and the shape; see [`crate::variable_too_large`].
    #[error("requested array exceeds the maximum possible variable size")]
    VariableTooLarge,
}
