//! Open the watch-server URL in a browser.
//!
//! Default policy (locked-in #8): auto-open when stderr is a TTY and
//! `CI` is unset. That misses IDE / launcher launches where stderr is
//! not a TTY. Override with:
//!
//! - `--browser` / `--no-browser` (CLI; `--no-browser` wins)
//! - `$RUSTLAB_NOTEBOOK_BROWSER` — `1`/`true`/`on`/`yes` force-open,
//!   `0`/`false`/`off`/`no` never-open, or a command (`firefox`,
//!   `chrome %s`) used as the opener
//! - `$BROWSER` — standard Unix opener command, used when no explicit
//!   command was given
//!
//! Platform fallbacks after any override:
//!
//! | OS | Candidates |
//! |---|---|
//! | macOS | `open` |
//! | Windows | `cmd /c start "" <url>`, then PowerShell `Start-Process` |
//! | WSL | `wslview` (Windows host browser), `cmd.exe /c start`, `explorer.exe`, then Linux list |
//! | Linux | `xdg-open`, `gio open`, `sensible-browser`, `x-www-browser` |

use anyhow::{Context, Result};
use std::io::IsTerminal;

/// How strongly the user asked to open (or not open) a browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserPref {
    /// TTY + `CI` unset (the default).
    Auto,
    /// Open even when stderr is not a TTY (IDE / launcher).
    Always,
    /// Never open.
    Never,
}

/// Parsed `$RUSTLAB_NOTEBOOK_BROWSER` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BrowserEnv {
    Off,
    On,
    /// Opener command, e.g. `firefox` or `google-chrome %s`.
    Command(String),
}

pub(crate) fn parse_browser_env(s: &str) -> BrowserEnv {
    match s.trim() {
        s if eq_ignore_ascii(s, &["0", "false", "off", "no", "never"]) => BrowserEnv::Off,
        s if eq_ignore_ascii(s, &["1", "true", "on", "yes", "always"]) => BrowserEnv::On,
        other => BrowserEnv::Command(other.trim().to_string()),
    }
}

fn eq_ignore_ascii(s: &str, opts: &[&str]) -> bool {
    opts.iter().any(|o| s.eq_ignore_ascii_case(o))
}

/// CLI flags beat the env var. `--no-browser` beats `--browser`.
pub(crate) fn resolve_browser_pref(
    cli_browser: bool,
    cli_no_browser: bool,
    env_value: Option<&str>,
) -> BrowserPref {
    if cli_no_browser {
        return BrowserPref::Never;
    }
    if cli_browser {
        return BrowserPref::Always;
    }
    match env_value.map(str::trim).filter(|s| !s.is_empty()) {
        None => BrowserPref::Auto,
        Some(s) => match parse_browser_env(s) {
            BrowserEnv::Off => BrowserPref::Never,
            BrowserEnv::On | BrowserEnv::Command(_) => BrowserPref::Always,
        },
    }
}

/// `CI` is a hard off — GitHub Actions and friends must never spawn a
/// GUI, even with `--browser` or the env var set in the job.
pub(crate) fn should_open_browser(pref: BrowserPref, ci: bool, stderr_is_tty: bool) -> bool {
    if ci {
        return false;
    }
    match pref {
        BrowserPref::Never => false,
        BrowserPref::Always => true,
        BrowserPref::Auto => stderr_is_tty,
    }
}

pub(crate) fn should_auto_open_browser(cli_browser: bool, cli_no_browser: bool) -> bool {
    let env = std::env::var("RUSTLAB_NOTEBOOK_BROWSER").ok();
    let pref = resolve_browser_pref(cli_browser, cli_no_browser, env.as_deref());
    let ci = std::env::var_os("CI").is_some();
    should_open_browser(pref, ci, std::io::stderr().is_terminal())
}

