//! `rustlab run` profiler output: the in-script `profile()` reports at exit,
//! and `--profile` / `--profile=ops` choose what is recorded.

use std::process::Command;
use tempfile::TempDir;

const SCRIPT: &str = "function y = inner(x)\n  y = x * x';\nend\n\
                      function y = outer(x)\n  y = inner(x) + 1;\nend\n";

/// Run `body` (appended to `SCRIPT`) with extra `run` flags; return stderr.
fn stderr_of(flags: &[&str], body: &str) -> String {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("prof.rlab");
    std::fs::write(&path, format!("{SCRIPT}{body}")).expect("write script");
    let mut args = vec!["run"];
    args.extend_from_slice(flags);
    args.push(path.to_str().unwrap());
    let out = Command::new(env!("CARGO_BIN_EXE_rustlab"))
        .args(&args)
        .current_dir(dir.path())
        .output()
        .expect("launch rustlab");
    assert!(out.status.success(), "rustlab failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn has_row(stderr: &str, name: &str) -> bool {
    stderr.lines().any(|l| l.split_whitespace().next() == Some(name))
}

#[test]
fn in_script_profile_reports_at_exit() {
    let err = stderr_of(&[], "profile()\nA = randn(16, 16);\nz = outer(A);\n");
    assert!(err.contains("Self (ms)"), "no report on stderr:\n{err}");
    for row in ["outer", "inner", "[matmul]", "[top-level]", "TOTAL"] {
        assert!(has_row(&err, row), "missing {row}:\n{err}");
    }
}

#[test]
fn bare_profile_flag_still_takes_the_script_path() {
    let err = stderr_of(&["--profile"], "A = randn(16, 16);\nz = outer(A);\n");
    assert!(has_row(&err, "inner") && has_row(&err, "[matmul]"), "{err}");
    assert!(!has_row(&err, "[+]"), "op rows need --profile=ops:\n{err}");
}

#[test]
fn profile_ops_adds_operator_rows() {
    // `x * x'` multiplies by a transposed view, so the transpose row comes
    // from the standalone `A'`.
    let err = stderr_of(&["--profile=ops"], "A = randn(16, 16);\nz = outer(A);\nT = A';\n");
    for row in ["[+]", "[transpose]", "[matmul]", "inner"] {
        assert!(has_row(&err, row), "missing {row}:\n{err}");
    }
}

#[test]
fn matmul_row_prints_throughput() {
    // 32×32 real matrices: two inputs are 16.00 KB, the product is 8.00 KB.
    let err = stderr_of(&[], "profile()\nA = rand(32, 32);\nB = rand(32, 32);\nC = A * B;\n");
    let line = err
        .lines()
        .find(|l| l.split_whitespace().next() == Some("[matmul]"))
        .unwrap_or_else(|| panic!("missing [matmul]:\n{err}"));
    let cols: Vec<_> = line.split_whitespace().collect();
    assert_eq!(cols.len(), 8, "{line}");
    assert_eq!(cols[5], "16.00", "In (KB): {line}");
    assert_eq!(cols[6], "8.00", "Out (KB): {line}");
    let mbit: f64 = cols[7].parse().expect(line);
    assert!(mbit > 0.0, "Mbit/s was 0: {line}");
}

#[test]
fn no_report_without_profiling() {
    let err = stderr_of(&[], "z = outer(ones(4, 4));\n");
    assert!(!err.contains("Self (ms)"), "unexpected report:\n{err}");
}
