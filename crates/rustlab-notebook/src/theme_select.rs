//! Which theme a render uses, and the `theme list / show / check` commands.
//!
//! Precedence, first match wins:
//!
//! 1. `--theme NAME` or `--theme-file PATH`
//! 2. notebook frontmatter `theme: NAME` (a name, never a path)
//! 3. `rustlab.toml` `[notebook] theme` (nearest ancestor directory, not above the `.git` root)
//! 4. `~/.rustlabrc` / `$XDG_CONFIG_HOME/rustlab/config.toml` `[notebook]
//!    theme`, then `[plot] theme`
//! 5. `mocha`
//!
//! The binary resolves 1, 3, 4, and 5 once per run ([`select_default`]) and
//! installs a [`ThemePolicy`]. Each notebook's frontmatter is applied per
//! render by [`for_source`], unless a CLI flag forced the theme.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use rustlab_plot::{intern_theme, theme_colors, ThemeColors};

use crate::theme_file::{self, LoadedTheme, ThemeOrigin, ThemeSearch};

/// `render --theme-contrast`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContrastMode {
    /// Print one warning per failing pair and render anyway.
    #[default]
    Warn,
    /// Refuse a theme with any failing pair.
    Error,
}

/// What every render in this process consults for frontmatter themes.
#[derive(Debug, Clone, Default)]
pub struct ThemePolicy {
    /// A CLI flag chose the theme; frontmatter is ignored.
    pub forced: bool,
    pub search: ThemeSearch,
    pub contrast: ContrastMode,
}

static POLICY: RwLock<Option<Arc<ThemePolicy>>> = RwLock::new(None);
static WARNED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Install the process policy. Without one, frontmatter `theme:` still
/// resolves builtin names, and file themes are not searched.
pub fn install_policy(policy: ThemePolicy) {
    *POLICY.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(policy));
}

fn policy() -> Arc<ThemePolicy> {
    POLICY
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default()
}

/// Turn a resolved theme into the `&'static` palette renderers take.
/// Builtins return their static, so a builtin page is byte-identical to
/// one rendered by name. Contrast failures warn once per theme, or are an
/// error under [`ContrastMode::Error`].
pub fn activate(
    loaded: LoadedTheme,
    contrast: ContrastMode,
) -> Result<&'static ThemeColors, String> {
    let failures = loaded.contrast_failures();
    if !failures.is_empty() {
        let name = loaded.theme.name.clone();
        let lines: Vec<String> = failures
            .iter()
            .map(|c| {
                format!(
                    "theme `{name}`: {} on {} is {:.2}:1, needs {}:1 ({})",
                    c.fg_role,
                    c.bg_role,
                    c.ratio().unwrap_or(0.0),
                    c.need.min(),
                    c.what
                )
            })
            .collect();
        if contrast == ContrastMode::Error {
            return Err(format!(
                "theme `{name}` fails {} contrast check(s) (--theme-contrast=error):\n  {}",
                failures.len(),
                lines.join("\n  ")
            ));
        }
        let first = WARNED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.clone());
        if first {
            for l in &lines {
                eprintln!("warning: {l}");
            }
            eprintln!("warning: run `rustlab-notebook theme check {name}` for the full table");
        }
    }
    if loaded.origin == ThemeOrigin::Builtin {
        if let Some(c) = theme_colors(&loaded.theme.name) {
            return Ok(c);
        }
    }
    Ok(&intern_theme(loaded.theme).colors)
}

/// The theme for one notebook: its frontmatter `theme:` unless the CLI
/// forced one, else `default`.
pub fn for_source<'a>(default: &'a ThemeColors, source: &str) -> Result<&'a ThemeColors, String> {
    let policy = policy();
    if policy.forced {
        return Ok(default);
    }
    let Some(raw) = crate::parse::extract_frontmatter(source).0.theme else {
        return Ok(default);
    };
    let name = raw.trim().to_ascii_lowercase();
    if !theme_file::is_builtin(&name) && !rustlab_config::is_theme_name(&name) {
        return Err(format!(
            "frontmatter theme: \"{raw}\" is not a theme name (frontmatter names a theme; it cannot give a path)"
        ));
    }
    let loaded = theme_file::resolve(&name, &policy.search)
        .map_err(|e| format!("frontmatter theme: {e}"))?;
    activate(loaded, policy.contrast).map(|c| c as &'a ThemeColors)
}

