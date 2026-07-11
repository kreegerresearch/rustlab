//! Socket listener for incoming rustlab connections.

use rustlab_proto::{default_socket_path, read_msg, write_msg, ViewerMsg, ViewerReply};
use std::io::BufWriter;
use std::sync::mpsc;
use std::sync::Arc;

/// Wakes the GUI event loop after a message is queued. The app does no
/// repaint polling (an idle viewer draws no frames), so without this call
/// a queued message would sit in the channel until some input event
/// happened to produce a frame.
pub type WakeFn = Arc<dyn Fn() + Send + Sync>;

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
pub fn start_listener(wake: WakeFn) -> mpsc::Receiver<ViewerMsg> {
    let (tx, rx) = mpsc::sync_channel(APP_CHANNEL_BOUND);

    std::thread::Builder::new()
        .name("viewer-listener".into())
        .spawn(move || {
            if let Err(e) = run_listener(tx, wake) {
                eprintln!("rustlab-viewer: listener error: {}", e);
            }
        })
        .expect("failed to spawn listener thread");

    rx
}

fn run_listener(tx: mpsc::SyncSender<ViewerMsg>, wake: WakeFn) -> std::io::Result<()> {
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
                    let wake = wake.clone();
                    std::thread::Builder::new()
                        .name("viewer-conn".into())
                        .spawn(move || handle_connection(stream, tx, wake))
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
                    let wake = wake.clone();
                    std::thread::Builder::new()
                        .name("viewer-conn".into())
                        .spawn(move || handle_connection(stream, tx, wake))
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
    wake: WakeFn,
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
                    wake();
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    fn open_fig(id: u32) -> ViewerMsg {
        ViewerMsg::FigureOpen {
            id,
            rows: 1,
            cols: 1,
            title: String::new(),
        }
    }

    /// Mirror `connect_viewer_impl` + a plot: Ping(read Pong), Reset(read Ok),
    /// FigureOpen(read Ok), PanelUpdate(no read), Redraw(read Ok). A reply
    /// imbalance would hang here, so we put a read timeout on the stream.
    fn client_session(path: &std::path::Path, id: u32) {
        let mut s = UnixStream::connect(path).expect("connect");
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

        write_msg(&mut s, &ViewerMsg::Ping).unwrap();
        assert!(matches!(
            read_msg::<_, ViewerReply>(&mut s).unwrap(),
            Some(ViewerReply::Pong)
        ));

        write_msg(&mut s, &ViewerMsg::Reset).unwrap();
        assert!(matches!(
            read_msg::<_, ViewerReply>(&mut s).unwrap(),
            Some(ViewerReply::Ok)
        ));

        write_msg(&mut s, &open_fig(id)).unwrap();
        assert!(matches!(
            read_msg::<_, ViewerReply>(&mut s).unwrap(),
            Some(ViewerReply::Ok)
        ));

        // fire-and-forget — server must NOT reply
        write_msg(
            &mut s,
            &ViewerMsg::PanelUpdate {
                fig_id: id,
                panel: 0,
                series: vec![],
            },
        )
        .unwrap();

        write_msg(&mut s, &ViewerMsg::Redraw { fig_id: id }).unwrap();
        assert!(matches!(
            read_msg::<_, ViewerReply>(&mut s).unwrap(),
            Some(ViewerReply::Ok)
        ));
        s.flush().ok();
    }

    /// "Connect twice" / "new session while the viewer is running": two
    /// clients drive a full session against one live listener. The app-side
    /// receiver keeps draining so backpressure can't wedge the listener.
    fn unique_sock(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rustlab-viewer-test-{}-{}.sock",
            std::process::id(),
            tag
        ))
    }

    /// `RUSTLAB_VIEWER_SOCK` is process-global; the two session tests
    /// below each set/remove it, so they must not run concurrently or
    /// one test's listener can bind the other's (or a missing) path.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn two_sequential_client_sessions_against_one_listener() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let sock = unique_sock("seq");
        let _ = std::fs::remove_file(&sock);
        std::env::set_var("RUSTLAB_VIEWER_SOCK", &sock);

        let rx = start_listener(Arc::new(|| {}));
        // Drain in the background so tx.send never blocks the conn threads.
        std::thread::spawn(move || while rx.recv().is_ok() {});

        // Wait for the listener to bind.
        let mut tries = 0;
        while !sock.exists() && tries < 200 {
            std::thread::sleep(Duration::from_millis(10));
            tries += 1;
        }
        assert!(sock.exists(), "listener never bound the socket");

        client_session(&sock, (111u32 << 16) | 0);
        client_session(&sock, (222u32 << 16) | 0);

        std::env::remove_var("RUSTLAB_VIEWER_SOCK");
        let _ = std::fs::remove_file(&sock);
    }

    /// Two clients connected *simultaneously*, interleaving traffic — closest
    /// to two rustlab REPLs both holding `viewer on`.
    #[test]
    fn two_concurrent_client_sessions() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let sock = unique_sock("conc");
        let _ = std::fs::remove_file(&sock);
        std::env::set_var("RUSTLAB_VIEWER_SOCK", &sock);

        let rx = start_listener(Arc::new(|| {}));
        std::thread::spawn(move || while rx.recv().is_ok() {});

        let mut tries = 0;
        while !sock.exists() && tries < 200 {
            std::thread::sleep(Duration::from_millis(10));
            tries += 1;
        }
        assert!(sock.exists());

        let p1 = sock.clone();
        let p2 = sock.clone();
        let a = std::thread::spawn(move || client_session(&p1, (111u32 << 16) | 0));
        let b = std::thread::spawn(move || client_session(&p2, (222u32 << 16) | 0));
        a.join().unwrap();
        b.join().unwrap();

        std::env::remove_var("RUSTLAB_VIEWER_SOCK");
        let _ = std::fs::remove_file(&sock);
    }

    /// In-memory stand-in for a client socket: replays a fixed byte stream,
    /// discards replies. `handle_connection` is generic over Read + Write
    /// precisely so it can be driven without a real socket.
    struct MockStream {
        input: std::io::Cursor<Vec<u8>>,
        replies: Vec<u8>,
    }

    impl std::io::Read for MockStream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            std::io::Read::read(&mut self.input, buf)
        }
    }

    impl std::io::Write for MockStream {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.replies.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// The GUI has no repaint polling, so every forwarded message must fire
    /// the wake callback — and connection-level messages (Ping) must not,
    /// or an idle client keepalive would keep repainting the window.
    #[test]
    fn wake_fires_per_forwarded_message_not_for_ping() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let mut input = Vec::new();
        write_msg(&mut input, &ViewerMsg::Ping).unwrap();
        write_msg(&mut input, &open_fig(7)).unwrap();
        write_msg(
            &mut input,
            &ViewerMsg::PanelUpdate {
                fig_id: 7,
                panel: 0,
                series: vec![],
            },
        )
        .unwrap();
        write_msg(&mut input, &ViewerMsg::Redraw { fig_id: 7 }).unwrap();

        let (tx, rx) = mpsc::sync_channel(8);
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = wakes.clone();
        let wake: WakeFn = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        let stream = MockStream {
            input: std::io::Cursor::new(input),
            replies: Vec::new(),
        };
        handle_connection(stream, tx, wake);

        assert_eq!(rx.try_iter().count(), 3, "Ping must not be forwarded");
        assert_eq!(wakes.load(Ordering::SeqCst), 3);
    }
}
