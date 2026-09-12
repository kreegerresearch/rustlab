//! Integration tests for remote viewer sessions.
//!
//! `rustlab remote` forwards a Unix socket over SSH so a remote `rustlab` can
//! draw on a local `rustlab-viewer`. The whole arrangement rests on one fact:
//! `RUSTLAB_VIEWER_SOCK` redirects the client to an arbitrary socket path. That
//! is what's pinned here — with a mock viewer standing in for the GUI, so the
//! test needs no display, and a relay standing in for the SSH hop, so the
//! client is genuinely talking through a proxy rather than straight to the
//! listener.

#![cfg(unix)]

use rustlab_proto::{read_msg, write_msg, ViewerMsg, ViewerReply};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;

fn rustlab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustlab"))
}

/// Socket paths must clear `sockaddr_un`'s ~104-byte limit, which the usual
/// `TempDir` path does not, so these live directly in `/tmp` under a
/// pid-tagged name and are cleaned up by `SockPath`.
struct SockPath(PathBuf);

impl SockPath {
    fn new(tag: &str) -> Self {
        let p = PathBuf::from(format!("/tmp/rl-test-{}-{}.sock", std::process::id(), tag));
        let _ = std::fs::remove_file(&p);
        Self(p)
    }
    fn as_str(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for SockPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A stand-in for `rustlab-viewer`: speaks the wire protocol, has no GUI.
/// Returns the names of the messages it received, in order.
fn spawn_mock_viewer(path: &Path) -> mpsc::Receiver<String> {
    let listener = UnixListener::bind(path).expect("bind mock viewer socket");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        while let Ok(Some(msg)) = read_msg::<_, ViewerMsg>(&mut stream) {
            let name = match &msg {
                ViewerMsg::Ping => "Ping",
                ViewerMsg::Reset => "Reset",
                ViewerMsg::FigureOpen { .. } => "FigureOpen",
                ViewerMsg::PanelUpdate { .. } => "PanelUpdate",
                _ => "Other",
            };
            if tx.send(name.to_string()).is_err() {
                return;
            }
            let reply = if matches!(msg, ViewerMsg::Ping) {
                ViewerReply::Pong
            } else {
                ViewerReply::Ok
            };
            if write_msg(&mut stream, &reply).is_err() {
                return;
            }
        }
    });
    rx
}

/// A stand-in for the `ssh -R` hop: accepts on `listen` and pumps bytes to
/// `target`, so the client reaches the viewer through a proxy exactly as it
/// would across a forwarded connection.
fn spawn_relay(listen: &Path, target: &Path) {
    let listener = UnixListener::bind(listen).expect("bind relay socket");
    let target = target.to_path_buf();
    std::thread::spawn(move || {
        while let Ok((client, _)) = listener.accept() {
            let Ok(upstream) = UnixStream::connect(&target) else {
                return;
            };
            let (c2, u2) = (
                client.try_clone().expect("clone"),
                upstream.try_clone().expect("clone"),
            );
            std::thread::spawn(move || pump(client, upstream));
            std::thread::spawn(move || pump(u2, c2));
        }
    });
}

fn pump(mut from: UnixStream, mut to: UnixStream) {
    let mut buf = [0u8; 8192];
    loop {
        match from.read(&mut buf) {
            Ok(0) | Err(_) => {
                let _ = to.shutdown(std::net::Shutdown::Write);
                return;
            }
            Ok(n) => {
                if to.write_all(&buf[..n]).is_err() {
                    return;
                }
            }
        }
    }
}

fn collect(rx: &mpsc::Receiver<String>) -> Vec<String> {
    let mut got = Vec::new();
    while let Ok(name) = rx.recv_timeout(std::time::Duration::from_secs(5)) {
        got.push(name);
    }
    got
}

