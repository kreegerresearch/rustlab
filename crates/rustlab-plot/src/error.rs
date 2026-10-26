use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlotError {
    #[error("terminal error: {0}")]
    Terminal(String),
    #[error("empty data")]
    EmptyData,
    #[error(transparent)]
    Core(#[from] rustlab_core::CoreError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("file output error: {0}")]
    FileOutput(String),
    #[error("figure_live requires a real terminal (stdout is not a tty)")]
    NotATty,
    #[error("figure_live is disabled under --plot none; use --plot tui or --plot viewer")]
    HeadlessDisabled,
    #[error("interrupted")]
    Interrupted,
    #[error("viewer connection error: {0}")]
    ViewerConnection(String),
    /// The plot was not sent. The socket is still open; the caller draws
    /// this figure in the terminal and leaves later plots on the viewer.
    #[error("{0}")]
    ViewerPayloadTooLarge(String),
}
