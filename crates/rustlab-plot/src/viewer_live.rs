//! `ViewerFigure` — a `LivePlot` backend that sends data to `rustlab-viewer`
//! over a Unix socket instead of rendering in the terminal.
//!
//! Also provides `connect_viewer()` / `disconnect_viewer()` / `viewer_active()`
//! / `sync_viewer()` for routing regular (non-live) plot commands to the viewer.

use crate::decimate::{envelope_decimate, VIEWER_ENVELOPE_COLUMNS, VIEWER_ENVELOPE_MAX_POINTS};
use crate::figure::{FigureState, LineStyle, PlotKind, Series, SeriesColor, FIGURE};
use crate::plot_abort::{self, SigintGuard};
use crate::viewer_client::ViewerClient;
use crate::viewer_warn::{
    bulk_warning, envelope_warning, print_warning, record_scan, take_warning, CostRates,
    RASTER_NANOS_PER_PIXEL,
};
use crate::{LivePlot, PlotError};
use rustlab_proto::{
    ViewerMsg, WireColor, WireHeatmap, WireLineStyle, WirePlotKind, WireSeries, WireSurface,
    MAX_MSG_LEN,
};
use std::cell::{Cell, RefCell};
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
    client: Option<ViewerClient>,
    fig_id: u32,
    rows: usize,
    cols: usize,
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
        Some(Self {
            client: Some(client),
            fig_id,
            rows,
            cols,
        })
    }

    fn dial(&self) -> Option<ViewerClient> {
        let session = VIEWER_SESSION.with(|s| s.borrow().clone());
        let mut client = match session.as_deref() {
            Some(name) => ViewerClient::connect_named(name)?,
            None => ViewerClient::connect()?,
        };
        client
            .send(&ViewerMsg::FigureOpen {
                id: self.fig_id,
                rows: self.rows as u16,
                cols: self.cols as u16,
                title: String::new(),
            })
            .ok()?;
        Some(client)
    }

    fn ensure(&mut self) -> bool {
        if self.client.is_some() {
            return true;
        }
        self.client = self.dial();
        self.client.is_some()
    }

    /// A half-written frame drops this figure's socket. The next update
    /// opens a new connection and re-sends FigureOpen for the same id.
    /// A finished frame keeps the socket. Neither path sends Reset, so
    /// other figures already on the viewer stay.
    fn note_send(&mut self, err: &PlotError) {
        match err {
            PlotError::Interrupted => {
                plot_abort::report_aborted();
                if plot_abort::take_partial() {
                    self.client = None;
                }
            }
            PlotError::ViewerConnection(_) => self.client = None,
            _ => {}
        }
    }
}

impl LivePlot for ViewerFigure {
    fn update_panel(&mut self, idx: usize, x: Vec<f64>, y: Vec<f64>) {
        let _guard = SigintGuard::arm();
        if !self.ensure() {
            if plot_abort::requested() {
                self.note_send(&PlotError::Interrupted);
            } else {
                eprintln!("viewer: reconnect failed — live panel not sent");
            }
            return;
        }
        // Warn before the envelope scan. The slot latches only when a line
        // is actually printed, so a small first frame does not hide a later
        // large one, and a streaming update does not repeat it.
        let n = x.len().min(y.len());
        let rates = self.client.as_ref().unwrap().cost_rates();
        if let Some(line) = live_line_warning(n, &rates) {
            print_warning(&line);
        }
        let prepared = match prepare_series_for_viewer(&PlotKind::Line, &x, &y) {
            Ok(prepared) => prepared,
            Err(PlotError::Interrupted) => {
                self.note_send(&PlotError::Interrupted);
                return;
            }
            Err(e) => {
                eprintln!("viewer: {e} — live panel not sent");
                return;
            }
        };
        let msg = ViewerMsg::PanelUpdate {
            fig_id: self.fig_id,
            panel: idx as u16,
            series: vec![WireSeries {
                label: String::new(),
                x: prepared.x,
                y: prepared.y,
                color: WireColor::Named("cyan".into()),
                style: WireLineStyle::Solid,
                kind: WirePlotKind::Line,
                x_labels: None,
            }],
        };
        let send = self.client.as_mut().unwrap().send_nowait(&msg);
        if let Err(ref e) = send {
            if matches!(e, PlotError::ViewerPayloadTooLarge(_)) {
                eprintln!("viewer: {e} — live panel not sent");
            }
            self.note_send(e);
        } else if plot_abort::requested() {
            // The frame finished, so the socket stays in sync. Stop the
            // script anyway; this send was already recorded.
            self.note_send(&PlotError::Interrupted);
        }
    }

