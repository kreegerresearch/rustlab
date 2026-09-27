//! Open the watch-server URL in a browser.
//!
//! Default policy (locked-in #8): auto-open when stderr is a TTY and
//! `CI` is unset. That misses IDE / launcher launches where stderr is
//! not a TTY. Overrides, highest precedence first:
//!
//! - `--browser` / `--no-browser` (CLI; `--no-browser` wins)
//! - `$RUSTLAB_NOTEBOOK_BROWSER` — `1`/`true`/`on`/`yes`/`always`
//!   force-open, `0`/`false`/`off`/`no`/`never` never-open, `auto`, or
//!   a command (`firefox`, `chrome %s`) used as the opener (implies
//!   always)
//! - `~/.rustlabrc` `[notebook] browser` — same grammar;
//!   [`rustlab_config::BrowserOpen`] is the one parser for both
//! - `$BROWSER` — standard Unix opener command, used when neither the
//!   env var nor the rc key names a command
//!
//! Platform fallbacks after any override:
//!
//! | OS | Candidates |
//! |---|---|
//! | macOS | `open` |
//! | Windows | `cmd /c start "" <url>`, then PowerShell `Start-Process` |
//! | WSL | `wslview` (Windows host browser), `cmd.exe /c start`, `explorer.exe`, then Linux list |
//! | Linux | `xdg-open`, `gio open`, `sensible-browser`, `x-www-browser` |
//!
//! Every candidate is spawned and polled for at most [`OPENER_WAIT`]:
//! exit 0 or still running counts as success; a non-zero exit or a
//! missing binary falls through to the next candidate. A browser
//! binary that keeps running (`firefox` with no instance open,
//! `x-www-browser`) therefore never blocks the server from starting.

use anyhow::{Context, Result};
use rustlab_config::BrowserOpen;
use std::io::IsTerminal;
use std::time::{Duration, Instant};

/// How long to wait for an opener before assuming it is the browser
/// itself (still running = success).
pub(crate) const OPENER_WAIT: Duration = Duration::from_millis(1000);
const OPENER_POLL: Duration = Duration::from_millis(25);

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

/// CLI flags beat the env var, which beats the rc key. `--no-browser`
/// beats `--browser`. An unset or blank env var falls through to `rc`.
pub(crate) fn resolve_browser_pref(
    cli_browser: bool,
    cli_no_browser: bool,
    env_value: Option<&str>,
    rc: Option<&BrowserOpen>,
) -> BrowserPref {
    if cli_no_browser {
        return BrowserPref::Never;
    }
    if cli_browser {
        return BrowserPref::Always;
    }
    let setting = env_value
        .and_then(BrowserOpen::parse)
        .or_else(|| rc.cloned())
        .unwrap_or_default();
    match setting {
        BrowserOpen::Auto => BrowserPref::Auto,
        BrowserOpen::Never => BrowserPref::Never,
        BrowserOpen::Always | BrowserOpen::Command(_) => BrowserPref::Always,
    }
}

/// `CI` is a hard off — GitHub Actions and friends must never spawn a
/// GUI, even with `--browser`, the env var, or the rc key set.
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

pub(crate) fn should_auto_open_browser(
    cli_browser: bool,
    cli_no_browser: bool,
    rc: Option<&BrowserOpen>,
) -> bool {
    let env = std::env::var("RUSTLAB_NOTEBOOK_BROWSER").ok();
    let pref = resolve_browser_pref(cli_browser, cli_no_browser, env.as_deref(), rc);
    let ci = std::env::var_os("CI").is_some();
    should_open_browser(pref, ci, std::io::stderr().is_terminal())
}