/// Command override from `$RUSTLAB_NOTEBOOK_BROWSER` (when it is a
/// command) or `$BROWSER`.
fn command_override() -> Option<String> {
    if let Ok(s) = std::env::var("RUSTLAB_NOTEBOOK_BROWSER") {
        if let BrowserEnv::Command(cmd) = parse_browser_env(&s) {
            return Some(cmd);
        }
    }
    std::env::var("BROWSER")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Shell out to the platform's URL opener. Errors propagate so the
/// caller can log a hint instead of failing the server.
pub(crate) fn open_browser(url: &str) -> Result<()> {
    let override_cmd = command_override();
    let candidates = opener_candidates(url, override_cmd.as_deref(), is_wsl());
    let mut last_err: Option<anyhow::Error> = None;
    for (cmd, args) in &candidates {
        match try_open(cmd, args) {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no URL opener available")))
}

/// Ordered opener candidates for `url`.
///
/// `override_cmd` is a user-supplied command (`firefox`, `chrome %s`).
/// `wsl` is injected so tests can pin the WSL list without a real distro.
pub(crate) fn opener_candidates(
    url: &str,
    override_cmd: Option<&str>,
    wsl: bool,
) -> Vec<(String, Vec<String>)> {
    let mut c = Vec::new();
    if let Some(spec) = override_cmd {
        c.push(parse_opener_command(spec, url));
    }
    c.extend(platform_candidates(url, wsl));
    c
}

/// Split an opener spec into `(cmd, args)`.
///
/// `firefox` → `("firefox", [url])`. `google-chrome --new-window %s` →
/// `%s` is replaced by the URL and no extra arg is appended.
pub(crate) fn parse_opener_command(spec: &str, url: &str) -> (String, Vec<String>) {
    let parts: Vec<&str> = spec.split_whitespace().collect();
    if parts.is_empty() {
        return (spec.to_string(), vec![url.to_string()]);
    }
    let cmd = parts[0].to_string();
    let mut args: Vec<String> = parts[1..].iter().map(|p| p.replace("%s", url)).collect();
    if !spec.contains("%s") {
        args.push(url.to_string());
    }
    (cmd, args)
}

fn platform_candidates(url: &str, wsl: bool) -> Vec<(String, Vec<String>)> {
    if cfg!(target_os = "macos") {
        return vec![("open".into(), vec![url.into()])];
    }
    if cfg!(target_os = "windows") {
        return windows_candidates(url);
    }
    // Unix (Linux, *BSD, WSL).
    let mut c = Vec::new();
    if wsl {
        c.push(("wslview".into(), vec![url.into()]));
        // wslu is optional; cmd.exe / explorer.exe still open the
        // Windows host browser. WSL2 localhost forwarding makes
        // 127.0.0.1 in the host browser reach the WSL server.
        c.push((
            "cmd.exe".into(),
            vec!["/c".into(), "start".into(), "".into(), url.into()],
        ));
        c.push(("explorer.exe".into(), vec![url.into()]));
    }
    c.push(("xdg-open".into(), vec![url.into()]));
    c.push(("gio".into(), vec!["open".into(), url.into()]));
    c.push(("sensible-browser".into(), vec![url.into()]));
    c.push(("x-www-browser".into(), vec![url.into()]));
    c
}

fn windows_candidates(url: &str) -> Vec<(String, Vec<String>)> {
    // `cmd /c start "" <url>` — the empty "" is start's required
    // window-title arg, otherwise start treats <url> as the title.
    vec![
        (
            "cmd".into(),
            vec!["/c".into(), "start".into(), "".into(), url.into()],
        ),
        (
            "cmd.exe".into(),
            vec!["/c".into(), "start".into(), "".into(), url.into()],
        ),
        (
            "powershell".into(),
            vec![
                "-NoProfile".into(),
                "-Command".into(),
                format!("Start-Process '{}'", url.replace('\'', "''")),
            ],
        ),
    ]
}

/// Spawn one opener candidate. Returns `Err` if the binary is missing
/// (so the caller falls through to the next candidate) or exits non-zero.
fn try_open(cmd: &str, args: &[String]) -> Result<()> {
    let status = std::process::Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("spawning `{cmd}`"))?;
    if !status.success() {
        anyhow::bail!("`{cmd}` exited with {status}");
    }
    Ok(())
}

/// Best-effort WSL detection. WSL2 sets `WSL_DISTRO_NAME` and
/// `WSL_INTEROP`; either is sufficient. Cheap env check — no file IO.
pub(crate) fn is_wsl() -> bool {
    std::env::var_os("WSL_DISTRO_NAME").is_some() || std::env::var_os("WSL_INTEROP").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_browser_env_recognises_booleans() {
        for s in ["0", "false", "OFF", "No", "never"] {
            assert_eq!(parse_browser_env(s), BrowserEnv::Off, "{s}");
        }
        for s in ["1", "true", "ON", "Yes", "always"] {
            assert_eq!(parse_browser_env(s), BrowserEnv::On, "{s}");
        }
        assert_eq!(
            parse_browser_env(" firefox "),
            BrowserEnv::Command("firefox".into())
        );
        assert_eq!(
            parse_browser_env("google-chrome %s"),
            BrowserEnv::Command("google-chrome %s".into())
        );
    }

    #[test]
    fn resolve_pref_cli_no_browser_wins() {
        assert_eq!(
            resolve_browser_pref(true, true, Some("1")),
            BrowserPref::Never
        );
        assert_eq!(
            resolve_browser_pref(false, true, Some("firefox")),
            BrowserPref::Never
        );
    }

    #[test]
    fn resolve_pref_cli_browser_forces() {
        assert_eq!(
            resolve_browser_pref(true, false, Some("0")),
            BrowserPref::Always
        );
        assert_eq!(resolve_browser_pref(true, false, None), BrowserPref::Always);
    }

    #[test]
    fn resolve_pref_env_on_off_and_command() {
        assert_eq!(
            resolve_browser_pref(false, false, Some("0")),
            BrowserPref::Never
        );
        assert_eq!(
            resolve_browser_pref(false, false, Some("1")),
            BrowserPref::Always
        );
        assert_eq!(
            resolve_browser_pref(false, false, Some("firefox")),
            BrowserPref::Always
        );
        assert_eq!(resolve_browser_pref(false, false, None), BrowserPref::Auto);
        assert_eq!(
            resolve_browser_pref(false, false, Some("  ")),
            BrowserPref::Auto
        );
    }

    #[test]
    fn should_open_skips_ci_always() {
        assert!(!should_open_browser(BrowserPref::Always, true, true));
        assert!(!should_open_browser(BrowserPref::Auto, true, true));
        assert!(!should_open_browser(BrowserPref::Never, false, true));
    }

    #[test]
    fn should_open_auto_requires_tty() {
        assert!(should_open_browser(BrowserPref::Auto, false, true));
        assert!(!should_open_browser(BrowserPref::Auto, false, false));
        assert!(should_open_browser(BrowserPref::Always, false, false));
    }

    #[test]
    fn parse_opener_appends_url_unless_placeholder() {
        assert_eq!(
            parse_opener_command("firefox", "http://127.0.0.1:8042"),
            ("firefox".into(), vec!["http://127.0.0.1:8042".into()])
        );
        assert_eq!(
            parse_opener_command("google-chrome --new-window %s", "http://x"),
            (
                "google-chrome".into(),
                vec!["--new-window".into(), "http://x".into()]
            )
        );
    }

    #[test]
    fn macos_candidates_use_open() {
        if cfg!(target_os = "macos") {
            let c = opener_candidates("http://127.0.0.1:8042", None, false);
            assert_eq!(c[0], ("open".into(), vec!["http://127.0.0.1:8042".into()]));
        }
    }

    #[test]
    fn windows_candidates_include_start_and_powershell() {
        if cfg!(target_os = "windows") {
            let c = opener_candidates("http://127.0.0.1:8042", None, false);
            assert_eq!(c[0].0, "cmd");
            assert_eq!(c[0].1, vec!["/c", "start", "", "http://127.0.0.1:8042"]);
            assert!(c.iter().any(|(cmd, _)| cmd == "powershell"));
        }
    }

    #[test]
    fn wsl_candidates_try_host_browser_first() {
        let c = opener_candidates("http://127.0.0.1:8042", None, true);
        if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            return; // platform_candidates short-circuits before wsl
        }
        assert_eq!(c[0].0, "wslview");
        assert_eq!(c[1].0, "cmd.exe");
        assert_eq!(c[1].1, vec!["/c", "start", "", "http://127.0.0.1:8042"]);
        assert_eq!(c[2].0, "explorer.exe");
        assert!(c.iter().any(|(cmd, _)| cmd == "xdg-open"));
    }

    #[test]
    fn linux_candidates_cover_common_openers() {
        if cfg!(target_os = "linux") {
            let c = opener_candidates("http://x", None, false);
            let cmds: Vec<&str> = c.iter().map(|(cmd, _)| cmd.as_str()).collect();
            assert_eq!(
                cmds,
                ["xdg-open", "gio", "sensible-browser", "x-www-browser"]
            );
        }
    }

    #[test]
    fn override_command_is_tried_first() {
        let c = opener_candidates("http://x", Some("firefox"), false);
        assert_eq!(c[0], ("firefox".into(), vec!["http://x".into()]));
    }
}
