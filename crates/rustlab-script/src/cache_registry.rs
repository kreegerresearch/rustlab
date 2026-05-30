//! Per-evaluator state for the persistent function-result cache.
//!
//! Holds the active [`rustlab_cache::Store`] (if any), the scope set
//! that decides which user functions are eligible for caching, and
//! per-process counters surfaced via `cache status`. Every in-scope
//! call is looked up; every miss with a serialisable result is stored
//! (no time-based gating — the original threshold design was removed
//! on 2026-05-24).
//!
//! The dispatcher (Phase 3d) will consult [`CacheRegistry::is_in_scope`]
//! at every user-function call site. The evaluator (Phase 3c) will
//! call the lifecycle methods (`enable` / `off` / `add_*` / `remove`)
//! from the `StmtKind::Cache` arm.
//!
//! Locked design choices (from `dev/plans/persistent_function_cache.md`):
//!
//! - **One active store per process.** `enable` while another store is
//!   open closes the prior one and resets scope + counters.
//! - **Default scope = `all` user-defined functions.** Cleared on `off`.
//! - **No time-based gating.** Every in-scope call is looked up; every
//!   miss that produces a serializable result is stored. (The original
//!   threshold-based design was removed on 2026-05-24.)
//! - **`remove` beats `add` beats `all`.** A function explicitly
//!   removed stays out of scope even with `all_scope = true`.

use rustlab_cache::{CacheError, Store};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Per-project default store location. Resolved relative to the
/// process's current working directory at `enable` time.
pub const DEFAULT_STORE_PATH: &str = ".rustlab/cache.db";

/// Per-process counters surfaced by `cache status`. Reset on `enable`
/// and `off`. Not shared across processes — multi-instance setups
/// each see their own counts.
#[derive(Debug, Clone, Default)]
pub struct CacheCounters {
    /// Calls that returned a stored value without recomputing.
    pub hits: u64,
    /// Calls that missed the cache and recomputed.
    pub misses: u64,
    /// Calls whose function failed the impurity check at registration
    /// time — `all` mode silently skips these.
    pub impurity_skips: u64,
    /// Same, for free-variable failures.
    pub free_var_skips: u64,
    /// Cache hits the dispatcher couldn't return because the stored
    /// blob didn't deserialize. Rare; surfaces in `status` so an
    /// upgrade-day spike is visible.
    pub serialization_skips: u64,
    /// Calls whose argument list contained an uncacheable value
    /// (NaN, non-fingerprintable type). Function ran normally; cache
    /// was bypassed for this invocation only.
    pub uncacheable_arg_skips: u64,
    /// Per-function hit/miss tally. Populated by the dispatcher in
    /// step with the global `hits`/`misses`. Surfaced as a sorted
    /// table by `cache status` so users can see which functions are
    /// actually benefiting from the cache.
    pub per_fn: BTreeMap<String, FnCounters>,
}

/// Hit/miss tally for a single function name.
#[derive(Debug, Clone, Copy, Default)]
pub struct FnCounters {
    pub hits: u64,
    pub misses: u64,
}

/// Metadata captured when a function is loaded via `cache add file`.
#[derive(Debug, Clone)]
pub struct FileLoadedFn {
    /// BLAKE3 of the file's AST at load time. The dispatcher composes
    /// `entry_id = function_entry_id(file_ast_hash, fn_name)`; editing
    /// any function in the file rotates this hash and busts every
    /// cached entry derived from it.
    pub file_ast_hash: [u8; 32],
    /// User-facing path the file was loaded from. Stored for
    /// `cache status` display; the store key is derived from
    /// `file_ast_hash`, not the path.
    pub file_path: PathBuf,
}

/// Per-evaluator cache state. Cloned by `Evaluator::deep_clone` and
/// by the notebook prefix cache; the store handle is `Arc<Store>` so
/// clones are cheap and share the same SQLite connection.
#[derive(Clone)]
pub struct CacheRegistry {
    store: Option<Arc<Store>>,
    store_path: Option<PathBuf>,
    all_scope: bool,
    explicit_fns: BTreeSet<String>,
    removed_fns: BTreeSet<String>,
    file_loaded_fns: BTreeMap<String, FileLoadedFn>,
    counters: CacheCounters,
}

impl Default for CacheRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheRegistry {
    /// Fresh registry — no active store, default threshold, no scope
    /// entries (`is_in_scope` returns `false` for every name because
    /// there's no store).
    pub fn new() -> Self {
        Self {
            store: None,
            store_path: None,
            all_scope: true,
            explicit_fns: BTreeSet::new(),
            removed_fns: BTreeSet::new(),
            file_loaded_fns: BTreeMap::new(),
            counters: CacheCounters::default(),
        }
    }