/// The end-to-end shape of a remote session, minus SSH: plot data reaches a
/// viewer at a non-default socket path, through a relay.
#[test]
#[cfg_attr(not(feature = "viewer"), ignore = "requires the viewer feature")]
fn plots_reach_a_viewer_through_a_forwarded_socket() {
    let viewer_sock = SockPath::new("viewer");
    let fwd_sock = SockPath::new("fwd");
    let rx = spawn_mock_viewer(&viewer_sock.0);
    spawn_relay(&fwd_sock.0, &viewer_sock.0);

    let dir = tempfile::tempdir().expect("tempdir");
    let script = dir.path().join("p.rlab");
    std::fs::write(&script, "plot(sin(linspace(0, 6.28, 64)))\n").expect("write script");

    let out = rustlab()
        // The forwarded path, not the default — this is the mechanism under test.
        .env("RUSTLAB_VIEWER_SOCK", fwd_sock.as_str())
        .args([
            "run",
            script.to_str().unwrap(),
            "--plot",
            "viewer",
        ])
        .output()
        .expect("run script");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("viewer: connected"),
        "expected a viewer connection, got:\n{stderr}"
    );

    let got = collect(&rx);
    for expected in ["Ping", "FigureOpen", "PanelUpdate"] {
        assert!(
            got.iter().any(|m| m == expected),
            "expected {expected} at the viewer end, got {got:?}\nstderr:\n{stderr}"
        );
    }
}

/// Without a viewer at the other end, the client says so and names the path it
/// tried — the detail that makes a broken forward diagnosable.
#[test]
#[cfg_attr(not(feature = "viewer"), ignore = "requires the viewer feature")]
fn a_dead_socket_reports_the_path_it_tried() {
    let missing = "/tmp/rl-test-nonexistent.sock";
    let _ = std::fs::remove_file(missing);

    let dir = tempfile::tempdir().expect("tempdir");
    let script = dir.path().join("p.rlab");
    std::fs::write(&script, "x = 1\n").expect("write script");

    let out = rustlab()
        .env("RUSTLAB_VIEWER_SOCK", missing)
        .args(["run", script.to_str().unwrap(), "--plot", "viewer"])
        .output()
        .expect("run script");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("could not connect"),
        "expected a connection failure, got:\n{stderr}"
    );
    assert!(
        stderr.contains(missing),
        "the failure should name the socket it tried, got:\n{stderr}"
    );
}

/// `--print` renders the ssh command without contacting anything, so users can
/// read, script, or adapt it.
#[test]
fn remote_print_renders_the_ssh_command() {
    let out = rustlab()
        .args([
            "remote",
            "me@host",
            "--remote-socket",
            "/tmp/fwd.sock",
            "--print",
            "--no-check",
        ])
        .env("RUSTLAB_VIEWER_SOCK", "/tmp/local-viewer.sock")
        .output()
        .expect("remote --print runs");
    assert!(out.status.success(), "exited with {}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stdout.starts_with("ssh "), "{stdout}");
    // Remote forward in remote:local order — the direction is the whole point.
    assert!(
        stdout.contains("-R /tmp/fwd.sock:/tmp/local-viewer.sock"),
        "{stdout}"
    );
    assert!(stdout.contains("ExitOnForwardFailure=yes"), "{stdout}");
    assert!(stdout.contains("me@host"), "{stdout}");
    assert!(stdout.contains("RUSTLAB_VIEWER_SOCK="), "{stdout}");
}

/// Forgetting to start the viewer is the common mistake; it should be caught
/// before any SSH connection is attempted.
#[test]
fn remote_without_a_local_viewer_fails_early() {
    let out = rustlab()
        .args(["remote", "me@host"])
        .env("RUSTLAB_VIEWER_SOCK", "/tmp/rl-test-definitely-not-here.sock")
        .output()
        .expect("remote runs");
    assert!(!out.status.success(), "should have failed");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no viewer listening"),
        "expected the missing-viewer message, got:\n{stderr}"
    );
    assert!(stderr.contains("rustlab-viewer"), "{stderr}");
}
