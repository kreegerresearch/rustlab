//! Per-block notebook execution cache for `notebook watch`.
//!
//! The watcher re-runs the executor on every `.md` save, but most saves
//! change only *some* of the executable blocks — often none of them, and
//! often only the last one. Re-running every slow block from the top of
//! the notebook on each keystroke is the user-visible reason the watcher
//! feels heavy.
//!
//! This module is a **prefix-array cache**: one entry per executable
//! block (rustlab Code + Mermaid) in document order, holding the cached
//! output and a snapshot of the live state *after* that block ran. On a
//! new render:
//!
//! 1. Hash each executable block's source.
//! 2. Find `valid_k` = the longest common prefix where each cached
//!    entry's hash still matches the new source.
//! 3. Truncate cache to `valid_k`. Restore live state from the last
//!    surviving snapshot (or fresh state if `valid_k == 0`).
//! 4. Walk the new block list in document order. Blocks `0..valid_k`
//!    return their cached output (and incrementally restore state to
//!    "after this block" so subsequent markdown interpolation sees
//!    the right values). Blocks `valid_k..n` actually execute against
//!    the live state and append new cache entries.
//!
//! The result: editing block 5 of 10 → blocks 0–4 are skipped (state
//! restored from `entries[4].snapshot`), blocks 5–10 execute. Editing a
//! prose paragraph between blocks 3 and 4 → no executable block source
//! changes, full cache hit, no execution at all. Editing block 0 →
//! cache invalidates, everything runs from scratch.
//!
//! Limitations (deliberate — keep it simple):
//!
//! - In-memory, watcher-session-scoped. Restart the watcher → empty
//!   cache → first render is full.
//! - No memory cap (yet). Each entry holds a full evaluator/figure/RNG
//!   snapshot. A pathological 1 GB-tensor-per-block notebook would
//!   OOM. Future work.
//! - No opt-out directive per block. Edit the block to force re-execute.

use crate::execute::Rendered;
use rustlab_plot::PlotSnapshot;
use rustlab_script::eval::rng::RngSnapshot;
use rustlab_script::Evaluator;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// One cache slot. Indexed by document-order position among executable
/// blocks (Code + Mermaid), NOT by all-block position.
pub struct CacheEntry {
    /// Hash of just this block's source. Used to detect whether the
    /// user edited *this* block.
    pub block_hash: u64,
    /// The block's previously-rendered output. Replays verbatim on a
    /// cache hit (no execution).
    pub output: Rendered,
    /// Live state *after* this block ran the previous time. Restored
    /// when the cache hits this block, so any subsequent markdown
    /// interpolation sees the same values it did originally.
    pub snapshot: ExecState,
}

/// Bundle of every runtime state we need to roll back to a prior
/// point: the script evaluator, the plot thread-locals, and the RNG.
pub struct ExecState {
    pub evaluator: Evaluator,
    pub plot: PlotSnapshot,
    pub rng: RngSnapshot,
}

impl ExecState {
    /// Snapshot the live thread-local state. Uses `Evaluator::deep_clone`
    /// so mutable `Arc<Mutex<…>>` interiors in `Value` (e.g. `FirState`)
    /// don't alias between snapshot and live.
    pub fn capture(evaluator: &Evaluator) -> Self {
        ExecState {
            evaluator: evaluator.deep_clone(),
            plot: rustlab_plot::capture_thread_state(),
            rng: rustlab_script::eval::rng::capture(),
        }
    }

    /// Overwrite the live thread-locals from this snapshot. Returns a
    /// fresh `Evaluator` that the caller installs as its working copy.
    pub fn restore(&self) -> Evaluator {
        rustlab_plot::restore_thread_state(&self.plot);
        rustlab_script::eval::rng::restore(&self.rng);
        self.evaluator.deep_clone()
    }
}

/// Per-notebook output cache. One per source path, owned by the watcher
/// for its session.
#[derive(Default)]
pub struct NotebookCache {
    pub(crate) entries: Vec<CacheEntry>,
}

