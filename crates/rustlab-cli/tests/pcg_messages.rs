//! `pcg` messages, as MATLAB prints them: with fewer than two outputs a
//! converged run prints one line on stdout and a run that stops early
//! prints a warning on stderr; asking for `flag` silences both.

use std::process::Command;
use tempfile::TempDir;

fn run_script(source: &str) -> (Option<i32>, String, String) {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("script.rlab");
    std::fs::write(&path, source).expect("write script");
    let out = Command::new(env!("CARGO_BIN_EXE_rustlab"))
        .args(["run", "--plot", "none", path.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .expect("launch rustlab");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

const SETUP: &str = "A = -1 * laplacian_2d(20, 20); b = ones(400, 1);\n";

#[test]
fn maxit_warning_without_flag() {
    let (code, stdout, stderr) = run_script(&format!("{SETUP}x = pcg(A, b, 1e-12, 3);\n"));
    assert_eq!(code, Some(0));
    assert!(
        stderr.contains(
            "warning: pcg stopped at iteration 3 without converging to the desired tolerance 1e-12"
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains("because the maximum number of iterations was reached"),
        "{stderr}"
    );
    assert!(stderr.contains("The iterate returned (number "), "{stderr}");
    assert!(!stdout.contains("pcg"), "{stdout}");
}

#[test]
fn flag_output_is_silent() {
    let (code, stdout, stderr) = run_script(&format!(
        "{SETUP}[x, flag] = pcg(A, b, 1e-12, 3);\n[x, flag, relres, iter, resvec] = pcg(A, b, 1e-8, 500);\n"
    ));
    assert_eq!(code, Some(0));
    assert!(!stderr.contains("pcg"), "{stderr}");
    assert!(!stdout.contains("pcg"), "{stdout}");
}

#[test]
fn converged_message_on_stdout() {
    let (code, stdout, stderr) = run_script(&format!("{SETUP}x = pcg(A, b, 1e-8, 500);\n"));
    assert_eq!(code, Some(0));
    assert!(stdout.contains("pcg converged at iteration"), "{stdout}");
    assert!(!stderr.contains("pcg"), "{stderr}");
}

#[test]
fn breakdown_and_tolerance_warnings() {
    let (_, _, stderr) = run_script("x = pcg(sparse([1, 0; 0, -1]), [1; 1]);\n");
    assert!(
        stderr.contains("a scalar quantity became too small or too large"),
        "{stderr}"
    );
    let (_, _, stderr) = run_script(&format!("{SETUP}[x, flag] = pcg(A, b, 1e-20, 500);\n"));
    assert!(stderr.contains("smaller than eps"), "{stderr}");
    let (_, _, stderr) = run_script(&format!("{SETUP}[x, flag] = pcg(A, b, 2, 500);\n"));
    assert!(stderr.contains("bigger than 1"), "{stderr}");
}
