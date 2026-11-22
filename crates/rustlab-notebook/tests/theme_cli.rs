//! Theme files through the real binary: `theme list / show / check`,
//! `render --theme-file`, `--theme-contrast`, frontmatter, `rustlab.toml`,
//! the rc file, and a live `notebook watch` page.
//!
//! `HOME` and `XDG_CONFIG_HOME` point at a temp dir so the developer's
//! own rc file and user themes never leak in.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_rustlab-notebook");

const COURSE: &str = r##"
inherits = "latte"
[palette]
brand = "#5b21b6"
[colors]
accent_primary = "brand"
banner_bg = "#7a1f3d"
[typography]
measure = "68ch"
body = ["Charter", "Georgia", "serif"]
"##;

/// Fails `syn_comment` on the code panel (and nothing else on latte).
const FAINT: &str = "inherits = \"latte\"\n[colors]\nsyn_comment = \"#c0c4d0\"\n";

struct Env {
    home: TempDir,
}

impl Env {
    fn new() -> Self {
        Env {
            home: TempDir::new().unwrap(),
        }
    }

    fn xdg(&self) -> PathBuf {
        self.home.path().join(".config")
    }

    fn cmd(&self, dir: &Path) -> Command {
        let mut c = Command::new(BIN);
        c.current_dir(dir)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.xdg())
            .env("CI", "1")
            .env_remove("RUSTLAB_VIEWER_SOCK");
        c
    }

    fn run(&self, dir: &Path, args: &[&str]) -> Output {
        self.cmd(dir).args(args).output().unwrap()
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

/// A project: `rustlab.toml`, `themes/course.toml`, `themes/faint.toml`,
/// and a notebook.
fn project(rustlab_toml: &str, notebook: &str) -> TempDir {
    let d = TempDir::new().unwrap();
    std::fs::write(d.path().join("rustlab.toml"), rustlab_toml).unwrap();
    std::fs::create_dir_all(d.path().join("themes")).unwrap();
    std::fs::write(d.path().join("themes/course.toml"), COURSE).unwrap();
    std::fs::write(d.path().join("themes/faint.toml"), FAINT).unwrap();
    std::fs::create_dir_all(d.path().join("notes")).unwrap();
    std::fs::write(d.path().join("notes/nb.md"), notebook).unwrap();
    d
}

fn render_html(env: &Env, dir: &Path, extra: &[&str]) -> (Output, String) {
    let out = dir.join("out.html");
    let _ = std::fs::remove_file(&out);
    let mut args = vec!["render", "notes/nb.md", "-o"];
    let out_s = out.to_str().unwrap().to_string();
    args.push(&out_s);
    args.extend_from_slice(extra);
    let o = env.run(dir, &args);
    let html = std::fs::read_to_string(&out).unwrap_or_default();
    (o, html)
}

const NB: &str = "# Notes\n\nSome text.\n\n```rustlab\nx = 1 + 1\n```\n";

#[test]
fn theme_check_list_and_show() {
    let env = Env::new();
    let p = project("", NB);
    let dir = p.path();
    let o = env.run(dir, &["theme", "check", "course"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains("chain:   course -> latte"));
    let o = env.run(dir, &["theme", "check", "faint"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains("syn_comment on code_bg"));
    let o = env.run(dir, &["theme", "check", "themes/course.toml"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    for builtin in ["mocha", "macchiato", "frappe", "latte", "dark", "light"] {
        let o = env.run(dir, &["theme", "check", builtin]);
        assert_eq!(o.status.code(), Some(0), "{builtin}: {}", text(&o));
    }
    std::fs::write(
        dir.join("bad.toml"),
        "[colors]\nbg = \"red; } body { display: none\"\n",
    )
    .unwrap();
    let o = env.run(dir, &["theme", "check", "bad.toml"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("colors.bg"),
        "{}",
        text(&o)
    );

    let o = env.run(dir, &["theme", "list"]);
    assert!(o.status.success(), "{}", text(&o));
    let listed = String::from_utf8_lossy(&o.stdout).to_string();
    for want in ["mocha", "latte", "course", "faint", "project"] {
        assert!(listed.contains(want), "{want} not listed:\n{listed}");
    }
    // --dir starts the rustlab.toml search elsewhere.
    let other = TempDir::new().unwrap();
    let o = env.run(
        other.path(),
        &[
            "theme",
            "list",
            "--dir",
            dir.join("notes").to_str().unwrap(),
        ],
    );
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("course"),
        "{}",
        text(&o)
    );

    let o = env.run(dir, &["theme", "show", "course"]);
    let shown = String::from_utf8_lossy(&o.stdout);
    assert!(shown.contains("--rl-accent-primary: #5b21b6;"), "{shown}");
    assert!(shown.contains("--rl-measure: 68ch;"), "{shown}");
    assert!(shown.contains("--rl-banner-bg: #7a1f3d;"), "{shown}");
}

#[test]
fn render_theme_file_and_contrast_switch() {
    let env = Env::new();
    let p = project("", NB);
    let dir = p.path();
    let (o, html) = render_html(&env, dir, &["--theme-file", "themes/course.toml"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(html.contains("--rl-accent-primary: #5b21b6;"));
    assert!(html.contains("--rl-measure: 68ch;"));
    assert!(html.contains(r#"--rl-font-body: "Charter", "Georgia", serif;"#));
    assert!(html.contains("color-scheme: light"));

    // A failing theme warns and renders...
    let (o, html) = render_html(&env, dir, &["--theme", "faint"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(!html.is_empty());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("warning: theme `faint`: syn_comment on code_bg"),
        "{err}"
    );
    // ...--strict is about cells and ignores contrast...
    let (o, _) = render_html(&env, dir, &["--theme", "faint", "--strict"]);
    assert!(o.status.success(), "{}", text(&o));
    // ...and --theme-contrast=error refuses before writing anything.
    let (o, html) = render_html(&env, dir, &["--theme", "faint", "--theme-contrast=error"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(html.is_empty(), "no output may be written");
    let (o, _) = render_html(&env, dir, &["--theme", "course", "--theme-contrast=error"]);
    assert!(o.status.success(), "{}", text(&o));

    // --theme and --theme-file conflict; an unknown name is a usage error.
    let o = env.run(
        dir,
        &[
            "render",
            "notes/nb.md",
            "-t",
            "latte",
            "--theme-file",
            "themes/course.toml",
        ],
    );
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    let o = env.run(dir, &["render", "notes/nb.md", "-t", "nope"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stderr).contains("unknown theme `nope`"));
}

#[test]
fn precedence_cli_then_frontmatter_then_project_then_rc_then_mocha() {
    let env = Env::new();
    // rc names a user theme that lives in $XDG_CONFIG_HOME/rustlab/themes.
    std::fs::create_dir_all(env.xdg().join("rustlab/themes")).unwrap();
    std::fs::write(
        env.xdg().join("rustlab/themes/mine.toml"),
        "inherits = \"frappe\"\n[colors]\naccent_tertiary = \"#99d1db\"\n",
    )
    .unwrap();
    std::fs::write(
        env.xdg().join("rustlab/config.toml"),
        "[notebook]\ntheme = \"mine\"\n",
    )
    .unwrap();

    // No rustlab.toml: the rc theme applies.
    let bare = TempDir::new().unwrap();
    std::fs::create_dir_all(bare.path().join("notes")).unwrap();
    std::fs::write(bare.path().join("notes/nb.md"), NB).unwrap();
    let (o, html) = render_html(&env, bare.path(), &[]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        html.contains("--rl-accent-tertiary: #99d1db;"),
        "rc theme not applied"
    );
    assert!(
        html.contains("--rl-bg: #303446;"),
        "rc theme inherits frappe"
    );

    // rustlab.toml beats the rc file.
    let p = project("[notebook]\ntheme = \"course\"\n", NB);
    let (o, html) = render_html(&env, p.path(), &[]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        html.contains("--rl-accent-primary: #5b21b6;"),
        "project theme not applied"
    );

    // Frontmatter beats rustlab.toml.
    std::fs::write(
        p.path().join("notes/nb.md"),
        format!("---\ntheme: macchiato\n---\n{NB}"),
    )
    .unwrap();
    let (_, html) = render_html(&env, p.path(), &[]);
    assert!(
        html.contains("--rl-bg: #24273a;"),
        "frontmatter theme not applied"
    );

    // The CLI beats frontmatter.
    let (_, html) = render_html(&env, p.path(), &["-t", "light"]);
    assert!(html.contains("--rl-bg: #eff1f5;") && !html.contains("#5b21b6"));

    // A frontmatter path is refused and names the file.
    std::fs::write(
        p.path().join("notes/nb.md"),
        format!("---\ntheme: ../themes/course.toml\n---\n{NB}"),
    )
    .unwrap();
    let (o, _) = render_html(&env, p.path(), &[]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("cannot give a path"),
        "{}",
        text(&o)
    );

    // Nothing anywhere: mocha.
    let plain = Env::new();
    let (_, html) = render_html(&plain, bare.path(), &[]);
    assert!(html.contains("--rl-bg: #1e1e2e;"));
}

#[test]
fn project_theme_dir_and_its_escape() {
    let env = Env::new();
    let p = project("[notebook]\ntheme = \"brand\"\ntheme_dir = \"style\"\n", NB);
    std::fs::create_dir_all(p.path().join("style")).unwrap();
    std::fs::write(
        p.path().join("style/brand.toml"),
        "[colors]\naccent_secondary = \"#a5c8ff\"\n",
    )
    .unwrap();
    let (o, html) = render_html(&env, p.path(), &[]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(html.contains("--rl-accent-secondary: #a5c8ff;"));

    std::fs::write(
        p.path().join("rustlab.toml"),
        "[notebook]\ntheme_dir = \"../elsewhere\"\n",
    )
    .unwrap();
    let o = env.run(p.path(), &["render", "notes/nb.md"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("leaves the project root"),
        "{}",
        text(&o)
    );
}

/// `GET path` against the loopback server; `None` until it answers.
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

fn watch_page(env: &Env, dir: &Path, args: &[&str], slug: &str) -> String {
    let port = free_port();
    let port_s = port.to_string();
    let mut all = vec!["watch"];
    all.extend_from_slice(args);
    all.extend_from_slice(&["--no-browser", "--port", &port_s]);
    let _child = Child(
        env.cmd(dir)
            .args(&all)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
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
        assert!(Instant::now() < deadline, "watch server did not answer");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Customising `notebook watch` is a primary use of theme files: the
/// page, the toolbar chrome, and the chrome-only tokens all follow it.
#[test]
fn watch_serves_a_custom_theme() {
    let env = Env::new();
    let p = project("", NB);
    let page = watch_page(
        &env,
        p.path(),
        &[
            "notes/nb.md",
            "--theme-file",
            "themes/course.toml",
            "--editable",
        ],
        "nb",
    );
    assert!(
        page.contains("--rl-accent-primary: #5b21b6;"),
        "page tokens"
    );
    assert!(page.contains("--rl-measure: 68ch;"));
    assert!(page.contains("--rl-banner-bg: #7a1f3d;"), "chrome tokens");
    assert!(page.contains("id=\"rl-toolbar\""), "chrome present");
    assert!(page.contains("color-scheme: light"));

    // rustlab.toml and frontmatter reach watch too.
    let q = project(
        "[notebook]\ntheme = \"course\"\n",
        &format!("---\ntheme: frappe\n---\n{NB}"),
    );
    std::fs::write(q.path().join("notes/other.md"), NB).unwrap();
    let page = watch_page(&env, q.path(), &["notes/"], "nb");
    assert!(
        page.contains("--rl-bg: #303446;"),
        "frontmatter theme in watch"
    );
    let other = watch_page(&env, q.path(), &["notes/"], "other");
    assert!(
        other.contains("--rl-accent-primary: #5b21b6;"),
        "project theme in watch"
    );
}
