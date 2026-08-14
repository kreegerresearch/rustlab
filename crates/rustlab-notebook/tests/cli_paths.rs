//! CLI-level path-resolution checks that need a real process with its own
//! cwd — in-process tests race on the process-global cwd with parallel
//! renders, and `cmd_render`'s error paths call `process::exit`.

use std::path::Path;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustlab-notebook"))
}

#[test]
fn render_single_file_honours_a_relative_output_path() {
    // A relative `-o` used to resolve AFTER the chdir to the notebook's
    // parent — output landed inside the source dir while the summary
    // printed the cwd-relative path the user asked for.
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("note.md"), "# Note\n\nprose\n").unwrap();

    let status = bin()
        .current_dir(dir.path())
        .args(["render", "src/note.md", "-o", "out/note.html"])
        .status()
        .expect("failed to run rustlab-notebook");
    assert!(status.success(), "render exited with {status}");

    assert!(
        dir.path().join("out/note.html").is_file(),
        "output must resolve against the invoking cwd"
    );
    assert!(
        !src_dir.join("out").exists(),
        "output landed inside the source dir"
    );
}

#[test]
fn render_dir_relative_output_resolves_against_cwd() {
    // The directory form already absolutized; pin it so the two entry
    // points can't drift apart again.
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("nb");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("a.md"), "# A\n\nprose\n").unwrap();

    let status = bin()
        .current_dir(dir.path())
        .args(["render", "nb", "-o", "site"])
        .status()
        .expect("failed to run rustlab-notebook");
    assert!(status.success(), "render exited with {status}");
    assert!(Path::new(&dir.path().join("site/a.html")).is_file());
    assert!(!src_dir.join("site").exists());
}
