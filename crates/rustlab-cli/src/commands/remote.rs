//! `rustlab remote` — run rustlab on another machine, plots on this one.
//!
//! The viewer is the *server*: `rustlab-viewer` binds a Unix socket and
//! `rustlab` connects to it. So when the compute is remote and the GUI is
//! local, the socket has to travel the other way down the SSH connection —
//! a **remote** forward (`ssh -R`), not the local one people reach for first.
//!
//! This subcommand runs on the machine with the screen. It resolves the local
//! viewer socket, checks something is actually listening, clears a stale socket
//! on the far side, then hands off to `ssh` with the forward set up and
//! `RUSTLAB_VIEWER_SOCK` pointed at the forwarded path:
//!
//! ```text
//!   local:  rustlab-viewer ──binds──> /tmp/rustlab-viewer-501.sock
//!                                            ▲
//!                                       ssh -R tunnel
//!                                            │
//!   remote: rustlab ──connects──> /tmp/rustlab-fwd-1001.sock
//!                                 (RUSTLAB_VIEWER_SOCK)
//! ```
//!
//! See `docs/remote-viewer.md` for the manual `ssh` recipe and per-platform
//! notes (Linux, macOS, WSL).

use anyhow::{bail, Result};
use clap::Args;

#[derive(Args)]
pub struct RemoteArgs {
    /// SSH destination, e.g. `user@host` or a `~/.ssh/config` host alias.
    pub destination: String,

    /// Socket path to create on the remote machine. Defaults to
    /// `/tmp/rustlab-fwd-<remote-uid>.sock`, which is keyed on the remote
    /// user so two people forwarding to the same box don't collide.
    #[arg(long, value_name = "PATH")]
    pub remote_socket: Option<String>,

    /// Command to run on the remote machine.
    #[arg(long, value_name = "CMD", default_value = "rustlab repl --viewer")]
    pub command: String,

    /// Extra option passed verbatim to ssh (repeatable), e.g.
    /// `--ssh-opt -p --ssh-opt 2222`. For anything involved, a `~/.ssh/config`
    /// entry is easier to live with.
    #[arg(long = "ssh-opt", value_name = "OPT")]
    pub ssh_opt: Vec<String>,

    /// Print the ssh command instead of running it. Skips the local viewer
    /// check; still asks the remote for its uid unless `--remote-socket` says
    /// which path to use, in which case it contacts nothing at all.
    #[arg(long)]
    pub print: bool,

    /// Skip the "is a viewer actually listening?" check.
    #[arg(long)]
    pub no_check: bool,
}

/// Longest path a `sockaddr_un` can hold, minus room for the NUL. macOS caps
/// `sun_path` at 104 bytes and Linux at 108; we check against the smaller so a
/// path that works on Linux doesn't fail only on a Mac.
const MAX_SOCKET_PATH: usize = 103;

/// Default socket path to create on the remote side.
///
/// Keyed on the *remote* uid, not the local one: the file lands in the remote
/// `/tmp`, so it's the remote user that has to be unique there.
pub(crate) fn default_remote_socket(remote_uid: &str) -> String {
    format!("/tmp/rustlab-fwd-{remote_uid}.sock")
}

/// Single-quote a string for safe interpolation into the remote shell command.
pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Build the argv for the session ssh call.
///
/// Kept pure so the interesting part — which flags we pass and how user input
/// is escaped — is unit-testable without an SSH server.
pub(crate) fn build_ssh_args(
    destination: &str,
    local_socket: &str,
    remote_socket: &str,
    command: &str,
    ssh_opts: &[String],
) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    // A TTY: the default remote command is an interactive REPL.
    args.push("-t".to_string());
    // Without this a refused forward is silent, and the failure resurfaces
    // later as a baffling "could not connect" from inside the REPL.
    args.push("-o".to_string());
    args.push("ExitOnForwardFailure=yes".to_string());
    args.push("-R".to_string());
    args.push(format!("{remote_socket}:{local_socket}"));
    args.extend(ssh_opts.iter().cloned());
    args.push(destination.to_string());
    // Set the variable inline rather than with `SetEnv`, which would need
    // `AcceptEnv RUSTLAB_VIEWER_SOCK` in the remote sshd_config.
    args.push(format!(
        "RUSTLAB_VIEWER_SOCK={} exec {}",
        shell_quote(remote_socket),
        command
    ));
    args
}

