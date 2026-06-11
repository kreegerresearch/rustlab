/// Integration tests for the rustlab CLI subcommands: filter, convolve,
/// window, plot, info — plus the `run` error paths (syntax / runtime).
///
/// Style mirrors tests/examples.rs: each test spawns the binary via
/// CARGO_BIN_EXE_rustlab in a fresh TempDir so any input fixtures are
/// auto-cleaned when the TempDir drops. Everything here runs headless —
/// plot-producing paths are no-ops because stdout is not a TTY under
/// `cargo test`.
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

/// Spawn `rustlab <args...>` in `dir` and capture stdout/stderr.
fn run_in(dir: &Path, args: &[&str]) -> Output {
    let bin = env!("CARGO_BIN_EXE_rustlab");
    Command::new(bin)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to launch rustlab {args:?}: {e}"))
}

/// Spawn `rustlab <args...>` in a throwaway temp dir (no fixtures needed).
fn run(args: &[&str]) -> Output {
    let dir = TempDir::new().expect("failed to create temp dir");
    run_in(dir.path(), args)
}

fn stdout_str(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_str(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Assert the command failed and printed `needle` on stderr.
fn assert_error_contains(out: &Output, needle: &str) {
    assert!(
        !out.status.success(),
        "expected non-zero exit, got {}\nstdout: {}\nstderr: {}",
        out.status,
        stdout_str(out),
        stderr_str(out)
    );
    let stderr = stderr_str(out);
    assert!(
        stderr.contains(needle),
        "stderr missing {needle:?}; actual stderr:\n{stderr}"
    );
}

/// Write a small CSV fixture into `dir` and return nothing (path is `name`).
fn write_fixture(dir: &Path, name: &str, contents: &str) {
    std::fs::write(dir.join(name), contents)
        .unwrap_or_else(|e| panic!("failed to write fixture {name}: {e}"));
}

// ── filter ─────────────────────────────────────────────────────────────────

#[test]
fn filter_fir_lowpass_prints_header_and_taps() {
    let out = run(&["filter", "fir", "--taps", "8", "--cutoff", "1000"]);
    assert!(out.status.success(), "exit {}: {}", out.status, stderr_str(&out));

    let stdout = stdout_str(&out);
    assert!(
        stdout.contains("# FIR low filter: 8 taps, cutoff=1000 Hz, sr=44100 Hz, window=hann"),
        "header line missing; stdout:\n{stdout}"
    );
    // Exactly 8 coefficient lines, h[0]..h[7].
    let coeff_lines: Vec<&str> = stdout.lines().filter(|l| l.starts_with("h[")).collect();
    assert_eq!(coeff_lines.len(), 8, "expected 8 taps; stdout:\n{stdout}");
    // Deterministic spot check (Hann-windowed sinc, tap 1).
    assert!(
        stdout.contains("h[  1] = +0.05294145 + +0.00000000j"),
        "tap h[1] value drifted; stdout:\n{stdout}"
    );
}

#[test]
fn filter_fir_bandpass_requires_cutoff_high() {
    let out = run(&["filter", "fir", "--cutoff", "1000", "--type", "band"]);
    assert_error_contains(&out, "--cutoff-high is required for bandpass filter");
}

#[test]
fn filter_fir_rejects_unknown_type() {
    let out = run(&["filter", "fir", "--cutoff", "1000", "--type", "notch"]);
    assert_error_contains(&out, "unknown filter type 'notch'");
}

#[test]
fn filter_iir_lowpass_prints_b_and_a_coefficients() {
    let out = run(&["filter", "iir", "--order", "2", "--cutoff", "1000"]);
    assert!(out.status.success(), "exit {}: {}", out.status, stderr_str(&out));

    let stdout = stdout_str(&out);
    assert!(
        stdout.contains("# IIR Butterworth low filter: order=2, cutoff=1000 Hz, sr=44100 Hz"),
        "header line missing; stdout:\n{stdout}"
    );
    // 2nd-order biquad: 3 numerator + 3 denominator coefficients.
    let b_lines = stdout.lines().filter(|l| l.starts_with("b[")).count();
    let a_lines = stdout.lines().filter(|l| l.starts_with("a[")).count();
    assert_eq!((b_lines, a_lines), (3, 3), "stdout:\n{stdout}");
    // Denominator is normalized: a[0] == 1.
    assert!(
        stdout.contains("a[  0] = +1.0000000000"),
        "a[0] not normalized to 1; stdout:\n{stdout}"
    );
}

#[test]
fn filter_iir_rejects_order_zero() {
    let out = run(&["filter", "iir", "--order", "0", "--cutoff", "1000"]);
    assert_error_contains(&out, "invalid filter order: 0");
}

// ── convolve ───────────────────────────────────────────────────────────────

#[test]
fn convolve_direct_computes_full_convolution() {
    let dir = TempDir::new().expect("failed to create temp dir");
    write_fixture(dir.path(), "sig.csv", "1\n2\n3\n");
    write_fixture(dir.path(), "ker.csv", "1\n1\n");

    let out = run_in(
        dir.path(),
        &["convolve", "--signal", "sig.csv", "--kernel", "ker.csv"],
    );
    assert!(out.status.success(), "exit {}: {}", out.status, stderr_str(&out));

    // [1,2,3] * [1,1] = [1,3,5,3], printed as "re,im" per line.
    let expected = "\
+1.0000000000,+0.0000000000
+3.0000000000,+0.0000000000
+5.0000000000,+0.0000000000
+3.0000000000,+0.0000000000
";
    assert_eq!(stdout_str(&out), expected);
}

#[test]
fn convolve_overlap_add_matches_direct_result() {
    let dir = TempDir::new().expect("failed to create temp dir");
    write_fixture(dir.path(), "sig.csv", "1\n2\n3\n");
    write_fixture(dir.path(), "ker.csv", "1\n1\n");

    let out = run_in(
        dir.path(),
        &[
            "convolve",
            "--signal",
            "sig.csv",
            "--kernel",
            "ker.csv",
            "--method",
            "overlap-add",
        ],
    );
    assert!(out.status.success(), "exit {}: {}", out.status, stderr_str(&out));

    // FFT-based path: compare numerically (it may print -0.0000000000 for im).
    let stdout = stdout_str(&out);
    let values: Vec<(f64, f64)> = stdout
        .lines()
        .map(|l| {
            let (re, im) = l.split_once(',').unwrap_or_else(|| panic!("bad line: {l:?}"));
            (re.parse().unwrap(), im.parse().unwrap())
        })
        .collect();
    let expected = [(1.0, 0.0), (3.0, 0.0), (5.0, 0.0), (3.0, 0.0)];
    assert_eq!(values.len(), expected.len(), "stdout:\n{stdout}");
    for (i, ((re, im), (ere, eim))) in values.iter().zip(expected.iter()).enumerate() {
        assert!(
            (re - ere).abs() < 1e-9 && (im - eim).abs() < 1e-9,
            "sample {i}: got ({re}, {im}), expected ({ere}, {eim})"
        );
    }
}

#[test]
fn convolve_missing_signal_file_is_an_error() {
    let out = run(&["convolve", "--signal", "nope.csv", "--kernel", "also_nope.csv"]);
    assert_error_contains(&out, "failed to read \"nope.csv\"");
}

#[test]
fn convolve_rejects_non_numeric_input_with_line_number() {
    let dir = TempDir::new().expect("failed to create temp dir");
    write_fixture(dir.path(), "sig.csv", "1\nabc\n");
    write_fixture(dir.path(), "ker.csv", "1\n");

    let out = run_in(
        dir.path(),
        &["convolve", "--signal", "sig.csv", "--kernel", "ker.csv"],
    );
    assert_error_contains(&out, "line 2: cannot parse 'abc' as f64");
}

#[test]
fn convolve_rejects_unknown_method() {
    let dir = TempDir::new().expect("failed to create temp dir");
    write_fixture(dir.path(), "sig.csv", "1\n");
    write_fixture(dir.path(), "ker.csv", "1\n");

    let out = run_in(
        dir.path(),
        &[
            "convolve",
            "--signal",
            "sig.csv",
            "--kernel",
            "ker.csv",
            "--method",
            "magic",
        ],
    );
    assert_error_contains(&out, "unknown method 'magic': use direct or overlap-add");
}

// ── window ─────────────────────────────────────────────────────────────────

#[test]
fn window_hann_prints_expected_samples() {
    let out = run(&["window", "--type", "hann", "--length", "4"]);
    assert!(out.status.success(), "exit {}: {}", out.status, stderr_str(&out));

    let expected = "\
w[   0] = +0.0000000000
w[   1] = +0.7500000000
w[   2] = +0.7500000000
w[   3] = +0.0000000000
";
    assert_eq!(stdout_str(&out), expected);
}

#[test]
fn window_rejects_unknown_type() {
    let out = run(&["window", "--type", "bogus", "--length", "8"]);
    assert_error_contains(&out, "unknown window: bogus");
}

#[test]
fn window_plot_flag_is_headless_safe() {
    // --plot must not hang or open a TUI when stdout is not a terminal;
    // the samples are still printed and the plot call is a silent no-op.
    let out = run(&["window", "--type", "hamming", "--length", "8", "--plot"]);
    assert!(out.status.success(), "exit {}: {}", out.status, stderr_str(&out));
    let samples = stdout_str(&out).lines().filter(|l| l.starts_with("w[")).count();
    assert_eq!(samples, 8, "stdout:\n{}", stdout_str(&out));
}

// ── plot ───────────────────────────────────────────────────────────────────

#[test]
fn plot_line_is_silent_noop_without_a_tty() {
    let dir = TempDir::new().expect("failed to create temp dir");
    write_fixture(dir.path(), "sig.csv", "1\n2\n3\n");

    let out = run_in(dir.path(), &["plot", "--input", "sig.csv"]);
    assert!(out.status.success(), "exit {}: {}", out.status, stderr_str(&out));
    // Under cargo test stdout is not a TTY, so rendering is skipped entirely.
    assert!(out.stdout.is_empty(), "unexpected stdout: {}", stdout_str(&out));
    assert!(out.stderr.is_empty(), "unexpected stderr: {}", stderr_str(&out));
}

#[test]
fn plot_rejects_unknown_type() {
    let dir = TempDir::new().expect("failed to create temp dir");
    write_fixture(dir.path(), "sig.csv", "1\n2\n");

    let out = run_in(dir.path(), &["plot", "--input", "sig.csv", "--type", "pie"]);
    assert_error_contains(&out, "unknown plot type 'pie': use line or stem");
}

#[test]
fn plot_missing_input_file_is_an_error() {
    let out = run(&["plot", "--input", "nope.csv"]);
    assert_error_contains(&out, "failed to read \"nope.csv\"");
}

// ── info ───────────────────────────────────────────────────────────────────

#[test]
fn info_prints_version_and_usage_summary() {
    let out = run(&["info"]);
    assert!(out.status.success(), "exit {}: {}", out.status, stderr_str(&out));

    let stdout = stdout_str(&out);
    let first = stdout.lines().next().unwrap_or("");
    // `info` and the test binary are built from the same crate, so the
    // version must match this crate's version exactly.
    assert_eq!(
        first,
        format!("rustlab {}", env!("CARGO_PKG_VERSION")),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("DSP toolkit"),
        "usage summary missing; stdout:\n{stdout}"
    );
}

// ── run: error reporting ───────────────────────────────────────────────────
//
// Note: `rustlab run` currently exits 0 even when the script fails to parse
// or hits a runtime error — `run_script_source` prints the diagnostic to
// stderr and swallows the error. These tests therefore lock in the stderr
// contract only; if exit codes are ever fixed to be non-zero, tighten them.

#[test]
fn run_syntax_error_reports_error_on_stderr() {
    let dir = TempDir::new().expect("failed to create temp dir");
    write_fixture(dir.path(), "broken.rlab", "x = [1, 2, 3\n");

    let out = run_in(dir.path(), &["run", "broken.rlab"]);
    let stderr = stderr_str(&out);
    assert!(
        stderr.lines().any(|l| l.starts_with("error:") && l.contains("parse error")),
        "expected an `error: ... parse error ...` line on stderr; actual stderr:\n{stderr}"
    );
    // Script produced no output before the parse failure.
    assert!(out.stdout.is_empty(), "unexpected stdout: {}", stdout_str(&out));
}

#[test]
fn run_runtime_error_reports_line_number_on_stderr() {
    let dir = TempDir::new().expect("failed to create temp dir");
    write_fixture(
        dir.path(),
        "oob.rlab",
        "v = [1, 2, 3];\nprintln(v(10));\n",
    );

    let out = run_in(dir.path(), &["run", "oob.rlab"]);
    let stderr = stderr_str(&out);
    assert!(
        stderr.contains("error: line 2:"),
        "expected `error: line 2:` on stderr; actual stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("index 10 out of bounds"),
        "expected out-of-bounds message on stderr; actual stderr:\n{stderr}"
    );
}
