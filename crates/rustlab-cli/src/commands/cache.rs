//! `rustlab cache ...` — inspect, prune, and clear persistent function
//! caches outside of a running script. Mirrors the in-script `cache
//! status / list / clear / prune` directives but operates on any
//! `.rcache` file given via `--store`. Defaults to the per-project
//! `.rustlab/cache.db` when `--store` is omitted.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use rustlab_cache::{parse_duration_secs, Store};
use std::path::PathBuf;

/// Per-project default. Resolved against the process's current
/// working directory at command time.
const DEFAULT_STORE_PATH: &str = ".rustlab/cache.db";

#[derive(Subcommand)]
pub enum CacheCommands {
    /// Print store path, entry count, and total stored bytes
    Status(CommonArgs),
    /// List cached entries (key, size, version, timestamp) — never prints values
    List(ListArgs),
    /// Drop every cached entry; keeps the DB file
    Clear(CommonArgs),
    /// Drop entries older than a duration and/or to fit a max byte cap
    Prune(PruneArgs),
}

#[derive(Args, Clone)]
pub struct CommonArgs {
    /// Path to the `.rcache` / `.db` store to operate on
    #[arg(long, value_name = "PATH")]
    pub store: Option<PathBuf>,
}

#[derive(Args, Clone)]
pub struct ListArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// Cap the number of rows shown (newest first); default is everything
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
}

#[derive(Args, Clone)]
pub struct PruneArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// Age cutoff: drops entries older than this. Format: `30d`, `12h`,
    /// `500ms`, etc. (units: ms, s, m, h, d, w)
    #[arg(long, value_name = "DURATION")]
    pub older_than: Option<String>,
    /// Size cap in bytes: drops oldest entries until total ≤ this
    #[arg(long, value_name = "BYTES")]
    pub max_size: Option<u64>,
}

pub fn execute(cmd: CacheCommands) -> Result<()> {
    match cmd {
        CacheCommands::Status(a) => cmd_status(a),
        CacheCommands::List(a) => cmd_list(a),
        CacheCommands::Clear(a) => cmd_clear(a),
        CacheCommands::Prune(a) => cmd_prune(a),
    }
}

fn resolve_store_path(common: &CommonArgs) -> PathBuf {
    common
        .store
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_STORE_PATH))
}

/// Open the requested store. Surfaces a friendly "no cache at <path>"
/// message when the file doesn't exist rather than auto-creating one —
/// the user asked to *inspect* a cache, so silently creating an empty
/// DB would mask the missing-path problem.
fn open_store(path: &std::path::Path, must_exist: bool) -> Result<Store> {
    if must_exist && !path.exists() {
        anyhow::bail!(
            "no cache file at {} (use `rustlab run` with `cache enable` first, or pass --store PATH)",
            path.display(),
        );
    }
    Store::open(path).with_context(|| format!("opening cache at {}", path.display()))
}

fn cmd_status(args: CommonArgs) -> Result<()> {
    let path = resolve_store_path(&args);
    if !path.exists() {
        println!("cache: no store at {}", path.display());
        return Ok(());
    }
    let store = open_store(&path, true)?;
    let count = store.entry_count()?;
    let bytes = store.total_bytes()?;
    let schema = store
        .schema_meta("version")?
        .unwrap_or_else(|| "<absent>".to_string());
    let rl_version = store
        .schema_meta("rustlab_version")?
        .unwrap_or_else(|| "<absent>".to_string());
    println!("cache: {}", path.display());
    println!("  schema version: {schema}");
    println!("  rustlab version: {rl_version}");
    println!("  entries: {count}");
    println!("  stored bytes: {bytes}");
    if store.is_disabled() {
        println!("  status: DISABLED (schema is newer than this binary supports)");
    }
    Ok(())
}

fn cmd_list(args: ListArgs) -> Result<()> {
    let path = resolve_store_path(&args.common);
    let store = open_store(&path, true)?;
    let rows = store.list_entries(args.limit)?;
    if rows.is_empty() {
        println!("cache: no entries in {}", path.display());
        return Ok(());
    }
    // Header tuned for an 80-column terminal: function name, short
    // hex keys, size, version, timestamp. `fn_name` is `<unknown>`
    // for rows written by an older binary that predates the
    // `fn_metadata` table. The REPL's `cache list` adds a `status`
    // column that shows whether each entry's function is currently
    // loaded — that's REPL-only because the CLI has no live
    // evaluator state to check against.
    println!(
        "{:<20}  {:>9}  {:>10}  {:>10}  {:>9}  created_at",
        "fn name", "entry_id", "input_hash", "bytes", "rl_ver"
    );
    for row in rows {
        let fn_name = row.fn_name.unwrap_or_else(|| "<unknown>".to_string());
        println!(
            "{:<20}  {:>9}  {:>10}  {:>10}  {:>9}  {}",
            fn_name,
            row.entry_id_short,
            row.input_hash_short,
            row.bytes,
            row.rustlab_version,
            row.created_at,
        );
    }
    Ok(())
}

fn cmd_clear(args: CommonArgs) -> Result<()> {
    let path = resolve_store_path(&args);
    let store = open_store(&path, true)?;
    let removed = store.clear()?;
    println!("cache: cleared {removed} entries from {}", path.display());
    Ok(())
}

fn cmd_prune(args: PruneArgs) -> Result<()> {
    let path = resolve_store_path(&args.common);
    let store = open_store(&path, true)?;

    // No kwargs → default age-based (30 days) so the bare command
    // does *something* useful rather than nothing.
    let did_specify = args.older_than.is_some() || args.max_size.is_some();
    let mut total_removed: usize = 0;
    let mut notes: Vec<String> = Vec::new();

    if let Some(s) = args.older_than.as_deref() {
        let secs = parse_duration_secs(s)
            .with_context(|| format!("--older-than {s}"))?;
        let n = store.prune_older_than(secs)?;
        total_removed += n;
        notes.push(format!("{n} older than {secs}s"));
    }
    if let Some(max) = args.max_size {
        let n = store.prune_to_max_size(max)?;
        total_removed += n;
        notes.push(format!("{n} to fit max_size={max}"));
    }
    if !did_specify {
        const THIRTY_DAYS: u64 = 30 * 24 * 60 * 60;
        let n = store.prune_older_than(THIRTY_DAYS)?;
        total_removed += n;
        notes.push(format!("{n} older than 30 days (default)"));
    }

    println!(
        "cache: pruned {total_removed} entries from {} ({})",
        path.display(),
        notes.join(", "),
    );
    Ok(())
}
