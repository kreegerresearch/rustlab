//! Main eframe application for rustlab-viewer.

use rustlab_proto::ViewerMsg;
use std::collections::HashMap;
use std::sync::mpsc;

use crate::figure::{FigureWindow, HeatmapImage};
use crate::surface::Surface3dData;

/// The viewer application state.
pub struct ViewerApp {
    rx: mpsc::Receiver<ViewerMsg>,
    figures: HashMap<u32, FigureWindow>,
}

impl ViewerApp {
    pub fn new(rx: mpsc::Receiver<ViewerMsg>) -> Self {
        Self {
            rx,
            figures: HashMap::new(),
        }
    }

    /// Drain all pending messages from the socket listener.
    fn process_messages(&mut self, ctx: &egui::Context) {
        let mut any_update = false;
        while let Ok(msg) = self.rx.try_recv() {
            any_update = true;
            match msg {
                ViewerMsg::FigureOpen {
                    id,
                    rows,
                    cols,
                    title,
                } => {
                    // Upsert: a repeat FigureOpen for an existing figure
                    // updates the title (and reshapes panels if the layout
                    // genuinely changed) but preserves panel data. This lets
                    // the client re-send FigureOpen later when the script
                    // calls `title("...")` after `figure(); surf(...)` —
                    // without wiping out the panel that just rendered.
                    let rows = rows as usize;
                    let cols = cols as usize;
                    match self.figures.get_mut(&id) {
                        Some(fig) if fig.rows == rows && fig.cols == cols => {
                            fig.title = title;
                            fig.dirty = true;
                        }
                        _ => {
                            self.figures
                                .insert(id, FigureWindow::new(rows, cols, title));
                        }
                    }
                }
                ViewerMsg::PanelUpdate {
                    fig_id,
                    panel,
                    series,
                } => {
                    if let Some(fig) = self.figures.get_mut(&fig_id) {
                        let idx = panel as usize;
                        if idx < fig.panels.len() {
                            fig.panels[idx].series = series;
                            fig.dirty = true;
                        }
                    }
                }
                ViewerMsg::PanelLabels {
                    fig_id,
                    panel,
                    title,
                    xlabel,
                    ylabel,
                } => {
                    if let Some(fig) = self.figures.get_mut(&fig_id) {
                        let idx = panel as usize;
                        if idx < fig.panels.len() {
                            fig.panels[idx].title = title;
                            fig.panels[idx].xlabel = xlabel;
                            fig.panels[idx].ylabel = ylabel;
                        }
                    }
                }
                ViewerMsg::PanelLimits {
                    fig_id,
                    panel,
                    xlim,
                    ylim,
                } => {
                    if let Some(fig) = self.figures.get_mut(&fig_id) {
                        let idx = panel as usize;
                        if idx < fig.panels.len() {
                            fig.panels[idx].xlim = xlim;
                            fig.panels[idx].ylim = ylim;
                        }
                    }
                }
                ViewerMsg::PanelAxisEqual {
                    fig_id,
                    panel,
                    equal,
                } => {
                    if let Some(fig) = self.figures.get_mut(&fig_id) {
                        let idx = panel as usize;
                        if idx < fig.panels.len() {
                            fig.panels[idx].axis_equal = equal;
                            fig.dirty = true;
                        }
                    }
                }
                ViewerMsg::PanelHeatmap {
                    fig_id,
                    panel,
                    heatmap,
                } => {
                    if let Some(fig) = self.figures.get_mut(&fig_id) {
                        let idx = panel as usize;
                        if idx < fig.panels.len() {
                            fig.panels[idx].heatmap = Some(HeatmapImage {
                                width: heatmap.width,
                                height: heatmap.height,
                                rgba: heatmap.rgba,
                                smooth: heatmap.smooth,
                                texture: None, // created on first render
                                x_extent: heatmap.x_extent,
                                y_extent: heatmap.y_extent,
                                value_min: heatmap.value_min,
                                value_max: heatmap.value_max,
                                colorscale: heatmap.colorscale,
                            });
                            fig.dirty = true;
                        }
                    }
                }
                ViewerMsg::PanelSurface {
                    fig_id,
                    panel,
                    surface,
                } => {
                    if let Some(fig) = self.figures.get_mut(&fig_id) {
                        let idx = panel as usize;
                        if idx < fig.panels.len() {
                            let data = Surface3dData {
                                nrows: surface.nrows as usize,
                                ncols: surface.ncols as usize,
                                x: surface.x,
                                y: surface.y,
                                z: surface.z,
                                colorscale: surface.colorscale,
                            };
                            // Preserve camera if user was already rotating
                            // this panel; otherwise start at the default view.
                            let cam = fig.panels[idx]
                                .surface
                                .as_ref()
                                .map(|(_, c)| *c)
                                .unwrap_or_default();
                            fig.panels[idx].surface = Some((data, cam));
                            // A surface replaces any heatmap/series in this panel.
                            fig.panels[idx].heatmap = None;
                            fig.panels[idx].series.clear();
                            fig.dirty = true;
                        }
                    }
                }
                ViewerMsg::Redraw { fig_id } => {
                    if let Some(fig) = self.figures.get_mut(&fig_id) {
                        fig.dirty = true;
                    }
                }
                ViewerMsg::Close { fig_id } => {
                    self.figures.remove(&fig_id);
                }
                ViewerMsg::Reset => {
                    self.figures.clear();
                }
                ViewerMsg::Ping => {} // handled at the connection level
            }
        }
        if any_update {
            ctx.request_repaint();
        }
    }
}

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // No repaint polling: the socket listener requests a repaint
        // whenever it queues a message (see net::WakeFn), so an idle
        // viewer draws no frames. Don't add request_repaint_after here —
        // continuous repaint costs real CPU under WSLg's RDP/software-GL
        // pipeline even when nothing changes.
        self.process_messages(ctx);
        self.render_ui(ctx);
    }
}

