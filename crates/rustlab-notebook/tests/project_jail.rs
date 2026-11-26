//! `[notebook] jail_root` in `rustlab.toml` through the real binary.
//!
//! The project is `proj/rustlab.toml`, `proj/lib/helper.rlab`, and
//! `proj/lessons/x.md`, whose cell runs `../lib/helper.rlab`. Without a
//! wider jail that read escapes the notebook directory.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_rustlab-notebook");

const NOTEBOOK: &str =
    "# Lesson\n\n```rustlab\nrun \"../lib/helper.rlab\"\ndisp(shared_value)\n```\n";
const ESCAPE: &str = "path escapes notebook directory";

struct Project {
    home: TempDir,
    dir: TempDir,
}

impl Project {
    /// `toml` is the body of `rustlab.toml`; `None` writes no file.
    fn new(toml: Option<&str>) -> Self {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::create_dir_all(root.join("lessons")).unwrap();
        std::fs::write(root.join("lib/helper.rlab"), "shared_value = 4242;\n").unwrap();
        std::fs::write(root.join("lessons/x.md"), NOTEBOOK).unwrap();
        if let Some(body) = toml {
            std::fs::write(root.join("rustlab.toml"), body).unwrap();
        }
        Project {
            home: TempDir::new().unwrap(),
            dir,
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new(BIN);
        c.current_dir(self.root())
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("CI", "1")
            .env_remove("RUSTLAB_VIEWER_SOCK");
        c
    }

    fn render(&self, extra: &[&str]) -> (Output, Option<String>) {
        let out = self.root().join("out.html");
        let _ = std::fs::remove_file(&out);
        let mut args = vec!["render", "lessons/x.md", "-o", "out.html"];
        args.extend_from_slice(extra);
        let o = self.cmd().args(&args).output().unwrap();
        (o, std::fs::read_to_string(&out).ok())
    }
}

fn text(o: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        o.status.code(),
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn without_a_jail_root_the_shared_lib_is_refused() {
    let p = Project::new(None);
    let (o, html) = p.render(&[]);
    assert!(o.status.success(), "{}", text(&o));
    let html = html.expect("output written");
    assert!(html.contains(ESCAPE), "expected the jail to refuse ../lib");
    assert!(!html.contains("4242"));
}

#[test]
fn project_jail_root_lets_a_lesson_load_the_shared_lib() {
    let p = Project::new(Some("[notebook]\njail_root = \".\"\n"));
    let (o, html) = p.render(&[]);
    assert!(o.status.success(), "{}", text(&o));
    let html = html.expect("output written");
    assert!(html.contains("4242"), "helper did not run:\n{}", text(&o));
    assert!(!html.contains(ESCAPE));
}

#[test]
fn project_jail_root_is_found_from_a_nested_working_directory() {
    let p = Project::new(Some("[notebook]\njail_root = \".\"\n"));
    let o = p
        .cmd()
        .current_dir(p.root().join("lessons"))
        .args(["render", "x.md", "-o", "../nested.html"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let html = std::fs::read_to_string(p.root().join("nested.html")).unwrap();
    assert!(html.contains("4242"));
}

#[test]
fn cli_jail_root_overrides_the_project_value() {
    let p = Project::new(Some("[notebook]\njail_root = \".\"\n"));
    let (o, html) = p.render(&["--jail-root", "lessons"]);
    assert!(o.status.success(), "{}", text(&o));
    let html = html.expect("output written");
    assert!(
        html.contains(ESCAPE),
        "--jail-root lessons must narrow the jail"
    );
}

#[test]
fn directory_render_uses_the_project_jail_root() {
    let p = Project::new(Some("[notebook]\njail_root = \".\"\n"));
    let o = p
        .cmd()
        .args(["render", "lessons", "-o", "site"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let html = std::fs::read_to_string(p.root().join("site/x.html")).unwrap();
    assert!(html.contains("4242"));
}

fn assert_refused(p: &Project, needle: &str) {
    let (o, html) = p.render(&[]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    let err = stderr(&o);
    assert!(err.contains("notebook.jail_root"), "{err}");
    assert!(err.contains(needle), "wanted {needle:?}: {err}");
    assert!(html.is_none(), "nothing may be rendered");
}

#[test]
fn project_jail_root_escapes_are_refused() {
    for value in ["..", "../x", "lib/../..", "lib/../../x"] {
        let p = Project::new(Some(&format!("[notebook]\njail_root = \"{value}\"\n")));
        assert_refused(&p, "escapes");
    }
    for value in ["/", "/tmp"] {
        let p = Project::new(Some(&format!("[notebook]\njail_root = \"{value}\"\n")));
        assert_refused(&p, "must be relative");
    }
}

#[cfg(unix)]
#[test]
fn project_jail_root_symlink_out_is_refused() {
    let outside = TempDir::new().unwrap();
    let p = Project::new(Some("[notebook]\njail_root = \"link\"\n"));
    std::os::unix::fs::symlink(outside.path(), p.root().join("link")).unwrap();
    assert_refused(&p, "escapes");
    let p = Project::new(Some("[notebook]\njail_root = \"lib/up\"\n"));
    std::os::unix::fs::symlink(p.root().parent().unwrap(), p.root().join("lib/up")).unwrap();
    assert_refused(&p, "escapes");
}

#[test]
fn project_jail_root_must_contain_the_notebook() {
    let p = Project::new(Some("[notebook]\njail_root = \"lib\"\n"));
    assert_refused(&p, "does not contain");
}

#[test]
fn cli_jail_root_may_widen_past_the_project() {
    let p = Project::new(Some("[notebook]\njail_root = \"lessons\"\n"));
    let root = p.root().display().to_string();
    let (o, html) = p.render(&["--jail-root", &root]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(html.unwrap().contains("4242"));
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

fn watch_page(p: &Project, input: &str, slug: &str) -> String {
    let port = free_port();
    let port_s = port.to_string();
    let log: PathBuf = p.home.path().join("watch.log");
    let _child = Child(
        p.cmd()
            .args(["watch", input, "--no-browser", "--port", &port_s])
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log).unwrap())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(body) = http_get(port, &format!("/n/{slug}")) {
            if body.starts_with("HTTP/1.1 200") {
                return body;
            }
        }
        assert!(
            Instant::now() < deadline,
            "watch server did not answer:\n{}",
            std::fs::read_to_string(&log).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn watch_uses_the_project_jail_root() {
    let p = Project::new(Some("[notebook]\njail_root = \".\"\n"));
    let page = watch_page(&p, "lessons/x.md", "x");
    assert!(page.contains("4242"), "watch page did not run the helper");
    assert!(!page.contains(ESCAPE));
}

#[test]
fn watch_refuses_files_whose_projects_disagree_on_the_jail() {
    let p = Project::new(Some("[notebook]\njail_root = \".\"\n"));
    let other = TempDir::new().unwrap();
    let stray = other.path().join("stray.md");
    std::fs::write(&stray, "# Stray\n").unwrap();
    let o = p
        .cmd()
        .args(["watch", "lessons/x.md"])
        .arg(&stray)
        .args(["--no-browser", "--port", "1"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    let err = stderr(&o);
    assert!(err.contains("different jails"), "{err}");
    assert!(err.contains("--jail-root"), "{err}");
}

#[test]
fn watch_refuses_an_escaping_project_jail_root() {
    let p = Project::new(Some("[notebook]\njail_root = \"..\"\n"));
    let o = p
        .cmd()
        .args(["watch", "lessons/x.md", "--no-browser", "--port", "1"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(stderr(&o).contains("notebook.jail_root"), "{}", text(&o));
}