    /// `cache enable [path]`. Opens (or creates) the store at `path`
    /// or, if `path` is `None`, the per-project default at
    /// `.rustlab/cache.db`. Closes any prior active store and resets
    /// scope + counters — the user is declaring a fresh session.
    pub fn enable(&mut self, path: Option<PathBuf>) -> Result<(), CacheError> {
        let path = path.unwrap_or_else(|| PathBuf::from(DEFAULT_STORE_PATH));
        let store = Store::open(&path)?;
        self.store = Some(Arc::new(store));
        self.store_path = Some(path);
        self.reset_scope_and_counters();
        Ok(())
    }

    /// `cache off`. Closes the active store. DB rows are preserved on
    /// disk; only the in-process routing is dropped.
    pub fn off(&mut self) {
        self.store = None;
        self.store_path = None;
        self.reset_scope_and_counters();
    }

    fn reset_scope_and_counters(&mut self) {
        self.all_scope = true;
        self.explicit_fns.clear();
        self.removed_fns.clear();
        self.file_loaded_fns.clear();
        self.counters = CacheCounters::default();
    }

    /// `true` when a store is open.
    pub fn is_active(&self) -> bool {
        self.store.is_some()
    }

    /// Borrow the active store handle, if any. The dispatcher uses
    /// this for `get` / `put`; the eval-side CLI handlers use it for
    /// `clear` / `prune`.
    pub fn store(&self) -> Option<&Store> {
        self.store.as_deref()
    }

    /// Path the active store was opened from, for `cache status`.
    pub fn store_path(&self) -> Option<&Path> {
        self.store_path.as_deref()
    }

    /// Per-process counters snapshot. Cheap to clone (it's all
    /// primitive scalars).
    pub fn counters(&self) -> &CacheCounters {
        &self.counters
    }

    /// Mutable counter handle for the dispatcher to bump.
    pub fn counters_mut(&mut self) -> &mut CacheCounters {
        &mut self.counters
    }

    /// `cache add function <name>, …`. Re-adding a previously-removed
    /// function clears its removal marker.
    pub fn add_functions(&mut self, names: impl IntoIterator<Item = String>) {
        for n in names {
            self.removed_fns.remove(&n);
            self.explicit_fns.insert(n);
        }
    }

    /// Register a function loaded via `cache add file ...`. The
    /// dispatcher uses `file_ast_hash` to compose the entry id.
    pub fn register_file_function(
        &mut self,
        name: String,
        file_ast_hash: [u8; 32],
        file_path: PathBuf,
    ) {
        self.removed_fns.remove(&name);
        self.file_loaded_fns.insert(
            name,
            FileLoadedFn {
                file_ast_hash,
                file_path,
            },
        );
    }

    /// `cache remove function <name>`. Stops dispatch routing for the
    /// name; DB rows are kept (a later `add function` reuses them).
    pub fn remove_function(&mut self, name: &str) {
        self.explicit_fns.remove(name);
        self.file_loaded_fns.remove(name);
        self.removed_fns.insert(name.to_string());
    }

    /// `true` iff calls to a user function named `name` should consult
    /// the cache. Resolution order:
    ///
    /// 1. No active store → `false`.
    /// 2. Name is explicitly removed → `false`.
    /// 3. Name was explicitly added (function or file) → `true`.
    /// 4. `all_scope` is on (the default) → `true`.
    pub fn is_in_scope(&self, name: &str) -> bool {
        if self.store.is_none() {
            return false;
        }
        if self.removed_fns.contains(name) {
            return false;
        }
        self.explicit_fns.contains(name)
            || self.file_loaded_fns.contains_key(name)
            || self.all_scope
    }

    /// File-loaded info for `name`, if it was registered via
    /// `cache add file`. Returns `None` for inline-defined functions —
    /// the dispatcher must compute their entry id from the live AST.
    pub fn file_loaded(&self, name: &str) -> Option<&FileLoadedFn> {
        self.file_loaded_fns.get(name)
    }

    /// Names currently marked as removed from scope. Used by the
    /// evaluator's FunctionDef hook to re-check whether a freshly
    /// defined sibling has resolved a prior free-variable failure
    /// (mutual recursion case: `f` defined first calls `g` defined
    /// later — the initial gate fails free-var, but defining `g`
    /// makes `f` legitimate).
    pub fn removed_fns_snapshot(&self) -> Vec<String> {
        self.removed_fns.iter().cloned().collect()
    }