impl ViewerApp {
    /// Paint the current figure set. Split out of `eframe::App::update` so a
    /// headless test can drive the real render path through `Context::run`
    /// without constructing an `eframe::Frame`.
    fn render_ui(&mut self, ctx: &egui::Context) {
        // Dark theme is fixed once at startup (main.rs).
        if self.figures.is_empty() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label("Waiting for rustlab connection...");
                });
            });
            return;
        }

        // Render each figure in an egui Window (or central panel if only one)
        if self.figures.len() == 1 {
            let (&id, fig) = self.figures.iter_mut().next().unwrap();
            egui::CentralPanel::default().show(ctx, |ui| {
                let heading = display_figure_title(id, &fig.title);
                ui.heading(&heading);
                fig.render(ui, id);
            });
        } else {
            egui::CentralPanel::default().show(ctx, |_ui| {});
            let ids: Vec<u32> = self.figures.keys().copied().collect();
            for id in ids {
                let fig = self.figures.get_mut(&id).unwrap();
                let title = display_figure_title(id, &fig.title);
                egui::Window::new(&title)
                    .id(egui::Id::new(format!("fig_{}", id)))
                    .resizable(true)
                    .show(ctx, |ui| {
                        fig.render(ui, id);
                    });
            }
        }
    }
}

