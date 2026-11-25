//! `rustlab-notebook watch` with several paths: argument errors exit
//! before a server starts.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Run `watch <args>` and return (exit code, stderr). A process still
/// running after 20 s started a server it should not have: kill it and fail.
fn watch(args: &[&str], cwd: &std::path::Path) -> (Option<i32>, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rustlab-notebook"))
        .arg("watch")
        .args(args)
        .arg("--no-browser")
        .current_dir(cwd)
        .env("CI", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rustlab-notebook");
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let out = child.wait_with_output().unwrap();
            return (
                status.code(),
                String::from_utf8_lossy(&out.stderr).to_string(),
            );
        }
        if start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            panic!("watch {args:?} kept running instead of refusing its arguments");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("ch2")).unwrap();
    std::fs::write(dir.path().join("intro.md"), "# Intro\n").unwrap();
    std::fs::write(dir.path().join("ch2/filters.md"), "# Filters\n").unwrap();
    std::fs::write(dir.path().join("demo.rlab"), "disp(1)\n").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "plain\n").unwrap();
    dir
}

#[test]
fn missing_paths_exit_2_and_name_each_one() {
    let dir = tree();
    let (code, err) = watch(&["intro.md", "nope.md", "ch2/gone.md"], dir.path());
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("nope.md") && err.contains("gone.md"), "{err}");
    assert!(err.contains("does not exist"), "{err}");
}

#[test]
fn a_directory_mixed_with_files_exits_2() {
    let dir = tree();
    let (code, err) = watch(&["ch2", "intro.md"], dir.path());
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("directory") && err.contains("alone"), "{err}");
}

#[test]
fn an_unsupported_file_type_exits_2() {
    let dir = tree();
    let (code, err) = watch(&["intro.md", "notes.txt"], dir.path());
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("notes.txt") && err.contains(".rlab"), "{err}");
}

#[test]
fn several_paths_need_the_interactive_server() {
    let dir = tree();
    for flag in [&["--obsidian"][..], &["-o", "out"][..]] {
        let mut args = vec!["intro.md", "ch2/filters.md"];
        args.extend_from_slice(flag);
        let (code, err) = watch(&args, dir.path());
        assert_eq!(code, Some(2), "{flag:?}: {err}");
        assert!(err.contains("one directory"), "{err}");
    }
}

#[test]
fn a_quoted_pattern_that_matches_nothing_exits_2_and_names_it() {
    let dir = tree();
    let (code, err) = watch(&["intro.md", "ch9/**/*.md"], dir.path());
    assert_eq!(code, Some(2), "{err}");
    assert!(
        err.contains("ch9/**/*.md") && err.contains("matched no"),
        "{err}"
    );
}

#[test]
fn a_quoted_pattern_with_the_rerender_mode_exits_2() {
    let dir = tree();
    let (code, err) = watch(&["**/*.md", "--obsidian"], dir.path());
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("pattern"), "{err}");
}

#[test]
fn help_documents_several_files() {
    let out = Command::new(env!("CARGO_BIN_EXE_rustlab-notebook"))
        .args(["watch", "--help"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("watch intro.md ch2/filters.md demo.rlab"),
        "{text}"
    );
    assert!(text.contains("common folder"), "{text}");
    assert!(text.contains("'course/**/*.md'"), "{text}");
}
