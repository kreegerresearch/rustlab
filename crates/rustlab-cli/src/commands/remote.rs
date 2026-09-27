//! `rustlab remote` — run rustlab on another machine, plots on this one.
//!
//! The viewer is the *server*: `rustlab-viewer` binds a Unix socket and
//! `rustlab` connects to it. So when the compute is remote and the GUI is
//! local, the socket has to travel the other way down the SSH connection —
//! a **remote** forward (`ssh -R`), not the local one people reach for first.
//!
//! This subcommand runs on the machine with the screen. It checks a viewer
//! is listening, then hands off to `ssh` with the forward set up and
//! `RUSTLAB_VIEWER_SOCK` pointed at the forwarded path:
//!
//! ```text
//!   local:  rustlab-viewer ──binds──> /tmp/rustlab-viewer-501.sock
//!                                            ▲
//!                                       ssh -R tunnel
//!                                            │
//!   remote: rustlab ──connects──> /tmp/rustlab-fwd-<session>.sock
//!                                 (RUSTLAB_VIEWER_SOCK)
//! ```
//!
//! Every session gets a fresh socket name and removes it when the remote
//! command exits, so a leftover file from a crashed session can never block
//! the next one and two sessions never collide. That is what makes the
//! wrapper small: no probe of the remote, no stale-socket cleanup up front.
//!
//! See `docs/remote-viewer.md` for the manual `ssh` recipe and per-platform
//! notes (Linux, macOS, WSL).

use anyhow::{bail, Result};
use clap::Args;

#[derive(Args)]
pub struct RemoteArgs {
    /// SSH destination, e.g. `user@host` or a `~/.ssh/config` host alias.
    pub destination: String,

    /// Socket path to create on the remote machine. Default: a fresh
    /// `/tmp/rustlab-fwd-<session>.sock` per session, removed when the
    /// remote command exits.
    #[arg(long, value_name = "PATH")]
    pub remote_socket: Option<String>,

    /// Command to run on the remote machine (through its login shell, which
    /// must be POSIX-compatible: sh, bash, zsh).
    #[arg(long, value_name = "CMD", default_value = "rustlab repl --viewer")]
    pub command: String,

    /// Extra option passed verbatim to ssh (repeatable), e.g.
    /// `--ssh-opt -p --ssh-opt 2222`. For anything involved, a `~/.ssh/config`
    /// entry is easier to live with.
    // ssh options start with a dash, which clap would otherwise read as an
    // unknown flag; `allow_hyphen_values` makes `--ssh-opt -p` work as
    // documented.
    #[arg(long = "ssh-opt", value_name = "OPT", allow_hyphen_values = true)]
    pub ssh_opt: Vec<String>,

    /// Print the ssh command instead of running it. Contacts nothing.
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

/// A socket path no other session will pick: this process id plus the clock.
/// Uniqueness is all that is needed — the socket itself is created owner-only
/// by sshd (`StreamLocalBindMask` defaults to 0177).
pub(crate) fn session_socket() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("/tmp/rustlab-fwd-{}-{:x}.sock", std::process::id(), nanos)
}

/// Single-quote a string for safe interpolation into the remote shell command.
pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The script the remote login shell runs: export the socket path, arrange
/// for the socket file to go away when the command exits (sshd leaves it
/// behind otherwise), then run the command.
pub(crate) fn remote_script(remote_socket: &str, command: &str) -> String {
    let q = shell_quote(remote_socket);
    let cleanup = shell_quote(&format!("rm -f {q}"));
    format!("RUSTLAB_VIEWER_SOCK={q}; export RUSTLAB_VIEWER_SOCK; trap {cleanup} EXIT; {command}")
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
    // The variable is set in the script rather than with `SetEnv`, which
    // would need `AcceptEnv RUSTLAB_VIEWER_SOCK` in the remote sshd_config.
    args.push(remote_script(remote_socket, command));
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
    matches!(
        read_msg::<_, ViewerReply>(&mut stream),
        Ok(Some(ViewerReply::Pong))
    )
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

    let remote_socket = args.remote_socket.clone().unwrap_or_else(session_socket);

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
    fn session_sockets_are_unique_and_fit_in_sockaddr_un() {
        let a = session_socket();
        let b = session_socket();
        assert!(a.starts_with("/tmp/rustlab-fwd-"), "{a}");
        assert!(a.ends_with(".sock"), "{a}");
        assert!(a.len() <= MAX_SOCKET_PATH, "{a}");
        // Two sessions started back to back must not share a path.
        assert_ne!(a, b);
    }

    #[test]
    fn remote_script_exports_the_socket_and_cleans_up_on_exit() {
        let s = remote_script("/tmp/f.sock", "rustlab repl --viewer");
        assert_eq!(
            s,
            "RUSTLAB_VIEWER_SOCK='/tmp/f.sock'; export RUSTLAB_VIEWER_SOCK; \
             trap 'rm -f '\\''/tmp/f.sock'\\''' EXIT; rustlab repl --viewer"
        );
    }

    #[test]
    fn ssh_args_carry_the_forward_and_the_env() {
        let args = build_ssh_args(
            "me@host",
            "/tmp/rustlab-viewer-501.sock",
            "/tmp/rustlab-fwd-1.sock",
            "rustlab repl --viewer",
            &[],
        );
        let joined = args.join(" ");
        // Remote forward, in remote:local order — the direction is the whole
        // point, so pin it.
        assert!(
            joined.contains("-R /tmp/rustlab-fwd-1.sock:/tmp/rustlab-viewer-501.sock"),
            "{joined}"
        );
        assert!(joined.contains("ExitOnForwardFailure=yes"), "{joined}");
        assert!(joined.contains("-t"), "{joined}");
        assert!(
            joined.contains("RUSTLAB_VIEWER_SOCK='/tmp/rustlab-fwd-1.sock'; export"),
            "{joined}"
        );
        assert!(joined.ends_with("EXIT; rustlab repl --viewer"), "{joined}");
        // Destination precedes the remote command.
        let dest = args.iter().position(|a| a == "me@host").unwrap();
        let cmd = args
            .iter()
            .position(|a| a.starts_with("RUSTLAB_VIEWER_SOCK="))
            .unwrap();
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
        assert!(
            port < dest,
            "ssh options must precede the destination: {args:?}"
        );
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
        // The quote is escaped, so the injected text stays one argument —
        // both in the export and inside the cleanup trap.
        assert!(cmd.contains(r"'\''"), "{cmd}");
        assert!(!cmd.contains("; rm -rf ~; echo ;"), "{cmd}");
    }

    #[test]
    fn shell_quote_wraps_and_escapes() {
        assert_eq!(shell_quote("/tmp/x.sock"), "'/tmp/x.sock'");
        assert_eq!(shell_quote("a'b"), r"'a'\''b'");
    }
}