/// Inputs to [`select_default`], gathered by the binary.
#[derive(Debug, Clone, Default)]
pub struct ThemeRequest {
    pub cli_theme: Option<String>,
    pub cli_file: Option<PathBuf>,
    /// `rustlab.toml` `[notebook] theme` and that file's path.
    pub project_theme: Option<(String, PathBuf)>,
    /// rc `[notebook] theme` (else `[plot] theme`) and the rc file's path.
    pub rc_theme: Option<(String, Option<PathBuf>)>,
}

impl ThemeRequest {
    pub fn forced(&self) -> bool {
        self.cli_theme.is_some() || self.cli_file.is_some()
    }
}

/// Steps 1, 3, 4, 5 of the precedence. Errors name where the theme came
/// from (flag, `rustlab.toml`, or the rc file).
pub fn select_default(req: &ThemeRequest, search: &ThemeSearch) -> Result<LoadedTheme, String> {
    if let Some(path) = &req.cli_file {
        return theme_file::load_path(path, search).map_err(|e| format!("--theme-file: {e}"));
    }
    if let Some(name) = &req.cli_theme {
        return theme_file::resolve(name, search).map_err(|e| format!("--theme: {e}"));
    }
    if let Some((name, path)) = &req.project_theme {
        return theme_file::resolve(name, search)
            .map_err(|e| format!("{}: notebook.theme: {e}", path.display()));
    }
    if let Some((name, path)) = &req.rc_theme {
        return theme_file::resolve(name, search).map_err(|e| match path {
            Some(p) => format!("{}: notebook.theme: {e}", p.display()),
            None => format!("notebook.theme: {e}"),
        });
    }
    theme_file::resolve("mocha", search)
}

/// Where `rustlab.toml` lookup starts for an input path.
pub fn project_start(input: &Path) -> PathBuf {
    if input.is_dir() {
        return input.to_path_buf();
    }
    match input.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// The search directories for a project config (if any).
pub fn search_for(project: Option<&rustlab_config::ProjectConfig>) -> ThemeSearch {
    ThemeSearch {
        project: project.map(|p| p.theme_dir.clone().unwrap_or_else(|| p.root.join("themes"))),
        user: ThemeSearch::user_dir_from_env(),
    }
}

/// `theme list`. Returns the exit code.
pub fn cmd_theme_list(search: &ThemeSearch, out: &mut dyn std::io::Write) -> i32 {
    let rows = theme_file::list(search);
    let width = rows.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let _ = writeln!(out, "{:<width$}  {:<7}  details", "name", "origin");
    for r in rows {
        let detail = match &r.path {
            Some(p) => format!("{} ({})", r.note, p.display()),
            None => r.note.clone(),
        };
        let _ = writeln!(out, "{:<width$}  {:<7}  {detail}", r.name, r.origin.label());
    }
    for (label, dir) in [("project", &search.project), ("user", &search.user)] {
        match dir {
            Some(d) => {
                let _ = writeln!(out, "{label} themes: {}", d.display());
            }
            None => {
                let _ = writeln!(out, "{label} themes: (none)");
            }
        }
    }
    0
}

/// `NAME` resolves through the search path; anything with a `/` or a
/// `.toml` suffix is a file path.
fn load_target(target: &str, search: &ThemeSearch) -> Result<LoadedTheme, String> {
    if target.ends_with(".toml") || target.contains('/') || target.contains('\\') {
        theme_file::load_path(Path::new(target), search)
    } else {
        theme_file::resolve(target, search)
    }
}

fn header(t: &LoadedTheme, out: &mut dyn std::io::Write) {
    let _ = writeln!(out, "theme:   {}", t.theme.name);
    let _ = writeln!(out, "origin:  {}", t.origin.label());
    if let Some(p) = &t.path {
        let _ = writeln!(out, "file:    {}", p.display());
    }
    let _ = writeln!(out, "chain:   {}", t.chain.join(" -> "));
    let _ = writeln!(out, "mode:    {}", t.theme.colors.color_scheme());
}

/// `theme show`: the resolved tokens as CSS declarations.
pub fn cmd_theme_show(target: &str, search: &ThemeSearch, out: &mut dyn std::io::Write) -> i32 {
    let t = match load_target(target, search) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(out, "error: {e}");
            return 1;
        }
    };
    header(&t, out);
    let _ = writeln!(out, ":root {{");
    let _ = writeln!(out, "{}", t.theme.css_custom_properties());
    for (n, v) in t.theme.colors.chrome_tokens() {
        let _ = writeln!(out, "    {n}: {v};");
    }
    let _ = writeln!(out, "}}");
    0
}

