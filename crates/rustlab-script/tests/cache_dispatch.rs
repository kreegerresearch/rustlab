//! Phase 3d end-to-end: drive the full pipeline through real cache
//! hits/misses. We define a user function, call it twice with the
//! same args, and assert the second call short-circuits via the
//! cache (visible through counters and a side-effect sentinel that
//! changes between runs).

use rustlab_script::eval::{Evaluator, Value};
use rustlab_script::lexer::tokenize;
use rustlab_script::parser::parse;

fn run(ev: &mut Evaluator, src: &str) -> Result<(), rustlab_script::ScriptError> {
    let toks = tokenize(src)?;
    let stmts = parse(toks)?;
    ev.run_script(&stmts)
}

fn assert_scalar(ev: &Evaluator, name: &str, expected: f64) {
    match ev.get(name) {
        Some(Value::Scalar(n)) => assert!(
            (n - expected).abs() < 1e-12,
            "{name}: expected {expected}, got {n}"
        ),
        other => panic!("{name}: expected Scalar({expected}), got {other:?}"),
    }
}

#[test]
fn second_call_with_same_args_is_a_cache_hit() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("h.rcache");
    let src = format!(
        "cache enable \"{}\"\n\
         function y = add_one(x)\n  y = x + 1\nend\n\
         a = add_one(5)\n\
         b = add_one(5)\n",
        store.display()
    );
    let mut ev = Evaluator::new();
    run(&mut ev, &src).expect("script");
    assert_scalar(&ev, "a", 6.0);
    assert_scalar(&ev, "b", 6.0);
    let c = ev.cache_counters();
    assert_eq!(c.misses, 1, "first call misses");
    assert_eq!(c.hits, 1, "second call hits");
}

#[test]
fn distinct_args_produce_distinct_misses() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("d.rcache");
    let src = format!(
        "cache enable \"{}\"\n\
         function y = sq(x)\n  y = x * x\nend\n\
         a = sq(2)\n\
         b = sq(3)\n\
         c = sq(2)\n",
        store.display()
    );
    let mut ev = Evaluator::new();
    run(&mut ev, &src).expect("script");
    assert_scalar(&ev, "a", 4.0);
    assert_scalar(&ev, "b", 9.0);
    assert_scalar(&ev, "c", 4.0);
    let counters = ev.cache_counters();
    assert_eq!(counters.misses, 2);
    assert_eq!(counters.hits, 1);
}

#[test]
fn hit_returns_stored_value_verbatim_across_function_edit() {
    // Bug-detector test: define `id`, populate cache, then redefine
    // `id` to return something different. Without an AST-hash
    // dispatch key, the second redefinition would silently keep
    // returning the old cached value — which is the exact failure
    // we want to prevent.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("e.rcache");

    let mut ev = Evaluator::new();
    run(&mut ev, &format!("cache enable \"{}\"\n", store.display())).unwrap();
    run(
        &mut ev,
        "function y = id(x)\n  y = x + 100\nend\n\
         a = id(7)\n",
    )
    .unwrap();
    assert_scalar(&ev, "a", 107.0);
    assert_eq!(ev.cache_counters().misses, 1);

    // Redefine `id` — different body → different AST hash → cache miss.
    run(
        &mut ev,
        "function y = id(x)\n  y = x + 200\nend\n\
         b = id(7)\n",
    )
    .unwrap();
    assert_scalar(&ev, "b", 207.0);
    assert_eq!(ev.cache_counters().misses, 2, "redefinition busts entry id");
    assert_eq!(ev.cache_counters().hits, 0);

    // Calling the new body again with the same arg is now a hit.
    run(&mut ev, "c = id(7)\n").unwrap();
    assert_scalar(&ev, "c", 207.0);
    assert_eq!(ev.cache_counters().hits, 1);
}

