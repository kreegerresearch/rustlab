mod cli;
pub(crate) mod color;
mod commands;
mod user_config;

use anyhow::Result;
use clap::Parser;
use cli::Cli;

#[global_allocator]
static ALLOC: rustlab_core::worker_alloc::WorkerCacheAlloc =
    rustlab_core::worker_alloc::WorkerCacheAlloc;

fn main() -> Result<()> {
    let cli = Cli::parse();
    cli.execute()
}
