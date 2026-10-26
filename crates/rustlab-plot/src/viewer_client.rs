//! Thin socket client for communicating with `rustlab-viewer`.

use rustlab_proto::{
    default_socket_path, read_msg, socket_path_for_name, write_msg, ViewerMsg, ViewerReply,
};
use std::io::BufWriter;

/// Connection to a running `rustlab-viewer` process.
pub struct ViewerClient {
    #[cfg(unix)]
    stream: std::os::unix::net::UnixStream,
    #[cfg(not(unix))]
    stream: std::net::TcpStream,
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
            Some(Self { stream })
        }
        #[cfg(not(unix))]
        {
            let stream = std::net::TcpStream::connect("127.0.0.1:19847").ok()?;
            Some(Self { stream })
        }
    }

    /// Send a message and wait for the reply.
    pub fn send(&mut self, msg: &ViewerMsg) -> Result<ViewerReply, crate::PlotError> {
        write_msg(&mut BufWriter::new(&mut self.stream), msg).map_err(map_write_err)?;
        let reply: Option<ViewerReply> = read_msg(&mut self.stream)
            .map_err(|e| crate::PlotError::ViewerConnection(e.to_string()))?;
        reply.ok_or_else(|| crate::PlotError::ViewerConnection("viewer closed connection".into()))
    }

    /// Send a message without waiting for a reply (fire-and-forget).
    pub fn send_nowait(&mut self, msg: &ViewerMsg) -> Result<(), crate::PlotError> {
        write_msg(&mut BufWriter::new(&mut self.stream), msg).map_err(map_write_err)
    }
}

/// A frame the peer would reject is not a dead connection: `write_msg`
/// returns before any byte is written, and the caller can keep the socket.
fn map_write_err(err: std::io::Error) -> crate::PlotError {
    let msg = err.to_string();
    if err.kind() == std::io::ErrorKind::InvalidData && msg.contains("exceeds maximum") {
        crate::PlotError::ViewerPayloadTooLarge(msg)
    } else {
        crate::PlotError::ViewerConnection(msg)
    }
}
