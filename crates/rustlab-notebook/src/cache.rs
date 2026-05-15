//! Whole-notebook output cache for `notebook watch`.
//!
//! The watcher re-runs the executor on every `.md` save, but most saves
//! are *prose* edits — the rustlab code blocks are unchanged. Re-running
//! every `randn(10000)` or `fir_lowpass(...)` block on each keystroke
//! wastes seconds and is the user-visible reason the watcher feels slow.
//!
//! This module caches the *outputs* of executable blocks (rustlab Code +
//! Mermaid) plus the final evaluator/plot/RNG state. When the next render
//! comes in:
//!
//! * Hash every executable block's source. If the hash matches the cache,
//!   it's a **cache hit**: restore the saved state, walk the parsed
//!   blocks emitting cached executable outputs and *freshly-interpolated*
//!   prose, no execution.
//! * On a hash mismatch, fall through to the normal executor and store
//!   its outputs + final state for next time.
//!
//! Limitations of this layer (deliberate — keeps it simple):
//!
//! - Whole-notebook only. *Any* executable-block change invalidates the
//!   whole cache. Per-block partial execution requires snapshots between
//!   blocks; future work.
//! - In-memory, watcher-session-scoped. Restart the watcher → first
//!   render is full. No on-disk persistence.

use crate::execute::Rendered;
use rustlab_plot::PlotSnapshot;
use rustlab_script::eval::rng::RngSnapshot;
use rustlab_script::Evaluator;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Cached state needed to re-emit a notebook's rendered output without
/// re-executing its code blocks.
///
/// Hit/miss is determined by `exec_block_hash`: a stable hash over every
/// executable block's source, in document order. Prose / markdown content
/// is intentionally excluded — it gets re-emitted fresh from the new
/// source on every render so prose edits show up immediately.
pub struct NotebookCache {
    /// Hash of all executable (rustlab + mermaid) block sources joined
    /// with a NUL separator, in document order.
    exec_block_hash: u64,
    /// Rendered outputs for the executable blocks only, in document
    /// order. On cache hit these are spliced into the new block walk in
    /// place of fresh execution.
    exec_outputs: Vec<Rendered>,
    /// Number of exercise blocks seen *before* the cache was built —
    /// used to keep numbering stable across cache hits/misses.
    exercise_counter_start: usize,
    /// Captured state at the END of the previous successful render.
    /// Restored on cache hit so markdown interpolation (`${expr}`) and
    /// callouts see the same variables as the original execution.
    end_state: ExecState,
}

/// Bundle of every runtime state we need to roll back to a prior render:
/// the script evaluator, plot thread-locals, and the RNG.
pub struct ExecState {
    pub evaluator: Evaluator,
    pub plot: PlotSnapshot,
    pub rng: RngSnapshot,
}

impl ExecState {
    /// Snapshot the live thread-local state into an `ExecState`. Uses
    /// `Evaluator::deep_clone` so mutable `Arc<Mutex<…>>` interiors in
    /// `Value` (e.g. `FirState`) don't alias between snapshot and live.
    pub fn capture(evaluator: &Evaluator) -> Self {
        ExecState {
            evaluator: evaluator.deep_clone(),
            plot: rustlab_plot::capture_thread_state(),
            rng: rustlab_script::eval::rng::capture(),
        }
    }

    /// Overwrite the live thread-locals from this snapshot. Returns a
    /// fresh `Evaluator` that the caller installs as its working copy —
    /// the existing evaluator handle is replaced, the thread-local
    /// figure/store/rng state is rolled back.
    pub fn restore(&self) -> Evaluator {
        rustlab_plot::restore_thread_state(&self.plot);
        rustlab_script::eval::rng::restore(&self.rng);
        self.evaluator.deep_clone()
    }
}

