//! Open the watch-server URL in a browser.
//!
//! Policy: open unless `--no-browser` was passed or `CI` is set. There
//! is deliberately no TTY check — IDE, launcher, and piped launches
//! open the browser like a terminal launch does — and no opener
//! override: the OS default browser is used.
//!
//! Openers, tried in order until one succeeds:
//!
//! | OS | Candidates |
//! |---|---|
//! | macOS | `open` |
//! | Windows | `cmd /c start "" <url>`, then PowerShell `Start-Process` |
//! | WSL | `wslview` (Windows host browser), `cmd.exe /c start`, `explorer.exe`, then the Linux list |
//! | Linux | `xdg-open`, `gio open`, `sensible-browser`, `x-www-browser` |
//!
//! Every candidate is spawned and polled for at most [`OPENER_WAIT`]:
//! exit 0 or still running counts as success; a non-zero exit or a
//! missing binary falls through to the next candidate. A browser
//! binary that keeps running (`x-www-browser` with no instance open)
//! therefore never blocks the server from starting.

use anyhow::{Context, Result};
use std::time::{Duration, Instant};

/// How long to wait for an opener before assuming it is the browser
/// itself (still running = success).
pub(crate) const OPENER_WAIT: Duration = Duration::from_millis(1000);
const OPENER_POLL: Duration = Duration::from_millis(25);

/// `CI` is a hard off — GitHub Actions and friends must never spawn a
/// GUI. Otherwise only `--no-browser` suppresses the launch.
pub(crate) fn should_open_browser(no_browser: bool, ci: bool) -> bool {
    !no_browser && !ci
}

pub(crate) fn should_auto_open_browser(no_browser: bool) -> bool {
    should_open_browser(no_browser, std::env::var_os("CI").is_some())
}

/// Shell out to the platform's URL opener. Errors propagate so the
/// caller can log a hint instead of failing the server.
pub(crate) fn open_browser(url: &str) -> Result<()> {
    let mut last_err: Option<anyhow::Error> = None;
    for (cmd, args) in platform_candidates(url, is_wsl()) {
        match try_open(&cmd, &args, OPENER_WAIT) {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no URL opener available")))
}

/// Ordered opener candidates for `url`. `wsl` is injected so tests can
/// pin the WSL list without a real distro.
pub(crate) fn platform_candidates(url: &str, wsl: bool) -> Vec<(String, Vec<String>)> {
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
/// - still running after `wait` → `Ok`: it is the browser itself,
///   left running and reaped on a background thread so the server
///   can start.
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
    fn opens_unless_no_browser_or_ci() {
        assert!(should_open_browser(false, false));
        assert!(!should_open_browser(true, false));
        assert!(!should_open_browser(false, true));
        assert!(!should_open_browser(true, true));
    }

    #[test]
    fn macos_candidates_use_open() {
        if cfg!(target_os = "macos") {
            let c = platform_candidates("http://127.0.0.1:8042", false);
            assert_eq!(
                c,
                vec![("open".into(), vec!["http://127.0.0.1:8042".into()])]
            );
        }
    }

    #[test]
    fn windows_candidates_include_start_and_powershell() {
        if cfg!(target_os = "windows") {
            let c = platform_candidates("http://127.0.0.1:8042", false);
            assert_eq!(c[0].0, "cmd");
            assert_eq!(c[0].1, vec!["/c", "start", "", "http://127.0.0.1:8042"]);
            assert!(c.iter().any(|(cmd, _)| cmd == "powershell"));
        }
    }

    #[test]
    fn wsl_candidates_try_host_browser_first() {
        if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            return; // platform_candidates short-circuits before wsl
        }
        let c = platform_candidates("http://127.0.0.1:8042", true);
        assert_eq!(c[0].0, "wslview");
        assert_eq!(c[1].0, "cmd.exe");
        assert_eq!(c[1].1, vec!["/c", "start", "", "http://127.0.0.1:8042"]);
        assert_eq!(c[2].0, "explorer.exe");
        assert!(c.iter().any(|(cmd, _)| cmd == "xdg-open"));
    }

    #[test]
    fn linux_candidates_cover_common_openers() {
        if cfg!(target_os = "linux") {
            let c = platform_candidates("http://x", false);
            let cmds: Vec<&str> = c.iter().map(|(cmd, _)| cmd.as_str()).collect();
            assert_eq!(
                cmds,
                ["xdg-open", "gio", "sensible-browser", "x-www-browser"]
            );
        }
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
