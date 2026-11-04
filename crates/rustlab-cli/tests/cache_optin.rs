//! The SQLite function cache is off unless the user opts in.
//!
//! Default `rustlab run` of a script that says `cache enable` must not
//! create `.rustlab/`. `--cache` creates and uses the store. `--no-cache`
//! wins over `[cache] enabled = true`.

use std::path::Path;
use std::process::{Command, Output};

fn rustlab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustlab"))
}

fn isolate(cmd: &mut Command, home: &Path) {
    cmd.env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("xdg-empty"))
        .env_remove("RUSTLAB_NO_HISTORY");
}

fn script_body() -> &'static str {
    "cache enable\n\
     function y = id(x)\n  y = x\nend\n\
     a = id(3)\n\
     b = id(3)\n\
     cache status\n"
}

fn run_script(home: &Path, script: &Path, extra: &[&str]) -> Output {
    let mut cmd = rustlab();
    isolate(&mut cmd, home);
    cmd.arg("run").arg(script).arg("--plot").arg("none");
    for a in extra {
        cmd.arg(a);
    }
    cmd.output().expect("rustlab run")
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "rustlab run failed: {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn default_run_does_not_create_cache_db() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let script = work.path().join("nocache.rlab");
    std::fs::write(&script, script_body()).unwrap();

    let out = run_script(home.path(), &script, &[]);
    assert_ok(&out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("cache: disabled"),
        "expected the disabled notice, stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("cache: off"),
        "status should stay off, stdout:\n{stdout}"
    );
    assert!(
        !work.path().join(".rustlab").exists(),
        "default run must not create .rustlab/"
    );
}

#[test]
fn cache_flag_creates_and_uses_the_store() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let script = work.path().join("cached.rlab");
    std::fs::write(&script, script_body()).unwrap();

    let first = run_script(home.path(), &script, &["--cache"]);
    assert_ok(&first);
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(
        stdout.contains("cache: active"),
        "first run stdout:\n{stdout}"
    );
    assert!(
        work.path().join(".rustlab/cache.db").is_file(),
        "opt-in should create .rustlab/cache.db"
    );

    let second = run_script(home.path(), &script, &["--cache"]);
    assert_ok(&second);
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains("hits"),
        "second run should hit the store, stdout:\n{stdout}"
    );
}

#[test]
fn no_cache_overrides_config_enabled() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".rustlabrc"), "[cache]\nenabled = true\n").unwrap();
    let work = tempfile::tempdir().unwrap();
    let script = work.path().join("override.rlab");
    std::fs::write(&script, script_body()).unwrap();

    let allowed = run_script(home.path(), &script, &[]);
    assert_ok(&allowed);
    assert!(
        work.path().join(".rustlab/cache.db").is_file(),
        "config enabled = true should create the store"
    );
    let _ = std::fs::remove_dir_all(work.path().join(".rustlab"));

    let denied = run_script(home.path(), &script, &["--no-cache"]);
    assert_ok(&denied);
    let stdout = String::from_utf8_lossy(&denied.stdout);
    assert!(
        stdout.contains("cache: disabled"),
        "--no-cache stdout:\n{stdout}"
    );
    assert!(
        !work.path().join(".rustlab").exists(),
        "--no-cache must not create .rustlab/ even when the rc key is true"
    );
}

#[test]
fn cache_flag_overrides_config_disabled() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".rustlabrc"), "[cache]\nenabled = false\n").unwrap();
    let work = tempfile::tempdir().unwrap();
    let script = work.path().join("force.rlab");
    std::fs::write(&script, script_body()).unwrap();

    let out = run_script(home.path(), &script, &["--cache"]);
    assert_ok(&out);
    assert!(
        work.path().join(".rustlab/cache.db").is_file(),
        "--cache must override [cache] enabled = false"
    );
}
