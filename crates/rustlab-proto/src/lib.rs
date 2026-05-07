//! Wire protocol for rustlab ↔ rustlab-viewer IPC.
//!
//! Messages are length-prefixed msgpack: `[u32 BE length][msgpack bytes]`.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;

// ─── Wire types ─────────────────────────────────────────────────────────────

/// Message sent from rustlab (client) to rustlab-viewer (server).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ViewerMsg {
    /// Create or resize a figure window.
    FigureOpen {
        id: u32,
        rows: u16,
        cols: u16,
        title: String,
    },
    /// Replace all series data in one panel (0-based).
    PanelUpdate {
        fig_id: u32,
        panel: u16,
        series: Vec<WireSeries>,
    },
    /// Set axis labels and title for a panel.
    PanelLabels {
        fig_id: u32,
        panel: u16,
        title: String,
        xlabel: String,
        ylabel: String,
    },
    /// Set fixed axis limits for a panel.
    PanelLimits {
        fig_id: u32,
        panel: u16,
        xlim: (Option<f64>, Option<f64>),
        ylim: (Option<f64>, Option<f64>),
    },
    /// Replace heatmap data in one panel (0-based).
    PanelHeatmap {
        fig_id: u32,
        panel: u16,
        heatmap: WireHeatmap,
    },
    /// Replace 3D surface data in one panel (0-based).
    PanelSurface {
        fig_id: u32,
        panel: u16,
        surface: WireSurface,
    },
    /// Request a redraw of all panels in a figure.
    Redraw { fig_id: u32 },
    /// Close a figure window.
    Close { fig_id: u32 },
    /// Close all figures (sent on new `viewer on` connection).
    Reset,
    /// Keepalive ping.
    Ping,
}

/// Response from rustlab-viewer back to rustlab.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ViewerReply {
    Ok,
    Error(String),
    Pong,
}

/// A single data series on the wire.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WireSeries {
    pub label: String,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub color: WireColor,
    pub style: WireLineStyle,
    pub kind: WirePlotKind,
    /// Categorical x-axis tick labels (e.g. for bar charts with string categories).
    #[serde(default)]
    pub x_labels: Option<Vec<String>>,
}

/// Color on the wire.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WireColor {
    Named(String),
    Rgb(u8, u8, u8),
}

/// Line style on the wire.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WireLineStyle {
    Solid,
    Dashed,
}

/// Plot kind on the wire.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WirePlotKind {
    Line,
    Stem,
    Bar,
    Scatter,
}

/// Pre-rendered heatmap image on the wire.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WireHeatmap {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// RGBA pixel data, row-major, 4 bytes per pixel.
    pub rgba: Vec<u8>,
    /// Hint for the viewer: when true, sample with linear filtering so
    /// upscaling under zoom looks smooth (used for pre-rendered figure
    /// overlays — quiver, streamlines, contour). When false, the viewer
    /// uses nearest-neighbour filtering to preserve cell boundaries on
    /// raw data heatmaps. Defaults to false for backward compatibility.
    #[serde(default)]
    pub smooth: bool,
}

/// Raw 3D surface grid on the wire. The viewer handles projection/shading
/// so the user can rotate, tilt, and zoom interactively.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WireSurface {
    pub nrows: u32,
    pub ncols: u32,
    /// X coordinates, length = ncols.
    pub x: Vec<f64>,
    /// Y coordinates, length = nrows.
    pub y: Vec<f64>,
    /// Row-major z values, length = nrows * ncols.
    pub z: Vec<f64>,
    /// Colorscale name: "viridis" (default), "jet", "hot", "gray".
    pub colorscale: String,
}

// ─── Framing helpers ────────────────────────────────────────────────────────

/// Write a length-prefixed msgpack message.
pub fn write_msg<W: Write, T: Serialize>(w: &mut W, msg: &T) -> std::io::Result<()> {
    let payload = rmp_serde::to_vec(msg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let len = payload.len() as u32;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(&payload)?;
    w.flush()
}

/// Read a length-prefixed msgpack message.  Returns `None` on clean EOF.
pub fn read_msg<R: Read, T: for<'de> Deserialize<'de>>(r: &mut R) -> std::io::Result<Option<T>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    let msg = rmp_serde::from_slice(&buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(msg))
}