/// Build the user-facing title for a figure window.
///
/// Figure IDs on the wire encode the client's PID in the upper 16 bits
/// (`(pid << 16) | counter`) so multiple rustlab processes connected to one
/// viewer don't collide. That raw number shouldn't ever show up in the UI —
/// we only surface the counter portion.
pub(crate) fn display_figure_title(id: u32, user_title: &str) -> String {
    if !user_title.is_empty() {
        return user_title.to_string();
    }
    let counter = id & 0xFFFF;
    format!("Figure {}", counter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_title_takes_precedence() {
        let id = (12345u32 << 16) | 7;
        assert_eq!(display_figure_title(id, "Gaussian"), "Gaussian");
    }

    #[test]
    fn fallback_strips_pid_prefix() {
        // 50000 << 16 | 1 = 3276800001 — the kind of "random number" users saw.
        let id = (50000u32 << 16) | 1;
        assert_eq!(display_figure_title(id, ""), "Figure 1");
    }

    #[test]
    fn fallback_uses_counter_only_for_higher_counts() {
        let id = (777u32 << 16) | 42;
        assert_eq!(display_figure_title(id, ""), "Figure 42");
    }

    #[test]
    fn fallback_without_pid_is_still_counter() {
        // Tests run with no PID encoding still work (counter = id).
        assert_eq!(display_figure_title(3, ""), "Figure 3");
    }

    use rustlab_proto::{
        WireColor, WireHeatmap, WireLineStyle, WirePlotKind, WireSeries, WireSurface,
    };

    fn line_series() -> WireSeries {
        WireSeries {
            label: "s".into(),
            x: vec![0.0, 1.0, 2.0],
            y: vec![0.0, 1.0, 0.5],
            color: WireColor::Named("cyan".into()),
            style: WireLineStyle::Solid,
            kind: WirePlotKind::Line,
            x_labels: None,
        }
    }

    /// Drive `n` real (headless) egui frames over the app, each draining the
    /// channel and painting. Mirrors `eframe::App::update`.
    fn run_frames(app: &mut ViewerApp, n: usize) {
        run_frames_sized(app, n, egui::Vec2::new(1024.0, 768.0));
    }

    /// Like [`run_frames`] but forces a specific window size, so we can
    /// reproduce the cramped-layout case two REPLs hit (multiple figure
    /// windows competing for a small screen).
    fn run_frames_sized(app: &mut ViewerApp, n: usize, size: egui::Vec2) {
        let ctx = egui::Context::default();
        // eframe enables AccessKit; it validates that every widget has a
        // unique Id and panics on duplicates. A bare Context doesn't, so
        // turn it on here to mirror the real app.
        ctx.enable_accesskit();
        for _ in 0..n {
            let mut input = egui::RawInput::default();
            input.screen_rect =
                Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size));
            let output = ctx.run(input, |ctx| {
                app.process_messages(ctx);
                app.render_ui(ctx);
            });
            // `Context::run` only produces shapes; the real backend then
            // tessellates them. Run the tessellator too so a NaN/degenerate
            // mesh (a GUI-only crash) surfaces in the test.
            let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        }
    }

    /// "Connect twice": two rustlab processes (distinct PID prefixes) each
    /// open a figure, so the viewer enters the multi-window render path with
    /// a heatmap figure and a surface figure live at once. Must not panic.
    #[test]
    fn two_connections_two_figures_render_without_panic() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);

        // Connection A — first rustlab session, a heatmap figure.
        let id_a = (111u32 << 16) | 0; // local counter 0, like the first figure
        tx.send(ViewerMsg::FigureOpen {
            id: id_a,
            rows: 1,
            cols: 1,
            title: String::new(),
        })
        .unwrap();
        tx.send(ViewerMsg::PanelHeatmap {
            fig_id: id_a,
            panel: 0,
            heatmap: WireHeatmap {
                width: 2,
                height: 2,
                rgba: vec![0u8; 2 * 2 * 4],
                smooth: false,
                x_extent: None,
                y_extent: None,
                value_min: Some(-1.0),
                value_max: Some(1.0),
                colorscale: "viridis".into(),
            },
        })
        .unwrap();
        tx.send(ViewerMsg::Redraw { fig_id: id_a }).unwrap();

        // Connection B — a second rustlab session connects (same local
        // counter 0, different PID prefix) and opens a surface figure.
        let id_b = (222u32 << 16) | 0;
        tx.send(ViewerMsg::FigureOpen {
            id: id_b,
            rows: 1,
            cols: 1,
            title: String::new(),
        })
        .unwrap();
        tx.send(ViewerMsg::PanelSurface {
            fig_id: id_b,
            panel: 0,
            surface: WireSurface {
                nrows: 2,
                ncols: 2,
                x: vec![0.0, 1.0],
                y: vec![0.0, 1.0],
                z: vec![0.0, 1.0, 1.0, 0.0],
                colorscale: "viridis".into(),
            },
        })
        .unwrap();
        tx.send(ViewerMsg::Redraw { fig_id: id_b }).unwrap();

        run_frames(&mut app, 3);
        assert_eq!(app.figures.len(), 2, "both sessions' figures should be live");
    }

    /// A second connection's `Reset` (sent by `connect_viewer` on every new
    /// `viewer on`) followed by its own figure. Confirms the global reset
    /// path is panic-free and observe what it does to an existing figure.
    #[test]
    fn second_connection_reset_then_open() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);

        let id_a = (111u32 << 16) | 0;
        tx.send(ViewerMsg::FigureOpen {
            id: id_a,
            rows: 1,
            cols: 1,
            title: "A".into(),
        })
        .unwrap();
        tx.send(ViewerMsg::PanelUpdate {
            fig_id: id_a,
            panel: 0,
            series: vec![line_series()],
        })
        .unwrap();
        run_frames(&mut app, 1);
        assert_eq!(app.figures.len(), 1);

        // New session connects: Reset wipes everything, then opens its own.
        let id_b = (222u32 << 16) | 0;
        tx.send(ViewerMsg::Reset).unwrap();
        tx.send(ViewerMsg::FigureOpen {
            id: id_b,
            rows: 1,
            cols: 1,
            title: "B".into(),
        })
        .unwrap();
        tx.send(ViewerMsg::PanelUpdate {
            fig_id: id_b,
            panel: 0,
            series: vec![line_series()],
        })
        .unwrap();
        run_frames(&mut app, 1);
        assert!(app.figures.contains_key(&id_b));
    }

    /// Two REPLs both `viewer on` and each plot a multi-panel, titled figure,
    /// landing in the multi-window path on a *small* screen so per-panel
    /// dimensions get squeezed. Probes for an unclamped negative/zero plot
    /// size panicking in the real layout. Must not panic.
    #[test]
    fn two_figures_cramped_layout_render_without_panic() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);

        for (i, pid) in [111u32, 222u32].into_iter().enumerate() {
            let id = (pid << 16) | i as u32;
            tx.send(ViewerMsg::FigureOpen {
                id,
                rows: 2,
                cols: 2,
                title: String::new(),
            })
            .unwrap();
            for panel in 0..4u16 {
                tx.send(ViewerMsg::PanelLabels {
                    fig_id: id,
                    panel,
                    title: format!("panel {panel}"),
                    xlabel: "x".into(),
                    ylabel: "y".into(),
                })
                .unwrap();
                tx.send(ViewerMsg::PanelUpdate {
                    fig_id: id,
                    panel,
                    series: vec![line_series()],
                })
                .unwrap();
            }
            tx.send(ViewerMsg::Redraw { fig_id: id }).unwrap();
        }

        // Tiny window → two windows each subdivided into 2×2 titled panels.
        run_frames_sized(&mut app, 3, egui::Vec2::new(120.0, 90.0));
        assert_eq!(app.figures.len(), 2);
    }
}
