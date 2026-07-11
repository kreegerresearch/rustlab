//! Exit-code contract for `rustlab run`.
//!
//! A failing script must exit nonzero so CI / make targets can gate on
//! it — previously the error text was printed but the process exited 0,
//! which let broken scripts pass silently through pipelines.

use std::process::Command;
use tempfile::TempDir;

/// Write `source` to a temp .rlab file and run it, returning the exit code.
fn run_script(source: &str) -> Option<i32> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("script.rlab");
    std::fs::write(&path, source).expect("write script");
    Command::new(env!("CARGO_BIN_EXE_rustlab"))
        .args(["run", path.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .expect("launch rustlab")
        .status
        .code()
}

#[test]
fn happy_script_exits_zero() {
    assert_eq!(run_script("x = 1 + 1;\nprint(x)\n"), Some(0));
}

#[test]
fn runtime_error_exits_one() {
    // Undefined variable → runtime error mid-script.
    assert_eq!(run_script("x = 1;\ny = undefined_variable_xyz + 1;\n"), Some(1));
}

#[test]
fn error_builtin_exits_one() {
    assert_eq!(run_script("error(\"boom\")\n"), Some(1));
}

#[test]
fn syntax_error_exits_one() {
    assert_eq!(run_script("x = = 1\n"), Some(1));
}

#[test]
fn error_after_output_still_exits_one() {
    // The failure happens after successful statements — the exit code
    // must still reflect it.
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("late_fail.rlab");
    std::fs::write(&path, "print(42)\nerror(\"late\")\n").expect("write script");
    let out = Command::new(env!("CARGO_BIN_EXE_rustlab"))
        .args(["run", path.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .expect("launch rustlab");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("42"), "pre-error output still printed: {stdout}");
}

#[test]
fn missing_script_exits_nonzero() {
    let dir = TempDir::new().expect("temp dir");
    let code = Command::new(env!("CARGO_BIN_EXE_rustlab"))
        .args(["run", "definitely_missing_file.rlab"])
        .current_dir(dir.path())
        .output()
        .expect("launch rustlab")
        .status
        .code();
    assert_ne!(code, Some(0), "missing script must not exit 0");
}

#[test]
fn profile_mode_failing_script_exits_nonzero() {
    // --profile already exited nonzero via anyhow; lock the behavior in.
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("prof_fail.rlab");
    std::fs::write(&path, "error(\"boom\")\n").expect("write script");
    let code = Command::new(env!("CARGO_BIN_EXE_rustlab"))
        .args(["run", "--profile", path.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .expect("launch rustlab")
        .status
        .code();
    assert_ne!(code, Some(0));
}
