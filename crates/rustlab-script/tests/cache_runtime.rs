//! Phase 3c — end-to-end: drives the full `lex → parse → eval`
//! pipeline through `Evaluator::run_script` for each `cache ...`
//! subcommand. The dispatcher (Phase 3d) isn't wired yet, so these
//! tests do NOT exercise hits/misses — they verify the *control*
//! plane: store open/close, scope updates, purity rejection at file
//! load, counters, error paths.

use rustlab_script::eval::Evaluator;
use rustlab_script::lexer::tokenize;
use rustlab_script::parser::parse;

fn run(ev: &mut Evaluator, src: &str) -> Result<(), rustlab_script::ScriptError> {
    let toks = tokenize(src)?;
    let stmts = parse(toks)?;
    ev.run_script(&stmts)
}

fn run_in_dir(src: &str) -> (tempfile::TempDir, Evaluator) {
    // Switch CWD into a fresh tempdir so `cache enable` (no path) lands
    // its `.rustlab/cache.db` somewhere we can inspect without
    // polluting the workspace. Drop guard restores CWD on return.
    let dir = tempfile::tempdir().expect("tempdir");
    let prev = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(dir.path()).expect("chdir");
    let mut ev = Evaluator::new();
    let result = run(&mut ev, src);
    std::env::set_current_dir(prev).expect("restore cwd");
    result.expect("script");
    (dir, ev)
}

// ── enable / off ────────────────────────────────────────────────────

#[test]
fn enable_default_path_creates_project_db() {
    let (dir, ev) = run_in_dir("cache enable\n");
    assert!(ev.cache_active(), "cache should be active after enable");
    let db = dir.path().join(".rustlab/cache.db");
    assert!(db.exists(), "default DB should land at .rustlab/cache.db");
}

#[test]
fn enable_named_path_opens_user_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("my.rcache");
    let src = format!("cache enable \"{}\"\n", path.display());
    let mut ev = Evaluator::new();
    run(&mut ev, &src).expect("script");
    assert!(ev.cache_active());
    assert!(path.exists(), "named store file should exist");
}

#[test]
fn off_closes_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.rcache");
    let mut ev = Evaluator::new();
    run(&mut ev, &format!("cache enable \"{}\"\n", path.display())).unwrap();
    assert!(ev.cache_active());
    run(&mut ev, "cache off\n").unwrap();
    assert!(!ev.cache_active());
}

// ── cache add file ──────────────────────────────────────────────────

#[test]
fn add_file_with_pure_functions_installs_and_registers() {
    let dir = tempfile::tempdir().unwrap();
    let helpers = dir.path().join("helpers.rlab");
    std::fs::write(
        &helpers,
        "function y = add_one(x)\n  y = x + 1\nend\n\
         function y = double(x)\n  y = x * 2\nend\n",
    )
    .unwrap();
    let store = dir.path().join("cache.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\ncache add file \"{}\"\n",
            store.display(),
            helpers.display()
        ),
    )
    .expect("script");

    assert!(ev.is_user_fn_defined("add_one"));
    assert!(ev.is_user_fn_defined("double"));
    assert!(ev.is_fn_cache_scoped("add_one"));
    assert!(ev.is_fn_cache_scoped("double"));
}

#[test]
fn add_file_rejects_free_variable_as_hard_error() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.rlab");
    // References `k` which isn't a param or local — must hard-error.
    std::fs::write(
        &bad,
        "function y = f(x)\n  y = x + k\nend\n",
    )
    .unwrap();
    let store = dir.path().join("c.rcache");
    let mut ev = Evaluator::new();
    run(&mut ev, &format!("cache enable \"{}\"\n", store.display())).unwrap();
    let err = run(&mut ev, &format!("cache add file \"{}\"\n", bad.display()))
        .expect_err("free var → hard error");
    let msg = format!("{err}");
    assert!(msg.contains("unbound") || msg.contains("free"), "{msg}");
    assert!(msg.contains("k"), "should name the free var: {msg}");
}

#[test]
fn add_file_silently_skips_impure_function() {
    let dir = tempfile::tempdir().unwrap();
    let mixed = dir.path().join("mixed.rlab");
    std::fs::write(
        &mixed,
        "function y = pure(x)\n  y = x + 1\nend\n\
         function y = noisy(x)\n  y = x + rand()\nend\n",
    )
    .unwrap();
    let store = dir.path().join("m.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\ncache add file \"{}\"\n",
            store.display(),
            mixed.display()
        ),
    )
    .expect("script");

    // Both fns installed (so they can be called normally), but only
    // the pure one is cache-scoped.
    assert!(ev.is_user_fn_defined("pure"));
    assert!(ev.is_user_fn_defined("noisy"));
    assert!(ev.is_fn_cache_scoped("pure"));
    assert!(!ev.is_fn_cache_scoped("noisy"));
    assert_eq!(ev.cache_counters().impurity_skips, 1);
}

#[test]
fn add_file_with_no_functions_errors() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty.rlab");
    std::fs::write(&empty, "x = 1\n").unwrap();
    let store = dir.path().join("e.rcache");
    let mut ev = Evaluator::new();
    run(&mut ev, &format!("cache enable \"{}\"\n", store.display())).unwrap();
    let err = run(&mut ev, &format!("cache add file \"{}\"\n", empty.display()))
        .expect_err("no fns → error");
    assert!(format!("{err}").contains("no function definitions"));
}

