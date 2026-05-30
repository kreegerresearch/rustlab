//! Phase 3a: grammar tests for the `cache` statement. Drives the
//! full lex → parse pipeline and inspects the resulting `CacheStmt`
//! shape. No runtime — the evaluator deliberately errors on the new
//! variant until Phase 3b/c lands.

use rustlab_script::ast::{CacheStmt, Stmt, StmtKind};
use rustlab_script::lexer::tokenize;
use rustlab_script::parser::parse;

fn parse_one(src: &str) -> CacheStmt {
    let stmts = parse(tokenize(src).expect("tokenize")).expect("parse");
    assert_eq!(stmts.len(), 1, "expected exactly one top-level stmt: {stmts:?}");
    match &stmts[0].kind {
        StmtKind::Cache(c) => c.clone(),
        other => panic!("expected StmtKind::Cache, got {other:?}"),
    }
}

fn parse_err(src: &str) -> String {
    let toks = match tokenize(src) {
        Ok(t) => t,
        Err(e) => return format!("{e}"),
    };
    match parse(toks) {
        Ok(stmts) => panic!("expected parse error, got {stmts:?}"),
        Err(e) => format!("{e}"),
    }
}

// ── cache enable ────────────────────────────────────────────────────

#[test]
fn enable_with_no_args_opens_default_store() {
    let c = parse_one("cache enable\n");
    assert!(matches!(c, CacheStmt::Enable { path: None }));
}

#[test]
fn enable_with_quoted_path() {
    let c = parse_one("cache enable \"my_cache.rcache\"\n");
    match c {
        CacheStmt::Enable { path } => {
            assert_eq!(path.as_deref(), Some("my_cache.rcache"));
        }
        _ => panic!("expected Enable"),
    }
}

#[test]
fn enable_with_bareword_path() {
    let c = parse_one("cache enable my_cache.rcache\n");
    match c {
        CacheStmt::Enable { path } => {
            assert_eq!(path.as_deref(), Some("my_cache.rcache"));
        }
        _ => panic!("expected Enable"),
    }
}

// ── sugar: `cache "path"` and `cache path` ──────────────────────────

#[test]
fn sugar_quoted_path_means_enable() {
    let c = parse_one("cache \"sugar.rcache\"\n");
    match c {
        CacheStmt::Enable { path } => {
            assert_eq!(path.as_deref(), Some("sugar.rcache"));
        }
        _ => panic!("expected Enable from sugar"),
    }
}

#[test]
fn sugar_bareword_path_means_enable() {
    let c = parse_one("cache foo.rcache\n");
    match c {
        CacheStmt::Enable { path } => {
            assert_eq!(path.as_deref(), Some("foo.rcache"));
        }
        _ => panic!("expected Enable from sugar"),
    }
}

// ── cache off ───────────────────────────────────────────────────────

#[test]
fn off_parses() {
    let c = parse_one("cache off\n");
    assert!(matches!(c, CacheStmt::Off));
}

// ── cache add ───────────────────────────────────────────────────────

#[test]
fn add_file_quoted_path() {
    let c = parse_one("cache add file \"helpers.rlab\"\n");
    match c {
        CacheStmt::AddFile { path } => assert_eq!(path, "helpers.rlab"),
        _ => panic!("expected AddFile"),
    }
}

#[test]
fn add_file_bareword_path() {
    let c = parse_one("cache add file helpers.rlab\n");
    match c {
        CacheStmt::AddFile { path } => assert_eq!(path, "helpers.rlab"),
        _ => panic!("expected AddFile"),
    }
}

#[test]
fn add_file_requires_path() {
    let err = parse_err("cache add file\n");
    assert!(err.contains("path"), "{err}");
}

#[test]
fn add_function_single() {
    let c = parse_one("cache add function expensive\n");
    match c {
        CacheStmt::AddFunctions { names } => assert_eq!(names, vec!["expensive".to_string()]),
        _ => panic!("expected AddFunctions"),
    }
}

#[test]
fn add_function_comma_list() {
    let c = parse_one("cache add function f1, f2, f3\n");
    match c {
        CacheStmt::AddFunctions { names } => {
            assert_eq!(names, vec!["f1".to_string(), "f2".to_string(), "f3".to_string()]);
        }
        _ => panic!("expected AddFunctions"),
    }
}

#[test]
fn add_function_requires_at_least_one_name() {
    let err = parse_err("cache add function\n");
    assert!(err.contains("function"), "{err}");
}

#[test]
fn add_requires_file_or_function() {
    let err = parse_err("cache add nope\n");
    assert!(err.contains("file") || err.contains("function"), "{err}");
}

// ── cache remove ────────────────────────────────────────────────────

#[test]
fn remove_function_parses() {
    let c = parse_one("cache remove function expensive\n");
    match c {
        CacheStmt::RemoveFunction { name } => assert_eq!(name, "expensive"),
        _ => panic!("expected RemoveFunction"),
    }
}

#[test]
fn remove_requires_function_keyword() {
    let err = parse_err("cache remove expensive\n");
    assert!(err.contains("function"), "{err}");
}

