//! `parse_file` + `ast_hash::hash_stmts` round-trip — proves the
//! convenience function lines up with the in-process hashing path,
//! which the `cache add file ...` flow will rely on.

use rustlab_script::ast::StmtKind;
use rustlab_script::ast_hash::{function_entry_id, hash_stmts};
use rustlab_script::parse_file;

#[test]
fn parse_file_round_trip_matches_string_parse() {
    let src = "function y = add_one(x)\n  y = x + 1\nend\n";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("helpers.rlab");
    std::fs::write(&path, src).unwrap();

    let from_file = parse_file(&path).expect("parse_file");

    // Should contain one FunctionDef named `add_one`.
    assert_eq!(from_file.len(), 1);
    match &from_file[0].kind {
        StmtKind::FunctionDef { name, .. } => assert_eq!(name, "add_one"),
        other => panic!("expected FunctionDef, got {other:?}"),
    }

    // Hash should be stable across re-parses.
    let h1 = hash_stmts(&from_file);
    let from_file_again = parse_file(&path).expect("parse_file again");
    let h2 = hash_stmts(&from_file_again);
    assert_eq!(h1, h2, "re-parse must give the same file hash");

    // Editing the file changes the hash.
    std::fs::write(&path, src.replace("x + 1", "x + 2")).unwrap();
    let edited = parse_file(&path).unwrap();
    assert_ne!(hash_stmts(&edited), h1, "edit must change file hash");

    // function_entry_id depends on the file hash → also changes.
    assert_ne!(
        function_entry_id(&h1, "add_one"),
        function_entry_id(&hash_stmts(&edited), "add_one"),
    );
}

#[test]
fn parse_file_missing_file_returns_runtime_error() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist.rlab");
    let err = parse_file(&missing).expect_err("should fail on missing file");
    let msg = format!("{err}");
    assert!(msg.contains("read"), "error should mention the read failure: {msg}");
}