impl NotebookCache {
    /// Outcome of consulting the cache for a new render.
    pub fn lookup<'a>(
        &'a mut self,
        exec_block_sources: &[&str],
    ) -> CacheLookup<'a> {
        let new_hash = hash_exec_blocks(exec_block_sources);
        if new_hash == self.exec_block_hash
            && exec_block_sources.len() == self.exec_outputs.len()
        {
            CacheLookup::Hit {
                exec_outputs: &self.exec_outputs,
                end_state: &self.end_state,
                exercise_counter_start: self.exercise_counter_start,
            }
        } else {
            CacheLookup::Miss
        }
    }

    /// Replace the cache contents with the outcome of a fresh execution.
    /// Called by the executor after a cache miss.
    pub fn store(
        &mut self,
        exec_block_sources: &[&str],
        exec_outputs: Vec<Rendered>,
        exercise_counter_start: usize,
        end_state: ExecState,
    ) {
        debug_assert_eq!(
            exec_block_sources.len(),
            exec_outputs.len(),
            "executable source/output counts must agree",
        );
        self.exec_block_hash = hash_exec_blocks(exec_block_sources);
        self.exec_outputs = exec_outputs;
        self.exercise_counter_start = exercise_counter_start;
        self.end_state = end_state;
    }

    /// True if anything is stored. Useful for logging on the first call.
    pub fn is_populated(&self) -> bool {
        !self.exec_outputs.is_empty() || self.exec_block_hash != 0
    }
}

impl Default for NotebookCache {
    fn default() -> Self {
        NotebookCache {
            exec_block_hash: 0,
            exec_outputs: Vec::new(),
            exercise_counter_start: 0,
            end_state: ExecState {
                evaluator: Evaluator::new(),
                plot: rustlab_plot::capture_thread_state(),
                rng: rustlab_script::eval::rng::capture(),
            },
        }
    }
}

/// Outcome of looking up a cache entry against a new source's executable
/// blocks. Borrows the cache so callers can use the cached outputs
/// without cloning the whole thing.
pub enum CacheLookup<'a> {
    Hit {
        exec_outputs: &'a [Rendered],
        end_state: &'a ExecState,
        exercise_counter_start: usize,
    },
    Miss,
}

