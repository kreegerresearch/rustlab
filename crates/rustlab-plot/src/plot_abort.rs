//! Ctrl+C during a plot sets a flag the send path polls between chunks.
//!
//! A length-prefixed frame cannot be resumed after a short write, so the
//! caller drops that socket when any byte of the frame was written. The
//! handler is installed only while a plot on this thread is in progress
//! and only when stdin is a terminal, then the previous disposition is
//! restored. Ctrl+C at the REPL prompt and during a script that is not
//! inside a plot keep their existing behavior.
//!
//! `std` retries `EINTR` inside `write_all`, so the socket write itself
//! is non-blocking and waits in `poll` (50 ms). The flag is checked
//! between chunks and on each wake. No extra crates.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

#[cfg(any(feature = "viewer", test))]
use std::io::{self, Write};

#[cfg(any(feature = "viewer", test))]
use crate::PlotError;

/// Bytes per `write` while a frame is going out. The flag is checked
/// between these chunks and whenever a non-blocking write waits.
#[cfg(any(feature = "viewer", test))]
pub(crate) const CHUNK_BYTES: usize = 64 * 1024;

/// How long a blocked socket waits before the flag is checked again.
#[cfg(feature = "viewer")]
pub(crate) const POLL_MS: i32 = 50;

/// Set by the signal handler. Any thread may run that handler, so this
/// cannot be thread-local. Cleared when the plot on a thread finishes.
static ABORT: AtomicBool = AtomicBool::new(false);
static SIGNAL_DEPTH: AtomicUsize = AtomicUsize::new(0);
static INSTALL: Mutex<()> = Mutex::new(());

thread_local! {
    /// Polling is per thread. A test that sets the flag does not abort
    /// another thread's envelope scan.
    static LOCAL_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// [`request`] sets this. Tests stay off the process-wide flag.
    static LOCAL_ABORT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static STICKY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static PARTIAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// This thread is inside a plot and Ctrl+C (or [`request`]) asked to stop.
///
/// The sticky flag counts too. Dropping the guard clears the signal
/// flag so the next command is not aborted, but the rest of this
/// statement (another panel in the same plot) still stops.
pub(crate) fn requested() -> bool {
    LOCAL_DEPTH.with(|d| {
        d.get() > 0
            && (LOCAL_ABORT.with(|c| c.get())
                || STICKY.with(|c| c.get())
                || ABORT.load(Ordering::Relaxed))
    })
}

/// Ask this thread's in-progress plot or long computation to stop.
/// Tests use this. The signal handler sets the process-wide flag
/// instead, because it may run on a different thread.
pub fn request_interrupt() {
    LOCAL_ABORT.with(|c| c.set(true));
}

/// Same as [`request_interrupt`]. In-crate tests call this name.
#[cfg(test)]
pub(crate) fn request() {
    request_interrupt();
}

#[cfg(test)]
pub(crate) fn clear_request() {
    LOCAL_ABORT.with(|c| c.set(false));
    ABORT.store(false, Ordering::Relaxed);
}

/// The frame write stopped after at least one byte. The socket is no
/// longer framed; the caller drops it.
#[cfg(any(feature = "viewer", test))]
pub(crate) fn mark_partial() {
    PARTIAL.with(|c| c.set(true));
}

/// `true` once, if a frame was left half-written. Clears the mark.
#[cfg(any(feature = "viewer", test))]
pub(crate) fn take_partial() -> bool {
    PARTIAL.with(|c| c.replace(false))
}

/// Remember that a plot was aborted so the next statement stops even
/// when a caller ignored the [`PlotError::Interrupted`] result.
pub fn note_plot_aborted() {
    STICKY.with(|c| c.set(true));
}

/// Stderr line for a cancelled plot, plus the sticky flag the script
/// loop reads. A second call in the same plot (another frame in the
/// same statement) does not print again.
pub(crate) fn report_aborted() {
    let first = STICKY.with(|c| {
        let already = c.get();
        c.set(true);
        !already
    });
    if first {
        eprintln!("plot aborted");
    }
}

/// Take the sticky abort, if one is pending.
pub fn take_plot_interrupt() -> bool {
    STICKY.with(|c| c.replace(false))
}

/// Drop a sticky abort left over from the previous command. The REPL
/// calls this before reading the next line so that line still runs.
pub fn clear_plot_interrupt() {
    STICKY.with(|c| c.set(false));
    LOCAL_ABORT.with(|c| c.set(false));
    ABORT.store(false, Ordering::Relaxed);
    PARTIAL.with(|c| c.set(false));
}

/// Enables [`requested`] on this thread. Does not install a signal handler.
pub(crate) struct PollGuard;

impl PollGuard {
    pub(crate) fn arm() -> Self {
        LOCAL_DEPTH.with(|d| d.set(d.get().saturating_add(1)));
        Self
    }
}

impl Drop for PollGuard {
    fn drop(&mut self) {
        LOCAL_DEPTH.with(|d| {
            let n = d.get().saturating_sub(1);
            d.set(n);
            // The outermost plot on this thread is done. A leftover flag
            // would abort the next plot that arms a guard on a reused
            // test thread. Partial is consumed by the caller first.
            if n == 0 {
                LOCAL_ABORT.with(|c| c.set(false));
                ABORT.store(false, Ordering::Relaxed);
            }
        });
    }
}

/// Arms Ctrl+C polling for a long computation on this thread.
///
/// On a terminal this installs the same handler plots use: the handler
/// only stores an atomic, and [`interrupt_requested`] sees it between
/// steps. Drop restores the previous disposition. When stdin is not a
/// terminal the handler is not installed, but [`request_interrupt`] still
/// stops the computation. Nested with a plot guard; the outermost drop
/// restores the signal.
pub struct InterruptGuard {
    /// Held so its `Drop` runs with this guard. Not read otherwise.
    _poll: PollGuard,
    installed: bool,
}

impl InterruptGuard {
    pub fn arm() -> Self {
        Self {
            _poll: PollGuard::arm(),
            installed: install_if_tty(),
        }
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        if self.installed {
            restore_if_outer();
        }
    }
}

/// This thread is inside [`InterruptGuard`] or a plot, and Ctrl+C
/// (or [`request_interrupt`]) asked to stop.
pub fn interrupt_requested() -> bool {
    requested()
}

/// Installs a Ctrl+C handler for the duration of a plot.
///
/// Nested guards share one handler. The previous action is restored
/// when the outermost guard drops. When stdin is not a terminal (a
/// notebook, a pipe, CI) this only arms [`PollGuard`] so an explicit
/// [`request`] still works and the process signal disposition is unchanged.
#[cfg(feature = "viewer")]
pub(crate) struct SigintGuard {
    /// Held so its `Drop` runs with this guard. Not read otherwise.
    _poll: PollGuard,
    installed: bool,
}

#[cfg(feature = "viewer")]
impl SigintGuard {
    pub(crate) fn arm() -> Self {
        let poll = PollGuard::arm();
        Self {
            _poll: poll,
            installed: install_if_tty(),
        }
    }
}

#[cfg(feature = "viewer")]
impl Drop for SigintGuard {
    fn drop(&mut self) {
        if self.installed {
            restore_if_outer();
        }
    }
}

fn install_lock() -> std::sync::MutexGuard<'static, ()> {
    INSTALL.lock().unwrap_or_else(|err| err.into_inner())
}