#[test]
fn hit_persists_across_evaluator_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("p.rcache");
    let prefix = format!(
        "cache enable \"{}\"\n\
         function y = times_three(x)\n  y = x * 3\nend\n",
        store.display()
    );

    {
        // First process: populate the store.
        let mut ev = Evaluator::new();
        run(&mut ev, &format!("{prefix}a = times_three(11)\n")).unwrap();
        assert_scalar(&ev, "a", 33.0);
        assert_eq!(ev.cache_counters().misses, 1);
    }
    {
        // Second process: same fn definition + same call → should hit.
        let mut ev = Evaluator::new();
        run(&mut ev, &format!("{prefix}b = times_three(11)\n")).unwrap();
        assert_scalar(&ev, "b", 33.0);
        let c = ev.cache_counters();
        assert_eq!(c.hits, 1, "second evaluator should hit the persisted entry");
        assert_eq!(c.misses, 0);
    }
}

#[test]
fn nan_argument_bypasses_cache_but_still_runs() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("n.rcache");
    let src = format!(
        "cache enable \"{}\"\n\
         function y = id(x)\n  y = x\nend\n\
         a = id(NaN)\n\
         b = id(NaN)\n",
        store.display()
    );
    let mut ev = Evaluator::new();
    run(&mut ev, &src).expect("script");
    // The cache should have been bypassed for both calls — no hits,
    // no misses, two uncacheable_arg_skips. (Hits/misses are only
    // counted when we got far enough to attempt a get.)
    let c = ev.cache_counters();
    assert_eq!(c.hits, 0);
    assert_eq!(c.misses, 0);
    assert_eq!(c.uncacheable_arg_skips, 2);
}

#[test]
fn cache_off_disables_hits() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("o.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = id(x)\n  y = x\nend\n\
             a = id(1)\n\
             cache off\n\
             b = id(1)\n",
            store.display()
        ),
    )
    .unwrap();
    let c = ev.cache_counters();
    // After `cache off`, counters were reset → only the post-off run
    // is visible, and it's outside scope → no counter movement.
    assert_eq!(c.hits, 0);
    assert_eq!(c.misses, 0);
}

#[test]
fn cache_remove_function_keeps_db_entries_but_stops_routing() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("rm.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = id(x)\n  y = x\nend\n\
             a = id(1)\n",
            store.display()
        ),
    )
    .unwrap();
    assert_eq!(ev.cache_counters().misses, 1);

    run(&mut ev, "cache remove function id\n").unwrap();
    // The DB still holds the (id, 1) → 1 row, but `id` is no longer
    // in scope, so a second call doesn't consult the cache.
    run(&mut ev, "b = id(1)\n").unwrap();
    let c = ev.cache_counters();
    assert_eq!(c.hits, 0, "removed fn doesn't route through cache");
    assert_eq!(c.misses, 1, "no new miss either — bypassed entirely");

    // Re-adding restores the routing → next call hits the old entry.
    run(&mut ev, "cache add function id\nc = id(1)\n").unwrap();
    assert_eq!(
        ev.cache_counters().hits,
        1,
        "re-added fn finds the stored row from before remove"
    );
}

#[test]
fn matrix_argument_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("m.rcache");
    let src = format!(
        "cache enable \"{}\"\n\
         function y = scale(m)\n  y = m * 2\nend\n\
         A = [1, 2; 3, 4]\n\
         B = scale(A)\n\
         C = scale(A)\n",
        store.display()
    );
    let mut ev = Evaluator::new();
    run(&mut ev, &src).expect("script");
    let c = ev.cache_counters();
    assert_eq!(c.misses, 1);
    assert_eq!(c.hits, 1);
    // Identity check: B and C should be the same matrix.
    let (b, c) = (ev.get("B").cloned().unwrap(), ev.get("C").cloned().unwrap());
    match (&b, &c) {
        (Value::Matrix(b), Value::Matrix(c)) => assert_eq!(b, c, "B and C match"),
        _ => panic!("expected matrices, got {b:?} / {c:?}"),
    }
}

