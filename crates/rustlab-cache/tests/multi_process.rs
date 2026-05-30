//! Multi-process stress test. Spawns four child processes that hammer a
//! shared cache file with a slow "compute" (5 ms sleep) keyed on one of
//! ten random inputs. The plan calls this out as the mandatory Phase 1
//! gate for WAL configuration: it catches missing `INSERT OR IGNORE`,
//! lock-timeout regressions, and corrupted WAL handling in one cheap
//! test.
//!
//! The parent test invokes `Command::new(current_exe())` with a filter
//! that runs only `worker_entry` inside each child, plus an env var the
//! worker reads to discover the shared DB path. Without the env var,
//! `worker_entry` is a no-op so a plain `cargo test` doesn't loop.

use rustlab_cache::Store;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ENV_KEY: &str = "RUSTLAB_CACHE_STRESS";
const DISTINCT_KEYS: u8 = 10;
const WORKER_BUDGET: Duration = Duration::from_secs(2);
const NUM_WORKERS: usize = 4;

#[test]
fn worker_entry() {
    let Ok(arg) = std::env::var(ENV_KEY) else {
        // Parent run — nothing to do; we only execute as a child.
        return;
    };
    let (db_path, worker_id) = arg.split_once(':').expect("malformed worker arg");
    let worker_id: u32 = worker_id.parse().expect("worker id");
    let store = Store::open(db_path).expect("worker: open shared store");

    let deadline = Instant::now() + WORKER_BUDGET;
    // Simple LCG so we don't pull in rand; deterministic per worker.
    let mut state = worker_id as u64 ^ 0x9E37_79B9_7F4A_7C15;
    let mut puts = 0usize;
    let mut hits = 0usize;
    let mut misses = 0usize;

    while Instant::now() < deadline {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let idx = (state % DISTINCT_KEYS as u64) as u8;
        let key = [idx; 32];
        match store.get(&key, &key).expect("worker: get") {
            Some(_) => hits += 1,
            None => {
                misses += 1;
                // Pretend to compute. The 5 ms sleep gives concurrent
                // workers a fighting chance to collide on the same key.
                std::thread::sleep(Duration::from_millis(5));
                store.put(&key, &key, b"value").expect("worker: put");
                puts += 1;
            }
        }
    }

    eprintln!("worker {worker_id}: hits={hits} misses={misses} puts={puts}");
}

#[test]
fn multi_process_stress() {
    // Skip recursively — when we spawn children of this same test binary
    // they'll re-enter this exact test if we don't gate on the env var.
    if std::env::var(ENV_KEY).is_ok() {
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("shared.db");

    // Initialise the schema once from the parent so children don't all
    // race on the first-open. (Not strictly required — WAL handles it —
    // but it isolates this test from open-time races.)
    drop(Store::open(&db).expect("parent: prime db"));

    let mut children = Vec::new();
    for i in 0..NUM_WORKERS {
        let child = Command::new(std::env::current_exe().expect("current_exe"))
            .args(["--exact", "worker_entry", "--nocapture", "--test-threads=1"])
            .env(ENV_KEY, format!("{}:{i}", db.display()))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn worker");
        children.push(child);
    }

    let mut total_put_count = 0usize;
    for (i, child) in children.into_iter().enumerate() {
        let output = child.wait_with_output().expect("wait worker");
        assert!(
            output.status.success(),
            "worker {i} exited {status:?}\nstdout:\n{out}\nstderr:\n{err}",
            status = output.status,
            out = String::from_utf8_lossy(&output.stdout),
            err = String::from_utf8_lossy(&output.stderr),
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        for line in stderr.lines() {
            if let Some(rest) = line.strip_prefix(&format!("worker {i}: ")) {
                if let Some(puts) = rest
                    .split_whitespace()
                    .find_map(|tok| tok.strip_prefix("puts="))
                {
                    total_put_count += puts.parse::<usize>().unwrap_or(0);
                }
            }
        }
    }

    let store = Store::open(&db).expect("parent: reopen");
    let entries = store.entry_count().expect("entry count");
    assert!(
        entries <= DISTINCT_KEYS as u64,
        "table should contain at most {} rows (got {})",
        DISTINCT_KEYS,
        entries,
    );
    // Sanity check: we expect more total puts than rows because
    // simultaneous cold-misses both compute. Without contention this
    // assertion would still hold against the 10-key cap.
    eprintln!("stress: {NUM_WORKERS} workers, {total_put_count} total puts, {entries} final rows");
}
