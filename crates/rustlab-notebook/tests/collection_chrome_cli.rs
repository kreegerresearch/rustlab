//! `--no-toc`, `--file-browser` / `--no-file-browser`, and
//! `rustlab.toml` `[notebook] file_browser` through the real binary, for
//! directory `render` and directory `watch`.
//!
//! `HOME` and `XDG_CONFIG_HOME` point at a temp dir so the developer's
//! own rc file never leaks in.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_rustlab-notebook");

fn cmd(home: &Path, dir: &Path) -> Command {
    let mut c = Command::new(BIN);
    c.current_dir(dir)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("CI", "1")
        .env_remove("RUSTLAB_VIEWER_SOCK");
    c
}

fn text(o: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        o.status.code(),
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// `course/` with three notebooks, and an optional `rustlab.toml` at the root.
fn project(rustlab_toml: Option<&str>) -> TempDir {
    let d = TempDir::new().unwrap();
    if let Some(t) = rustlab_toml {
        std::fs::write(d.path().join("rustlab.toml"), t).unwrap();
    }
    let c = d.path().join("course");
    std::fs::create_dir_all(&c).unwrap();
    std::fs::write(c.join("a.md"), "# Alpha\n\n## One\n\ntext\n").unwrap();
    std::fs::write(c.join("b.md"), "# Beta\n\n## Two\n\ntext\n").unwrap();
    std::fs::write(c.join("c.md"), "# Gamma\n\n## Three\n\ntext\n").unwrap();
    d
}

fn render(home: &Path, dir: &Path, extra: &[&str]) -> (Output, String, String) {
    let out = dir.join("site");
    let _ = std::fs::remove_dir_all(&out);
    let mut args = vec!["render", "course", "-o", "site"];
    args.extend_from_slice(extra);
    let o = cmd(home, dir).args(&args).output().unwrap();
    let page = std::fs::read_to_string(out.join("b.html")).unwrap_or_default();
    let index = std::fs::read_to_string(out.join("index.html")).unwrap_or_default();
    (o, page, index)
}

#[test]
fn directory_render_accepts_no_toc_and_no_file_browser() {
    let home = TempDir::new().unwrap();
    let p = project(None);
    let (o, page, index) = render(home.path(), p.path(), &["--no-toc", "--no-file-browser"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(page.contains("<body class=\"no-toc\">"), "{page}");
    assert!(!page.contains("file-browser"));
    assert!(!page.contains("<nav class=\"sidebar\""));
    assert!(page.contains("<nav class=\"page-nav\">"));
    assert!(page.contains("href=\"a.html\"") && page.contains("href=\"c.html\""));
    assert!(!index.contains("file-browser"));
    assert!(index.contains("<body>"));

    // Defaults are unchanged.
    let (o, page, _) = render(home.path(), p.path(), &[]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(page.contains("<body class=\"has-files\">"));
    assert!(page.contains("<nav class=\"sidebar\">"));

    // --index still names a single notebook.
    let (o, _, _) = render(home.path(), p.path(), &["--index", "/x/"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stderr).contains("not a directory"));
}

#[test]
fn project_toml_turns_the_browser_off_and_the_flag_wins() {
    let home = TempDir::new().unwrap();
    let p = project(Some("[notebook]\nfile_browser = false\n"));
    let (o, page, index) = render(home.path(), p.path(), &[]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(!page.contains("file-browser"), "toml off");
    assert!(!index.contains("file-browser"));
    assert!(page.contains("<nav class=\"sidebar\">"), "toc stays");
    assert!(
        !String::from_utf8_lossy(&o.stderr).contains("unknown setting"),
        "{}",
        text(&o)
    );

    let (o, page, index) = render(home.path(), p.path(), &["--file-browser"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        page.contains("<nav class=\"file-browser\""),
        "CLI on beats toml off"
    );
    assert!(index.contains("<nav class=\"file-browser\""));

    let q = project(Some("[notebook]\nfile_browser = true\n"));
    let (o, page, _) = render(home.path(), q.path(), &["--no-file-browser"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(!page.contains("file-browser"), "CLI off beats toml on");

    let bad = project(Some("[notebook]\nfile_browser = \"no\"\n"));
    let (o, _, _) = render(home.path(), bad.path(), &[]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
}

#[test]
fn browser_flags_are_html_only_and_refuse_each_other() {
    let home = TempDir::new().unwrap();
    let p = project(None);
    let o = cmd(home.path(), p.path())
        .args([
            "render",
            "course",
            "-o",
            "tex",
            "-f",
            "latex",
            "--no-file-browser",
        ])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stderr).contains("only to HTML"));
    let o = cmd(home.path(), p.path())
        .args(["render", "course", "--file-browser", "--no-file-browser"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    // A single notebook has no browser: the flag is a no-op with a warning.
    let o = cmd(home.path(), p.path())
        .args(["render", "course/a.md", "-o", "a.html", "--no-file-browser"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stderr).contains("--no-file-browser"));
}

fn http_get(port: u16, path: &str) -> Option<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut buf = String::new();
    s.read_to_string(&mut buf).ok()?;
    Some(buf)
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `(page /n/b, index /)` from a directory watch with `args`.
fn watch(home: &Path, dir: &Path, args: &[&str]) -> (String, String) {
    let port = free_port();
    let port_s = port.to_string();
    let mut all = vec!["watch", "course"];
    all.extend_from_slice(args);
    all.extend_from_slice(&["--no-browser", "--port", &port_s]);
    let _child = Child(
        cmd(home, dir)
            .args(&all)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(page) = http_get(port, "/n/b") {
            if page.starts_with("HTTP/1.1 200") {
                let index = http_get(port, "/").unwrap_or_default();
                return (page, index);
            }
        }
        assert!(Instant::now() < deadline, "watch server did not answer");
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn watch_hides_the_drawer_parts_it_is_told_to() {
    let home = TempDir::new().unwrap();
    let p = project(None);

    let (page, index) = watch(home.path(), p.path(), &[]);
    assert!(page.contains("id=\"rl-drawer\""));
    assert!(page.contains("<nav class=\"file-browser\""));
    assert!(page.contains("<nav class=\"sidebar\">"));
    assert!(index.contains("<nav class=\"file-browser\""));

    let (page, index) = watch(home.path(), p.path(), &["--no-file-browser"]);
    assert!(
        page.contains("id=\"rl-drawer\""),
        "outline still in the drawer"
    );
    assert!(!page.contains("<nav class=\"file-browser\""));
    assert!(page.contains("<nav class=\"sidebar\">"));
    assert!(!index.contains("<nav class=\"file-browser\""));
    assert!(
        !index.contains("id=\"rl-drawer-toggle\""),
        "nothing to put in a drawer"
    );

    let (page, _) = watch(home.path(), p.path(), &["--no-toc"]);
    assert!(page.contains("<nav class=\"file-browser\""));
    assert!(!page.contains("<nav class=\"sidebar\">"));

    let (page, _) = watch(home.path(), p.path(), &["--no-toc", "--no-file-browser"]);
    assert!(
        !page.contains("id=\"rl-drawer\""),
        "empty drawer is not installed"
    );
    assert!(!page.contains("id=\"rl-drawer-toggle\""));
    assert!(page.contains("<nav class=\"page-nav\">"), "prev/next kept");

    let q = project(Some("[notebook]\nfile_browser = false\n"));
    let (page, _) = watch(home.path(), q.path(), &[]);
    assert!(
        !page.contains("<nav class=\"file-browser\""),
        "toml reaches watch"
    );
}