#[test]
fn inline_defined_impure_fn_under_all_scope_is_not_cached() {
    // Phase 6a: when `cache enable` is on, defining an inline function
    // that touches an impure builtin (rand, plot, …) must NOT route
    // through the cache. The function still runs normally on every
    // call; only the dispatcher's `is_in_scope` check changes via the
    // gate added in StmtKind::FunctionDef. Counter is bumped so users
    // can see why their fn isn't speeding up.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("imp.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = noisy(x)\n  y = x + rand()\nend\n\
             a = noisy(1)\n\
             b = noisy(1)\n",
            store.display()
        ),
    )
    .unwrap();
    // Both calls really executed `rand()` — they should disagree
    // (probability of collision is astronomical) AND no caching
    // happened.
    let c = ev.cache_counters();
    assert_eq!(c.hits, 0, "impure fn must never hit");
    assert_eq!(c.misses, 0, "impure fn must never miss either (skipped before lookup)");
    assert!(c.impurity_skips >= 1, "impurity gate should fire at FunctionDef time");
    assert!(!ev.is_fn_cache_scoped("noisy"));
}

#[test]
fn pre_existing_user_fn_gets_gated_on_cache_enable() {
    // Define an impure function *before* `cache enable`. The enable
    // path should scan existing user_fns and drop impure ones from
    // scope — otherwise the same silent-staleness bug would resurface
    // for users who toggle the cache on partway through a session.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("scan.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        "function y = noisy(x)\n  y = x + rand()\nend\n\
         function y = pure(x)\n  y = x + 1\nend\n",
    )
    .unwrap();
    run(&mut ev, &format!("cache enable \"{}\"\n", store.display())).unwrap();
    assert!(!ev.is_fn_cache_scoped("noisy"), "scan should drop impure fns");
    assert!(ev.is_fn_cache_scoped("pure"), "pure fns survive the scan");
}

#[test]
fn pure_inline_fn_still_caches_normally_after_gate() {
    // Regression check: the gate must not affect pure functions.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("ok.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = pure(x)\n  y = x + 100\nend\n\
             a = pure(1)\n\
             b = pure(1)\n",
            store.display()
        ),
    )
    .unwrap();
    let c = ev.cache_counters();
    assert_eq!(c.hits, 1);
    assert_eq!(c.misses, 1);
}

#[test]
fn redefining_a_pure_fn_as_impure_drops_it_from_scope() {
    // User defines `f` pure, cache enables, calls it (warm), then
    // redefines `f` with an impure body. The FunctionDef gate runs on
    // the *new* body and marks the name removed — so the next call
    // doesn't replay the old cached value.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("redef.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = f(x)\n  y = x + 1\nend\n\
             a = f(7)\n",
            store.display()
        ),
    )
    .unwrap();
    assert!(ev.is_fn_cache_scoped("f"));

    run(
        &mut ev,
        "function y = f(x)\n  y = x + rand()\nend\n",
    )
    .unwrap();
    assert!(!ev.is_fn_cache_scoped("f"), "impure redefinition drops scope");
}

#[test]
fn transitive_impurity_via_user_fn_chain_is_caught() {
    // Phase 6c: define helper `g` that calls `rand()`, THEN define
    // caller `f` that calls `g`. With cache active, the gate on `f`
    // walks transitively and sees `g`'s `rand` call → marks `f`
    // removed.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("trans.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = g(x)\n  y = x + rand()\nend\n\
             function y = f(x)\n  y = g(x) + 1\nend\n",
            store.display()
        ),
    )
    .unwrap();
    assert!(!ev.is_fn_cache_scoped("g"), "g calls rand directly");
    assert!(
        !ev.is_fn_cache_scoped("f"),
        "f → g → rand is transitively impure"
    );
}

#[test]
fn transitive_purity_handles_mutual_recursion_without_loop() {
    // Two pure functions that call each other (silly but legal).
    // The walker must not loop forever on the cycle.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("cyc.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = ping(x)\n  if x < 1; y = 0; else; y = pong(x - 1); end\nend\n\
             function y = pong(x)\n  if x < 1; y = 1; else; y = ping(x - 1); end\nend\n",
            store.display()
        ),
    )
    .unwrap();
    assert!(ev.is_fn_cache_scoped("ping"));
    assert!(ev.is_fn_cache_scoped("pong"));
}

