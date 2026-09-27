mod cli;
pub(crate) mod color;
mod commands;
mod user_config;

use anyhow::Result;
use clap::Parser;
use cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    cli.execute()
}