/// Stable hash over the executable block sources joined with a NUL
/// separator. `DefaultHasher` is not stable across Rust versions, but
/// the cache lives only for a watcher session — no cross-version
/// concern. NUL separator avoids collisions where adjacent blocks could
/// merge under concatenation.
fn hash_exec_blocks(sources: &[&str]) -> u64 {
    let mut h = DefaultHasher::new();
    for s in sources {
        s.hash(&mut h);
        0u8.hash(&mut h);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_changes_when_a_block_changes() {
        let h1 = hash_exec_blocks(&["a = 1", "b = 2"]);
        let h2 = hash_exec_blocks(&["a = 1", "b = 3"]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_stable_for_identical_input() {
        let h1 = hash_exec_blocks(&["plot(1:10)"]);
        let h2 = hash_exec_blocks(&["plot(1:10)"]);
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_differs_on_reorder() {
        let h1 = hash_exec_blocks(&["a", "b"]);
        let h2 = hash_exec_blocks(&["b", "a"]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_differs_when_block_added() {
        let h1 = hash_exec_blocks(&["a"]);
        let h2 = hash_exec_blocks(&["a", "b"]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn lookup_misses_on_fresh_cache() {
        let mut cache = NotebookCache::default();
        match cache.lookup(&["x = 1"]) {
            CacheLookup::Miss => {}
            CacheLookup::Hit { .. } => panic!("fresh cache must miss"),
        }
    }

    // ── Integration: execute_notebook_with_cache behaviour ────────────────
    //
    // These exercise the cache through the public executor entry point. The
    // structural property we care about: a second run with unchanged code
    // blocks returns `cache_hit = true` and reuses the prior outputs;
    // editing a code block forces a miss.

    use crate::execute::{execute_notebook_with_cache, Rendered};
    use crate::parse::parse_notebook;

    fn drive(source: &str, cache: &mut NotebookCache) -> bool {
        let blocks = parse_notebook(source);
        execute_notebook_with_cache(&blocks, Some(cache)).cache_hit
    }

    #[test]
    fn second_render_of_identical_source_hits_cache() {
        let mut cache = NotebookCache::default();
        let src = "# Demo\n\n```rustlab\nx = 1 + 2;\n```\n";
        assert!(!drive(src, &mut cache), "first render is always a miss");
        assert!(drive(src, &mut cache), "second identical render must hit");
    }

    #[test]
    fn prose_only_edit_still_hits_cache() {
        // User changes prose between two code blocks. Code block sources
        // unchanged → cache hit. This is the user's "prose edit should
        // not re-execute" case.
        let mut cache = NotebookCache::default();
        let first = "# Demo\n\n```rustlab\nx = 1 + 2;\n```\n\nFirst version of prose.\n";
        let second = "# Demo\n\n```rustlab\nx = 1 + 2;\n```\n\nEdited prose — totally different.\n";
        assert!(!drive(first, &mut cache));
        assert!(
            drive(second, &mut cache),
            "prose-only edit must hit the cache (code blocks unchanged)",
        );
    }

    #[test]
    fn code_block_edit_misses_cache() {
        let mut cache = NotebookCache::default();
        let v10 = "```rustlab\nplot(1:10)\n```\n";
        let v100 = "```rustlab\nplot(1:100)\n```\n";
        assert!(!drive(v10, &mut cache));
        assert!(
            !drive(v100, &mut cache),
            "code block source change must invalidate the cache",
        );
    }

    #[test]
    fn cache_hit_uses_cached_outputs_for_code_blocks() {
        // Pollute the cache by running once; then on the second
        // (cache-hit) render, replace the executor's behaviour by
        // editing the cache's stored outputs directly. The output
        // emitted on the second pass should match the cache, not what
        // the source would produce — proof that code wasn't re-executed.
        let mut cache = NotebookCache::default();
        let src = "```rustlab\nx = 42;\n```\n";
        let blocks = parse_notebook(src);
        let _ = execute_notebook_with_cache(&blocks, Some(&mut cache));

        // Tamper: stash a recognisable text_output in the cache.
        for r in cache.exec_outputs.iter_mut() {
            if let Rendered::Code { text_output, .. } = r {
                *text_output = "SENTINEL FROM CACHE".to_string();
            }
        }

        let outcome = execute_notebook_with_cache(&blocks, Some(&mut cache));
        assert!(outcome.cache_hit, "second render must hit");
        let from_cache = outcome.rendered.iter().any(|r| matches!(
            r,
            Rendered::Code { text_output, .. } if text_output == "SENTINEL FROM CACHE"
        ));
        assert!(from_cache, "cache-hit render must use cached output, not re-execute");
    }

    #[test]
    fn prose_only_edit_re_interpolates_markdown_against_restored_state() {
        // First render establishes `n = 5` then renders prose that
        // interpolates ${n}. On a prose-only edit (different prose,
        // same template), the cache-hit path must restore the
        // evaluator so ${n} still resolves to 5.
        let mut cache = NotebookCache::default();
        let first = "```rustlab\nn = 5;\n```\n\nFirst: ${n}.\n";
        let second = "```rustlab\nn = 5;\n```\n\nSecond — different prose: ${n}.\n";

        let blocks1 = parse_notebook(first);
        let outcome1 = execute_notebook_with_cache(&blocks1, Some(&mut cache));
        assert!(!outcome1.cache_hit);

        let blocks2 = parse_notebook(second);
        let outcome2 = execute_notebook_with_cache(&blocks2, Some(&mut cache));
        assert!(outcome2.cache_hit, "code block unchanged ⇒ hit");

        let interpolated_ok = outcome2.rendered.iter().any(|r| matches!(
            r,
            Rendered::Markdown(text) if text.contains("Second — different prose: 5")
        ));
        assert!(
            interpolated_ok,
            "edited prose must re-interpolate against the restored evaluator state",
        );
    }
}