    /// Un-mark `name` as removed. The evaluator's rescan calls this
    /// when a previously free-var-failing function now passes its
    /// checks because the missing sibling has been defined.
    pub fn restore_removed(&mut self, name: &str) {
        self.removed_fns.remove(name);
    }

    /// `cache clear`. Wipes every entry in the active store; returns
    /// the row count removed. Returns `Ok(0)` when no store is open
    /// (no-op, no error — matches `cache off` ergonomics).
    ///
    /// Also resets the in-memory session counters — after a wipe
    /// they refer to a DB state that no longer exists, and leaving
    /// them in place makes `cache status` lie about what's in the
    /// store. Scope sets (`explicit_fns`, `file_loaded_fns`,
    /// `removed_fns`) are preserved because they reflect user
    /// routing choices, not DB state.
    pub fn clear(&mut self) -> Result<usize, CacheError> {
        let removed = match &self.store {
            Some(s) => s.clear()?,
            None => 0,
        };
        self.counters = CacheCounters::default();
        Ok(removed)
    }

    /// `cache prune older=<DUR>`. With `None`, defaults to 30 days
    /// per the locked design.
    pub fn prune_older(&self, older_secs: Option<u64>) -> Result<usize, CacheError> {
        const THIRTY_DAYS_SECS: u64 = 30 * 24 * 60 * 60;
        match &self.store {
            Some(s) => s.prune_older_than(older_secs.unwrap_or(THIRTY_DAYS_SECS)),
            None => Ok(0),
        }
    }

    /// `cache prune max_size=<BYTES>`. Drops the oldest entries until
    /// total stored bytes ≤ `max_bytes`. No-op when no store is open.
    pub fn prune_to_max_size(&self, max_bytes: u64) -> Result<usize, CacheError> {
        match &self.store {
            Some(s) => s.prune_to_max_size(max_bytes),
            None => Ok(0),
        }
    }

    /// Render `cache status` output. Caller prints. Format is stable
    /// enough that REPL users can grep it; not a machine-readable
    /// interface.
    pub fn status_text(&self) -> String {
        let mut out = String::new();
        if !self.is_active() {
            out.push_str("cache: off\n");
            return out;
        }
        out.push_str(&format!(
            "cache: active ({})\n",
            self.store_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<unknown>".to_string()),
        ));
        out.push_str(&format!(
            "  scope: {}\n",
            if self.all_scope {
                "all user-defined functions"
            } else {
                "explicit only"
            },
        ));
        if !self.explicit_fns.is_empty() {
            out.push_str(&format!(
                "  explicit fns: {}\n",
                join_csv(self.explicit_fns.iter()),
            ));
        }
        if !self.file_loaded_fns.is_empty() {
            out.push_str(&format!(
                "  loaded fns: {}\n",
                join_csv(self.file_loaded_fns.keys()),
            ));
        }
        if !self.removed_fns.is_empty() {
            out.push_str(&format!(
                "  removed fns: {}\n",
                join_csv(self.removed_fns.iter()),
            ));
        }
        let c = &self.counters;
        out.push_str(&format!(
            "  this session: {} hits, {} misses\n",
            c.hits, c.misses,
        ));
        out.push_str(&format!(
            "    skipped: {} impure, {} free-var, {} non-cacheable arg, {} stale-blob\n",
            c.impurity_skips,
            c.free_var_skips,
            c.uncacheable_arg_skips,
            c.serialization_skips,
        ));
        if !c.per_fn.is_empty() {
            out.push_str("  per function:\n");
            // BTreeMap iteration is sorted by key already; users can
            // grep this output by fn name without extra sorting.
            for (name, fc) in &c.per_fn {
                out.push_str(&format!(
                    "    {:<24}  {:>6} hits, {:>6} misses\n",
                    name, fc.hits, fc.misses,
                ));
            }
        }
        out
    }

    /// Record a hit for `fn_name`, bumping both the global and
    /// per-function counters. Called by the dispatcher.
    pub fn record_hit(&mut self, fn_name: &str) {
        self.counters.hits += 1;
        self.counters
            .per_fn
            .entry(fn_name.to_string())
            .or_default()
            .hits += 1;
    }

    /// Record a miss for `fn_name`, bumping both the global and
    /// per-function counters. Called by the dispatcher.
    pub fn record_miss(&mut self, fn_name: &str) {
        self.counters.misses += 1;
        self.counters
            .per_fn
            .entry(fn_name.to_string())
            .or_default()
            .misses += 1;
    }
}

