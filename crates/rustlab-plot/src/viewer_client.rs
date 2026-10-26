//! Thin socket client for communicating with `rustlab-viewer`.

use crate::viewer_warn::{
    record_encode, CostRates, RateModel, MIN_RATE_BYTES, TRANSFER_NANOS_PER_BYTE,
};
use rustlab_proto::{
    default_socket_path, encoded_payload, read_msg, socket_path_for_name, write_frame, ViewerMsg,
    ViewerReply,
};
use std::io::BufWriter;
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
    /// sample includes that round trip. A failed write or read does not
    /// update the transfer model.
    fn transmit(
        &mut self,
        msg: &ViewerMsg,
        wait: bool,
    ) -> Result<Option<ViewerReply>, crate::PlotError> {
        let started = Instant::now();
        let payload = encoded_payload(msg).map_err(map_write_err)?;
        record_encode(payload.len() as u128, started.elapsed().as_nanos());

        let started = Instant::now();
        write_frame(&mut BufWriter::new(&mut self.stream), &payload).map_err(map_write_err)?;
        let reply = if wait {
            let reply: Option<ViewerReply> = read_msg(&mut self.stream)
                .map_err(|e| crate::PlotError::ViewerConnection(e.to_string()))?;
            Some(reply.ok_or_else(|| {
                crate::PlotError::ViewerConnection("viewer closed connection".into())
            })?)
        } else {
            None
        };
        self.transfer.observe(
            payload.len() as u128,
            started.elapsed().as_nanos(),
            MIN_RATE_BYTES,
        );
        Ok(reply)
    }
}

/// A frame the peer would reject is not a dead connection: `encoded_payload`
/// returns before any byte is written, and the caller can keep the socket.
fn map_write_err(err: std::io::Error) -> crate::PlotError {
    let msg = err.to_string();
    if err.kind() == std::io::ErrorKind::InvalidData && msg.contains("exceeds maximum") {
        crate::PlotError::ViewerPayloadTooLarge(msg)
    } else {
        crate::PlotError::ViewerConnection(msg)
    }
}