fn install_if_tty() -> bool {
    #[cfg(unix)]
    {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            return false;
        }
        let _lock = install_lock();
        let depth = SIGNAL_DEPTH.load(Ordering::Relaxed);
        if depth == 0 && !install_handler() {
            return false;
        }
        SIGNAL_DEPTH.store(depth.saturating_add(1), Ordering::Relaxed);
        true
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn restore_if_outer() {
    #[cfg(unix)]
    {
        let _lock = install_lock();
        let depth = SIGNAL_DEPTH.load(Ordering::Relaxed).saturating_sub(1);
        SIGNAL_DEPTH.store(depth, Ordering::Relaxed);
        if depth == 0 {
            restore_handler();
        }
    }
}

#[cfg(unix)]
fn install_handler() -> bool {
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handle_sigint as *const () as libc::sighandler_t;
        // No SA_RESTART: poll returns EINTR. No SA_RESETHAND: stay installed
        // for the whole plot. The handler only stores an atomic.
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        let mut old: libc::sigaction = std::mem::zeroed();
        if libc::sigaction(libc::SIGINT, &action, &mut old) != 0 {
            return false;
        }
        *saved_action().lock().unwrap_or_else(|err| err.into_inner()) = Some(old);
        true
    }
}

#[cfg(unix)]
fn restore_handler() {
    unsafe {
        if let Some(old) = saved_action()
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .take()
        {
            libc::sigaction(libc::SIGINT, &old, std::ptr::null_mut());
        }
    }
}

#[cfg(unix)]
fn saved_action() -> &'static Mutex<Option<libc::sigaction>> {
    static PREV: Mutex<Option<libc::sigaction>> = Mutex::new(None);
    &PREV
}

#[cfg(unix)]
extern "C" fn handle_sigint(_: libc::c_int) {
    // Process-wide: this handler may run on a thread other than the
    // one blocked in the plot. `requested` on the plotting thread sees it.
    ABORT.store(true, Ordering::Relaxed);
}

#[cfg(any(feature = "viewer", test))]
fn note_written(written: usize) {
    if written > 0 {
        mark_partial();
    }
}