// ── cache status / clear ────────────────────────────────────────────

#[test]
fn status_parses() {
    let c = parse_one("cache status\n");
    assert!(matches!(c, CacheStmt::Status));
}

#[test]
fn clear_parses() {
    let c = parse_one("cache clear\n");
    assert!(matches!(c, CacheStmt::Clear));
}

// ── cache prune ─────────────────────────────────────────────────────

#[test]
fn prune_no_kwargs() {
    let c = parse_one("cache prune\n");
    assert!(matches!(
        c,
        CacheStmt::Prune {
            older: None,
            max_size_bytes: None
        }
    ));
}

#[test]
fn prune_older_quoted_string() {
    let c = parse_one("cache prune older=\"30d\"\n");
    match c {
        CacheStmt::Prune { older, .. } => assert_eq!(older.as_deref(), Some("30d")),
        _ => panic!("expected Prune"),
    }
}

#[test]
fn prune_older_bareword_number_unit() {
    // `older=30d` lexes as Number(30) Ident("d"). The duration parser
    // re-assembles them into "30d".
    let c = parse_one("cache prune older=30d\n");
    match c {
        CacheStmt::Prune { older, .. } => assert_eq!(older.as_deref(), Some("30d")),
        _ => panic!("expected Prune"),
    }
}

#[test]
fn prune_max_size() {
    let c = parse_one("cache prune max_size=500000\n");
    match c {
        CacheStmt::Prune { max_size_bytes, .. } => assert_eq!(max_size_bytes, Some(500_000)),
        _ => panic!("expected Prune"),
    }
}

#[test]
fn prune_both_kwargs() {
    let c = parse_one("cache prune older=\"30d\" max_size=1000\n");
    match c {
        CacheStmt::Prune {
            older,
            max_size_bytes,
        } => {
            assert_eq!(older.as_deref(), Some("30d"));
            assert_eq!(max_size_bytes, Some(1000));
        }
        _ => panic!("expected Prune"),
    }
}

// ── cache list ──────────────────────────────────────────────────────

#[test]
fn list_with_no_args() {
    let c = parse_one("cache list\n");
    assert!(matches!(c, CacheStmt::List { limit: None }));
}

#[test]
fn list_with_limit_kwarg() {
    let c = parse_one("cache list limit=10\n");
    match c {
        CacheStmt::List { limit } => assert_eq!(limit, Some(10)),
        _ => panic!("expected List"),
    }
}

// ── strict-mode unknown subcommand errors ──────────────────────────

#[test]
fn unknown_subcommand_errors_with_helpful_message() {
    // `cache list` was the canonical foot-gun — previously silently
    // misparsed as `cache enable list`, opening a new store named
    // `list`. We still test the broader rule via a totally unrelated
    // bareword so the test isn't sensitive to which subcommands exist.
    let err = parse_err("cache nopecmd\n");
    assert!(err.contains("unknown subcommand"), "{err}");
    assert!(err.contains("nopecmd"), "{err}");
    // The error should point the user toward the explicit forms.
    assert!(err.contains("enable"), "{err}");
}

#[test]
fn bareword_path_with_extension_still_works_as_sugar() {
    // Path-shaped sugar must keep working — `foo.rcache` has a `.`
    // so the parser recognises it as a path and allows the sugar.
    let c = parse_one("cache foo.rcache\n");
    match c {
        CacheStmt::Enable { path } => assert_eq!(path.as_deref(), Some("foo.rcache")),
        _ => panic!("expected Enable from sugar"),
    }
}

#[test]
fn quoted_path_sugar_is_unambiguous() {
    // Quoted strings always mean a path — no disambiguation needed.
    let c = parse_one("cache \"plainstore\"\n");
    match c {
        CacheStmt::Enable { path } => assert_eq!(path.as_deref(), Some("plainstore")),
        _ => panic!("expected Enable"),
    }
}

// ── general errors ──────────────────────────────────────────────────

#[test]
fn bare_cache_alone_errors() {
    let err = parse_err("cache\n");
    assert!(err.contains("subcommand") || err.contains("path"), "{err}");
}

// ── interaction with surrounding statements ─────────────────────────

#[test]
fn multiple_cache_statements_in_a_program() {
    let src = "\
cache enable
cache add file helpers.rlab
cache add function expensive
cache status
cache off
";
    let stmts: Vec<Stmt> = parse(tokenize(src).unwrap()).unwrap();
    assert_eq!(stmts.len(), 5);
    for s in &stmts {
        assert!(matches!(s.kind, StmtKind::Cache(_)), "got {s:?}");
    }
}

#[test]
fn cache_in_a_notebook_style_script_with_normal_code() {
    // Realistic top-of-notebook usage.
    let src = "\
cache enable
x = 1:10
y = sum(x);
";
    let stmts: Vec<Stmt> = parse(tokenize(src).unwrap()).unwrap();
    assert_eq!(stmts.len(), 3);
    assert!(matches!(stmts[0].kind, StmtKind::Cache(_)));
}