    fn set_panel_labels(&mut self, idx: usize, title: &str, xlabel: &str, ylabel: &str) {
        let msg = ViewerMsg::PanelLabels {
            fig_id: self.fig_id,
            panel: idx as u16,
            title: title.to_string(),
            xlabel: xlabel.to_string(),
            ylabel: ylabel.to_string(),
        };
        if let Some(client) = self.client.as_mut() {
            let _ = client.send_nowait(&msg);
        }
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
        if let Some(client) = self.client.as_mut() {
            let _ = client.send_nowait(&msg);
        }
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
        // Estimate the RGBA frame before allocating `z` or rasterizing.
        // A live heatmap that does not fit is dropped; it is not drawn in
        // the terminal (there is no figure fallback on this path).
        let bytes = nrows
            .saturating_mul(ncols)
            .saturating_mul(4)
            .saturating_add(64);
        let _guard = SigintGuard::arm();
        if !self.ensure() {
            if plot_abort::requested() {
                self.note_send(&PlotError::Interrupted);
            }
            return;
        }
        if plot_abort::requested() {
            self.note_send(&PlotError::Interrupted);
            return;
        }
        let rates = self.client.as_ref().unwrap().cost_rates();
        if let Some(line) = live_heatmap_warning(ncols, nrows, bytes, &rates) {
            print_warning(&line);
        }
        if bytes > MAX_MSG_LEN as usize {
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
        let rgba = match crate::file::render_heatmap_cells_to_rgba(&hm) {
            Ok(rgba) => rgba,
            Err(_) => return,
        };
        if rgba.is_empty() {
            return;
        }
        if rgba.len().saturating_add(64) > MAX_MSG_LEN as usize {
            return;
        }
        let send = self
            .client
            .as_mut()
            .unwrap()
            .send_nowait(&ViewerMsg::PanelHeatmap {
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
        if plot_abort::requested() {
            self.note_send(&PlotError::Interrupted);
            return;
        }
        if let Err(ref e) = send {
            self.note_send(e);
        }
    }

    fn redraw(&mut self) -> Result<(), PlotError> {
        let _guard = SigintGuard::arm();
        if !self.ensure() {
            if plot_abort::requested() {
                self.note_send(&PlotError::Interrupted);
                return Err(PlotError::Interrupted);
            }
            return Err(PlotError::ViewerConnection(
                "viewer reconnect failed".into(),
            ));
        }
        let msg = ViewerMsg::Redraw {
            fig_id: self.fig_id,
        };
        let result = self.client.as_mut().unwrap().send(&msg);
        if let Err(ref e) = result {
            self.note_send(e);
        } else if plot_abort::requested() {
            self.note_send(&PlotError::Interrupted);
            return Err(PlotError::Interrupted);
        }
        result.map(|_| ())
    }
}

impl Drop for ViewerFigure {
    fn drop(&mut self) {
        let msg = ViewerMsg::Close {
            fig_id: self.fig_id,
        };
        if let Some(client) = self.client.as_mut() {
            let _ = client.send_nowait(&msg);
        }
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
        Err(PlotError::Interrupted) => return Err(PlotError::Interrupted),
        _ => return Ok(false),
    }
    // Clear any figures from previous sessions. A Ctrl+C here must not
    // store a half-written socket.
    if let Err(PlotError::Interrupted) = client.send(&ViewerMsg::Reset) {
        return Err(PlotError::Interrupted);
    }
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

/// Send the current FIGURE state to the viewer.
///
/// A line that would not fit in one IPC frame is reduced to a min/max
/// envelope first (see [`envelope_decimate`]). If a payload is still over
/// [`MAX_MSG_LEN`], the socket is left open, the reason is printed, and
/// this figure is drawn in the terminal. A real I/O failure (the viewer
/// window was closed) tears the connection down, prints that the
/// connection was lost, and renders the pending figure in the terminal
/// so the plot is not silently lost.
///
/// Ctrl+C during the scan or the socket write returns
/// [`PlotError::Interrupted`]. A frame that already had a byte written
/// drops this socket only (`VIEWER_SESSION` and `FigureOutput::Viewer`
/// stay), so the next plot dials again. Local and remote are the same
/// path: `rustlab remote` forwards this unix socket. The aborted send
/// is not fed to the rate estimate.
pub fn sync_viewer() -> Result<(), PlotError> {
    let _guard = SigintGuard::arm();
    if plot_abort::requested() {
        return abort_sync();
    }
    match ensure_viewer_conn() {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("viewer: reconnect failed — drawing this figure in the terminal");
            return draw_terminal_once();
        }
        Err(PlotError::Interrupted) => return abort_sync(),
        Err(err) => return Err(err),
    }
    // One envelope line and one bulk line per plot call. Live updates use
    // their own slots and are not cleared here.
    reset_static_warning_slots();
    let send_err = VIEWER_CONN.with(|c| {
        let mut guard = c.borrow_mut();
        let conn = guard.as_mut()?;
        FIGURE.with(|fig| {
            let fig = fig.borrow();
            send_figure_state(conn, &fig).err()
        })
    });

    let Some(err) = send_err else {
        return Ok(());
    };
    if matches!(err, PlotError::Interrupted) {
        return abort_sync();
    }
    if matches!(err, PlotError::ViewerPayloadTooLarge(_)) {
        // Nothing was written for the rejected frame, so the socket is
        // still usable. Draw this figure in the terminal, then put the
        // figure back on the viewer for the next plot. The up-front
        // over-cap warning already names this fallback; print a line
        // here only when that warning did not. Terminal mode is set
        // first so this draw does not call back into sync_viewer.
        if !FALLBACK_WARNED.with(|c| c.get()) {
            eprintln!(
                "viewer: {err} — drawing this figure in the terminal. The viewer stays connected."
            );
        }
        let previous = crate::figure::current_figure_output();
        crate::figure::set_current_figure_output(crate::figure::FigureOutput::Terminal);
        let drawn = crate::ascii::render_figure_terminal();
        if viewer_active() {
            crate::figure::set_current_figure_output(previous);
        }
        // Ctrl+C on that screen already printed `plot aborted`. The
        // socket was not written, so it stays up.
        return drawn;
    }
    // Drop the dead connection.
    VIEWER_CONN.with(|c| *c.borrow_mut() = None);
    VIEWER_SESSION.with(|s| *s.borrow_mut() = None);
    // Route the current (and future) figure output back to the terminal.
    crate::figure::set_current_figure_output(crate::figure::FigureOutput::Terminal);
    eprintln!("viewer: connection lost ({err}) — falling back to terminal rendering");
    // Best-effort TUI render so the user actually sees their plot.
    crate::ascii::render_figure_terminal()
}

/// After a partial abort the connection is gone but the figure is still
/// routed at the viewer. Dial the same session again. Ping and Reset run
/// inside [`connect_viewer_impl`]; Reset clears figures the viewer still
/// has open. A live connection is left alone.
fn ensure_viewer_conn() -> Result<bool, PlotError> {
    if viewer_active() {
        return Ok(true);
    }
    let session = VIEWER_SESSION.with(|s| s.borrow().clone());
    let client = match session.as_deref() {
        Some(name) => ViewerClient::connect_named(name),
        None => ViewerClient::connect(),
    };
    connect_viewer_impl(client)
}

/// Drop a half-written viewer socket and stop the plot. The session name
/// and `FigureOutput::Viewer` stay, so the next plot reconnects instead
/// of switching to the terminal for good.
fn abort_sync() -> Result<(), PlotError> {
    plot_abort::report_aborted();
    if plot_abort::take_partial() {
        VIEWER_CONN.with(|c| *c.borrow_mut() = None);
    }
    Err(PlotError::Interrupted)
}

/// Draw once in the terminal, then put the figure back on the viewer so
/// the next plot tries the socket again.
fn draw_terminal_once() -> Result<(), PlotError> {
    let previous = crate::figure::current_figure_output();
    crate::figure::set_current_figure_output(crate::figure::FigureOutput::Terminal);
    let drawn = crate::ascii::render_figure_terminal();
    crate::figure::set_current_figure_output(previous);
    drawn
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
    // Before any envelope scan, heatmap raster, or surface flatten.
    // Rates are the measurements from earlier sends, not this one.
    let rates = conn.client.cost_rates();
    refuse_oversized_plot(fig, &rates)?;
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
        let needs_prerender = !panel.contours.is_empty()
            || !panel.quivers.is_empty()
            || !panel.streamlines.is_empty();
        if needs_prerender {
            let theme = crate::theme::default_theme();
            // High-resolution pre-render so quiver/streamline/contour panels
            // stay legible under several stops of zoom in the viewer. Paired
            // with `smooth: true` so egui samples the texture with linear
            // filtering instead of showing blocky pixels on magnification.
            let (w, h) = (2400u32, 1600u32);
            if let Ok(rgba) = crate::file::render_panel_to_rgba(panel, theme.colors(), w, h) {
                ensure_frame_budget(
                    rgba.len().saturating_add(64),
                    &format!("pre-rendered panel is {w} by {h}"),
                )?;
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
                let z_len = nrows * ncols;
                let bytes = 80 + (5 + 9 * sf.x.len()) + (5 + 9 * sf.y.len()) + (5 + 9 * z_len);
                ensure_frame_budget(bytes, &format!("surface is {nrows} by {ncols}"))?;
                let mut z_flat = Vec::with_capacity(z_len);
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
                        let rgba = crate::file::render_heatmap_cells_to_rgba(hm)?;
                        (width, height, rgba)
                    }
                }
            };
            if width > 0 && height > 0 {
                ensure_frame_budget(
                    rgba.len().saturating_add(64),
                    &format!("heatmap is {width} by {height}"),
                )?;
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

        // Line / scatter / stem series longer than a wide panel are reduced
        // to a min/max envelope here, before msgpack, so a 1e8-point plot
        // never becomes a multi-gigabyte frame the viewer has to reject.
        // The size warning, when there is one, was printed above.
        let (wire_series, _, _) = series_to_wire(&panel.series, &panel.x_labels)?;

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

struct PreparedSeries {
    x: Vec<f64>,
    y: Vec<f64>,
    input_len: usize,
    output_len: usize,
}

/// Reduce a waveform to the viewer envelope. Bar charts stay whole and
/// fail with [`PlotError::ViewerPayloadTooLarge`] when one series cannot
/// fit in an IPC frame — merging bars into an envelope would change the chart.
fn prepare_series_for_viewer(
    kind: &PlotKind,
    x: &[f64],
    y: &[f64],
) -> Result<PreparedSeries, PlotError> {
    let n = x.len().min(y.len());
    let x = &x[..n];
    let y = &y[..n];
    if matches!(kind, PlotKind::Bar) {
        let bytes = estimated_xy_bytes(n, n);
        if bytes > MAX_MSG_LEN as usize {
            return Err(PlotError::ViewerPayloadTooLarge(format!(
                "bar series has {n} bars (about {bytes} bytes), over the {MAX_MSG_LEN}-byte viewer IPC limit"
            )));
        }
        return Ok(PreparedSeries {
            x: x.to_vec(),
            y: y.to_vec(),
            input_len: n,
            output_len: n,
        });
    }
    let started = std::time::Instant::now();
    let (dx, dy) = envelope_decimate(x, y, VIEWER_ENVELOPE_COLUMNS);
    // An aborted scan returns empty vectors. Do not record it, and do
    // not send the partial envelope.
    if plot_abort::requested() {
        return Err(PlotError::Interrupted);
    }
    if n > VIEWER_ENVELOPE_MAX_POINTS {
        record_scan(n as u128, started.elapsed().as_nanos());
    }
    Ok(PreparedSeries {
        output_len: dx.len(),
        input_len: n,
        x: dx,
        y: dy,
    })
}

fn series_to_wire(
    series: &[Series],
    x_labels: &Option<Vec<String>>,
) -> Result<(Vec<WireSeries>, usize, usize), PlotError> {
    let mut wire = Vec::with_capacity(series.len());
    let mut input_points = 0usize;
    let mut output_points = 0usize;
    for (i, s) in series.iter().enumerate() {
        let prepared = prepare_series_for_viewer(&s.kind, &s.x_data, &s.y_data)?;
        input_points += prepared.input_len;
        output_points += prepared.output_len;
        // Categorical labels index the original samples. After a reduction
        // those indices no longer match, so the labels are left off.
        let keep_labels = i == 0 && prepared.output_len == prepared.input_len;
        wire.push(WireSeries {
            label: s.label.clone(),
            x: prepared.x,
            y: prepared.y,
            color: color_to_wire(&s.color),
            style: style_to_wire(&s.style),
            kind: kind_to_wire(&s.kind),
            x_labels: if keep_labels { x_labels.clone() } else { None },
        });
    }
    let bytes = panel_update_bytes(&wire);
    if bytes > MAX_MSG_LEN as usize {
        return Err(PlotError::ViewerPayloadTooLarge(format!(
            "plot message is about {bytes} bytes ({output_points} points), over the {MAX_MSG_LEN}-byte viewer IPC limit"
        )));
    }
    Ok((wire, input_points, output_points))
}

/// msgpack array32 header plus one 9-byte float64 per element, plus slack
/// for the series struct. High enough that a passing estimate still encodes
/// under [`MAX_MSG_LEN`].
fn estimated_xy_bytes(nx: usize, ny: usize) -> usize {
    96 + (5 + 9 * nx) + (5 + 9 * ny)
}

fn panel_update_bytes(series: &[WireSeries]) -> usize {
    let mut n = 64usize;
    for s in series {
        n += estimated_xy_bytes(s.x.len(), s.y.len());
        n += s.label.len();
        if let Some(labels) = &s.x_labels {
            for lab in labels {
                n += lab.len() + 8;
            }
        }
    }
    n
}

fn ensure_frame_budget(bytes: usize, what: &str) -> Result<(), PlotError> {
    if bytes > MAX_MSG_LEN as usize {
        Err(PlotError::ViewerPayloadTooLarge(format!(
            "{what} ({bytes} bytes), over the {MAX_MSG_LEN}-byte viewer IPC limit"
        )))
    } else {
        Ok(())
    }
}

// One envelope line and one bar/heatmap/surface line per `sync_viewer`
// call. Live updates latch separately, and only after a line is printed.
thread_local! {
    static ENVELOPE_SLOT: Cell<bool> = const { Cell::new(false) };
    static BULK_SLOT: Cell<bool> = const { Cell::new(false) };
    static FALLBACK_WARNED: Cell<bool> = const { Cell::new(false) };
    static LIVE_ENVELOPE_SLOT: Cell<bool> = const { Cell::new(false) };
    static LIVE_BULK_SLOT: Cell<bool> = const { Cell::new(false) };
}

fn reset_static_warning_slots() {
    ENVELOPE_SLOT.with(|c| c.set(false));
    BULK_SLOT.with(|c| c.set(false));
    FALLBACK_WARNED.with(|c| c.set(false));
}

struct PlotSizeWarning {
    envelope: Option<String>,
    bulk: Option<String>,
    over_cap: bool,
    /// Totals the warning was built from. Tests assert the sum; the
    /// warning text is what production reads.
    #[cfg(test)]
    samples: usize,
    #[cfg(test)]
    scan_samples: usize,
    #[cfg(test)]
    sent_points: usize,
    #[cfg(test)]
    sent_bytes: usize,
}

struct BulkNote {
    line: String,
    over: bool,
}

/// Keep the first cost warning, and replace it with the first frame that
/// is over the IPC cap. A `None` line is under both gates.
fn note_bulk(best: &mut Option<BulkNote>, line: Option<String>, over: bool) {
    let Some(line) = line else {
        return;
    };
    match best {
        Some(cur) if cur.over => {}
        Some(_) if !over => {}
        _ => *best = Some(BulkNote { line, over }),
    }
}

/// RGBA payload of a heatmap that has not been rasterized yet.
fn heatmap_frame_bytes(hm: &crate::figure::HeatmapData) -> Option<(usize, usize, usize)> {
    match hm.kind {
        crate::figure::HeatmapKind::ImageRgba => {
            let buf = hm.rgba.as_ref()?;
            let width = hm.rgba_width as usize;
            let height = hm.rgba_height as usize;
            if width == 0 || height == 0 {
                return None;
            }
            Some((width, height, buf.len().saturating_add(64)))
        }
        _ => {
            let nrows = hm.z.len();
            let ncols = if nrows > 0 { hm.z[0].len() } else { 0 };
            if nrows == 0 || ncols == 0 {
                return None;
            }
            let bytes = nrows
                .saturating_mul(ncols)
                .saturating_mul(4)
                .saturating_add(64);
            Some((ncols, nrows, bytes))
        }
    }
}

/// Sizes for the stderr warning. Does not allocate pixel buffers or scan
/// the samples. A panel that will be pre-rendered (contour, quiver,
/// streamline) is skipped: that frame is a fixed bitmap, and its series
/// are not sent as lines.
fn plot_size_warning(fig: &FigureState, rates: &CostRates) -> PlotSizeWarning {
    let n_panels = fig.subplot_rows.saturating_mul(fig.subplot_cols);
    let mut samples = 0usize;
    let mut scan_samples = 0usize;
    let mut sent_points = 0usize;
    let mut sent_bytes = 0usize;
    let mut bulk: Option<BulkNote> = None;

    for panel in fig.subplots.iter().take(n_panels) {
        let prerender = !panel.contours.is_empty()
            || !panel.quivers.is_empty()
            || !panel.streamlines.is_empty();
        if prerender {
            continue;
        }
        if let Some(sf) = &panel.surface {
            let nrows = sf.z.len();
            let ncols = if nrows > 0 { sf.z[0].len() } else { 0 };
            if nrows > 0 && ncols > 0 {
                let z_len = nrows.saturating_mul(ncols);
                let bytes = 80 + (5 + 9 * sf.x.len()) + (5 + 9 * sf.y.len()) + (5 + 9 * z_len);
                let copies = sf.x.len().saturating_add(sf.y.len()).saturating_add(z_len);
                let extra = copies.saturating_mul(8).saturating_add(bytes);
                let over = bytes > MAX_MSG_LEN as usize;
                note_bulk(
                    &mut bulk,
                    bulk_warning(
                        &format!("surface {nrows} by {ncols}"),
                        bytes,
                        extra,
                        0,
                        true,
                        rates,
                    ),
                    over,
                );
            }
        }
        if let Some(hm) = &panel.heatmap {
            if let Some((ncols, nrows, bytes)) = heatmap_frame_bytes(hm) {
                let (extra, work) = heatmap_extra(hm, ncols, nrows, bytes);
                let over = bytes > MAX_MSG_LEN as usize;
                note_bulk(
                    &mut bulk,
                    bulk_warning(
                        &format!("heatmap {ncols} by {nrows}"),
                        bytes,
                        extra,
                        work,
                        true,
                        rates,
                    ),
                    over,
                );
            }
        }
        for s in &panel.series {
            let n = s.x_data.len().min(s.y_data.len());
            if matches!(s.kind, PlotKind::Bar) {
                let payload = estimated_xy_bytes(n, n);
                let extra = n.saturating_mul(16).saturating_add(payload);
                let over = payload > MAX_MSG_LEN as usize;
                note_bulk(
                    &mut bulk,
                    bulk_warning(
                        &format!("bar series of {n} bars"),
                        payload,
                        extra,
                        0,
                        true,
                        rates,
                    ),
                    over,
                );
                continue;
            }
            samples = samples.saturating_add(n);
            let kept = n.min(VIEWER_ENVELOPE_MAX_POINTS);
            sent_points = sent_points.saturating_add(kept);
            sent_bytes = sent_bytes.saturating_add(estimated_xy_bytes(kept, kept));
            if n > VIEWER_ENVELOPE_MAX_POINTS {
                scan_samples = scan_samples.saturating_add(n);
            }
        }
    }

    let envelope = if samples > 0 {
        envelope_warning(samples, scan_samples, sent_points, sent_bytes, rates)
    } else {
        None
    };
    let (bulk_line, over_cap) = match bulk {
        Some(b) => (Some(b.line), b.over),
        None => (None, false),
    };
    PlotSizeWarning {
        envelope,
        bulk: bulk_line,
        over_cap,
        #[cfg(test)]
        samples,
        #[cfg(test)]
        scan_samples,
        #[cfg(test)]
        sent_points,
        #[cfg(test)]
        sent_bytes,
    }
}

/// Extra bytes and raster work for a heatmap frame.
/// `ImageRgba` already holds the pixels, so the extra cost is the msgpack
/// copy. A value grid is rasterized first (12 ns/pixel) into an RGBA
/// buffer that stays live next to that copy.
fn heatmap_extra(
    hm: &crate::figure::HeatmapData,
    ncols: usize,
    nrows: usize,
    payload: usize,
) -> (usize, u128) {
    let pixels = ncols.saturating_mul(nrows);
    let rgba = pixels.saturating_mul(4);
    if matches!(hm.kind, crate::figure::HeatmapKind::ImageRgba) {
        (payload, 0)
    } else {
        let work = (pixels as u128).saturating_mul(RASTER_NANOS_PER_PIXEL);
        (rgba.saturating_add(payload), work)
    }
}

fn emit_slot(slot: &Cell<bool>, line: Option<String>) {
    if let Some(line) = take_warning(slot, line) {
        print_warning(&line);
    }
}

/// Which lines to print. An over-cap figure is not scanned or rasterized,
/// so the envelope line (which quotes a scan time) is left off. The bulk
/// line already says the figure goes to the terminal.
fn lines_to_print(warning: PlotSizeWarning) -> (Option<String>, Option<String>, bool) {
    if warning.over_cap {
        (None, warning.bulk, true)
    } else {
        (warning.envelope, warning.bulk, false)
    }
}

/// Print at most one line warning and one bulk warning, then refuse the
/// whole figure when a non-decimated frame is over the IPC cap. The
/// refusal happens before the envelope scan and before any raster.
fn refuse_oversized_plot(fig: &FigureState, rates: &CostRates) -> Result<(), PlotError> {
    let (envelope, bulk, over_cap) = lines_to_print(plot_size_warning(fig, rates));
    ENVELOPE_SLOT.with(|slot| emit_slot(slot, envelope));
    BULK_SLOT.with(|slot| emit_slot(slot, bulk));
    if over_cap {
        FALLBACK_WARNED.with(|c| c.set(true));
        return Err(PlotError::ViewerPayloadTooLarge(
            "plot is over the 32 MiB viewer IPC limit".into(),
        ));
    }
    Ok(())
}

fn live_line_warning(n: usize, rates: &CostRates) -> Option<String> {
    if n <= VIEWER_ENVELOPE_MAX_POINTS {
        return None;
    }
    let sent = estimated_xy_bytes(VIEWER_ENVELOPE_MAX_POINTS, VIEWER_ENVELOPE_MAX_POINTS);
    LIVE_ENVELOPE_SLOT.with(|slot| {
        take_warning(
            slot,
            envelope_warning(n, n, VIEWER_ENVELOPE_MAX_POINTS, sent, rates),
        )
    })
}

fn live_heatmap_warning(
    ncols: usize,
    nrows: usize,
    bytes: usize,
    rates: &CostRates,
) -> Option<String> {
    let pixels = ncols.saturating_mul(nrows);
    let rgba = pixels.saturating_mul(4);
    let work = (pixels as u128).saturating_mul(RASTER_NANOS_PER_PIXEL);
    let extra = rgba.saturating_add(bytes);
    LIVE_BULK_SLOT.with(|slot| {
        take_warning(
            slot,
            bulk_warning(
                &format!("heatmap {ncols} by {nrows}"),
                bytes,
                extra,
                work,
                false,
                rates,
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prior() -> CostRates {
        CostRates::prior()
    }

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

    fn sine_series(n: usize, kind: PlotKind) -> Series {
        Series {
            label: "s".into(),
            x_data: (0..n).map(|i| i as f64 / (n.max(2) - 1) as f64).collect(),
            y_data: (0..n).map(|i| (i as f64 * 0.02).sin()).collect(),
            color: SeriesColor::Blue,
            style: LineStyle::Solid,
            kind,
        }
    }

    #[test]
    fn raw_line_of_1e8_exceeds_ipc_limit() {
        let n = 100_000_000usize;
        let bytes = estimated_xy_bytes(n, n);
        assert!(
            bytes > MAX_MSG_LEN as usize,
            "{bytes} should exceed {MAX_MSG_LEN}"
        );
        // Two msgpack float64 arrays: 9 bytes each, plus a 5-byte header.
        assert!(
            bytes > 1_700_000_000,
            "1e8 pairs are about 1.7 GiB, got {bytes}"
        );
    }

    #[test]
    fn long_line_is_enveloped_and_roundtrips_under_the_limit() {
        let n = 50_000usize;
        let series = sine_series(n, PlotKind::Line);
        let (wire, input, output) = series_to_wire(&[series], &None).unwrap();
        assert_eq!(input, n);
        assert!(output < n);
        assert!(output <= crate::decimate::VIEWER_ENVELOPE_MAX_POINTS);
        assert!(wire[0].y.iter().copied().fold(f64::NEG_INFINITY, f64::max) > 0.99);
        assert!(wire[0].y.iter().copied().fold(f64::INFINITY, f64::min) < -0.99);
        assert_eq!(wire[0].x[0], 0.0);
        let msg = ViewerMsg::PanelUpdate {
            fig_id: 1,
            panel: 0,
            series: wire,
        };
        let mut buf = Vec::new();
        rustlab_proto::write_msg(&mut buf, &msg).unwrap();
        assert!(buf.len() < MAX_MSG_LEN as usize, "frame {}", buf.len());
        let mut cur = std::io::Cursor::new(buf);
        let decoded: ViewerMsg = rustlab_proto::read_msg(&mut cur).unwrap().unwrap();
        match decoded {
            ViewerMsg::PanelUpdate { series, .. } => {
                assert_eq!(series.len(), 1);
                assert_eq!(series[0].x.len(), output);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn short_line_is_not_reduced_and_keeps_category_labels() {
        let series = sine_series(100, PlotKind::Line);
        let labels = Some(vec!["a".into(), "b".into()]);
        let (wire, input, output) = series_to_wire(&[series], &labels).unwrap();
        assert_eq!(input, output);
        assert_eq!(wire[0].x_labels, labels);
    }

    #[test]
    fn reduced_line_drops_category_labels() {
        let series = sine_series(20_000, PlotKind::Line);
        let labels = Some(vec!["a".into()]);
        let (wire, _, _) = series_to_wire(&[series], &labels).unwrap();
        assert!(wire[0].x_labels.is_none());
        assert!(wire[0].x.len() <= crate::decimate::VIEWER_ENVELOPE_MAX_POINTS);
    }

    #[test]
    fn scatter_and_stem_use_the_envelope() {
        for kind in [PlotKind::Scatter, PlotKind::Stem] {
            let series = sine_series(20_000, kind);
            let (_, input, output) = series_to_wire(&[series], &None).unwrap();
            assert_eq!(input, 20_000);
            assert!(output < input);
        }
    }

    #[test]
    fn oversized_bar_series_reports_the_limit() {
        let n = 2_000_000usize;
        let series = Series {
            label: String::new(),
            x_data: vec![0.0; n],
            y_data: vec![1.0; n],
            color: SeriesColor::Blue,
            style: LineStyle::Solid,
            kind: PlotKind::Bar,
        };
        let err = series_to_wire(&[series], &None).unwrap_err();
        let PlotError::ViewerPayloadTooLarge(msg) = err else {
            panic!("expected a size error, got {err}");
        };
        assert!(msg.contains("bar series"), "{msg}");
        assert!(
            msg.contains("33554432") || msg.contains(&MAX_MSG_LEN.to_string()),
            "{msg}"
        );
        assert!(msg.contains(&n.to_string()), "{msg}");
    }

    #[test]
    fn modest_reduction_is_quiet() {
        let mut fig = FigureState::new();
        fig.current_mut()
            .series
            .push(sine_series(9_000, PlotKind::Line));
        let rates = prior();
        let w = plot_size_warning(&fig, &rates);
        assert!(w.envelope.is_none(), "a short reduction stays quiet");
        assert_eq!(w.scan_samples, 9_000);
        assert_eq!(w.sent_points, VIEWER_ENVELOPE_MAX_POINTS);
        assert!(w.bulk.is_none());
        assert!(!w.over_cap);
    }

    #[test]
    fn half_million_line_is_under_both_gates() {
        let n = 500_000usize;
        let mut fig = FigureState::new();
        fig.current_mut()
            .series
            .push(sine_series(n, PlotKind::Line));
        let rates = prior();
        let w = plot_size_warning(&fig, &rates);
        assert_eq!(w.samples, n);
        assert_eq!(w.scan_samples, n);
        assert_eq!(w.sent_points, VIEWER_ENVELOPE_MAX_POINTS);
        assert_eq!(
            w.envelope,
            envelope_warning(
                w.samples,
                w.scan_samples,
                w.sent_points,
                w.sent_bytes,
                &rates
            )
        );
        assert!(w.envelope.is_none(), "0.5e6 estimates a few milliseconds");
        assert!(!w.over_cap);
        assert!(w.bulk.is_none());
    }

    #[test]
    fn line_warning_sums_every_series() {
        let mut fig = FigureState::new();
        fig.current_mut()
            .series
            .push(sine_series(500_000, PlotKind::Line));
        fig.current_mut()
            .series
            .push(sine_series(100, PlotKind::Line));
        let rates = prior();
        let w = plot_size_warning(&fig, &rates);
        assert_eq!(w.samples, 500_100);
        assert_eq!(w.scan_samples, 500_000);
        assert_eq!(w.sent_points, VIEWER_ENVELOPE_MAX_POINTS + 100);
        let short = estimated_xy_bytes(100, 100);
        let kept = estimated_xy_bytes(VIEWER_ENVELOPE_MAX_POINTS, VIEWER_ENVELOPE_MAX_POINTS);
        assert_eq!(w.sent_bytes, kept + short);
        assert_eq!(
            w.envelope.as_deref(),
            envelope_warning(
                w.samples,
                w.scan_samples,
                w.sent_points,
                w.sent_bytes,
                &rates
            )
            .as_deref()
        );
    }

    #[test]
    fn oversized_bar_warning_is_the_terminal_fallback_line() {
        let n = 2_000_000usize;
        let mut fig = FigureState::new();
        fig.current_mut().series.push(Series {
            label: String::new(),
            x_data: vec![0.0; n],
            y_data: vec![1.0; n],
            color: SeriesColor::Blue,
            style: LineStyle::Solid,
            kind: PlotKind::Bar,
        });
        let rates = prior();
        let w = plot_size_warning(&fig, &rates);
        let bytes = estimated_xy_bytes(n, n);
        let extra = n.saturating_mul(16).saturating_add(bytes);
        assert_eq!(
            w.bulk.as_deref(),
            bulk_warning(
                &format!("bar series of {n} bars"),
                bytes,
                extra,
                0,
                true,
                &rates
            )
            .as_deref()
        );
        assert!(w.over_cap);
        assert!(w.envelope.is_none());
    }

    #[test]
    fn over_cap_plot_warns_with_the_fallback_line_only() {
        let (envelope, bulk, over) = lines_to_print(PlotSizeWarning {
            envelope: Some("line scan".into()),
            bulk: Some("terminal fallback".into()),
            over_cap: true,
            samples: 0,
            scan_samples: 0,
            sent_points: 0,
            sent_bytes: 0,
        });
        assert!(envelope.is_none());
        assert_eq!(bulk.as_deref(), Some("terminal fallback"));
        assert!(over);
        let (envelope, bulk, over) = lines_to_print(PlotSizeWarning {
            envelope: Some("line scan".into()),
            bulk: Some("sent whole".into()),
            over_cap: false,
            samples: 0,
            scan_samples: 0,
            sent_points: 0,
            sent_bytes: 0,
        });
        assert_eq!(envelope.as_deref(), Some("line scan"));
        assert_eq!(bulk.as_deref(), Some("sent whole"));
        assert!(!over);
    }

    #[test]
    fn bulk_note_keeps_the_first_over_cap_item() {
        let mut best = None;
        note_bulk(&mut best, None, false);
        assert!(best.is_none());
        note_bulk(&mut best, Some("near".into()), false);
        note_bulk(&mut best, Some("over".into()), true);
        note_bulk(&mut best, Some("later".into()), true);
        assert_eq!(best.unwrap().line, "over");

        let mut best = None;
        note_bulk(&mut best, Some("first".into()), false);
        note_bulk(&mut best, Some("second".into()), false);
        assert_eq!(best.unwrap().line, "first");
    }

    #[test]
    fn static_warning_slot_resets_each_plot_and_notebooks_do_not_consume_it() {
        reset_static_warning_slots();
        let line = Some("viewer: warning: example".to_string());
        ENVELOPE_SLOT.with(|slot| {
            assert_eq!(
                take_warning(slot, line.clone()).as_deref(),
                Some("viewer: warning: example")
            );
            assert!(take_warning(slot, line.clone()).is_none());
        });
        reset_static_warning_slots();
        let prev = crate::figure::plot_context();
        crate::figure::set_plot_context(crate::figure::PlotContext::Notebook);
        ENVELOPE_SLOT.with(|slot| {
            assert!(take_warning(slot, line.clone()).is_none());
            assert!(!slot.get());
        });
        crate::figure::set_plot_context(crate::figure::PlotContext::Terminal);
        ENVELOPE_SLOT.with(|slot| {
            assert_eq!(
                take_warning(slot, line).as_deref(),
                Some("viewer: warning: example")
            );
        });
        crate::figure::set_plot_context(prev);
        reset_static_warning_slots();
    }

    #[test]
    fn live_line_warning_latches_only_after_a_large_frame() {
        LIVE_ENVELOPE_SLOT.with(|slot| slot.set(false));
        let rates = prior();
        assert!(live_line_warning(1_000, &rates).is_none());
        assert!(
            live_line_warning(1_000_000, &rates).is_none(),
            "1e6 estimates about 4 ms"
        );
        assert!(
            live_line_warning(100_000_000, &rates).is_none(),
            "1e8 estimates 0.4 s"
        );
        assert!(
            live_line_warning(125_000_000, &rates).is_none(),
            "1.25e8 estimates 0.5 s"
        );
        let first = live_line_warning(250_000_000, &rates).unwrap();
        assert!(
            first.starts_with(
                "viewer: warning: plotting 250000000 samples will take an estimated 1.0 s"
            ),
            "{first}"
        );
        assert!(live_line_warning(250_000_000, &rates).is_none());
        LIVE_ENVELOPE_SLOT.with(|slot| slot.set(false));
    }

    #[test]
    fn live_heatmap_over_the_cap_names_no_terminal_fallback() {
        LIVE_BULK_SLOT.with(|slot| slot.set(false));
        let bytes = 4096usize * 2048 * 4 + 64;
        let rates = prior();
        let line = live_heatmap_warning(4096, 2048, bytes, &rates).unwrap();
        assert_eq!(
            line,
            "viewer: warning: heatmap 4096 by 2048 is about 32.0 MiB, over the 32 MiB viewer limit — not sending."
        );
        assert!(live_heatmap_warning(4096, 2048, bytes, &rates).is_none());
        LIVE_BULK_SLOT.with(|slot| slot.set(false));
    }
}