#[test]
fn transitive_walk_terminates_on_self_recursion() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("self.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = fact(n)\n  if n < 2; y = 1; else; y = n * fact(n - 1); end\nend\n",
            store.display()
        ),
    )
    .unwrap();
    assert!(ev.is_fn_cache_scoped("fact"));
}

#[test]
fn multi_output_call_is_cached_and_hits_second_time() {
    // Phase 6d: `[a, b] = stats(x)` should go through the cache.
    // First call misses; second call with the same args hits and
    // returns identical tuple components.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("mo.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function [s, q] = stats(x)\n  s = x + 1\n  q = x * x\nend\n\
             [a1, b1] = stats(4)\n\
             [a2, b2] = stats(4)\n",
            store.display()
        ),
    )
    .unwrap();
    assert_scalar(&ev, "a1", 5.0);
    assert_scalar(&ev, "b1", 16.0);
    assert_scalar(&ev, "a2", 5.0);
    assert_scalar(&ev, "b2", 16.0);
    let c = ev.cache_counters();
    assert_eq!(c.misses, 1);
    assert_eq!(c.hits, 1);
}

#[test]
fn mixed_nargout_calls_share_a_single_cache_entry() {
    // The cache key is nargout-independent because the body always
    // produces the full canonical output set. A nargout=1 call should
    // therefore warm the cache for a later nargout=2 call (and vice
    // versa) with no extra body execution.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("mixed.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function [s, q] = stats(x)\n  s = x + 1\n  q = x * x\nend\n\
             p = stats(7)\n\
             [a, b] = stats(7)\n",
            store.display()
        ),
    )
    .unwrap();
    assert_scalar(&ev, "p", 8.0);
    assert_scalar(&ev, "a", 8.0);
    assert_scalar(&ev, "b", 49.0);
    let c = ev.cache_counters();
    assert_eq!(c.misses, 1, "first (nargout=1) call misses + populates");
    assert_eq!(c.hits, 1, "second (nargout=2) call hits the same entry");
}

#[test]
fn nargout_zero_warms_cache_for_later_calls() {
    // A statement-form call (`stats(3);`) computes the body and
    // throws the result away — but per Phase 6d's design, we still
    // store the canonical output so a subsequent nargout>=1 call
    // hits instead of recomputing.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("n0.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function [s, q] = stats(x)\n  s = x + 1\n  q = x * x\nend\n\
             stats(3);\n\
             [a, b] = stats(3)\n",
            store.display()
        ),
    )
    .unwrap();
    assert_scalar(&ev, "a", 4.0);
    assert_scalar(&ev, "b", 9.0);
    let c = ev.cache_counters();
    assert_eq!(c.misses, 1, "nargout=0 call misses + stores");
    assert_eq!(c.hits, 1, "nargout=2 follow-up hits the warmed entry");
}

#[test]
fn under_assignment_error_reproduces_on_cache_hit() {
    // A function that only assigns `s` (not `q`) works for nargout=1
    // (returns s) and errors for nargout=2 ("q was not assigned").
    // The cache must reproduce both behaviours from a single warmed
    // entry — exercise both call shapes after the same body run.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("partial.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function [s, q] = partial(x)\n  s = x + 1\nend\n\
             p = partial(5)\n",
            store.display()
        ),
    )
    .unwrap();
    assert_scalar(&ev, "p", 6.0);

    // Now the cache holds canonical = [Scalar(6), None]. A
    // nargout=2 call must error naming the unassigned output.
    let err = run(&mut ev, "[a, b] = partial(5)\n")
        .expect_err("nargout=2 with partial assignment must error");
    let msg = format!("{err}");
    assert!(
        msg.contains("'q' was not assigned"),
        "error should name the missing output: {msg}"
    );

    // A second nargout=1 call should still work (returns s). And it
    // should hit the cache, not recompute.
    run(&mut ev, "p2 = partial(5)\n").unwrap();
    assert_scalar(&ev, "p2", 6.0);
    let c = ev.cache_counters();
    // misses: 1 (initial p = partial(5))
    // hits:   1 successful (p2) ; the failing [a,b] call also hit
    //         and counted as a hit before its shape error
    assert_eq!(c.misses, 1);
    assert!(c.hits >= 1);
}

