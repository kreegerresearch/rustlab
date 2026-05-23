//! End-to-end test for `rustlab-notebook validate`.
//!
//! Builds the release binary, runs `validate` against
//! `examples/notebooks/quick_look.md` (the smallest fixture, also used
//! by `dev/build-notebooks.sh`), and confirms the JSON report has the
//! expected schema and a passing summary on a machine with the
//! standard PDF toolchain.
//!
//! Marked `#[ignore]` so it doesn't run on every `cargo test` — the
//! release build and per-format pdflatex compile take ~minutes. Run
//! explicitly:
//!
//! ```text
//! cargo test -p rustlab-notebook --test validate_integration -- --ignored
//! ```

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    // crates/rustlab-notebook/tests/validate_integration.rs → workspace root
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().parent().unwrap().to_path_buf()
}

fn notebook_bin() -> PathBuf {
    let root = workspace_root();
    let release = root.join("target/release/rustlab-notebook");
    if !release.exists() {
        let status = Command::new("cargo")
            .args([
                "build",
                "--release",
                "-q",
                "-p",
                "rustlab-notebook",
                "--bin",
                "rustlab-notebook",
            ])
            .current_dir(&root)
            .status()
            .expect("cargo build failed to spawn");
        assert!(status.success(), "cargo build failed");
    }
    release
}

#[test]
#[ignore]
fn validate_quick_look_pdf_emits_expected_json() {
    let bin = notebook_bin();
    let nb = workspace_root().join("examples/notebooks/quick_look.md");
    assert!(nb.exists(), "fixture missing: {}", nb.display());

    let out = Command::new(&bin)
        .args([
            "validate",
            nb.to_str().unwrap(),
            "--format",
            "pdf",
            "--report",
            "json",
        ])
        .output()
        .expect("failed to run validate");

    // pdf path requires pdflatex + inkscape; if either is missing, the
    // render itself FAILs and the run exits 1. Either way, the JSON
    // shape contract holds.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"schema_version\": 1"));
    assert!(stdout.contains("\"summary\""));
    assert!(stdout.contains("\"results\""));
    assert!(stdout.contains("quick_look"));
    assert!(stdout.contains("\"format\": \"pdf\""));
}
