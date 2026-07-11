//! rustlab-viewer — standalone interactive plot viewer for rustlab.
//!
//! Listens on a Unix socket for plot data from `rustlab` and renders it
//! using egui with zoom, pan, crosshairs, and point readout.
//!
//! Usage:
//!     rustlab-viewer                 # default socket path
//!     rustlab-viewer --socket PATH   # custom socket path
//!     rustlab-viewer --name work     # named session (separate socket)

mod app;
mod figure;
mod net;
mod render;
mod surface;

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// Set on the re-exec'd retry process so a persistent GUI failure exits with
/// an error message instead of retrying forever.
const RETRY_ENV: &str = "RUSTLAB_VIEWER_RETRIED";

/// Failures this soon after launch are treated as startup races (display
/// server not ready yet — seen on WSLg first launch after boot) and retried
/// once. Later failures mean an established session died; relaunching an
/// empty viewer window would be more confusing than an error message.
const STARTUP_RETRY_WINDOW: Duration = Duration::from_secs(10);

fn main() {
    // eframe/winit report GUI failures through `log`; without a logger the
    // real error is discarded and only a generic exit surfaces. Defaults to
    // the `error` level; RUST_LOG=debug shows the full startup trace.
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("rustlab-viewer {}", env!("CARGO_PKG_VERSION"));
        println!("Standalone interactive plot viewer for rustlab\n");
        println!("Usage: rustlab-viewer [OPTIONS]\n");
        println!("Options:");
        println!("  --name NAME    Named session (connect with `viewer on NAME`)");
        println!("  --socket PATH  Custom Unix socket path (overrides --name)");
        println!("  -h, --help     Print help");
        println!("  -V, --version  Print version");
        return;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("rustlab-viewer {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // Parse optional --socket argument (takes precedence)
    if let Some(pos) = args.iter().position(|a| a == "--socket") {
        if let Some(path) = args.get(pos + 1) {
            std::env::set_var("RUSTLAB_VIEWER_SOCK", path);
        }
    } else if let Some(pos) = args.iter().position(|a| a == "--name") {
        if let Some(name) = args.get(pos + 1) {
            let path = rustlab_proto::socket_path_for_name(name);
            std::env::set_var("RUSTLAB_VIEWER_SOCK", path);
        }
    }

    // Resolve the window title from the session name
    let title = if let Some(pos) = args.iter().position(|a| a == "--name") {
        args.get(pos + 1)
            .map(|n| format!("RustLab Viewer — {}", n))
            .unwrap_or_else(|| "RustLab Viewer".to_string())
    } else {
        "RustLab Viewer".to_string()
    };

    // Start socket listener in background. The app does no repaint
    // polling, so the listener wakes the GUI event loop whenever it queues
    // a message. The context cell is filled once the GUI is up; before
    // that, messages just wait in the channel for the first startup frame.
    // (The listener starts before run_native on purpose: it must bind the
    // socket and run its duplicate-viewer check before a window opens.)
    let repaint_ctx: Arc<OnceLock<egui::Context>> = Arc::new(OnceLock::new());
    let wake_ctx = Arc::clone(&repaint_ctx);
    let rx = net::start_listener(Arc::new(move || {
        if let Some(ctx) = wake_ctx.get() {
            ctx.request_repaint();
        }
    }));

    // Launch eframe GUI
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([1024.0, 768.0]),
        ..Default::default()
    };

    let started = Instant::now();
    if let Err(err) = eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            // Fixed dark theme regardless of system preference.
            cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
            let _ = repaint_ctx.set(cc.egui_ctx.clone());
            Ok(Box::new(app::ViewerApp::new(rx)))
        }),
    ) {
        handle_run_error(&err, started.elapsed());
    }

    // Clean up socket on exit
    let sock = rustlab_proto::default_socket_path();
    let _ = std::fs::remove_file(&sock);
}

/// A GUI failure right after launch is often transient (the display server
/// still coming up), so it gets one automatic relaunch before giving up
/// with troubleshooting hints.
fn handle_run_error(err: &eframe::Error, uptime: Duration) -> ! {
    eprintln!("rustlab-viewer: failed to run GUI: {err}");

    if should_retry(std::env::var_os(RETRY_ENV).is_some(), uptime) {
        eprintln!("rustlab-viewer: retrying once (the display server may not be ready yet)...");
        std::thread::sleep(Duration::from_secs(1));
        retry_exec();
    }

    eprintln!();
    eprintln!("The GUI could not start. Things to try:");
    eprintln!("  - run rustlab-viewer again; the first launch after boot can race the display server (common under WSLg)");
    #[cfg(target_os = "linux")]
    {
        eprintln!("  - force X11 instead of Wayland: WAYLAND_DISPLAY= rustlab-viewer");
        eprintln!("  - make sure GL/X libraries are installed (Debian/Ubuntu: libgl1 libegl1 libxkbcommon-x11-0)");
    }
    eprintln!("  - show the underlying error: RUST_LOG=debug rustlab-viewer");
    std::process::exit(1);
}

fn should_retry(already_retried: bool, uptime: Duration) -> bool {
    !already_retried && uptime <= STARTUP_RETRY_WINDOW
}

/// Re-exec the viewer with the same arguments, marking the child via
/// `RETRY_ENV` so it doesn't retry again. The stale socket file left behind
/// is handled by the listener's liveness check. Only returns if the
/// relaunch itself failed.
#[cfg(unix)]
fn retry_exec() {
    use std::os::unix::process::CommandExt;
    if let Ok(exe) = std::env::current_exe() {
        let err = std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .env(RETRY_ENV, "1")
            .exec();
        eprintln!("rustlab-viewer: relaunch failed: {err}");
    }
}

#[cfg(not(unix))]
fn retry_exec() {
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(status) = std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .env(RETRY_ENV, "1")
            .status()
        {
            std::process::exit(status.code().unwrap_or(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_once_within_startup_window_only() {
        // Fresh process failing fast → treated as a startup race, retry.
        assert!(should_retry(false, Duration::from_secs(1)));
        // Boundary: exactly at the window edge still counts as startup.
        assert!(should_retry(false, STARTUP_RETRY_WINDOW));
        // Already the retry process → give up with hints instead of looping.
        assert!(!should_retry(true, Duration::from_secs(1)));
        // Failure long after startup is a dead session, not a race → no relaunch.
        assert!(!should_retry(
            false,
            STARTUP_RETRY_WINDOW + Duration::from_secs(1)
        ));
    }
}
