use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name    = "rustlab",
    version = env!("CARGO_PKG_VERSION"),
    about   = "Matrix algebra and DSP toolkit with a scriptable .rlab language",
    long_about = None,
    after_help = "Builtins: rustlab docs  and  rustlab docs --json\n\
                  Notebooks, themes, and rendering: rustlab-notebook --help"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the interactive REPL (default when no subcommand given)
    Repl(crate::commands::repl::ReplArgs),
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
    /// Run rustlab on a remote machine with plots rendering in the local viewer
    /// (viewer-feature builds only — `make install` produces one)
    #[cfg(feature = "viewer")]
    Remote(crate::commands::remote::RemoteArgs),
    /// Print the version and pointers to builtin and notebook docs
    Info,
    /// Inspect, prune, or clear a persistent function-result cache
    #[command(subcommand)]
    Cache(crate::commands::cache::CacheCommands),
}

impl Cli {
    pub fn execute(self) -> Result<()> {
        let settings = crate::user_config::load_and_apply()?;
        match self
            .command
            .unwrap_or_else(|| Commands::Repl(Default::default()))
        {
            Commands::Repl(args) => crate::commands::repl::execute(args, &settings),
            Commands::Run(args) => crate::commands::run::execute(args, &settings),
            Commands::Filter(cmd) => crate::commands::filter::execute(cmd),
            Commands::Convolve(args) => crate::commands::convolve::execute(args),
            Commands::Window(args) => crate::commands::window::execute(args),
            Commands::Plot(args) => crate::commands::plot::execute(args),
            Commands::Docs(args) => crate::commands::docs::execute(args),
            #[cfg(feature = "viewer")]
            Commands::Remote(args) => crate::commands::remote::execute(args),
            Commands::Info => crate::commands::info::execute(),
            Commands::Cache(cmd) => crate::commands::cache::execute(cmd),
        }
    }
}
