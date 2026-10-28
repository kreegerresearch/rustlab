//! Thin socket client for communicating with `rustlab-viewer`.

use crate::plot_abort::{self, POLL_MS};
use crate::viewer_warn::{
    record_encode, CostRates, RateModel, MIN_RATE_BYTES, TRANSFER_NANOS_PER_BYTE,
};
use rustlab_proto::{
    default_socket_path, encoded_payload, read_msg, socket_path_for_name, ViewerMsg, ViewerReply,
};
use std::time::Instant;

/// Connection to a running `rustlab-viewer` process.
///
/// `transfer` is this socket's encode-independent send model (bandwidth
/// and round-trip). A new connection starts from the prior; dropping the
/// client drops the model.
pub struct ViewerClient {
    #[cfg(unix)]
    stream: std::os::unix::net::UnixStream,
    #[cfg(not(unix))]
    stream: std::net::TcpStream,
    transfer: RateModel,
}

impl std::fmt::Debug for ViewerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ViewerClient")
    }
}

impl ViewerClient {
    /// Try to connect to a running viewer.  Returns `None` if the viewer
    /// socket does not exist or the connection is refused.
    pub fn connect() -> Option<Self> {
        Self::connect_to_path(&default_socket_path())
    }

    /// Connect to a named viewer session (e.g. `viewer on work`).
    pub fn connect_named(name: &str) -> Option<Self> {
        Self::connect_to_path(&socket_path_for_name(name))
    }

    fn connect_to_path(path: &std::path::Path) -> Option<Self> {
        #[cfg(unix)]
        {
            let stream = std::os::unix::net::UnixStream::connect(path).ok()?;
            stream.set_nonblocking(false).ok()?;
            Some(Self {
                stream,
                transfer: RateModel::prior(TRANSFER_NANOS_PER_BYTE),
            })
        }
        #[cfg(not(unix))]
        {
            let stream = std::net::TcpStream::connect("127.0.0.1:19847").ok()?;
            Some(Self {
                stream,
                transfer: RateModel::prior(TRANSFER_NANOS_PER_BYTE),
            })
        }
    }

    /// Scan and encode rates for this process, plus this connection's
    /// transfer model. The snapshot does not include a send that has not
    /// finished yet.
    pub(crate) fn cost_rates(&self) -> CostRates {
        CostRates::current(self.transfer)
    }

    /// Send a message and wait for the reply.
    pub fn send(&mut self, msg: &ViewerMsg) -> Result<ViewerReply, crate::PlotError> {
        self.transmit(msg, true)?
            .ok_or_else(|| crate::PlotError::ViewerConnection("viewer closed connection".into()))
    }

    /// Send a message without waiting for a reply (fire-and-forget).
    pub fn send_nowait(&mut self, msg: &ViewerMsg) -> Result<(), crate::PlotError> {
        self.transmit(msg, false).map(|_| ())
    }

    /// Encode, then write. `wait` also reads the reply, so the transfer
    /// sample includes that round trip. A failed write, a failed read, or
    /// an abort does not update the encode or transfer model. msgpack
    /// itself runs to completion; the flag is checked before and after it.
    fn transmit(
        &mut self,
        msg: &ViewerMsg,
        wait: bool,
    ) -> Result<Option<ViewerReply>, crate::PlotError> {
        if plot_abort::requested() {
            return Err(crate::PlotError::Interrupted);
        }
        let started = Instant::now();
        let payload = encoded_payload(msg).map_err(map_write_err)?;
        let encode_ns = started.elapsed().as_nanos();
        // The payload is dropped on the way out if we abort here, before
        // any byte is written. That does not teach the estimator.
        if plot_abort::requested() {
            return Err(crate::PlotError::Interrupted);
        }

        let started = Instant::now();
        self.write_payload(&payload)?;
        // A nowait frame that finished is in sync. An abort while reading
        // the reply is reported by `read_reply` and is not recorded.
        let reply = if wait { Some(self.read_reply()?) } else { None };
        commit_rates(
            &mut self.transfer,
            payload.len() as u128,
            encode_ns,
            started.elapsed().as_nanos(),
            true,
        );
        Ok(reply)
    }

    fn write_payload(&mut self, payload: &[u8]) -> Result<(), crate::PlotError> {
        // Non-blocking so a full kernel buffer returns WouldBlock instead
        // of sitting inside `write`. The flag is checked on each wake.
        // Blocking mode is restored on every exit, including abort.
        self.stream
            .set_nonblocking(true)
            .map_err(|err| crate::PlotError::ViewerConnection(err.to_string()))?;
        // The fd is copied so the poll closure does not borrow `stream`
        // while `write` holds it mutably. Timeout or EINTR is `Ok(false)`;
        // the caller checks the flag and waits again.
        #[cfg(unix)]
        let fd = {
            use std::os::unix::io::AsRawFd;
            self.stream.as_raw_fd()
        };
        #[cfg(unix)]
        let mut wait = || wait_until_ready(fd, Wait::Write).map(|_| ());
        #[cfg(not(unix))]
        let mut wait = || wait_until_ready(Wait::Write).map(|_| ());
        let result = plot_abort::write_length_prefixed(&mut self.stream, payload, &mut wait);
        let _ = self.stream.set_nonblocking(false);
        result.map(|_| ())
    }