// ── Phase 7 (Option 3): canonical, rename-invariant identity ───────

/// Helper: run two scripts in two FRESH evaluators sharing a single
/// store path. Returns the second evaluator so the caller can assert
/// on its counters. The first run populates the cache; the second
/// reads its hits/misses.
fn two_session_counts(
    store_path: &std::path::Path,
    populate: &str,
    second_run: &str,
) -> rustlab_script::cache_registry::CacheCounters {
    let mut ev1 = Evaluator::new();
    run(&mut ev1, &format!("cache enable \"{}\"\n{populate}", store_path.display())).unwrap();
    let mut ev2 = Evaluator::new();
    run(
        &mut ev2,
        &format!("cache enable \"{}\"\n{second_run}", store_path.display()),
    )
    .unwrap();
    ev2.cache_counters()
}

#[test]
fn parameter_rename_preserves_cache_entry() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("p.rcache");
    let counters = two_session_counts(
        &store,
        "function y = sq(x); y = x * x; end\na = sq(7)\n",
        "function y = sq(z); y = z * z; end\nb = sq(7)\n",
    );
    assert_eq!(counters.hits, 1, "renaming x→z must not bust the cache");
    assert_eq!(counters.misses, 0);
}

#[test]
fn local_rename_preserves_cache_entry() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("l.rcache");
    let counters = two_session_counts(
        &store,
        "function y = f(x)\n  k = 2\n  y = x + k\nend\na = f(7)\n",
        "function y = f(x)\n  m = 2\n  y = x + m\nend\nb = f(7)\n",
    );
    assert_eq!(counters.hits, 1, "renaming local k→m must not bust");
    assert_eq!(counters.misses, 0);
}

#[test]
fn return_var_rename_preserves_cache_entry() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("r.rcache");
    let counters = two_session_counts(
        &store,
        "function y = f(x); y = x + 1; end\na = f(7)\n",
        "function z = f(x); z = x + 1; end\nb = f(7)\n",
    );
    assert_eq!(counters.hits, 1, "renaming return var y→z must not bust");
    assert_eq!(counters.misses, 0);
}

#[test]
fn function_name_rename_preserves_cache_entry() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("fn.rcache");
    let counters = two_session_counts(
        &store,
        "function y = expensive(x); y = x * x; end\na = expensive(7)\n",
        "function y = quick(x); y = x * x; end\nb = quick(7)\n",
    );
    assert_eq!(counters.hits, 1, "renaming function expensive→quick must not bust");
    assert_eq!(counters.misses, 0);
}

#[test]
fn literal_change_still_busts_cache() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("lit.rcache");
    let counters = two_session_counts(
        &store,
        "function y = f(x); y = x + 1; end\na = f(7)\n",
        "function y = f(x); y = x + 2; end\nb = f(7)\n",
    );
    assert_eq!(counters.hits, 0, "literal change must bust");
    assert_eq!(counters.misses, 1);
}

#[test]
fn operator_change_still_busts_cache() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("op.rcache");
    let counters = two_session_counts(
        &store,
        "function y = f(x); y = x + 1; end\na = f(7)\n",
        "function y = f(x); y = x - 1; end\nb = f(7)\n",
    );
    assert_eq!(counters.hits, 0, "operator change must bust");
    assert_eq!(counters.misses, 1);
}

#[test]
fn sibling_rename_without_body_change_preserves_caller_cache() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("sib.rcache");
    // Session 1: helper + caller. Both pure. Body unchanged in session 2;
    // only helper's NAME changes (and caller is updated to call the
    // new name). Caller's algorithm is structurally identical, so its
    // cache entry survives.
    let counters = two_session_counts(
        &store,
        "function y = helper(x); y = x * 2; end\n\
         function y = caller(x); y = helper(x) + 1; end\n\
         a = caller(5)\n",
        "function y = doubler(x); y = x * 2; end\n\
         function y = caller(x); y = doubler(x) + 1; end\n\
         b = caller(5)\n",
    );
    assert_eq!(counters.hits, 1, "caller hit despite helper rename");
    // The helper itself was called in session 1 (under the old name);
    // its entry is keyed on the same canonical hash, so the new
    // doubler call also hits.
    assert_eq!(counters.misses, 0);
}

