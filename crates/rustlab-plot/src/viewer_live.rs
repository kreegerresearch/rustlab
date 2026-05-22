//! `ViewerFigure` — a `LivePlot` backend that sends data to `rustlab-viewer`
//! over a Unix socket instead of rendering in the terminal.
//!
//! Also provides `connect_viewer()` / `disconnect_viewer()` / `viewer_active()`
//! / `sync_viewer()` for routing regular (non-live) plot commands to the viewer.

use crate::figure::{FigureState, LineStyle, PlotKind, SeriesColor, FIGURE};
use crate::viewer_client::ViewerClient;
use crate::{LivePlot, PlotError};
use rustlab_proto::{
    ViewerMsg, WireColor, WireHeatmap, WireLineStyle, WirePlotKind, WireSeries, WireSurface,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_FIG_ID: AtomicU32 = AtomicU32::new(0);

/// Initialize figure ID counter with a PID-based prefix to avoid collisions
/// when multiple rustlab processes connect to the same viewer.
/// Layout: upper 16 bits = PID (truncated), lower 16 bits = local counter.
fn next_fig_id() -> u32 {
    let local = NEXT_FIG_ID.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id() as u32;
    (pid << 16) | (local & 0xFFFF)
}

/// A live figure backed by the external `rustlab-viewer` process.
#[derive(Debug)]
pub struct ViewerFigure {
    client: ViewerClient,
    fig_id: u32,
}

impl ViewerFigure {
    /// Try to connect to a running viewer and open a figure.
    /// If a named session is active (set by `connect_viewer_named`), dial that
    /// socket; otherwise fall back to the default socket. Returns `None` if
    /// the viewer is not running.
    pub fn connect(rows: usize, cols: usize) -> Option<Self> {
        let session = VIEWER_SESSION.with(|s| s.borrow().clone());
        let mut client = match session {
            Some(ref name) => ViewerClient::connect_named(name)?,
            None => ViewerClient::connect()?,
        };
        let fig_id = next_fig_id();
        let msg = ViewerMsg::FigureOpen {
            id: fig_id,
            rows: rows as u16,
            cols: cols as u16,
            title: String::new(),
        };
        client.send(&msg).ok()?;
        Some(Self { client, fig_id })
    }
}

impl LivePlot for ViewerFigure {
    fn update_panel(&mut self, idx: usize, x: Vec<f64>, y: Vec<f64>) {
        let msg = ViewerMsg::PanelUpdate {
            fig_id: self.fig_id,
            panel: idx as u16,
            series: vec![WireSeries {
                label: String::new(),
                x,
                y,
                color: WireColor::Named("cyan".into()),
                style: WireLineStyle::Solid,
                kind: WirePlotKind::Line,
                x_labels: None,
            }],
        };
        let _ = self.client.send_nowait(&msg);
    }

    fn set_panel_labels(&mut self, idx: usize, title: &str, xlabel: &str, ylabel: &str) {
        let msg = ViewerMsg::PanelLabels {
            fig_id: self.fig_id,
            panel: idx as u16,
            title: title.to_string(),
            xlabel: xlabel.to_string(),
            ylabel: ylabel.to_string(),
        };
        let _ = self.client.send_nowait(&msg);
    }

    fn set_panel_limits(
        &mut self,
        idx: usize,
        xlim: (Option<f64>, Option<f64>),
        ylim: (Option<f64>, Option<f64>),
    ) {
        let msg = ViewerMsg::PanelLimits {
            fig_id: self.fig_id,
            panel: idx as u16,
            xlim,
            ylim,
        };
        let _ = self.client.send_nowait(&msg);
    }

    fn update_panel_heatmap(
        &mut self,
        idx: usize,
        matrix: &rustlab_core::RMatrix,
        colormap: &str,
        vmin: Option<f64>,
        vmax: Option<f64>,
        origin: crate::figure::HeatmapOrigin,
    ) {
        let (nrows, ncols) = (matrix.nrows(), matrix.ncols());
        if nrows == 0 || ncols == 0 {
            return;
        }
        // Send the heatmap to the viewer as a tight one-pixel-per-cell
        // RGBA buffer with no axes / title / labels baked in. The viewer
        // wraps this in `egui_plot::PlotImage` and draws its own axes at
        // native screen resolution. Doing the full-chart pre-rasterize
        // here (the way `render_panel_to_rgba` does for static-export
        // panels) produced a "double axes" artefact — one set baked into
        // the texture, one drawn natively by egui — with the baked set
        // rendering at 2400×1600 plotter scale and looking microscopic
        // when downscaled to fit the actual viewer panel.
        //
        // `origin` controls the vertical flip inside
        // `render_heatmap_cells_to_rgba`: Lower (default — spectrogram)
        // puts row 0 at the bottom; Upper (waterfall) puts row 0 at the
        // top so scrolling history reads newest-on-top.
        let z: Vec<Vec<f64>> = (0..nrows)
            .map(|r| (0..ncols).map(|c| matrix[(r, c)]).collect())
            .collect();
        let hm = crate::figure::HeatmapData {
            z,
            colorscale: colormap.to_string(),
            kind: crate::figure::HeatmapKind::Imagesc,
            x_labels: None,
            y_labels: None,
            rgba: None,
            rgba_width: 0,
            rgba_height: 0,
            value_min: vmin,
            value_max: vmax,
            origin,
        };
        let rgba = crate::file::render_heatmap_cells_to_rgba(&hm);
        if rgba.is_empty() {
            return;
        }
        let _ = self.client.send_nowait(&ViewerMsg::PanelHeatmap {
            fig_id: self.fig_id,
            panel: idx as u16,
            heatmap: WireHeatmap {
                width: ncols as u32,
                height: nrows as u32,
                rgba,
                // `smooth: true` lets egui linearly interpolate between
                // cells so the spectrogram doesn't show staircase aliasing
                // when the texture is upscaled to the panel size.
                smooth: true,
                // `x_extent` / `y_extent` are populated from the panel's
                // `plot_limits` on the viewer side at render time —
                // sending them here would duplicate that state and force
                // a wire round-trip every time the script tweaks limits.
                // Leaving them None lets the viewer use plot_limits
                // (which arrives via a separate PanelLimits message) as
                // the source of truth.
                x_extent: None,
                y_extent: None,
                value_min: vmin,
                value_max: vmax,
                colorscale: colormap.to_string(),
            },
        });
    }

    fn redraw(&mut self) -> Result<(), PlotError> {
        let msg = ViewerMsg::Redraw {
            fig_id: self.fig_id,
        };
        self.client.send(&msg)?;
        Ok(())
    }
}

impl Drop for ViewerFigure {
    fn drop(&mut self) {
        let msg = ViewerMsg::Close {
            fig_id: self.fig_id,
        };
        let _ = self.client.send_nowait(&msg);
    }
}

// ─── Conversion helpers ─────────────────────────────────────────────────────

pub fn color_to_wire(c: &SeriesColor) -> WireColor {
    match c {
        SeriesColor::Blue => WireColor::Named("blue".into()),
        SeriesColor::Red => WireColor::Named("red".into()),
        SeriesColor::Green => WireColor::Named("green".into()),
        SeriesColor::Cyan => WireColor::Named("cyan".into()),
        SeriesColor::Magenta => WireColor::Named("magenta".into()),
        SeriesColor::Yellow => WireColor::Named("yellow".into()),
        SeriesColor::Black => WireColor::Named("black".into()),
        SeriesColor::White => WireColor::Named("white".into()),
        SeriesColor::Rgb(r, g, b) => WireColor::Rgb(*r, *g, *b),
    }
}

pub fn style_to_wire(s: &LineStyle) -> WireLineStyle {
    match s {
        LineStyle::Solid => WireLineStyle::Solid,
        LineStyle::Dashed => WireLineStyle::Dashed,
    }
}

pub fn kind_to_wire(k: &PlotKind) -> WirePlotKind {
    match k {
        PlotKind::Line => WirePlotKind::Line,
        PlotKind::Stem => WirePlotKind::Stem,
        PlotKind::Bar => WirePlotKind::Bar,
        PlotKind::Scatter => WirePlotKind::Scatter,
    }
}

// ─── Viewer connection for regular (non-live) plotting ─────────────────────

struct ViewerConn {
    client: ViewerClient,
    fig_id: u32,
    /// Last-sent subplot layout (rows, cols) — avoid resending FigureOpen.
    layout: (usize, usize),
    /// Last-sent figure-level title; empty unless promoted from a single-
    /// subplot's `title(...)`. Re-sending FigureOpen with a new title is
    /// what gets the user's title onto the viewer's window title bar.
    last_figure_title: String,
}

thread_local! {
    /// Active viewer connection for regular plot commands.
    static VIEWER_CONN: RefCell<Option<ViewerConn>> = RefCell::new(None);
    /// Session name used for the active connection (None = default socket).
    /// Consulted by `ViewerFigure::connect` so `figure_live` hits the same
    /// session as static plots when the user passed `--viewer-name NAME`.
    static VIEWER_SESSION: RefCell<Option<String>> = RefCell::new(None);
}

/// Try to connect to a running viewer. Returns Ok(true) if connected,
/// Ok(false) if the viewer is not running.
pub fn connect_viewer() -> Result<bool, PlotError> {
    VIEWER_SESSION.with(|s| *s.borrow_mut() = None);
    connect_viewer_impl(ViewerClient::connect())
}

/// Connect to a named viewer session (e.g. `viewer on work`).
pub fn connect_viewer_named(name: &str) -> Result<bool, PlotError> {
    VIEWER_SESSION.with(|s| *s.borrow_mut() = Some(name.to_string()));
    connect_viewer_impl(ViewerClient::connect_named(name))
}

fn connect_viewer_impl(client: Option<ViewerClient>) -> Result<bool, PlotError> {
    let Some(mut client) = client else {
        return Ok(false);
    };
    // Verify the connection is live
    match client.send(&ViewerMsg::Ping) {
        Ok(rustlab_proto::ViewerReply::Pong) => {}
        _ => return Ok(false),
    }
    // Clear any figures from previous sessions
    let _ = client.send(&ViewerMsg::Reset);
    let fig_id = next_fig_id();
    VIEWER_CONN.with(|c| {
        *c.borrow_mut() = Some(ViewerConn {
            client,
            fig_id,
            layout: (0, 0), // forces FigureOpen on first sync
            last_figure_title: String::new(),
        })
    });
    Ok(true)
}

/// Disconnect from the viewer and return to TUI mode.
/// Closes all viewer figures.
pub fn disconnect_viewer() {
    VIEWER_CONN.with(|c| {
        if let Some(mut conn) = c.borrow_mut().take() {
            let _ = conn.client.send_nowait(&ViewerMsg::Close {
                fig_id: conn.fig_id,
            });
        }
    });
    VIEWER_SESSION.with(|s| *s.borrow_mut() = None);
}

/// Tell a connected viewer to close a single figure window. No-op when no
/// viewer is connected, so callers from `close()` can use it unconditionally.
/// Send is best-effort: a failure here just means the viewer window is
/// gone, in which case the user already has nothing to close.
pub fn viewer_close(fig_id: u32) {
    VIEWER_CONN.with(|c| {
        if let Some(ref mut conn) = *c.borrow_mut() {
            let _ = conn.client.send_nowait(&ViewerMsg::Close { fig_id });
        }
    });
}

/// Tell a connected viewer to close every figure window. No-op when no
/// viewer is connected. Used by `close all`. Keeps the connection itself
/// open so subsequent plots route to fresh viewer figures.
pub fn viewer_reset() {
    VIEWER_CONN.with(|c| {
        if let Some(ref mut conn) = *c.borrow_mut() {
            let _ = conn.client.send_nowait(&ViewerMsg::Reset);
            // The viewer just emptied its figure map; force a fresh
            // FigureOpen on the next sync by clearing layout/title state.
            conn.layout = (0, 0);
            conn.last_figure_title = String::new();
        }
    });
}

/// Start a new figure in the viewer, keeping the previous one visible.
/// No-op if the viewer is not connected.
pub fn viewer_new_figure() {
    VIEWER_CONN.with(|c| {
        let mut guard = c.borrow_mut();
        if let Some(ref mut conn) = *guard {
            conn.fig_id = next_fig_id();
            conn.layout = (0, 0); // forces FigureOpen on next sync
            conn.last_figure_title = String::new();
        }
    });
}

/// Returns true when a viewer connection is active for regular plotting.
pub fn viewer_active() -> bool {
    VIEWER_CONN.with(|c| c.borrow().is_some())
}

/// Get the current viewer figure ID, if connected.
pub fn get_viewer_fig_id() -> Option<u32> {
    VIEWER_CONN.with(|c| c.borrow().as_ref().map(|conn| conn.fig_id))
}

/// Set the viewer figure ID (for figure switching). Resets layout to force FigureOpen.
pub fn set_viewer_fig_id(id: u32) {
    VIEWER_CONN.with(|c| {
        if let Some(ref mut conn) = *c.borrow_mut() {
            conn.fig_id = id;
            conn.layout = (0, 0);
            conn.last_figure_title = String::new();
        }
    });
}

/// Allocate a new viewer figure ID without changing VIEWER_CONN state.
pub fn allocate_viewer_fig_id() -> u32 {
    next_fig_id()
}

/// Send the current FIGURE state to the viewer. No-op if not connected.
///
/// If the send fails (e.g. the viewer window was closed), tears down the
/// viewer connection, switches the current figure back to terminal output,
/// emits a warning, and renders the pending figure to the TUI so the plot
/// is not silently lost.
pub fn sync_viewer() {
    let send_err = VIEWER_CONN.with(|c| {
        let mut guard = c.borrow_mut();
        let Some(ref mut conn) = *guard else {
            return None;
        };
        FIGURE.with(|fig| {
            let fig = fig.borrow();
            send_figure_state(conn, &fig).err()
        })
    });

    if let Some(err) = send_err {
        // Drop the dead connection.
        VIEWER_CONN.with(|c| *c.borrow_mut() = None);
        VIEWER_SESSION.with(|s| *s.borrow_mut() = None);
        // Route the current (and future) figure output back to the terminal.
        crate::figure::set_current_figure_output(crate::figure::FigureOutput::Terminal);
        eprintln!(
            "viewer: connection lost ({}) — falling back to terminal rendering",
            err
        );
        // Best-effort TUI render so the user actually sees their plot.
        let _ = crate::ascii::render_figure_terminal();
    }
}

/// Compute the figure-level (window) title from the FigureState. If the
/// figure has exactly one subplot, promote that subplot's title — most
/// scripts use a single panel so this matches the user's intuition that
/// `title("Foo")` after a single `surf(...)` ought to label the figure
/// "Foo". Multi-subplot figures keep an empty figure title and rely on
/// per-panel headings, since no single subplot's title is canonical.
fn compute_figure_title(fig: &FigureState) -> String {
    if fig.subplot_rows == 1 && fig.subplot_cols == 1 {
        fig.subplots
            .first()
            .map(|s| s.title.clone())
            .unwrap_or_default()
    } else {
        String::new()
    }
}

/// Serialize a full FigureState to the viewer via protocol messages.
fn send_figure_state(conn: &mut ViewerConn, fig: &FigureState) -> Result<(), PlotError> {
    let rows = fig.subplot_rows;
    let cols = fig.subplot_cols;
    let n_panels = rows * cols;
    let fig_id = conn.fig_id;

    // Send FigureOpen on the first sync, when the layout changes, OR when
    // the promoted figure-level title changes. The viewer treats a repeat
    // FigureOpen on an existing fig_id with the same layout as a title
    // update (preserving panel data); a layout change still resets panels.
    let figure_title = compute_figure_title(fig);
    if conn.layout != (rows, cols) || conn.last_figure_title != figure_title {
        conn.client.send(&ViewerMsg::FigureOpen {
            id: fig_id,
            rows: rows as u16,
            cols: cols as u16,
            title: figure_title.clone(),
        })?;
        conn.layout = (rows, cols);
        conn.last_figure_title = figure_title;
    }

    for (idx, panel) in fig.subplots.iter().enumerate().take(n_panels) {
        // Panels containing contour/quiver/streamplot data: the viewer's
        // egui_plot backend doesn't render these natively, so pre-render the
        // entire panel (heatmap + series + overlays) into an RGBA bitmap via
        // plotters and ship it as a heatmap. Empty PanelUpdate clears any
        // stale series so the viewer doesn't double-draw.
        let needs_prerender =
            !panel.contours.is_empty() || !panel.quivers.is_empty() || !panel.streamlines.is_empty();
        if needs_prerender {
            let theme = crate::theme::Theme::default();
            // High-resolution pre-render so quiver/streamline/contour panels
            // stay legible under several stops of zoom in the viewer. Paired
            // with `smooth: true` so egui samples the texture with linear
            // filtering instead of showing blocky pixels on magnification.
            let (w, h) = (2400u32, 1600u32);
            if let Ok(rgba) =
                crate::file::render_panel_to_rgba(panel, theme.colors(), w, h)
            {
                conn.client.send_nowait(&ViewerMsg::PanelHeatmap {
                    fig_id,
                    panel: idx as u16,
                    heatmap: WireHeatmap {
                        width: w,
                        height: h,
                        rgba,
                        smooth: true,
                        // Pre-rendered overlays (contour / quiver /
                        // streamline) already have their axes baked into
                        // the pixels, so we leave the viewer's own axes
                        // off this texture's data range. No colorbar
                        // either — these aren't dB-clipped heatmaps.
                        x_extent: None,
                        y_extent: None,
                        value_min: None,
                        value_max: None,
                        colorscale: String::new(),
                    },
                })?;
            }
            conn.client.send_nowait(&ViewerMsg::PanelUpdate {
                fig_id,
                panel: idx as u16,
                series: Vec::new(),
            })?;
            conn.client.send_nowait(&ViewerMsg::PanelLabels {
                fig_id,
                panel: idx as u16,
                title: panel.title.clone(),
                xlabel: panel.xlabel.clone(),
                ylabel: panel.ylabel.clone(),
            })?;
            continue;
        }
        // Send 3D surface if present (takes precedence over heatmap in the viewer).
        if let Some(sf) = &panel.surface {
            let nrows = sf.z.len();
            let ncols = if nrows > 0 { sf.z[0].len() } else { 0 };
            if nrows > 0 && ncols > 0 {
                let mut z_flat = Vec::with_capacity(nrows * ncols);
                for row in &sf.z {
                    z_flat.extend_from_slice(row);
                }
                conn.client.send_nowait(&ViewerMsg::PanelSurface {
                    fig_id,
                    panel: idx as u16,
                    surface: WireSurface {
                        nrows: nrows as u32,
                        ncols: ncols as u32,
                        x: sf.x.clone(),
                        y: sf.y.clone(),
                        z: z_flat,
                        colorscale: sf.colorscale.clone(),
                    },
                })?;
            }
        }

        // Send heatmap if present
        if let Some(hm) = &panel.heatmap {
            let (width, height, rgba) = match hm.kind {
                crate::figure::HeatmapKind::ImageRgba => {
                    if let Some(buf) = &hm.rgba {
                        (hm.rgba_width as usize, hm.rgba_height as usize, buf.clone())
                    } else {
                        (0, 0, Vec::new())
                    }
                }
                _ => {
                    let height = hm.z.len();
                    let width = if height > 0 { hm.z[0].len() } else { 0 };
                    if width == 0 || height == 0 {
                        (0, 0, Vec::new())
                    } else {
                        // Same physics-convention layout as the live
                        // path: source row 0 lands at the BOTTOM of the
                        // texture so egui's y-axis tick labels (which
                        // read plot-coord directly) match the visible
                        // cell rows. Reuse the central renderer so it
                        // also picks up `value_min` / `value_max`.
                        let rgba = crate::file::render_heatmap_cells_to_rgba(hm);
                        (width, height, rgba)
                    }
                }
            };
            if width > 0 && height > 0 {
                conn.client.send_nowait(&ViewerMsg::PanelHeatmap {
                    fig_id,
                    panel: idx as u16,
                    heatmap: WireHeatmap {
                        width: width as u32,
                        height: height as u32,
                        rgba,
                        smooth: false,
                        // Static sync_viewer heatmaps inherit colour
                        // range from the producing builtin (spectrogram,
                        // scalogram, etc.) via HeatmapData's value_min /
                        // value_max. Extents come from plot_limits the
                        // same way as the live path.
                        x_extent: None,
                        y_extent: None,
                        value_min: hm.value_min,
                        value_max: hm.value_max,
                        colorscale: hm.colorscale.clone(),
                    },
                })?;
            }
        }

        // Convert series
        let wire_series: Vec<WireSeries> = panel
            .series
            .iter()
            .enumerate()
            .map(|(i, s)| {
                WireSeries {
                    label: s.label.clone(),
                    x: s.x_data.clone(),
                    y: s.y_data.clone(),
                    color: color_to_wire(&s.color),
                    style: style_to_wire(&s.style),
                    kind: kind_to_wire(&s.kind),
                    // Attach categorical labels to the first series
                    x_labels: if i == 0 { panel.x_labels.clone() } else { None },
                }
            })
            .collect();

        conn.client.send_nowait(&ViewerMsg::PanelUpdate {
            fig_id,
            panel: idx as u16,
            series: wire_series,
        })?;

        conn.client.send_nowait(&ViewerMsg::PanelLabels {
            fig_id,
            panel: idx as u16,
            title: panel.title.clone(),
            xlabel: panel.xlabel.clone(),
            ylabel: panel.ylabel.clone(),
        })?;

        conn.client.send_nowait(&ViewerMsg::PanelLimits {
            fig_id,
            panel: idx as u16,
            xlim: panel.xlim,
            ylim: panel.ylim,
        })?;

        conn.client.send_nowait(&ViewerMsg::PanelAxisEqual {
            fig_id,
            panel: idx as u16,
            equal: panel.axis_equal,
        })?;
    }

    conn.client.send(&ViewerMsg::Redraw { fig_id })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_fig_id_is_unique() {
        let a = next_fig_id();
        let b = next_fig_id();
        assert_ne!(a, b, "consecutive figure IDs should differ");
    }

    #[test]
    fn next_fig_id_embeds_pid() {
        let id = next_fig_id();
        let pid = std::process::id() as u32;
        let embedded_pid = id >> 16;
        assert_eq!(
            embedded_pid,
            pid & 0xFFFF,
            "upper 16 bits should contain truncated PID"
        );
    }

    #[test]
    fn next_fig_id_increments_lower_bits() {
        let a = next_fig_id();
        let b = next_fig_id();
        let low_a = a & 0xFFFF;
        let low_b = b & 0xFFFF;
        assert_eq!(low_b, low_a + 1, "lower 16 bits should increment");
    }

    #[test]
    fn color_to_wire_named_colors() {
        assert!(matches!(color_to_wire(&SeriesColor::Blue), WireColor::Named(s) if s == "blue"));
        assert!(matches!(color_to_wire(&SeriesColor::Red), WireColor::Named(s) if s == "red"));
        assert!(matches!(color_to_wire(&SeriesColor::Green), WireColor::Named(s) if s == "green"));
        assert!(matches!(color_to_wire(&SeriesColor::Cyan), WireColor::Named(s) if s == "cyan"));
        assert!(
            matches!(color_to_wire(&SeriesColor::Magenta), WireColor::Named(s) if s == "magenta")
        );
        assert!(
            matches!(color_to_wire(&SeriesColor::Yellow), WireColor::Named(s) if s == "yellow")
        );
        assert!(matches!(color_to_wire(&SeriesColor::Black), WireColor::Named(s) if s == "black"));
        assert!(matches!(color_to_wire(&SeriesColor::White), WireColor::Named(s) if s == "white"));
    }

    #[test]
    fn color_to_wire_rgb() {
        assert!(matches!(
            color_to_wire(&SeriesColor::Rgb(10, 20, 30)),
            WireColor::Rgb(10, 20, 30)
        ));
    }

    #[test]
    fn style_to_wire_variants() {
        assert!(matches!(
            style_to_wire(&LineStyle::Solid),
            WireLineStyle::Solid
        ));
        assert!(matches!(
            style_to_wire(&LineStyle::Dashed),
            WireLineStyle::Dashed
        ));
    }

    #[test]
    fn kind_to_wire_variants() {
        assert!(matches!(kind_to_wire(&PlotKind::Line), WirePlotKind::Line));
        assert!(matches!(kind_to_wire(&PlotKind::Stem), WirePlotKind::Stem));
        assert!(matches!(kind_to_wire(&PlotKind::Bar), WirePlotKind::Bar));
        assert!(matches!(
            kind_to_wire(&PlotKind::Scatter,),
            WirePlotKind::Scatter
        ));
    }

    #[test]
    fn compute_figure_title_promotes_single_subplot_title() {
        // The canonical surf.rlab pattern: figure(); surf(...); title("Foo")
        // ends up as a 1×1 figure with subplot 0 holding "Foo".
        let mut fig = FigureState::new();
        fig.current_mut().title = "Gaussian: exp(-(x^2 + y^2) / 2)".to_string();
        assert_eq!(
            compute_figure_title(&fig),
            "Gaussian: exp(-(x^2 + y^2) / 2)"
        );
    }

    #[test]
    fn compute_figure_title_empty_for_unset_subplot() {
        let fig = FigureState::new();
        assert_eq!(compute_figure_title(&fig), "");
    }

    #[test]
    fn compute_figure_title_empty_for_multi_subplot_layout() {
        // 2×2 with a title on subplot 0 — no single subplot title is canonical
        // for the whole figure, so the figure title stays empty and the
        // viewer's window-title fallback ("Figure {counter}") takes over.
        let mut fig = FigureState::new();
        fig.set_subplot(2, 2, 1);
        fig.current_mut().title = "Top-left only".to_string();
        assert_eq!(compute_figure_title(&fig), "");
    }
}
