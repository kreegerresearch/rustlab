//! Integration tests for `rustlab cache ...`. Drives the binary via
//! `Command` so the full clap → execute path is exercised. The cache
//! is first populated by running a `.rlab` script that exercises a
//! function call twice (miss + hit), then we round-trip through
//! `status` / `list` / `prune` / `clear`.

use std::path::Path;
use std::process::Command;

fn rustlab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustlab"))
}

/// Populate `<dir>/store.rcache` with one cached entry by running a
/// 4-line script through `rustlab run`. Returns the resolved store
/// path for subsequent commands.
fn populate_store(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let script = dir.path().join("populate.rlab");
    let store = dir.path().join("store.rcache");
    std::fs::write(
        &script,
        format!(
            "cache enable \"{}\"\n\
             function y = id(x)\n  y = x\nend\n\
             a = id(7)\n\
             b = id(7)\n",
            store.display(),
        ),
    )
    .expect("write script");

    let out = rustlab()
        .arg("run")
        .arg(&script)
        .arg("--plot")
        .arg("none")
        .output()
        .expect("run rustlab");
    assert!(
        out.status.success(),
        "rustlab run failed: {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(store.exists(), "populate should create the store");
    store
}

fn run_cache(store: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = rustlab();
    cmd.arg("cache");
    for a in args {
        cmd.arg(a);
    }
    cmd.arg("--store").arg(store);
    cmd.output().expect("cache subcommand runs")
}

#[test]
fn status_reports_one_entry_after_populate() {
    let dir = tempfile::tempdir().unwrap();
    let store = populate_store(&dir);
    let out = run_cache(&store, &["status"]);
    assert!(out.status.success(), "status failed: {:?}", out);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("entries: 1"), "stdout was:\n{s}");
    assert!(s.contains("schema version: 1"), "{s}");
}

#[test]
fn list_shows_fn_name_and_short_hex_keys() {
    let dir = tempfile::tempdir().unwrap();
    let store = populate_store(&dir);
    let out = run_cache(&store, &["list"]);
    assert!(out.status.success(), "list failed: {:?}", out);
    let s = String::from_utf8_lossy(&out.stdout);
    // Header line names every column we expose.
    assert!(s.contains("fn name"), "fn name header missing:\n{s}");
    assert!(s.contains("entry_id"), "entry_id header missing:\n{s}");
    assert!(s.contains("input_hash"), "input_hash header missing:\n{s}");
    let data_lines: Vec<&str> = s.lines().skip(1).collect();
    assert!(!data_lines.is_empty(), "expected at least one row:\n{s}");
    let cols: Vec<&str> = data_lines[0].split_whitespace().collect();
    assert!(cols.len() >= 5, "expected ≥5 columns, got {cols:?}");
    // The populating script defines `id(x)` and calls it once — so
    // the metadata should carry the function name we used.
    assert_eq!(cols[0], "id", "fn name column: '{}'", cols[0]);
    // The next two columns are short-hex keys (8 chars each).
    assert_eq!(cols[1].len(), 8, "entry_id short-hex: '{}'", cols[1]);
    assert_eq!(cols[2].len(), 8, "input_hash short-hex: '{}'", cols[2]);
}

#[test]
fn clear_empties_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = populate_store(&dir);
    let out = run_cache(&store, &["clear"]);
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("cleared 1 entries"), "stdout was:\n{s}");

    // Subsequent status should show 0 entries.
    let out = run_cache(&store, &["status"]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("entries: 0"), "stdout was:\n{s}");
}

#[test]
fn prune_default_30d_is_a_no_op_on_fresh_entries() {
    let dir = tempfile::tempdir().unwrap();
    let store = populate_store(&dir);
    let out = run_cache(&store, &["prune"]);
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("pruned 0"), "stdout was:\n{s}");
}

#[test]
fn prune_older_than_zero_seconds_drops_everything() {
    let dir = tempfile::tempdir().unwrap();
    let store = populate_store(&dir);
    let out = run_cache(&store, &["prune", "--older-than", "0s"]);
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("pruned 1"), "stdout was:\n{s}");
    let out = run_cache(&store, &["status"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("entries: 0"));
}

#[test]
fn prune_max_size_zero_drops_everything() {
    let dir = tempfile::tempdir().unwrap();
    let store = populate_store(&dir);
    let out = run_cache(&store, &["prune", "--max-size", "0"]);
    assert!(out.status.success(), "prune failed: {:?}", out);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("pruned 1"), "stdout was:\n{s}");
}

#[test]
fn prune_unknown_unit_errors_with_clear_message() {
    let dir = tempfile::tempdir().unwrap();
    let store = populate_store(&dir);
    let out = run_cache(&store, &["prune", "--older-than", "30q"]);
    assert!(!out.status.success(), "expected failure");
    let s = String::from_utf8_lossy(&out.stderr);
    assert!(s.contains("unknown unit"), "stderr was:\n{s}");
}

#[test]
fn missing_store_is_handled_gracefully_for_status_and_errors_for_others() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("absent.rcache");

    // status: friendly "no store at <path>" with success exit.
    let out = run_cache(&missing, &["status"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("no store at"));

    // list/clear/prune all expect existing stores.
    for sub in [&["list"][..], &["clear"][..], &["prune"][..]] {
        let out = run_cache(&missing, sub);
        assert!(!out.status.success(), "{sub:?} should error on missing store");
        let s = String::from_utf8_lossy(&out.stderr);
        assert!(
            s.contains("no cache file"),
            "stderr for {sub:?}:\n{s}"
        );
    }
}