/// Write one length-prefixed frame, checking [`requested`] between chunks.
///
/// `on_block` runs when `write` returns [`io::ErrorKind::WouldBlock`].
/// It should wait briefly (a socket polls for [`POLL_MS`]) and return.
/// Returns the number of bytes accepted by `w`. On interrupt, a non-zero
/// count has been recorded with [`mark_partial`] and the bytes in `w` are
/// only a prefix of the frame — the caller drops that connection. A later
/// plot writes a whole frame to a new connection.
#[cfg(any(feature = "viewer", test))]
pub(crate) fn write_length_prefixed<W: Write>(
    w: &mut W,
    payload: &[u8],
    on_block: &mut dyn FnMut() -> Result<(), PlotError>,
) -> Result<usize, PlotError> {
    if payload.len() > u32::MAX as usize {
        return Err(PlotError::ViewerPayloadTooLarge(format!(
            "IPC frame length {} exceeds maximum {} bytes",
            payload.len(),
            u32::MAX
        )));
    }
    if requested() {
        return Err(PlotError::Interrupted);
    }
    let len = (payload.len() as u32).to_be_bytes();
    let mut written = 0usize;
    write_bytes(w, &len, &mut written, on_block)?;
    write_bytes(w, payload, &mut written, on_block)?;
    if let Err(err) = w.flush() {
        note_written(written);
        return Err(PlotError::ViewerConnection(err.to_string()));
    }
    Ok(written)
}

#[cfg(any(feature = "viewer", test))]
fn write_bytes<W: Write>(
    w: &mut W,
    buf: &[u8],
    written: &mut usize,
    on_block: &mut dyn FnMut() -> Result<(), PlotError>,
) -> Result<(), PlotError> {
    let mut off = 0usize;
    while off < buf.len() {
        if requested() {
            note_written(*written);
            return Err(PlotError::Interrupted);
        }
        let end = (off + CHUNK_BYTES).min(buf.len());
        match w.write(&buf[off..end]) {
            Ok(0) => {
                note_written(*written);
                return Err(PlotError::ViewerConnection(
                    "viewer closed connection".into(),
                ));
            }
            Ok(n) => {
                off += n;
                *written += n;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => match on_block() {
                Err(PlotError::Interrupted) => {
                    note_written(*written);
                    return Err(PlotError::Interrupted);
                }
                Err(err) => return Err(err),
                Ok(()) => {
                    if requested() {
                        note_written(*written);
                        return Err(PlotError::Interrupted);
                    }
                }
            },
            Err(err) => {
                note_written(*written);
                return Err(PlotError::ViewerConnection(err.to_string()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StopAfter {
        buf: Vec<u8>,
        limit: usize,
    }

    impl Write for StopAfter {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            if self.buf.len() >= self.limit {
                request();
                return Err(io::Error::new(io::ErrorKind::WouldBlock, "stop"));
            }
            let room = self.limit - self.buf.len();
            let n = room.min(data.len()).min(8);
            self.buf.extend_from_slice(&data[..n]);
            Ok(n)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn block_if_requested() -> Result<(), PlotError> {
        if requested() {
            Err(PlotError::Interrupted)
        } else {
            Ok(())
        }
    }

    #[test]
    fn aborted_chunk_is_a_prefix_and_the_next_frame_is_intact() {
        let _guard = PollGuard::arm();
        clear_request();
        let payload = vec![0xABu8; 40];
        let mut stopped = StopAfter {
            buf: Vec::new(),
            limit: 10,
        };
        let err = write_length_prefixed(&mut stopped, &payload, &mut block_if_requested)
            .expect_err("abort");
        assert!(matches!(err, PlotError::Interrupted));
        assert_eq!(stopped.buf.len(), 10);
        assert!(stopped.buf.len() < 4 + payload.len());
        let declared = u32::from_be_bytes(stopped.buf[..4].try_into().unwrap()) as usize;
        assert_eq!(declared, payload.len());
        assert_ne!(stopped.buf.len(), 4 + declared);
        assert!(take_partial(), "a prefix must drop the socket");

        // The next plot is a new connection: a fresh buffer, the flag clear.
        clear_request();
        let mut fresh = Vec::new();
        let n = write_length_prefixed(&mut fresh, b"ok", &mut block_if_requested).expect("resend");
        assert_eq!(n, fresh.len());
        assert_eq!(&fresh[..4], &2u32.to_be_bytes());
        assert_eq!(&fresh[4..], b"ok");
        assert!(!take_partial());
    }

    #[test]
    fn abort_before_any_byte_keeps_the_connection() {
        let _guard = PollGuard::arm();
        request();
        let mut buf = Vec::new();
        let err =
            write_length_prefixed(&mut buf, b"hello", &mut block_if_requested).expect_err("abort");
        assert!(matches!(err, PlotError::Interrupted));
        assert!(buf.is_empty());
        assert!(!take_partial());
    }
}