fn join_csv<'a, I: Iterator<Item = &'a String>>(it: I) -> String {
    let mut s = String::new();
    let mut first = true;
    for n in it {
        if !first {
            s.push_str(", ");
        }
        s.push_str(n);
        first = false;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store_path(dir: &tempfile::TempDir, name: &str) -> PathBuf {
        dir.path().join(name)
    }

    #[test]
    fn fresh_registry_is_inactive_and_caches_nothing() {
        let r = CacheRegistry::new();
        assert!(!r.is_active());
        assert!(!r.is_in_scope("anything"));
        assert!(r.store().is_none());
    }

    #[test]
    fn enable_opens_store_and_default_scope_covers_every_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = CacheRegistry::new();
        r.enable(Some(temp_store_path(&dir, "x.db"))).unwrap();
        assert!(r.is_active());
        assert!(r.is_in_scope("anything"), "default scope = all user fns");
        assert!(r.store().is_some());
    }

    #[test]
    fn off_closes_store_and_drops_scope() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = CacheRegistry::new();
        r.enable(Some(temp_store_path(&dir, "x.db"))).unwrap();
        r.add_functions(["expensive".to_string()]);
        assert!(r.is_in_scope("expensive"));
        r.off();
        assert!(!r.is_active());
        assert!(!r.is_in_scope("expensive"));
    }

    #[test]
    fn remove_beats_explicit_add() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = CacheRegistry::new();
        r.enable(Some(temp_store_path(&dir, "x.db"))).unwrap();
        r.add_functions(["expensive".to_string()]);
        r.remove_function("expensive");
        assert!(!r.is_in_scope("expensive"));
    }

    #[test]
    fn remove_beats_all_scope() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = CacheRegistry::new();
        r.enable(Some(temp_store_path(&dir, "x.db"))).unwrap();
        assert!(r.is_in_scope("foo"));
        r.remove_function("foo");
        assert!(!r.is_in_scope("foo"));
    }

    #[test]
    fn re_add_after_remove_restores_scope() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = CacheRegistry::new();
        r.enable(Some(temp_store_path(&dir, "x.db"))).unwrap();
        r.remove_function("foo");
        assert!(!r.is_in_scope("foo"));
        r.add_functions(["foo".to_string()]);
        assert!(r.is_in_scope("foo"));
    }

    #[test]
    fn register_file_function_preserves_hash_and_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = CacheRegistry::new();
        r.enable(Some(temp_store_path(&dir, "x.db"))).unwrap();
        let hash = [42u8; 32];
        r.register_file_function("helper".to_string(), hash, PathBuf::from("helpers.rlab"));
        let info = r.file_loaded("helper").expect("registered");
        assert_eq!(info.file_ast_hash, hash);
        assert_eq!(info.file_path, PathBuf::from("helpers.rlab"));
        assert!(r.is_in_scope("helper"));
    }

    #[test]
    fn enable_swap_resets_scope_and_counters() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = CacheRegistry::new();
        r.enable(Some(temp_store_path(&dir, "a.db"))).unwrap();
        r.add_functions(["leftover".to_string()]);
        r.counters_mut().hits = 7;
        assert!(r.is_in_scope("leftover"));

        r.enable(Some(temp_store_path(&dir, "b.db"))).unwrap();
        assert_eq!(r.counters().hits, 0, "counters reset across enable swap");
        assert!(r.is_in_scope("leftover"));
        assert!(r.file_loaded("leftover").is_none());
    }

    #[test]
    fn arc_store_is_shared_across_clone() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = CacheRegistry::new();
        a.enable(Some(temp_store_path(&dir, "x.db"))).unwrap();
        let b = a.clone();
        let entry = [1u8; 32];
        a.store().unwrap().put(&entry, &entry, b"hello").unwrap();
        assert_eq!(
            b.store().unwrap().get(&entry, &entry).unwrap().as_deref(),
            Some(&b"hello"[..]),
            "clones share the same store via Arc",
        );
    }

    #[test]
    fn status_text_shows_off_when_inactive() {
        let r = CacheRegistry::new();
        assert!(r.status_text().contains("off"));
    }

    #[test]
    fn status_text_shows_active_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = CacheRegistry::new();
        r.enable(Some(temp_store_path(&dir, "shown.db"))).unwrap();
        let s = r.status_text();
        assert!(s.contains("active"));
        assert!(s.contains("shown.db"));
    }
}