#[test]
fn callee_body_edit_busts_caller_transitively() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("trans.rcache");
    // The correctness-bug case from the proposal: editing `helper`'s
    // body must rotate `caller`'s entry_id so cached caller results
    // don't replay stale.
    let counters = two_session_counts(
        &store,
        "function y = helper(x); y = x * 2; end\n\
         function y = caller(x); y = helper(x) + 1; end\n\
         a = caller(5)\n",
        "function y = helper(x); y = x * 3; end\n\
         function y = caller(x); y = helper(x) + 1; end\n\
         b = caller(5)\n",
    );
    assert_eq!(counters.hits, 0, "callee edit must invalidate caller");
}

#[test]
fn direct_recursion_is_stable_and_rename_invariant() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("rec.rcache");
    let counters = two_session_counts(
        &store,
        "function y = fact(n)\n  if n < 2; y = 1; else; y = n * fact(n - 1); end\nend\n\
         a = fact(5)\n",
        "function y = factorial(n)\n  if n < 2; y = 1; else; y = n * factorial(n - 1); end\nend\n\
         b = factorial(5)\n",
    );
    assert_eq!(
        counters.hits, 1,
        "self-recursive fn renamed fact→factorial keeps cache",
    );
    assert_eq!(counters.misses, 0);
}

#[test]
fn mutual_recursion_terminates_and_is_stable() {
    // Cycle case: ping ↔ pong. Under name-fallback, the cycle
    // participants lose rename invariance specifically for the
    // names in the cycle, but the hash is stable and terminating.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("mr.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = ping(n); if n<1; y=0; else; y = pong(n-1); end; end\n\
             function y = pong(n); if n<1; y=1; else; y = ping(n-1); end; end\n\
             a = ping(4)\n\
             b = ping(4)\n",
            store.display()
        ),
    )
    .unwrap();
    let c = ev.cache_counters();
    // First ping(4) cascades: ping(4) → pong(3) → ping(2) → pong(1) →
    // ping(0). Five distinct cache keys, five misses, the body of
    // each runs once. Second ping(4) hits the top-level entry
    // without descending.
    assert_eq!(c.hits, 1, "second ping(4) call hits the top-level entry");
    assert_eq!(c.misses, 5, "five intermediate cache keys from the recursion");
}

#[test]
fn lambda_rename_invariance_for_captured_outer_vars() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("lam.rcache");
    // Outer `x` renamed to `z`; lambda's param `t` renamed to `u`.
    // Both lambdas implement `(captured_outer + 1) * 2` then add the
    // captured outer. Algorithm is structurally identical.
    let counters = two_session_counts(
        &store,
        "function y = f(x)\n  g = @(t) (t + 1) * 2\n  y = g(x) + x\nend\n\
         a = f(7)\n",
        "function y = f(z)\n  g = @(u) (u + 1) * 2\n  y = g(z) + z\nend\n\
         b = f(7)\n",
    );
    assert_eq!(counters.hits, 1, "lambda + outer rename preserves cache");
    assert_eq!(counters.misses, 0);
}

