//! "Not rendered to the terminal" warnings are deferred and
//! savefig-aware: a scripted `quiver(...); savefig(...)` run must be
//! silent, while a vector plot that never reaches a file still warns —
//! once, at the end of the run, as one combined line.

use std::process::Command;
use tempfile::TempDir;

fn run_script(source: &str) -> String {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("script.rlab");
    std::fs::write(&path, source).expect("write script");
    let out = Command::new(env!("CARGO_BIN_EXE_rustlab"))
        .args(["run", path.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .expect("launch rustlab");
    assert!(out.status.success(), "script failed: {:?}", out);
    String::from_utf8_lossy(&out.stderr).into_owned()
}

const FIELD: &str = "U = [1, 2; 3, 4];\nV = [4, 3; 2, 1];\n";

#[test]
fn quiver_followed_by_savefig_is_silent() {
    let stderr = run_script(&format!("{FIELD}quiver(U, V);\nsavefig(\"q.svg\");\n"));
    assert!(
        !stderr.contains("not rendered"),
        "savefig consumed the figure — no warning expected, got:\n{stderr}"
    );
}

#[test]
fn quiver_without_savefig_warns_exactly_once() {
    let stderr = run_script(&format!("{FIELD}quiver(U, V);\nquiver(V, U);\n"));
    assert_eq!(
        stderr.matches("not rendered").count(),
        1,
        "expected exactly one deferred warning, got:\n{stderr}"
    );
    assert!(stderr.contains("quiver"), "warning names the plot kind:\n{stderr}");
}

#[test]
fn multiple_kinds_warn_in_one_combined_line() {
    let stderr = run_script(&format!(
        "{FIELD}quiver(U, V);\ncontour(U);\nstreamplot(U, V);\n"
    ));
    assert_eq!(
        stderr.matches("not rendered").count(),
        1,
        "one combined line for all kinds:\n{stderr}"
    );
    for kind in ["contour", "quiver", "streamplot"] {
        assert!(stderr.contains(kind), "missing {kind} in:\n{stderr}");
    }
}

#[test]
fn savefig_after_some_plots_clears_all_pending_kinds() {
    // contour + quiver both pending, then one savefig consumes the
    // figure — nothing left to warn about.
    let stderr = run_script(&format!(
        "{FIELD}contour(U);\nquiver(U, V);\nsavefig(\"both.svg\");\n"
    ));
    assert!(!stderr.contains("not rendered"), "got:\n{stderr}");
}

#[test]
fn plot_after_last_savefig_still_warns() {
    // The save consumed the first figure; the quiver drawn afterwards
    // never reached a file and must still warn.
    let stderr = run_script(&format!(
        "{FIELD}quiver(U, V);\nsavefig(\"q.svg\");\nstreamplot(U, V);\n"
    ));
    assert_eq!(stderr.matches("not rendered").count(), 1, "got:\n{stderr}");
    assert!(stderr.contains("streamplot"), "got:\n{stderr}");
    assert!(
        !stderr.contains("quiver,"),
        "the saved quiver must not reappear in the warning:\n{stderr}"
    );
}