/// Render an argv as a copy-pasteable command line.
fn render(args: &[String]) -> String {
    let mut out = String::from("ssh");
    for a in args {
        out.push(' ');
        if a.contains(' ') || a.contains('\'') {
            out.push_str(&shell_quote(a));
        } else {
            out.push_str(a);
        }
    }
    out
}

/// Ask the remote for its uid, confirm rustlab is installed, and remove a
/// leftover socket — one round trip, because each of these on its own would
/// otherwise cost a connection.
///
/// A stale socket is worth the trip: sshd refuses to bind over an existing
/// file unless the server sets `StreamLocalBindUnlink yes`, and the resulting
/// failure is far from obvious.
fn probe_remote(dest: &str, ssh_opts: &[String], remote_socket: Option<&str>) -> Result<Probe> {
    // When the caller named a socket we can clear it in this same trip;
    // otherwise the uid isn't known yet and cleanup happens on the next line.
    let rm = match remote_socket {
        Some(p) => format!("rm -f {}", shell_quote(p)),
        None => "rm -f \"/tmp/rustlab-fwd-$(id -u).sock\"".to_string(),
    };
    let script = format!(
        "id -u; command -v rustlab >/dev/null && echo HAVE_RUSTLAB || echo NO_RUSTLAB; {rm}"
    );

    let mut cmd = std::process::Command::new("ssh");
    cmd.args(ssh_opts).arg(dest).arg(&script);
    let out = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("could not run ssh: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!(
            "ssh {dest} failed: {}",
            stderr.trim().lines().last().unwrap_or("(no output)")
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut lines = stdout.lines().map(str::trim).filter(|l| !l.is_empty());
    let uid = lines.next().unwrap_or("").to_string();
    let has_rustlab = stdout.contains("HAVE_RUSTLAB");
    if uid.is_empty() || !uid.chars().all(|c| c.is_ascii_digit()) {
        bail!("unexpected reply from `ssh {dest} id -u`: {:?}", stdout);
    }
    Ok(Probe { uid, has_rustlab })
}

struct Probe {
    uid: String,
    has_rustlab: bool,
}

/// Verify a viewer is listening locally before we open an SSH connection.
///
/// Forgetting to start the viewer is the most common way this goes wrong, and
/// catching it here costs one local socket call instead of a confusing session.
#[cfg(unix)]
fn viewer_is_listening(path: &std::path::Path) -> bool {
    use rustlab_proto::{read_msg, write_msg, ViewerMsg, ViewerReply};
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(path) else {
        return false;
    };
    if write_msg(&mut stream, &ViewerMsg::Ping).is_err() {
        return false;
    }
    matches!(read_msg::<_, ViewerReply>(&mut stream), Ok(Some(ViewerReply::Pong)))
}

#[cfg(not(unix))]
fn viewer_is_listening(_path: &std::path::Path) -> bool {
    // Non-unix viewers listen on TCP, which SSH forwards without any of this
    // machinery; `rustlab remote` is a Unix-socket convenience.
    false
}

pub fn execute(args: RemoteArgs) -> Result<()> {
    if cfg!(not(unix)) {
        bail!(
            "rustlab remote forwards a Unix socket and is unix-only.\n  \
             On Windows the viewer listens on TCP instead — use:  ssh -R 19847:localhost:19847 <host>"
        );
    }

    let local_socket = rustlab_proto::default_socket_path();
    let local_display = local_socket.display().to_string();

    // `--print` just renders a command line; requiring a running viewer to
    // show you what would run would be obnoxious.
    if !args.no_check && !args.print && !viewer_is_listening(&local_socket) {
        bail!(
            "no viewer listening on {local_display}\n  \
             start one first:      rustlab-viewer &\n  \
             or point elsewhere:   RUSTLAB_VIEWER_SOCK=... rustlab remote {}\n  \
             (skip this check with --no-check)",
            args.destination
        );
    }

    // Skip the round trip when there is nothing to learn from it: a print with
    // an explicit socket path needs neither the remote uid nor the cleanup.
    let probe = if args.print && args.remote_socket.is_some() {
        None
    } else {
        Some(probe_remote(
            &args.destination,
            &args.ssh_opt,
            args.remote_socket.as_deref(),
        )?)
    };

    if let Some(p) = &probe {
        if !p.has_rustlab {
            eprintln!(
                "warning: no `rustlab` found on {} — the session will open but the command may fail",
                args.destination
            );
            eprintln!("  install it there, and make sure it has the viewer feature:");
            eprintln!(
                "    make install     (or: cargo install --path crates/rustlab-cli --features viewer)"
            );
        }
    }

    let remote_socket = match (args.remote_socket.clone(), &probe) {
        (Some(p), _) => p,
        (None, Some(p)) => default_remote_socket(&p.uid),
        // Unreachable: the probe is only skipped when a socket was given.
        (None, None) => unreachable!("probe is required when --remote-socket is absent"),
    };

    for (which, path) in [("local", &local_display), ("remote", &remote_socket)] {
        if path.len() > MAX_SOCKET_PATH {
            bail!(
                "{which} socket path is {} bytes; the limit is {MAX_SOCKET_PATH}\n  {path}",
                path.len()
            );
        }
    }

    let ssh_args = build_ssh_args(
        &args.destination,
        &local_display,
        &remote_socket,
        &args.command,
        &args.ssh_opt,
    );

    if args.print {
        println!("{}", render(&ssh_args));
        return Ok(());
    }

    eprintln!(
        "{} forwarding {} → {}:{}",
        crate::color::bold_cyan("viewer:"),
        local_display,
        args.destination,
        remote_socket
    );

    let status = std::process::Command::new("ssh")
        .args(&ssh_args)
        .status()
        .map_err(|e| anyhow::anyhow!("could not run ssh: {e}"))?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_socket_is_keyed_on_the_remote_uid() {
        assert_eq!(default_remote_socket("1001"), "/tmp/rustlab-fwd-1001.sock");
        // Two users on one box must not land on the same path.
        assert_ne!(default_remote_socket("1001"), default_remote_socket("1002"));
    }

    #[test]
    fn default_socket_paths_fit_in_sockaddr_un() {
        // A generous uid still leaves plenty of headroom.
        assert!(default_remote_socket("4294967295").len() <= MAX_SOCKET_PATH);
    }

    #[test]
    fn ssh_args_carry_the_forward_and_the_env() {
        let args = build_ssh_args(
            "me@host",
            "/tmp/rustlab-viewer-501.sock",
            "/tmp/rustlab-fwd-1001.sock",
            "rustlab repl --viewer",
            &[],
        );
        let joined = args.join(" ");
        // Remote forward, in remote:local order — the direction is the whole
        // point, so pin it.
        assert!(
            joined.contains("-R /tmp/rustlab-fwd-1001.sock:/tmp/rustlab-viewer-501.sock"),
            "{joined}"
        );
        assert!(joined.contains("ExitOnForwardFailure=yes"), "{joined}");
        assert!(joined.contains("-t"), "{joined}");
        assert!(
            joined.contains("RUSTLAB_VIEWER_SOCK='/tmp/rustlab-fwd-1001.sock' exec rustlab repl --viewer"),
            "{joined}"
        );
        // Destination precedes the remote command.
        let dest = args.iter().position(|a| a == "me@host").unwrap();
        let cmd = args.iter().position(|a| a.starts_with("RUSTLAB_VIEWER_SOCK=")).unwrap();
        assert!(dest < cmd, "destination must come before the command");
    }

    #[test]
    fn extra_ssh_opts_land_before_the_destination() {
        let args = build_ssh_args(
            "me@host",
            "/tmp/a.sock",
            "/tmp/b.sock",
            "rustlab",
            &["-p".to_string(), "2222".to_string()],
        );
        let port = args.iter().position(|a| a == "2222").unwrap();
        let dest = args.iter().position(|a| a == "me@host").unwrap();
        assert!(port < dest, "ssh options must precede the destination: {args:?}");
    }

    #[test]
    fn user_supplied_paths_cannot_break_out_of_the_quoting() {
        let args = build_ssh_args(
            "me@host",
            "/tmp/a.sock",
            "/tmp/evil'; rm -rf ~; echo '.sock",
            "rustlab",
            &[],
        );
        let cmd = args.last().unwrap();
        // The quote is escaped, so the injected text stays one argument.
        assert!(cmd.contains(r"'\''"), "{cmd}");
        assert!(!cmd.contains("; rm -rf ~; echo ;"), "{cmd}");
    }

    #[test]
    fn shell_quote_wraps_and_escapes() {
        assert_eq!(shell_quote("/tmp/x.sock"), "'/tmp/x.sock'");
        assert_eq!(shell_quote("a'b"), r"'a'\''b'");
    }
}
