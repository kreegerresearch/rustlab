//! Notebook renders do not create `.rustlab/cache.db` unless the user
//! opts in with `--cache` or `[cache] enabled = true`. `--no-cache`
//! overrides the config key.

use std::path::Path;
use std::process::{Command, Output};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustlab-notebook"))
}

fn isolate(cmd: &mut Command, home: &Path) {
    cmd.env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("xdg-empty"))
        .env("CI", "1");
}

const NOTE: &str = "\
# Cache opt-in

```rustlab
cache enable
function y = id(x)
  y = x
end
a = id(4)
b = id(4)
cache status
```
";

fn render(home: &Path, note: &Path, out: &Path, extra: &[&str]) -> Output {
    let mut cmd = bin();
    isolate(&mut cmd, home);
    cmd.arg("render")
        .arg(note)
        .arg("-o")
        .arg(out)
        .arg("-f")
        .arg("html");
    for a in extra {
        cmd.arg(a);
    }
    cmd.output().expect("rustlab-notebook render")
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "render failed: {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn default_render_does_not_create_cache_db() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let note = work.path().join("note.md");
    std::fs::write(&note, NOTE).unwrap();
    let html = work.path().join("note.html");

    let out = render(home.path(), &note, &html, &[]);
    assert_ok(&out);
    let page = std::fs::read_to_string(&html).unwrap();
    assert!(
        page.contains("cache: disabled"),
        "rendered page should show the disabled notice"
    );
    assert!(
        !work.path().join(".rustlab").exists(),
        "default notebook render must not create .rustlab/"
    );
}

#[test]
fn cache_flag_creates_the_store() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let note = work.path().join("note.md");
    std::fs::write(&note, NOTE).unwrap();
    let html = work.path().join("note.html");

    let out = render(home.path(), &note, &html, &["--cache"]);
    assert_ok(&out);
    assert!(
        work.path().join(".rustlab/cache.db").is_file(),
        "--cache should create .rustlab/cache.db next to the notebook"
    );
    let page = std::fs::read_to_string(&html).unwrap();
    assert!(
        page.contains("cache: active"),
        "rendered page should show the store as active"
    );
}

#[test]
fn no_cache_overrides_config_enabled() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".rustlabrc"), "[cache]\nenabled = true\n").unwrap();
    let work = tempfile::tempdir().unwrap();
    let note = work.path().join("note.md");
    std::fs::write(&note, NOTE).unwrap();
    let html = work.path().join("note.html");

    let out = render(home.path(), &note, &html, &["--no-cache"]);
    assert_ok(&out);
    assert!(
        !work.path().join(".rustlab").exists(),
        "--no-cache must override [cache] enabled = true"
    );
}