#[test]
fn add_file_requires_active_store() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path().join("h.rlab");
    std::fs::write(&h, "function y = id(x)\n  y = x\nend\n").unwrap();
    let mut ev = Evaluator::new();
    let err = run(&mut ev, &format!("cache add file \"{}\"\n", h.display()))
        .expect_err("no store → error");
    assert!(format!("{err}").contains("no active store"));
}

// ── cache add function ──────────────────────────────────────────────

#[test]
fn add_function_explicit_mode_succeeds_for_pure_fn() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("a.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = id(x)\n  y = x\nend\n\
             cache add function id\n",
            store.display()
        ),
    )
    .expect("script");
    assert!(ev.is_fn_cache_scoped("id"));
}

#[test]
fn add_function_unknown_name_errors() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("u.rcache");
    let mut ev = Evaluator::new();
    run(&mut ev, &format!("cache enable \"{}\"\n", store.display())).unwrap();
    let err = run(&mut ev, "cache add function nope\n")
        .expect_err("undefined fn → error");
    assert!(format!("{err}").contains("not a user-defined function"));
}

#[test]
fn add_function_explicit_mode_rejects_impure() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("i.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = noisy(x)\n  y = x + rand()\nend\n",
            store.display()
        ),
    )
    .unwrap();
    let err = run(&mut ev, "cache add function noisy\n")
        .expect_err("impure in explicit mode → error");
    let msg = format!("{err}");
    assert!(msg.contains("impure"), "{msg}");
    assert!(msg.contains("rand"), "should name the builtin: {msg}");
}

// ── cache remove ────────────────────────────────────────────────────

#[test]
fn remove_function_drops_scope() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("r.rcache");
    let mut ev = Evaluator::new();
    run(
        &mut ev,
        &format!(
            "cache enable \"{}\"\n\
             function y = id(x)\n  y = x\nend\n\
             cache add function id\n",
            store.display()
        ),
    )
    .unwrap();
    assert!(ev.is_fn_cache_scoped("id"));
    run(&mut ev, "cache remove function id\n").unwrap();
    assert!(!ev.is_fn_cache_scoped("id"));
}

// ── cache clear / prune ─────────────────────────────────────────────

#[test]
fn clear_returns_zero_on_empty_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("clr.rcache");
    let mut ev = Evaluator::new();
    run(&mut ev, &format!("cache enable \"{}\"\ncache clear\n", store.display()))
        .expect("script");
}

#[test]
fn clear_without_store_errors() {
    let mut ev = Evaluator::new();
    let err = run(&mut ev, "cache clear\n").expect_err("no store → error");
    assert!(format!("{err}").contains("no active store"));
}

#[test]
fn prune_default_30d() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("p.rcache");
    let mut ev = Evaluator::new();
    run(&mut ev, &format!("cache enable \"{}\"\ncache prune\n", store.display()))
        .expect("script");
}

#[test]
fn prune_with_duration_units_parses() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("p2.rcache");
    let mut ev = Evaluator::new();
    run(&mut ev, &format!("cache enable \"{}\"\n", store.display())).unwrap();
    // Each unit should parse without error.
    for src in [
        "cache prune older=500ms\n",
        "cache prune older=30s\n",
        "cache prune older=5m\n",
        "cache prune older=12h\n",
        "cache prune older=30d\n",
        "cache prune older=2w\n",
    ] {
        run(&mut ev, src).unwrap_or_else(|e| panic!("{src}: {e}"));
    }
}

#[test]
fn prune_with_unknown_unit_errors() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("pu.rcache");
    let mut ev = Evaluator::new();
    run(&mut ev, &format!("cache enable \"{}\"\n", store.display())).unwrap();
    // `older=30q` — "q" isn't a unit. The number-bareword parser in
    // Phase 3a assembles "30q" then runtime parser rejects.
    let err = run(&mut ev, "cache prune older=\"30q\"\n")
        .expect_err("bad unit → error");
    assert!(format!("{err}").contains("unknown unit"));
}

#[test]
fn prune_max_size_runs_against_empty_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("pm.rcache");
    let mut ev = Evaluator::new();
    run(&mut ev, &format!("cache enable \"{}\"\n", store.display())).unwrap();
    // Empty store + max_size cap: should succeed without removing
    // anything. End-to-end behaviour (actually evicting oldest rows
    // when over cap) is exercised in Phase 4 CLI tests where we can
    // populate the DB through a separate process first.
    run(&mut ev, "cache prune max_size=1000\n").expect("prune max_size");
}

// ── status / counters ───────────────────────────────────────────────

#[test]
fn status_does_not_error_in_either_state() {
    let mut ev = Evaluator::new();
    // Off state.
    run(&mut ev, "cache status\n").expect("status off");
    // Active state.
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("st.rcache");
    run(&mut ev, &format!("cache enable \"{}\"\ncache status\n", store.display()))
        .expect("status active");
}

#[test]
fn counters_start_at_zero() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("c.rcache");
    let mut ev = Evaluator::new();
    run(&mut ev, &format!("cache enable \"{}\"\n", store.display())).unwrap();
    let c = ev.cache_counters();
    assert_eq!(c.hits, 0);
    assert_eq!(c.misses, 0);
    assert_eq!(c.impurity_skips, 0);
}