    fn read_reply(&mut self) -> Result<ViewerReply, crate::PlotError> {
        // The request frame is already written. Stopping while the reply
        // is still unread would desync the next read, so this counts as
        // a partial exchange and the caller drops the socket. The reply
        // itself is short; the long wait is the poll loop above it.
        #[cfg(unix)]
        let fd = {
            use std::os::unix::io::AsRawFd;
            self.stream.as_raw_fd()
        };
        loop {
            if plot_abort::requested() {
                plot_abort::mark_partial();
                return Err(crate::PlotError::Interrupted);
            }
            #[cfg(unix)]
            let ready = wait_until_ready(fd, Wait::Read)?;
            #[cfg(not(unix))]
            let ready = wait_until_ready(Wait::Read)?;
            if !ready {
                continue;
            }
            self.stream
                .set_nonblocking(false)
                .map_err(|err| crate::PlotError::ViewerConnection(err.to_string()))?;
            let reply: Option<ViewerReply> = read_msg(&mut self.stream)
                .map_err(|err| crate::PlotError::ViewerConnection(err.to_string()))?;
            return reply.ok_or_else(|| {
                crate::PlotError::ViewerConnection("viewer closed connection".into())
            });
        }
    }
}

enum Wait {
    Read,
    Write,
}

/// `true` when the socket can proceed. `false` is a timeout; the caller
/// checks the abort flag and waits again. The fd is a copy so this can
/// run while `write` borrows the stream.
#[cfg(unix)]
fn wait_until_ready(fd: std::os::unix::io::RawFd, which: Wait) -> Result<bool, crate::PlotError> {
    let events = match which {
        Wait::Read => libc::POLLIN,
        Wait::Write => libc::POLLOUT,
    };
    let mut pfd = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    let rc = unsafe { libc::poll(&mut pfd, 1, POLL_MS) };
    if rc < 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(crate::PlotError::ViewerConnection(err.to_string()));
    }
    if rc == 0 {
        return Ok(false);
    }
    let bad = libc::POLLERR | libc::POLLHUP | libc::POLLNVAL;
    if pfd.revents & bad != 0 && pfd.revents & events == 0 {
        return Err(crate::PlotError::ViewerConnection(
            "viewer closed connection".into(),
        ));
    }
    Ok(true)
}

#[cfg(not(unix))]
fn wait_until_ready(which: Wait) -> Result<bool, crate::PlotError> {
    let _ = which;
    std::thread::sleep(std::time::Duration::from_millis(POLL_MS as u64));
    Ok(true)
}

/// Record encode and transfer only after the whole send succeeds.
/// An aborted or failed send passes `completed = false` and changes nothing.
pub(crate) fn commit_rates(
    transfer: &mut RateModel,
    bytes: u128,
    encode_ns: u128,
    transfer_ns: u128,
    completed: bool,
) {
    if !completed {
        return;
    }
    record_encode(bytes, encode_ns);
    transfer.observe(bytes, transfer_ns, MIN_RATE_BYTES);
}

/// A frame the peer would reject is not a dead connection: `encoded_payload`
/// returns before any byte is written, and the caller can keep the socket.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer_warn::{
        encode_snapshot, restore_encode, MIN_RATE_BYTES, TRANSFER_NANOS_PER_BYTE,
    };

    struct RestoreEncode(RateModel);
    impl Drop for RestoreEncode {
        fn drop(&mut self) {
            restore_encode(self.0);
        }
    }

    #[test]
    fn aborted_send_does_not_move_encode_or_transfer() {
        let saved = encode_snapshot();
        let _restore = RestoreEncode(saved);
        let mut transfer = RateModel::prior(TRANSFER_NANOS_PER_BYTE);
        commit_rates(
            &mut transfer,
            MIN_RATE_BYTES,
            8 * MIN_RATE_BYTES,
            10 * MIN_RATE_BYTES,
            false,
        );
        assert_eq!(transfer.nanos_per_unit(), TRANSFER_NANOS_PER_BYTE);
        assert_eq!(transfer.latency_ns(), 0);
        assert_eq!(encode_snapshot(), saved);

        commit_rates(
            &mut transfer,
            MIN_RATE_BYTES,
            8 * MIN_RATE_BYTES,
            10 * MIN_RATE_BYTES,
            true,
        );
        assert_ne!(transfer.nanos_per_unit(), TRANSFER_NANOS_PER_BYTE);
        assert_ne!(encode_snapshot(), saved);
    }
}

fn map_write_err(err: std::io::Error) -> crate::PlotError {
    let msg = err.to_string();
    if err.kind() == std::io::ErrorKind::InvalidData && msg.contains("exceeds maximum") {
        crate::PlotError::ViewerPayloadTooLarge(msg)
    } else {
        crate::PlotError::ViewerConnection(msg)
    }
}