#[test]
fn mutual_recursion_loses_rename_invariance_for_cycle_participants() {
    // Phase 7 limitation: when two functions form a cycle, the
    // canonical walker uses *name fallback* to break the cycle (it
    // feeds the callee's name as bytes instead of recursing into the
    // entry id). That means the cycle participants — and only the
    // cycle participants — bust the cache when renamed.
    //
    // This test makes the trade-off visible: ping + pong renamed to
    // foo + bar with the same bodies *does* invalidate the cache,
    // even though renaming a function NOT involved in a cycle would
    // have preserved the entry (covered by `function_name_rename_
    // preserves_cache_entry`).
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("mr_lim.rcache");

    // Session 1: populate.
    let mut ev1 = Evaluator::new();
    run(
        &mut ev1,
        &format!(
            "cache enable \"{}\"\n\
             function y = ping(n); if n<1; y=0; else; y = pong(n-1); end; end\n\
             function y = pong(n); if n<1; y=1; else; y = ping(n-1); end; end\n\
             a = ping(4)\n",
            store.display()
        ),
    )
    .unwrap();
    let s1 = ev1.cache_counters();
    assert_eq!(s1.misses, 5, "session 1 should cascade 5 misses");
    drop(ev1);

    // Session 2: rename both cycle participants, same algorithm.
    let mut ev2 = Evaluator::new();
    run(
        &mut ev2,
        &format!(
            "cache enable \"{}\"\n\
             function y = alpha(n); if n<1; y=0; else; y = beta(n-1); end; end\n\
             function y = beta(n); if n<1; y=1; else; y = alpha(n-1); end; end\n",
            store.display()
        ),
    )
    .unwrap();
    // Sanity: both renamed fns should be in cache scope.
    assert!(
        ev2.is_fn_cache_scoped("alpha"),
        "alpha should be cache-scoped after both definitions + rescan"
    );
    assert!(
        ev2.is_fn_cache_scoped("beta"),
        "beta should be cache-scoped after both definitions"
    );

    run(&mut ev2, "b = alpha(4)\n").unwrap();
    let s2 = ev2.cache_counters();
    // Five distinct keys cascade down the chain; under the canonical
    // hash, renaming cycle participants busts every one of them.
    assert_eq!(
        s2.hits, 0,
        "renaming both cycle participants busts the cache: {s2:?}",
    );
    assert_eq!(s2.misses, 5, "{s2:?}");
}

#[test]
fn stale_wire_version_blob_triggers_recompute_each_call() {
    // End-to-end test for the silent-recompute path AND the second
    // limitation it exposes: stale blobs persist forever until
    // manually cleared.
    //
    // When a row's value blob predates the current wire format, the
    // dispatcher's GET succeeds at the SQLite level but
    // `deserialize_value` rejects the version byte and returns None.
    // The dispatcher bumps `serialization_skips` and recomputes the
    // body. The PUT step then runs `INSERT OR IGNORE` — which is a
    // **no-op because the row already exists**. So the stale blob
    // stays in place and the same path fires on every call until
    // the user runs `cache clear` (or the row ages out via
    // `cache prune`).
    //
    // This is a deliberate trade-off: INSERT OR IGNORE keeps
    // simultaneous cold-misses on the same key from racing each
    // other (one writer wins, the other's INSERT no-ops). The cost
    // is that stale rows aren't auto-refreshed. After a rustlab
    // upgrade that bumps the wire format, users should
    // `cache clear` to drop the legacy rows.
    //
    // We simulate the upgrade by populating the cache with the
    // current binary, then patching the raw value BLOBs in SQLite
    // to a 1-byte blob containing wire version 1 (which the v2
    // dispatcher won't accept).
    use rusqlite::Connection;

    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("stale.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = sq(x); y = x * x; end\n\
             a = sq(7)\n",
            store_path.display()
        ),
    )
    .unwrap();
    assert_eq!(ev.cache_counters().misses, 1);
    assert_eq!(ev.cache_counters().serialization_skips, 0);
    drop(ev);

    // Corrupt every cached blob to look like a wire-version-1 entry.
    // Bind a brand-new 1-byte blob via the parameter API (SQLite's
    // `||` operator on BLOBs coerces to TEXT, which we don't want).
    {
        let conn = Connection::open(&store_path).unwrap();
        let stale_blob = vec![0x01u8];
        let n = conn
            .execute(
                "UPDATE cache_entries SET value = ?1",
                rusqlite::params![stale_blob],
            )
            .unwrap();
        assert_eq!(n, 1, "expected exactly one row to patch");
    }

    // Reopen and call again. The GET reads the stale bytes,
    // deserialize returns None, dispatcher bumps the skip counter
    // and recomputes. PUT no-ops because the row exists.
    let mut ev2 = Evaluator::new();
    run(
        &mut ev2,
        &format!(
            "cache enable \"{}\"\n\
             function y = sq(x); y = x * x; end\n\
             b = sq(7)\n\
             c = sq(7)\n",
            store_path.display()
        ),
    )
    .unwrap();
    assert_scalar(&ev2, "b", 49.0);
    assert_scalar(&ev2, "c", 49.0);
    let counters = ev2.cache_counters();
    assert_eq!(
        counters.serialization_skips, 2,
        "stale row triggers a skip on every call (no auto-refresh): {counters:?}",
    );
    assert_eq!(
        counters.hits, 0,
        "stale row never becomes a hit until cleared: {counters:?}",
    );

    // The documented escape hatch: clear the cache. The next call
    // re-populates the row with the current wire format and
    // subsequent calls hit it cleanly.
    run(&mut ev2, "cache clear\nd = sq(7)\ne = sq(7)\n").unwrap();
    let post_clear = ev2.cache_counters();
    assert_eq!(post_clear.hits, 1, "after `cache clear` the cache works again: {post_clear:?}");
    assert_eq!(post_clear.misses, 1);
}