/// Opener command override: `$RUSTLAB_NOTEBOOK_BROWSER` when it names a
/// command, else the rc key when it does, else `$BROWSER`.
pub(crate) fn pick_override(
    env_value: Option<&str>,
    rc: Option<&BrowserOpen>,
    browser_env: Option<&str>,
) -> Option<String> {
    let env_cmd = env_value
        .and_then(BrowserOpen::parse)
        .and_then(|b| b.command().map(str::to_string));
    env_cmd
        .or_else(|| rc.and_then(|b| b.command().map(str::to_string)))
        .or_else(|| {
            browser_env
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
}

fn command_override(rc: Option<&BrowserOpen>) -> Option<String> {
    let env = std::env::var("RUSTLAB_NOTEBOOK_BROWSER").ok();
    let browser = std::env::var("BROWSER").ok();
    pick_override(env.as_deref(), rc, browser.as_deref())
}

/// Shell out to the platform's URL opener. Errors propagate so the
/// caller can log a hint instead of failing the server.
pub(crate) fn open_browser(url: &str, rc: Option<&BrowserOpen>) -> Result<()> {
    let override_cmd = command_override(rc);
    let candidates = opener_candidates(url, override_cmd.as_deref(), is_wsl());
    let mut last_err: Option<anyhow::Error> = None;
    for (cmd, args) in &candidates {
        match try_open(cmd, args, OPENER_WAIT) {
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
    // `Command::new("cmd")` resolves cmd.exe itself, so one entry suffices.
    vec![
        (
            "cmd".into(),
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

/// Spawn one opener candidate and poll it for up to `wait`.
///
/// - binary missing → `Err` (caller falls through to the next candidate)
/// - exits non-zero within `wait` → `Err` (same)
/// - exits zero → `Ok`
/// - still running after `wait` → `Ok`: it is the browser itself
///   (`firefox`, `x-www-browser`), left running and reaped on a
///   background thread so the server can start.
pub(crate) fn try_open(cmd: &str, args: &[String], wait: Duration) -> Result<()> {
    let mut child = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("spawning `{cmd}`"))?;
    let deadline = Instant::now() + wait;
    loop {
        match child
            .try_wait()
            .with_context(|| format!("waiting for `{cmd}`"))?
        {
            Some(status) if status.success() => return Ok(()),
            Some(status) => anyhow::bail!("`{cmd}` exited with {status}"),
            None if Instant::now() >= deadline => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(());
            }
            None => std::thread::sleep(OPENER_POLL),
        }
    }
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
    fn resolve_pref_cli_no_browser_wins() {
        assert_eq!(
            resolve_browser_pref(true, true, Some("1"), Some(&BrowserOpen::Always)),
            BrowserPref::Never
        );
        assert_eq!(
            resolve_browser_pref(false, true, Some("firefox"), None),
            BrowserPref::Never
        );
    }

    #[test]
    fn resolve_pref_cli_browser_forces() {
        assert_eq!(
            resolve_browser_pref(true, false, Some("0"), Some(&BrowserOpen::Never)),
            BrowserPref::Always
        );
        assert_eq!(
            resolve_browser_pref(true, false, None, None),
            BrowserPref::Always
        );
    }

    #[test]
    fn resolve_pref_env_on_off_and_command() {
        assert_eq!(
            resolve_browser_pref(false, false, Some("0"), None),
            BrowserPref::Never
        );
        assert_eq!(
            resolve_browser_pref(false, false, Some("TRUE"), None),
            BrowserPref::Always
        );
        assert_eq!(
            resolve_browser_pref(false, false, Some("firefox"), None),
            BrowserPref::Always
        );
        assert_eq!(
            resolve_browser_pref(false, false, Some("auto"), None),
            BrowserPref::Auto
        );
        assert_eq!(
            resolve_browser_pref(false, false, None, None),
            BrowserPref::Auto
        );
        assert_eq!(
            resolve_browser_pref(false, false, Some("  "), None),
            BrowserPref::Auto
        );
    }

    #[test]
    fn resolve_pref_env_beats_rc_which_beats_auto() {
        // env wins over rc
        assert_eq!(
            resolve_browser_pref(false, false, Some("0"), Some(&BrowserOpen::Always)),
            BrowserPref::Never
        );
        // blank env falls through to rc
        assert_eq!(
            resolve_browser_pref(false, false, Some(""), Some(&BrowserOpen::Never)),
            BrowserPref::Never
        );
        assert_eq!(
            resolve_browser_pref(false, false, None, Some(&BrowserOpen::Always)),
            BrowserPref::Always
        );
        assert_eq!(
            resolve_browser_pref(
                false,
                false,
                None,
                Some(&BrowserOpen::Command("firefox".into()))
            ),
            BrowserPref::Always
        );
        assert_eq!(
            resolve_browser_pref(false, false, None, Some(&BrowserOpen::Auto)),
            BrowserPref::Auto
        );
    }

    #[test]
    fn pick_override_env_then_rc_then_browser_var() {
        let rc_cmd = BrowserOpen::Command("chromium %s".into());
        assert_eq!(
            pick_override(Some("firefox"), Some(&rc_cmd), Some("lynx")),
            Some("firefox".into())
        );
        // a boolean env value is not a command → rc command
        assert_eq!(
            pick_override(Some("1"), Some(&rc_cmd), Some("lynx")),
            Some("chromium %s".into())
        );
        // rc boolean is not a command → $BROWSER
        assert_eq!(
            pick_override(None, Some(&BrowserOpen::Always), Some(" lynx ")),
            Some("lynx".into())
        );
        assert_eq!(pick_override(None, None, Some("  ")), None);
        assert_eq!(pick_override(None, None, None), None);
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
            assert_eq!(
                c.iter().filter(|(cmd, _)| cmd.starts_with("cmd")).count(),
                1
            );
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

    #[cfg(unix)]
    #[test]
    fn try_open_exit_codes_and_missing_binary() {
        let none: [String; 0] = [];
        assert!(try_open("true", &none, OPENER_WAIT).is_ok());
        assert!(try_open("false", &none, OPENER_WAIT).is_err());
        let err = try_open("rustlab-no-such-opener-xyz", &none, OPENER_WAIT).unwrap_err();
        assert!(err.to_string().contains("spawning"), "{err:#}");
    }

    #[cfg(unix)]
    #[test]
    fn try_open_does_not_block_on_a_long_lived_browser() {
        // `sleep 5` stands in for a browser binary that keeps running.
        let started = Instant::now();
        let args = ["5".to_string()];
        assert!(try_open("sleep", &args, Duration::from_millis(200)).is_ok());
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "opener blocked for {:?}",
            started.elapsed()
        );
    }
}
