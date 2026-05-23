//! Socket listener for incoming rustlab connections.

use rustlab_proto::{default_socket_path, read_msg, write_msg, ViewerMsg, ViewerReply};
use std::io::BufWriter;
use std::sync::mpsc;

/// Bound on the listener → app channel. A `PanelHeatmap` from a streaming
/// builtin can carry ~1 MB of RGBA, so a small bound is enough to absorb a
/// normal render frame's worth of messages while ensuring the listener
/// blocks (and so the socket fills, and the producer's `send()` blocks) if
/// the egui app ever falls behind. Picked so the worst-case backlog stays
/// in low tens of MB, not hundreds.
const APP_CHANNEL_BOUND: usize = 64;

/// Start the Unix socket listener in a background thread.
/// Returns a receiver for incoming messages.
///
/// The channel is bounded so backpressure propagates back to the client:
/// when the egui app can't keep up, `tx.send` blocks the listener thread,
/// the socket's kernel buffer fills, and the producer's `write` (and thus
/// its `send` waiting for `Ok`) blocks. Without the bound a slow egui
/// frame would let RGBA-heavy messages pile up in memory unboundedly.
pub fn start_listener() -> mpsc::Receiver<ViewerMsg> {
    let (tx, rx) = mpsc::sync_channel(APP_CHANNEL_BOUND);

    std::thread::Builder::new()
        .name("viewer-listener".into())
        .spawn(move || {
            if let Err(e) = run_listener(tx) {
                eprintln!("rustlab-viewer: listener error: {}", e);
            }
        })
        .expect("failed to spawn listener thread");

    rx
}

fn run_listener(tx: mpsc::SyncSender<ViewerMsg>) -> std::io::Result<()> {
    let path = default_socket_path();

    // Check for existing socket — if a live viewer is listening, refuse to start
    if path.exists() {
        #[cfg(unix)]
        {
            if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&path) {
                // Try a ping to see if it's alive
                if write_msg(&mut stream, &ViewerMsg::Ping).is_ok() {
                    if let Ok(Some(ViewerReply::Pong)) = read_msg::<_, ViewerReply>(&mut stream) {
                        eprintln!(
                            "rustlab-viewer: another viewer is already running on {}",
                            path.display()
                        );
                        eprintln!("  use --name <NAME> to start a separate session");
                        std::process::exit(1);
                    }
                }
            }
        }
        // Stale socket — remove it
        std::fs::remove_file(&path)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::net::UnixListener;

        let listener = UnixListener::bind(&path)?;
        eprintln!("rustlab-viewer: listening on {}", path.display());

        // Clean up socket on exit
        let path_clone = path.clone();
        ctrlc_cleanup(path_clone);

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    eprintln!("rustlab-viewer: client connected");
                    let tx = tx.clone();
                    std::thread::Builder::new()
                        .name("viewer-conn".into())
                        .spawn(move || handle_connection(stream, tx))
                        .ok();
                }
                Err(e) => {
                    eprintln!("rustlab-viewer: accept error: {}", e);
                }
            }
        }
    }

    #[cfg(not(unix))]
    {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:19847")?;
        eprintln!("rustlab-viewer: listening on 127.0.0.1:19847");

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    eprintln!("rustlab-viewer: client connected");
                    let tx = tx.clone();
                    std::thread::Builder::new()
                        .name("viewer-conn".into())
                        .spawn(move || handle_connection(stream, tx))
                        .ok();
                }
                Err(e) => {
                    eprintln!("rustlab-viewer: accept error: {}", e);
                }
            }
        }
    }

    Ok(())
}

fn handle_connection<S: std::io::Read + std::io::Write>(
    mut stream: S,
    tx: mpsc::SyncSender<ViewerMsg>,
) {
    loop {
        match read_msg::<_, ViewerMsg>(&mut stream) {
            Ok(Some(msg)) => {
                // Only reply to messages the producer actually reads
                // back. `ViewerClient::send()` (sync) is used for
                // FigureOpen / Redraw / Reset / Ping; everything else
                // goes through `send_nowait()` and never reads the
                // socket again. Writing a reply for a fire-and-forget
                // message lets Ok bytes pile up in the producer's
                // incoming socket buffer with nothing to drain them.
                // macOS unix-stream buffers default to 8 KB
                // (`net.local.stream.recvspace`), so on the live
                // waterfall demo — which sends two `send_nowait`
                // messages per redraw at ~11 Hz — the reply buffer
                // fills in ~55 s, the listener's write blocks, the
                // producer's next send blocks, and both deadlock with
                // the egui app still responsive. Replying only when a
                // reply is read keeps the back-channel quiescent and
                // restores end-to-end progress.
                let reply = match &msg {
                    ViewerMsg::Ping => Some(ViewerReply::Pong),
                    ViewerMsg::FigureOpen { .. }
                    | ViewerMsg::Redraw { .. }
                    | ViewerMsg::Reset => Some(ViewerReply::Ok),
                    _ => None,
                };
                let is_ping = matches!(msg, ViewerMsg::Ping);
                if !is_ping {
                    if tx.send(msg).is_err() {
                        return; // app shut down
                    }
                }
                if let Some(reply) = reply {
                    let mut bw = BufWriter::new(&mut stream);
                    if write_msg(&mut bw, &reply).is_err() {
                        return;
                    }
                }
            }
            Ok(None) => return, // clean EOF
            Err(_) => return,   // broken pipe
        }
    }
}

#[cfg(unix)]
fn ctrlc_cleanup(path: std::path::PathBuf) {
    // Best-effort: remove socket on SIGINT/SIGTERM via atexit-like pattern.
    // The Drop-based cleanup in main is the primary mechanism.
    std::thread::Builder::new()
        .name("viewer-cleanup".into())
        .spawn(move || {
            // This thread just exists so the path is dropped on process exit
            // via the Drop guard below. We park it forever.
            let _guard = SocketCleanup(path);
            std::thread::park();
        })
        .ok();
}

#[cfg(unix)]
struct SocketCleanup(std::path::PathBuf);

#[cfg(unix)]
impl Drop for SocketCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
