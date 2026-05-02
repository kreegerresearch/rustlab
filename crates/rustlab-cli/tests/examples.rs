/// Integration tests for non-interactive example scripts.
///
/// Each test runs `rustlab run <example>` in a temporary directory so that
/// any output files (*.svg, *.npy, *.csv, *.npz) are automatically cleaned
/// up when the TempDir is dropped.
///
/// Only examples that produce no interactive terminal UI (plot / stem /
/// plotdb / histogram) are included here; the rest require a real terminal
/// and are covered by `make examples`.
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// Absolute path to the workspace root, derived from this crate's location.
fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR == .../crates/rustlab-cli
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap() // .../crates
        .parent()
        .unwrap() // workspace root
        .to_path_buf()
}

/// Run one example script in a fresh temp directory.
/// Returns the process exit status.
fn run_example(name: &str) -> std::process::ExitStatus {
    let dir = TempDir::new().expect("failed to create temp dir");
    let script = workspace_root().join("examples").join(format!("{name}.rlab"));
    let bin = env!("CARGO_BIN_EXE_rustlab");

    Command::new(bin)
        .args(["run", script.to_str().unwrap()])
        .current_dir(dir.path()) // output files land here and are auto-deleted
        .status()
        .unwrap_or_else(|e| panic!("failed to launch rustlab for example '{name}': {e}"))
    // dir is dropped here → temp directory deleted automatically
}

fn run_example_ok(name: &str) {
    let status = run_example(name);
    assert!(status.success(), "example '{name}' exited with {status}");
}

// ── Non-interactive examples ───────────────────────────────────────────────

#[test]
fn example_complex_basics() {
    let status = run_example("complex_basics");
    assert!(
        status.success(),
        "example 'complex_basics' exited with {status}"
    );
}

#[test]
fn example_save_load() {
    let status = run_example("save_load");
    assert!(status.success(), "example 'save_load' exited with {status}");
}

#[test]
fn example_firpm() {
    run_example_ok("firpm");
}

#[test]
fn example_ml_activations() {
    run_example_ok("ml_activations");
}

#[test]
fn example_matrix_ops() {
    run_example_ok("matrix_ops");
}

#[test]
fn example_stats() {
    run_example_ok("stats");
}

#[test]
fn example_trig_special() {
    run_example_ok("trig_special");
}

#[test]
fn example_fixed_point() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let script = workspace_root().join("examples").join("fixed_point.rlab");
    let bin = env!("CARGO_BIN_EXE_rustlab");

    let output = Command::new(bin)
        .args(["run", script.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .expect("failed to launch rustlab for example 'fixed_point'");

    assert!(
        output.status.success(),
        "fixed_point exited with {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    // Parse the 5 SNR values printed after each "SNR (dB):" label line.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    let snr_values: Vec<f64> = lines
        .windows(2)
        .filter(|w| w[0].contains("SNR (dB)"))
        .map(|w| {
            w[1].trim()
                .parse::<f64>()
                .unwrap_or_else(|_| panic!("could not parse SNR value from: {:?}", w[1]))
        })
        .collect();

    assert_eq!(
        snr_values.len(),
        5,
        "expected 5 SNR values, got {}; stdout:\n{}",
        snr_values.len(),
        stdout
    );

    // SNR must strictly increase with bitwidth (8 → 10 → 12 → 14 → 16 bit).
    for (i, w) in snr_values.windows(2).enumerate() {
        assert!(
            w[1] > w[0],
            "SNR not monotonically increasing at step {i}: {:.1} → {:.1} dB\nAll: {snr_values:?}",
            w[0],
            w[1]
        );
    }

    // Loose absolute bounds: 8-bit ~30 dB, 16-bit ~74 dB.
    assert!(
        snr_values[0] > 20.0 && snr_values[0] < 45.0,
        "8-bit SNR out of expected range [20, 45] dB: {:.1}",
        snr_values[0]
    );
    assert!(
        snr_values[4] > 60.0,
        "16-bit SNR below expected floor of 60 dB: {:.1}",
        snr_values[4]
    );
}

// ── Non-interactive examples (no terminal plot calls) ─────────────────────

#[test]
fn example_functions() {
    run_example_ok("functions");
}
#[test]
fn example_lambda() {
    run_example_ok("lambda");
}
#[test]
fn example_lambda_pipeline() {
    run_example_ok("lambda_pipeline");
}
#[test]
fn example_profiling() {
    run_example_ok("profiling");
}
#[test]
fn example_upfirdn() {
    run_example_ok("upfirdn");
}
#[test]
fn example_vectors() {
    run_example_ok("vectors");
}

// ── Plot-producing examples (render_figure_terminal is a no-op under `cargo
//    test` because stdout is not a TTY; savefig writes into the temp dir and
//    is cleaned up automatically when the TempDir drops) ────────────────────
#[test]
fn example_bandpass() {
    run_example_ok("bandpass");
}
#[test]
fn example_fft() {
    run_example_ok("fft");
}
#[test]
fn example_kaiser_fir() {
    run_example_ok("kaiser_fir");
}
#[test]
fn example_lowpass() {
    run_example_ok("lowpass");
}
#[test]
fn example_multi_figure() {
    run_example_ok("multi_figure");
}
#[test]
fn example_random() {
    run_example_ok("random");
}
#[test]
fn example_toml_filter_chain() {
    run_example_ok("toml_filter_chain");
}
#[test]
fn example_toml_io() {
    run_example_ok("toml_io");
}

// ── Interpreter banner ─────────────────────────────────────────────────────
//
// Locks in the always-on stderr banner that explicitly identifies rustlab as
// the handler for `.rlab` files. The banner is emitted from
// `commands/run::execute` before script evaluation begins.

#[test]
fn run_banner_emits_rustlab_identifier_to_stderr() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let script = workspace_root().join("examples").join("eig.rlab");
    let bin = env!("CARGO_BIN_EXE_rustlab");

    let output = Command::new(bin)
        .args(["run", script.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .expect("failed to launch rustlab for banner test");

    assert!(
        output.status.success(),
        "banner test: rustlab run failed with {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Single line-anchored check: catches truncation, missing version, or
    // word-reorder regressions that the looser `contains` checks would miss.
    let banner_line_present = stderr.lines().any(|line| {
        line.starts_with("rustlab ")
            && line.contains(" — interpreting ")
            && line.ends_with(" (.rlab)")
    });
    assert!(
        banner_line_present,
        "banner not found or mangled in stderr — expected a line of the form\n  \
         `rustlab <version> — interpreting <path> (.rlab)`\nactual stderr:\n{}",
        stderr
    );
}
