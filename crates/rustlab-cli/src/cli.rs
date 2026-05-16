use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name    = "rustlab",
    version = env!("CARGO_PKG_VERSION"),
    about   = "Matrix algebra and DSP toolkit with a scriptable .rlab language",
    long_about = None,
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the interactive REPL (default when no subcommand given)
    Repl,
    /// Execute a .rlab script file
    Run(crate::commands::run::RunArgs),
    /// Design and apply digital filters
    #[command(subcommand)]
    Filter(crate::commands::filter::FilterCommands),
    /// Convolve two signals
    Convolve(crate::commands::convolve::ConvolveArgs),
    /// Generate a window function
    Window(crate::commands::window::WindowArgs),
    /// Plot a signal from a CSV file (one value per line)
    Plot(crate::commands::plot::PlotArgs),
    /// Look up rustlab builtin function documentation (same data as the REPL `help` command)
    Docs(crate::commands::docs::DocsArgs),
    /// Show version and feature information
    Info,
}

impl Cli {
    pub fn execute(self) -> Result<()> {
        match self.command.unwrap_or(Commands::Repl) {
            Commands::Repl => crate::commands::repl::execute(),
            Commands::Run(args) => crate::commands::run::execute(args),
            Commands::Filter(cmd) => crate::commands::filter::execute(cmd),
            Commands::Convolve(args) => crate::commands::convolve::execute(args),
            Commands::Window(args) => crate::commands::window::execute(args),
            Commands::Plot(args) => crate::commands::plot::execute(args),
            Commands::Docs(args) => crate::commands::docs::execute(args),
            Commands::Info => crate::commands::info::execute(),
        }
    }
}
