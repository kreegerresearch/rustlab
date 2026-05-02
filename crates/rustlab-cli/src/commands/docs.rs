//! `rustlab docs` — surface the REPL's builtin-function help from the CLI.
//!
//! The REPL has had a rich `help` / `?` system (categories, per-function
//! detail with usage examples, "did you mean") for a long time. This
//! subcommand makes the same data reachable from a shell so users don't
//! have to launch the REPL just to look up `eig` or `firpm`.
//!
//! Surface:
//! - `rustlab docs`                — list every builtin grouped by category
//! - `rustlab docs <name>`         — show the detail block for one builtin
//! - `rustlab docs <category>`     — list a single category
//! - `rustlab docs --search <q>`   — substring search over names + briefs
//! - `rustlab docs --json`         — machine-readable dump (for tooling/LLMs)
//!
//! All four flow through the same `HELP` and `CATEGORIES` data that
//! `commands/repl.rs` already owns.

use anyhow::Result;
use clap::Args;

use crate::commands::repl::{HelpEntry, CATEGORIES, HELP};

#[derive(Args)]
pub struct DocsArgs {
    /// Builtin function name (e.g. `eig`, `firpm`) or category (e.g. `Plotting`).
    /// When omitted, prints the full categorised list.
    pub topic: Option<String>,

    /// Substring search over function names and briefs. Lists every entry
    /// whose name or brief contains the query (case-insensitive).
    #[arg(long, value_name = "QUERY", conflicts_with = "topic")]
    pub search: Option<String>,

    /// Emit the full help index as JSON (one object per builtin, with
    /// `name`, `brief`, `detail`, and `category` fields). Useful for
    /// editor extensions, autocomplete plugins, and AI tooling.
    #[arg(long, conflicts_with_all = ["topic", "search"])]
    pub json: bool,
}

pub fn execute(args: DocsArgs) -> Result<()> {
    if args.json {
        return emit_json();
    }
    if let Some(query) = args.search.as_deref() {
        return search(query);
    }
    match args.topic.as_deref() {
        None => {
            crate::commands::repl::print_help_list();
            Ok(())
        }
        Some(topic) => {
            let found = crate::commands::repl::print_help_detail(topic);
            if !found {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

fn search(query: &str) -> Result<()> {
    let q = query.to_ascii_lowercase();
    let matches: Vec<&HelpEntry> = HELP
        .iter()
        .filter(|e| {
            e.name.to_ascii_lowercase().contains(&q)
                || e.brief.to_ascii_lowercase().contains(&q)
        })
        .collect();

    if matches.is_empty() {
        eprintln!("rustlab docs: no entry name or brief matches \"{}\"", query);
        std::process::exit(1);
    }

    println!();
    println!(
        "  {} matches for \"{}\":",
        crate::color::bold(&matches.len().to_string()),
        query
    );
    println!();
    for e in matches {
        println!(
            "    {:<24}  {}",
            crate::color::cyan(e.name),
            e.brief
        );
    }
    println!();
    Ok(())
}

/// Map a builtin name to its category (the first category that lists it),
/// or `"Uncategorized"` if it doesn't appear in any list. Used by the JSON
/// export so consumers can group without re-implementing the categories
/// table on their side.
fn category_of(name: &str) -> &'static str {
    for (cat, entries) in CATEGORIES {
        if entries.iter().any(|n| *n == name) {
            return cat;
        }
    }
    "Uncategorized"
}

fn emit_json() -> Result<()> {
    #[derive(serde::Serialize)]
    struct JsonEntry<'a> {
        name: &'a str,
        category: &'a str,
        brief: &'a str,
        detail: &'a str,
    }
    let entries: Vec<JsonEntry<'_>> = HELP
        .iter()
        .map(|e| JsonEntry {
            name: e.name,
            category: category_of(e.name),
            brief: e.brief,
            detail: e.detail,
        })
        .collect();
    serde_json::to_writer_pretty(std::io::stdout(), &entries)?;
    println!();
    Ok(())
}