// ─── Socket path ────────────────────────────────────────────────────────────

/// Default Unix socket path for the viewer.
///
/// Precedence: `$RUSTLAB_VIEWER_SOCK` > `/tmp/rustlab-viewer-{uid}.sock`
pub fn default_socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("RUSTLAB_VIEWER_SOCK") {
        return PathBuf::from(p);
    }
    #[cfg(unix)]
    {
        let uid = unsafe { libc::getuid() };
        PathBuf::from(format!("/tmp/rustlab-viewer-{}.sock", uid))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from("/tmp/rustlab-viewer.sock")
    }
}

/// Socket path for a named viewer session.
///
/// Returns `/tmp/rustlab-viewer-{uid}-{name}.sock` (Unix) or
/// `/tmp/rustlab-viewer-{name}.sock` (non-Unix).
pub fn socket_path_for_name(name: &str) -> PathBuf {
    #[cfg(unix)]
    {
        let uid = unsafe { libc::getuid() };
        PathBuf::from(format!("/tmp/rustlab-viewer-{}-{}.sock", uid, name))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(format!("/tmp/rustlab-viewer-{}.sock", name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_viewer_msg() {
        let msg = ViewerMsg::PanelUpdate {
            fig_id: 1,
            panel: 0,
            series: vec![WireSeries {
                label: "test".into(),
                x: vec![1.0, 2.0, 3.0],
                y: vec![4.0, 5.0, 6.0],
                color: WireColor::Named("cyan".into()),
                style: WireLineStyle::Solid,
                kind: WirePlotKind::Line,
                x_labels: None,
            }],
        };
        let mut buf = Vec::new();
        write_msg(&mut buf, &msg).unwrap();
        let mut cursor = std::io::Cursor::new(&buf);
        let decoded: Option<ViewerMsg> = read_msg(&mut cursor).unwrap();
        let decoded = decoded.expect("should decode");
        match decoded {
            ViewerMsg::PanelUpdate {
                fig_id,
                panel,
                series,
            } => {
                assert_eq!(fig_id, 1);
                assert_eq!(panel, 0);
                assert_eq!(series.len(), 1);
                assert_eq!(series[0].x, vec![1.0, 2.0, 3.0]);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_reply() {
        let msg = ViewerReply::Error("test error".into());
        let mut buf = Vec::new();
        write_msg(&mut buf, &msg).unwrap();
        let mut cursor = std::io::Cursor::new(&buf);
        let decoded: Option<ViewerReply> = read_msg(&mut cursor).unwrap();
        match decoded.unwrap() {
            ViewerReply::Error(s) => assert_eq!(s, "test error"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn eof_returns_none() {
        let buf: Vec<u8> = vec![];
        let mut cursor = std::io::Cursor::new(&buf);
        let result: Option<ViewerMsg> = read_msg(&mut cursor).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn round_trip_series_with_x_labels() {
        let msg = ViewerMsg::PanelUpdate {
            fig_id: 42,
            panel: 0,
            series: vec![WireSeries {
                label: "sales".into(),
                x: vec![0.0, 1.0, 2.0],
                y: vec![100.0, 200.0, 150.0],
                color: WireColor::Named("blue".into()),
                style: WireLineStyle::Solid,
                kind: WirePlotKind::Bar,
                x_labels: Some(vec!["Jan".into(), "Feb".into(), "Mar".into()]),
            }],
        };
        let mut buf = Vec::new();
        write_msg(&mut buf, &msg).unwrap();
        let mut cursor = std::io::Cursor::new(&buf);
        let decoded: ViewerMsg = read_msg(&mut cursor).unwrap().unwrap();
        match decoded {
            ViewerMsg::PanelUpdate { series, .. } => {
                assert_eq!(
                    series[0].x_labels,
                    Some(vec!["Jan".into(), "Feb".into(), "Mar".into()])
                );
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn x_labels_default_none_for_backwards_compat() {
        // Serialize without x_labels, verify it deserializes as None
        let msg = ViewerMsg::PanelUpdate {
            fig_id: 1,
            panel: 0,
            series: vec![WireSeries {
                label: "test".into(),
                x: vec![1.0],
                y: vec![2.0],
                color: WireColor::Named("red".into()),
                style: WireLineStyle::Solid,
                kind: WirePlotKind::Line,
                x_labels: None,
            }],
        };
        let mut buf = Vec::new();
        write_msg(&mut buf, &msg).unwrap();
        let mut cursor = std::io::Cursor::new(&buf);
        let decoded: ViewerMsg = read_msg(&mut cursor).unwrap().unwrap();
        match decoded {
            ViewerMsg::PanelUpdate { series, .. } => {
                assert_eq!(series[0].x_labels, None);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn socket_path_for_name_contains_name() {
        let path = socket_path_for_name("work");
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains("work"),
            "path should contain session name: {}",
            path_str
        );
        assert!(
            path_str.ends_with(".sock"),
            "path should end with .sock: {}",
            path_str
        );
    }

    #[test]
    fn socket_path_for_name_differs_from_default() {
        let default = default_socket_path();
        let named = socket_path_for_name("test");
        assert_ne!(default, named);
    }

    #[test]
    fn different_names_get_different_paths() {
        let a = socket_path_for_name("alpha");
        let b = socket_path_for_name("beta");
        assert_ne!(a, b);
    }

    #[test]
    fn round_trip_heatmap() {
        let msg = ViewerMsg::PanelHeatmap {
            fig_id: 5,
            panel: 0,
            heatmap: WireHeatmap {
                width: 2,
                height: 2,
                rgba: vec![
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 128, 128, 128, 255,
                ],
                smooth: false,
            },
        };
        let mut buf = Vec::new();
        write_msg(&mut buf, &msg).unwrap();
        let mut cursor = std::io::Cursor::new(&buf);
        let decoded: ViewerMsg = read_msg(&mut cursor).unwrap().unwrap();
        match decoded {
            ViewerMsg::PanelHeatmap {
                fig_id,
                panel,
                heatmap,
            } => {
                assert_eq!(fig_id, 5);
                assert_eq!(panel, 0);
                assert_eq!(heatmap.width, 2);
                assert_eq!(heatmap.height, 2);
                assert_eq!(heatmap.rgba.len(), 16);
                assert_eq!(heatmap.rgba[0], 255); // red channel of first pixel
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_panel_labels() {
        let msg = ViewerMsg::PanelLabels {
            fig_id: 3,
            panel: 1,
            title: "Frequency Response".into(),
            xlabel: "Hz".into(),
            ylabel: "dB".into(),
        };
        let mut buf = Vec::new();
        write_msg(&mut buf, &msg).unwrap();
        let mut cursor = std::io::Cursor::new(&buf);
        let decoded: ViewerMsg = read_msg(&mut cursor).unwrap().unwrap();
        match decoded {
            ViewerMsg::PanelLabels {
                fig_id,
                panel,
                title,
                xlabel,
                ylabel,
            } => {
                assert_eq!(fig_id, 3);
                assert_eq!(panel, 1);
                assert_eq!(title, "Frequency Response");
                assert_eq!(xlabel, "Hz");
                assert_eq!(ylabel, "dB");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_panel_limits() {
        let msg = ViewerMsg::PanelLimits {
            fig_id: 2,
            panel: 0,
            xlim: (Some(0.0), Some(100.0)),
            ylim: (None, Some(50.0)),
        };
        let mut buf = Vec::new();
        write_msg(&mut buf, &msg).unwrap();
        let mut cursor = std::io::Cursor::new(&buf);
        let decoded: ViewerMsg = read_msg(&mut cursor).unwrap().unwrap();
        match decoded {
            ViewerMsg::PanelLimits {
                fig_id,
                panel,
                xlim,
                ylim,
            } => {
                assert_eq!(fig_id, 2);
                assert_eq!(panel, 0);
                assert_eq!(xlim, (Some(0.0), Some(100.0)));
                assert_eq!(ylim, (None, Some(50.0)));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_figure_open() {
        let msg = ViewerMsg::FigureOpen {
            id: 10,
            rows: 2,
            cols: 3,
            title: "Multi-panel".into(),
        };
        let mut buf = Vec::new();
        write_msg(&mut buf, &msg).unwrap();
        let mut cursor = std::io::Cursor::new(&buf);
        let decoded: ViewerMsg = read_msg(&mut cursor).unwrap().unwrap();
        match decoded {
            ViewerMsg::FigureOpen {
                id,
                rows,
                cols,
                title,
            } => {
                assert_eq!(id, 10);
                assert_eq!(rows, 2);
                assert_eq!(cols, 3);
                assert_eq!(title, "Multi-panel");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_close_and_redraw() {
        for msg in [
            ViewerMsg::Close { fig_id: 7 },
            ViewerMsg::Redraw { fig_id: 8 },
        ] {
            let mut buf = Vec::new();
            write_msg(&mut buf, &msg).unwrap();
            let mut cursor = std::io::Cursor::new(&buf);
            let decoded: ViewerMsg = read_msg(&mut cursor).unwrap().unwrap();
            match (&msg, &decoded) {
                (ViewerMsg::Close { fig_id: a }, ViewerMsg::Close { fig_id: b }) => {
                    assert_eq!(a, b)
                }
                (ViewerMsg::Redraw { fig_id: a }, ViewerMsg::Redraw { fig_id: b }) => {
                    assert_eq!(a, b)
                }
                _ => panic!("variant mismatch"),
            }
        }
    }

    #[test]
    fn round_trip_ping_pong() {
        let msg = ViewerMsg::Ping;
        let mut buf = Vec::new();
        write_msg(&mut buf, &msg).unwrap();
        let mut cursor = std::io::Cursor::new(&buf);
        let decoded: ViewerMsg = read_msg(&mut cursor).unwrap().unwrap();
        assert!(matches!(decoded, ViewerMsg::Ping));

        let reply = ViewerReply::Pong;
        buf.clear();
        write_msg(&mut buf, &reply).unwrap();
        let mut cursor = std::io::Cursor::new(&buf);
        let decoded: ViewerReply = read_msg(&mut cursor).unwrap().unwrap();
        assert!(matches!(decoded, ViewerReply::Pong));
    }

    #[test]
    fn round_trip_rgb_color() {
        let msg = ViewerMsg::PanelUpdate {
            fig_id: 1,
            panel: 0,
            series: vec![WireSeries {
                label: "rgb".into(),
                x: vec![0.0],
                y: vec![1.0],
                color: WireColor::Rgb(128, 64, 32),
                style: WireLineStyle::Dashed,
                kind: WirePlotKind::Scatter,
                x_labels: None,
            }],
        };
        let mut buf = Vec::new();
        write_msg(&mut buf, &msg).unwrap();
        let mut cursor = std::io::Cursor::new(&buf);
        let decoded: ViewerMsg = read_msg(&mut cursor).unwrap().unwrap();
        match decoded {
            ViewerMsg::PanelUpdate { series, .. } => {
                assert!(matches!(series[0].color, WireColor::Rgb(128, 64, 32)));
                assert!(matches!(series[0].style, WireLineStyle::Dashed));
                assert!(matches!(series[0].kind, WirePlotKind::Scatter));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn multiple_messages_in_stream() {
        let msgs = vec![
            ViewerMsg::FigureOpen {
                id: 1,
                rows: 1,
                cols: 1,
                title: "fig".into(),
            },
            ViewerMsg::PanelLabels {
                fig_id: 1,
                panel: 0,
                title: "t".into(),
                xlabel: "x".into(),
                ylabel: "y".into(),
            },
            ViewerMsg::Redraw { fig_id: 1 },
        ];
        let mut buf = Vec::new();
        for m in &msgs {
            write_msg(&mut buf, m).unwrap();
        }
        let mut cursor = std::io::Cursor::new(&buf);
        for _ in 0..3 {
            let decoded: Option<ViewerMsg> = read_msg(&mut cursor).unwrap();
            assert!(decoded.is_some());
        }
        // Next read should be EOF
        let eof: Option<ViewerMsg> = read_msg(&mut cursor).unwrap();
        assert!(eof.is_none());
    }
}