/// `theme check`: validation plus the contrast table. Exit 1 on a load
/// error or any failing pair.
pub fn cmd_theme_check(target: &str, search: &ThemeSearch, out: &mut dyn std::io::Write) -> i32 {
    let t = match load_target(target, search) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(out, "error: {e}");
            return 1;
        }
    };
    header(&t, out);
    let checks = rustlab_plot::contrast::screen_checks(&t.theme.colors);
    let mut failed = 0;
    let _ = writeln!(out, "\nresult  ratio   need     pair (where)");
    for c in &checks {
        let ok = c.passes();
        if !ok {
            failed += 1;
        }
        let _ = writeln!(
            out,
            "{}  {:>6}  {:<7}  {} on {} — {} ({})",
            if ok { "PASS  " } else { "FAIL  " },
            c.ratio()
                .map(|r| format!("{r:.2}"))
                .unwrap_or_else(|| "n/a".into()),
            c.need.label(),
            c.fg_role,
            c.bg_role,
            c.what,
            c.surface.label()
        );
    }
    let _ = writeln!(
        out,
        "\n{} of {} pairs pass. PDF always prints Latte, so it is not checked here.",
        checks.len() - failed,
        checks.len()
    );
    if failed > 0 {
        1
    } else {
        0
    }
}

#[cfg(test)]
pub(crate) static TEST_POLICY_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use rustlab_plot::Theme;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rl-theme-select-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }

    fn search(d: &Path) -> ThemeSearch {
        ThemeSearch {
            project: Some(d.to_path_buf()),
            user: None,
        }
    }

    #[test]
    fn precedence_is_cli_then_project_then_rc_then_mocha() {
        let d = dir("prec");
        std::fs::write(d.join("proj.toml"), "inherits = \"latte\"\n").unwrap();
        std::fs::write(d.join("user.toml"), "inherits = \"frappe\"\n").unwrap();
        let file = d.join("cli-file.toml");
        std::fs::write(&file, "inherits = \"macchiato\"\n").unwrap();
        let s = search(&d);
        let mut req = ThemeRequest {
            cli_theme: Some("light".into()),
            cli_file: None,
            project_theme: Some(("proj".into(), d.join("rustlab.toml"))),
            rc_theme: Some(("user".into(), None)),
        };
        assert_eq!(select_default(&req, &s).unwrap().theme.name, "latte");
        assert!(req.forced());
        req.cli_theme = None;
        req.cli_file = Some(file);
        assert_eq!(select_default(&req, &s).unwrap().theme.name, "cli-file");
        req.cli_file = None;
        assert!(!req.forced());
        assert_eq!(select_default(&req, &s).unwrap().theme.name, "proj");
        req.project_theme = None;
        assert_eq!(select_default(&req, &s).unwrap().theme.name, "user");
        req.rc_theme = None;
        assert_eq!(select_default(&req, &s).unwrap().theme.name, "mocha");
        // An unknown rc or project name says which file set it.
        req.project_theme = Some(("missing".into(), d.join("rustlab.toml")));
        let err = select_default(&req, &s).unwrap_err();
        assert!(
            err.contains("rustlab.toml: notebook.theme: unknown theme `missing`"),
            "{err}"
        );
        req.project_theme = None;
        req.rc_theme = Some(("missing".into(), Some(d.join(".rustlabrc"))));
        let err = select_default(&req, &s).unwrap_err();
        assert!(err.contains(".rustlabrc: notebook.theme"), "{err}");
    }

    #[test]
    fn builtins_activate_to_their_static() {
        let t = theme_file::resolve("dark", &ThemeSearch::default()).unwrap();
        let c = activate(t, ContrastMode::Error).unwrap();
        assert!(std::ptr::eq(c, Theme::Mocha.colors()));
    }

    #[test]
    fn contrast_error_mode_refuses_a_failing_theme() {
        let d = dir("contrast");
        std::fs::write(d.join("faint.toml"), "[colors]\ntext_dim = \"#45475a\"\n").unwrap();
        let t = theme_file::resolve("faint", &search(&d)).unwrap();
        let err = activate(t.clone(), ContrastMode::Error).unwrap_err();
        assert!(
            err.contains("--theme-contrast=error") && err.contains("text_dim on bg"),
            "{err}"
        );
        // Warn mode renders anyway, with the theme's own palette.
        let c = activate(t, ContrastMode::Warn).unwrap();
        assert_eq!(c.text_dim, "#45475a");
    }

    #[test]
    fn frontmatter_selects_a_theme_unless_the_cli_forced_one() {
        let _lock = TEST_POLICY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = dir("front");
        std::fs::write(
            d.join("course.toml"),
            "inherits = \"latte\"\n[colors]\naccent_primary = \"#5b21b6\"\n",
        )
        .unwrap();
        let src = "---\ntitle: T\ntheme: course\n---\n# T\n";
        let mocha = Theme::Mocha.colors();
        install_policy(ThemePolicy {
            forced: false,
            search: search(&d),
            contrast: ContrastMode::Warn,
        });
        let c = for_source(mocha, src).unwrap();
        assert_eq!(c.accent_primary, "#5b21b6");
        assert_eq!(c.bg, Theme::Latte.colors().bg);
        assert!(std::ptr::eq(
            for_source(mocha, "---\ntheme: Light\n---\n").unwrap(),
            Theme::Latte.colors()
        ));
        assert!(std::ptr::eq(
            for_source(mocha, "# no frontmatter\n").unwrap(),
            mocha
        ));
        for bad in ["../course", "/etc/passwd", "course.toml", "a b"] {
            let err = for_source(mocha, &format!("---\ntheme: {bad}\n---\n")).unwrap_err();
            assert!(err.contains("cannot give a path"), "{bad}: {err}");
        }
        let err = for_source(mocha, "---\ntheme: nope\n---\n").unwrap_err();
        assert!(err.contains("unknown theme `nope`"), "{err}");
        install_policy(ThemePolicy {
            forced: true,
            search: search(&d),
            contrast: ContrastMode::Warn,
        });
        assert!(std::ptr::eq(for_source(mocha, src).unwrap(), mocha));
        install_policy(ThemePolicy::default());
    }

    #[test]
    fn check_exits_nonzero_on_a_failing_pair_or_a_bad_file() {
        let d = dir("check");
        std::fs::write(d.join("good.toml"), "inherits = \"latte\"\n").unwrap();
        std::fs::write(
            d.join("faint.toml"),
            "[colors]\nsyn_comment = \"#313244\"\n",
        )
        .unwrap();
        std::fs::write(d.join("broken.toml"), "[colors]\nbg = \"red;}\"\n").unwrap();
        let s = search(&d);
        let mut out = Vec::new();
        assert_eq!(cmd_theme_check("good", &s, &mut out), 0);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("PASS") && !text.contains("FAIL "), "{text}");
        for builtin in ["mocha", "macchiato", "frappe", "latte", "dark", "light"] {
            assert_eq!(
                cmd_theme_check(builtin, &s, &mut Vec::new()),
                0,
                "{builtin}"
            );
        }
        let mut out = Vec::new();
        assert_eq!(cmd_theme_check("faint", &s, &mut out), 1);
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.lines()
                .any(|l| l.starts_with("FAIL") && l.contains("syn_comment on code_bg")),
            "{text}"
        );
        let mut out = Vec::new();
        let path = d.join("broken.toml");
        assert_eq!(cmd_theme_check(path.to_str().unwrap(), &s, &mut out), 1);
        assert!(String::from_utf8(out).unwrap().contains("colors.bg"));
        let mut out = Vec::new();
        assert_eq!(cmd_theme_show("good", &s, &mut out), 0);
        let shown = String::from_utf8(out).unwrap();
        assert!(shown.contains("chain:   good -> latte"), "{shown}");
        assert!(shown.contains("--rl-bg: #eff1f5;") && shown.contains("--rl-banner-bg:"));
        let mut out = Vec::new();
        assert_eq!(cmd_theme_list(&s, &mut out), 0);
        let listed = String::from_utf8(out).unwrap();
        assert!(
            listed.contains("good") && listed.contains("project"),
            "{listed}"
        );
    }
}
