//! End-to-end wiring of `~/.rustlabrc` / XDG config into `rustlab run`.

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn rustlab_with_home(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustlab"));
    cmd.env("HOME", home);
    // Isolate from the runner's XDG so only files we write can win.
    cmd.env("XDG_CONFIG_HOME", home.join(".config"));
    cmd
}

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

fn run_script(home: &Path, source: &str) -> std::process::Output {
    let script = home.join("script.rlab");
    fs::write(&script, source).unwrap();
    rustlab_with_home(home)
        .args(["run", script.to_str().unwrap(), "--plot", "none"])
        .output()
        .expect("launch rustlab")
}

#[test]
fn missing_rc_keeps_builtin_format() {
    let home = TempDir::new().unwrap();
    let out = run_script(home.path(), "x = 1234567\n");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1234567"),
        "expected default short format: {stdout}"
    );
    assert!(
        !stdout.contains("1,234,567"),
        "commas must not appear without an rc: {stdout}"
    );
}

#[test]
fn rustlabrc_sets_display_format() {
    let home = TempDir::new().unwrap();
    write(
        &home.path().join(".rustlabrc"),
        "[display]\nformat = \"commas\"\n",
    );
    let out = run_script(home.path(), "x = 1234567\n");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1,234,567"),
        "rc format=commas should apply: {stdout}"
    );
}

#[test]
fn in_script_format_overrides_rc() {
    let home = TempDir::new().unwrap();
    write(
        &home.path().join(".rustlabrc"),
        "[display]\nformat = \"commas\"\n",
    );
    let out = run_script(home.path(), "format short\nx = 1234567\n");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1234567") && !stdout.contains("1,234,567"),
        "in-script format short must beat rc commas: {stdout}"
    );
}

#[test]
fn xdg_wins_over_rustlabrc() {
    let home = TempDir::new().unwrap();
    write(
        &home.path().join(".rustlabrc"),
        "[display]\nformat = \"commas\"\n",
    );
    write(
        &home.path().join(".config/rustlab/config.toml"),
        "[display]\nformat = \"short\"\n",
    );
    let out = run_script(home.path(), "x = 1234567\n");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1234567") && !stdout.contains("1,234,567"),
        "XDG short must win over ~/.rustlabrc commas: {stdout}"
    );
}

#[test]
fn invalid_value_exits_nonzero_with_path_and_key() {
    let home = TempDir::new().unwrap();
    let rc = home.path().join(".rustlabrc");
    write(&rc, "[display]\nformat = \"banana\"\n");
    let out = run_script(home.path(), "print(1)\n");
    assert_ne!(out.status.code(), Some(0), "invalid rc must fail startup");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("display.format"),
        "stderr should name the key: {stderr}"
    );
    assert!(
        stderr.contains("banana"),
        "stderr should quote the bad value: {stderr}"
    );
    assert!(
        stderr.contains(".rustlabrc") || stderr.contains("display.format"),
        "stderr should identify the file: {stderr}"
    );
}

#[test]
fn unknown_key_warns_and_continues() {
    let home = TempDir::new().unwrap();
    write(
        &home.path().join(".rustlabrc"),
        "[display]\nformat = \"short\"\nmystery = true\n",
    );
    let out = run_script(home.path(), "x = 7\n");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown setting") && stderr.contains("display.mystery"),
        "expected unknown-key warning: {stderr}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains('7'), "script still ran: {stdout}");
}

#[test]
fn cli_plot_none_overrides_viewer_auto_connect() {
    // `--plot none` is an explicit CLI flag and must win over rc
    // auto_connect (which would otherwise select viewer mode).
    let home = TempDir::new().unwrap();
    write(
        &home.path().join(".rustlabrc"),
        "[viewer]\nauto_connect = true\nname = \"work\"\n",
    );
    let out = run_script(home.path(), "print(1)\n");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("auto-connected") && !stderr.contains("could not connect"),
        "--plot none must skip viewer auto_connect: {stderr}"
    );
}