#[test]
fn file_to_inline_move_preserves_cache_when_body_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("move.rcache");
    let helpers = dir.path().join("helpers.rlab");
    std::fs::write(
        &helpers,
        "function y = helper(x); y = x * 3; end\n",
    )
    .unwrap();

    // Session 1: load via `cache add file`, populate.
    let mut ev1 = Evaluator::new();
    run(
        &mut ev1,
        &format!(
            "cache enable \"{}\"\ncache add file \"{}\"\na = helper(7)\n",
            store.display(),
            helpers.display(),
        ),
    )
    .unwrap();

    // Session 2: define the same function inline. Under Option 3
    // (file_hash mixing dropped), the entry_id is purely
    // algorithmic — same body, same hash, so the second call hits.
    let mut ev2 = Evaluator::new();
    run(
        &mut ev2,
        &format!(
            "cache enable \"{}\"\nfunction y = helper(x); y = x * 3; end\nb = helper(7)\n",
            store.display()
        ),
    )
    .unwrap();
    let c = ev2.cache_counters();
    assert_eq!(c.hits, 1, "moving from file-load to inline preserves cache");
    assert_eq!(c.misses, 0);
}

#[test]
fn cache_clear_resets_session_counters() {
    // Caught during the notebook-render walkthrough: clearing the
    // store wipes its rows but used to leave the in-memory hit/miss
    // counters stale, so `cache status` reported "1 hits, 2003
    // misses" against an empty DB. Now `cache clear` resets the
    // counters too — the post-clear status should be all zeros.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("rst.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = pure(x)\n  y = x + 1\nend\n\
             a = pure(1)\n\
             b = pure(1)\n",
            store.display()
        ),
    )
    .unwrap();
    // Sanity: we did populate the counters before clearing.
    let before = ev.cache_counters();
    assert_eq!(before.hits, 1);
    assert_eq!(before.misses, 1);
    assert!(before.per_fn.contains_key("pure"));

    run(&mut ev, "cache clear\n").unwrap();
    let after = ev.cache_counters();
    assert_eq!(after.hits, 0, "clear must reset hits");
    assert_eq!(after.misses, 0, "clear must reset misses");
    assert!(after.per_fn.is_empty(), "clear must reset per-fn table");
}

#[test]
fn per_fn_stats_appear_in_status_after_calls() {
    // Phase 6b: status_text should surface per-function hit/miss
    // counts so users can see which functions are benefiting.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("stats.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = pure(x)\n  y = x + 1\nend\n\
             a = pure(1)\n\
             b = pure(1)\n\
             c = pure(2)\n",
            store.display()
        ),
    )
    .unwrap();
    let counters = ev.cache_counters();
    let pure = counters
        .per_fn
        .get("pure")
        .expect("per-fn entry for `pure`");
    assert_eq!(pure.hits, 1, "second pure(1) call hit");
    assert_eq!(pure.misses, 2, "pure(1) and pure(2) both missed first time");
}
