//! Integration tests for the `rustlab docs` subcommand.
//!
//! These exercise the public surface (no args / one positional / --search /
//! --json / not-found) at the process boundary — same as a user would.

use std::process::Command;

fn rustlab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustlab"))
}

#[test]
fn docs_lists_categories_and_entries() {
    let out = rustlab().arg("docs").output().expect("docs runs");
    assert!(out.status.success(), "docs exited with {}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Spot-check that several known category headers are present in the output.
    for cat in ["Math", "Plotting", "Linear Algebra", "DSP"] {
        assert!(
            stdout.contains(cat),
            "expected category '{}' in docs output:\n{}",
            cat,
            stdout
        );
    }
    // And at least one well-known entry name shows up.
    assert!(stdout.contains("eig"), "expected 'eig' in docs output");
}

#[test]
fn docs_name_prints_detail() {
    let out = rustlab()
        .args(["docs", "eig"])
        .output()
        .expect("docs eig runs");
    assert!(out.status.success(), "docs eig exited with {}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Detail block contains the brief + a sample of the body content.
    assert!(stdout.contains("Eigenvalues"), "expected eig brief text");
    assert!(stdout.contains("[V, D] = eig"), "expected eig usage example");
}

#[test]
fn docs_category_lists_just_that_category() {
    let out = rustlab()
        .args(["docs", "Plotting"])
        .output()
        .expect("docs Plotting runs");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("plot"), "Plotting list should include plot");
    assert!(stdout.contains("imagesc"), "Plotting list should include imagesc");
    // Should NOT include entries from unrelated categories.
    assert!(!stdout.contains("fir_lowpass"), "Plotting list should not contain DSP entries");
}

#[test]
fn docs_unknown_topic_exits_nonzero_with_message() {
    let out = rustlab()
        .args(["docs", "definitelynotabuiltin"])
        .output()
        .expect("docs runs even on missing topic");
    assert!(!out.status.success(), "missing topic must exit nonzero");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("No help found"),
        "expected 'No help found' message:\n{}",
        stdout
    );
}

#[test]
fn docs_search_filters_by_substring() {
    let out = rustlab()
        .args(["docs", "--search", "eigen"])
        .output()
        .expect("docs --search runs");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // 'eigen' substring should match the eig brief and the sparse eigs brief.
    assert!(stdout.contains("eig"), "search 'eigen' should hit eig");
    assert!(stdout.contains("eigs"), "search 'eigen' should hit eigs");
}

#[test]
fn docs_search_no_matches_exits_nonzero() {
    let out = rustlab()
        .args(["docs", "--search", "zzzzzznosuchword"])
        .output()
        .expect("docs --search runs");
    assert!(!out.status.success(), "no-match search must exit nonzero");
}

#[test]
fn docs_json_is_valid_and_includes_known_entries() {
    let out = rustlab()
        .args(["docs", "--json"])
        .output()
        .expect("docs --json runs");
    assert!(out.status.success(), "docs --json exited with {}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("--json output must be valid JSON");
    let arr = parsed.as_array().expect("top level must be an array");
    assert!(!arr.is_empty(), "JSON dump must contain entries");
    // Verify required fields on every entry.
    for entry in arr {
        let obj = entry.as_object().expect("each entry is an object");
        for field in ["name", "brief", "detail", "category"] {
            assert!(
                obj.contains_key(field),
                "every entry must have '{}' field",
                field
            );
        }
    }
    // Spot-check a known builtin is present.
    assert!(
        arr.iter().any(|e| e.get("name").and_then(|n| n.as_str()) == Some("eig")),
        "JSON dump must contain 'eig' entry"
    );
}