impl NotebookCache {
    /// Find the longest prefix where the cached entries' hashes still
    /// match the new source's per-block hashes. Returns the count of
    /// matching entries; callers truncate `entries` to this length
    /// before executing the divergent tail.
    pub fn valid_prefix(&self, new_block_hashes: &[u64]) -> usize {
        self.entries
            .iter()
            .zip(new_block_hashes.iter())
            .take_while(|(entry, h)| entry.block_hash == **h)
            .count()
    }

    /// Truncate the cache to keep only the first `n` entries. Used to
    /// discard everything from the first divergent block onward before
    /// re-executing.
    pub fn truncate(&mut self, n: usize) {
        self.entries.truncate(n);
    }

    /// Append a freshly-executed block's result to the cache.
    pub fn push(&mut self, entry: CacheEntry) {
        self.entries.push(entry);
    }

    /// Borrow a cached entry by its executable-block index.
    pub fn get(&self, idx: usize) -> Option<&CacheEntry> {
        self.entries.get(idx)
    }

    /// Number of cached executable blocks. After a render this should
    /// equal the new source's executable-block count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Stable per-block hash. `DefaultHasher` is not stable across Rust
/// versions, but the cache lives only for a watcher session — no
/// cross-version concern.
pub fn hash_block_source(source: &str) -> u64 {
    let mut h = DefaultHasher::new();
    source.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_changes_when_source_changes() {
        assert_ne!(hash_block_source("a = 1"), hash_block_source("a = 2"));
    }

    #[test]
    fn hash_stable_for_identical_source() {
        assert_eq!(hash_block_source("plot(1:10)"), hash_block_source("plot(1:10)"));
    }

    #[test]
    fn valid_prefix_returns_zero_for_empty_cache() {
        let cache = NotebookCache::default();
        assert_eq!(cache.valid_prefix(&[1, 2, 3]), 0);
    }

    #[test]
    fn valid_prefix_finds_longest_match() {
        let mut cache = NotebookCache::default();
        // Build a fake cache with hashes 10, 20, 30, 40.
        for h in [10, 20, 30, 40] {
            cache.push(CacheEntry {
                block_hash: h,
                output: Rendered::SolutionStart,
                snapshot: ExecState {
                    evaluator: Evaluator::new(),
                    plot: rustlab_plot::capture_thread_state(),
                    rng: rustlab_script::eval::rng::capture(),
                },
            });
        }

        // Identical hashes → full match.
        assert_eq!(cache.valid_prefix(&[10, 20, 30, 40]), 4);
        // Edit at index 2 → prefix 0..=1.
        assert_eq!(cache.valid_prefix(&[10, 20, 99, 40]), 2);
        // First block changed → 0.
        assert_eq!(cache.valid_prefix(&[99, 20, 30, 40]), 0);
        // Appended block → match the existing prefix in full.
        assert_eq!(cache.valid_prefix(&[10, 20, 30, 40, 50]), 4);
        // Shorter than cache → match limited by new length.
        assert_eq!(cache.valid_prefix(&[10, 20]), 2);
    }

    // ── Integration: execute_notebook_with_cache prefix behaviour ─────────

    use crate::execute::execute_notebook_with_cache;
    use crate::parse::parse_notebook;

    /// Drive the executor against `source` with `cache`. Returns the
    /// resulting outcome so tests can assert on cache_hit / cached_count
    /// without re-parsing.
    fn drive(source: &str, cache: &mut NotebookCache) -> crate::execute::ExecutionOutcome {
        let blocks = parse_notebook(source);
        execute_notebook_with_cache(&blocks, Some(cache))
    }

    #[test]
    fn second_render_of_identical_source_hits_every_block() {
        let mut cache = NotebookCache::default();
        let src = "# Demo\n\n```rustlab\na = 1;\n```\n\nProse.\n\n```rustlab\nb = a + 1;\n```\n";
        let first = drive(src, &mut cache);
        assert_eq!(first.cached_blocks, 0, "first render is all-miss");
        assert_eq!(first.total_blocks, 2);

        let second = drive(src, &mut cache);
        assert_eq!(second.cached_blocks, 2, "identical re-render must hit every block");
        assert_eq!(second.total_blocks, 2);
    }

    #[test]
    fn prose_only_edit_hits_every_block() {
        let mut cache = NotebookCache::default();
        let first = "```rustlab\nx = 5;\n```\n\nFirst prose.\n";
        let second = "```rustlab\nx = 5;\n```\n\nEntirely different prose.\n";
        drive(first, &mut cache);
        let outcome = drive(second, &mut cache);
        assert_eq!(outcome.cached_blocks, 1, "prose-only change → block stays cached");
    }

    #[test]
    fn editing_last_block_keeps_earlier_blocks_cached() {
        let mut cache = NotebookCache::default();
        let first = "```rustlab\na = 1;\n```\n\n```rustlab\nb = a + 10;\n```\n";
        let second = "```rustlab\na = 1;\n```\n\n```rustlab\nb = a + 999;\n```\n";
        drive(first, &mut cache);
        let outcome = drive(second, &mut cache);
        assert_eq!(
            outcome.cached_blocks, 1,
            "block 0 unchanged → cached; block 1 edited → re-executed",
        );
        assert_eq!(outcome.total_blocks, 2);
    }

    #[test]
    fn editing_first_block_invalidates_everything() {
        let mut cache = NotebookCache::default();
        let first = "```rustlab\nx = 1;\n```\n\n```rustlab\ny = x;\n```\n";
        let second = "```rustlab\nx = 2;\n```\n\n```rustlab\ny = x;\n```\n";
        drive(first, &mut cache);
        let outcome = drive(second, &mut cache);
        assert_eq!(outcome.cached_blocks, 0, "edit at the top cascades through");
    }

    #[test]
    fn appending_a_new_block_keeps_existing_cached() {
        let mut cache = NotebookCache::default();
        let first = "```rustlab\nx = 1;\n```\n";
        let second = "```rustlab\nx = 1;\n```\n\n```rustlab\ny = 2;\n```\n";
        drive(first, &mut cache);
        let outcome = drive(second, &mut cache);
        assert_eq!(outcome.cached_blocks, 1, "new block at end runs against cached prefix");
        assert_eq!(outcome.total_blocks, 2);
    }

    #[test]
    fn middle_block_edit_sees_correct_upstream_state() {
        // Block 0 sets a = 100. Block 1 sets b = a + 1 (= 101). Block 2
        // uses b. Edit block 1 to b = a * 2 (= 200). Block 2 must see
        // b = 200 — proves the snapshot restored upstream state correctly.
        let mut cache = NotebookCache::default();
        let first = "\
```rustlab\na = 100;\n```\n\n\
```rustlab\nb = a + 1;\n```\n\n\
```rustlab\nprint(b)\n```\n";
        let second = "\
```rustlab\na = 100;\n```\n\n\
```rustlab\nb = a * 2;\n```\n\n\
```rustlab\nprint(b)\n```\n";
        drive(first, &mut cache);
        let outcome = drive(second, &mut cache);
        assert_eq!(outcome.cached_blocks, 1, "block 0 cached; blocks 1 & 2 re-execute");

        // Third Rendered::Code must report 200 in its text_output.
        let third = outcome
            .rendered
            .iter()
            .filter_map(|r| match r {
                Rendered::Code { text_output, .. } => Some(text_output.as_str()),
                _ => None,
            })
            .nth(2)
            .expect("third code block missing");
        assert!(
            third.contains("200"),
            "block 2 must see b=200 after restoring state from block 0's snapshot: {third:?}",
        );
    }

    #[test]
    fn cache_hit_reuses_cached_output_verbatim() {
        // Pollute the cache with a sentinel value, render again, prove
        // the emitted output reflects the sentinel — i.e. the code did
        // not re-execute.
        let mut cache = NotebookCache::default();
        let src = "```rustlab\nx = 42;\n```\n";
        let first = drive(src, &mut cache);
        assert_eq!(first.cached_blocks, 0);

        // Tamper.
        for entry in cache.entries.iter_mut() {
            if let Rendered::Code { text_output, .. } = &mut entry.output {
                *text_output = "SENTINEL_TAMPERED".to_string();
            }
        }

        let second = drive(src, &mut cache);
        assert_eq!(second.cached_blocks, 1, "second render must hit");
        let saw_sentinel = second.rendered.iter().any(|r| matches!(
            r,
            Rendered::Code { text_output, .. } if text_output == "SENTINEL_TAMPERED"
        ));
        assert!(saw_sentinel, "cached output must be used verbatim (cache really skipped execution)");
    }
}
