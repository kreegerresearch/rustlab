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
                            // `set_limits` only re-arms the apply latch
                            // when the values actually changed, so a live
                            // plot re-sending the same limits every redraw
                            // doesn't clobber the user's zoom.
                            fig.panels[idx].set_limits(xlim, ylim);
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

    // ---------------------------------------------------------------
    // Scroll zoom / pan / Home
    //
    // These drive the real widgets over a *persistent* `egui::Context`:
    // egui_plot keeps a panel's bounds in context memory between frames,
    // so a fresh context per frame (what `run_frames` builds) would hide
    // exactly the bug this feature fixes.
    // ---------------------------------------------------------------

    /// One context, many frames, with synthetic pointer/wheel/key input.
    struct Harness {
        ctx: egui::Context,
        size: egui::Vec2,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            Self {
                ctx,
                size: egui::Vec2::new(1024.0, 768.0),
            }
        }

        fn frame(&self, app: &mut ViewerApp, events: Vec<egui::Event>) {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                events,
                ..Default::default()
            };
            let output = self.ctx.run(input, |ctx| {
                app.process_messages(ctx);
                app.render_ui(ctx);
            });
            let _ = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        }

        fn frames(&self, app: &mut ViewerApp, n: usize) {
            for _ in 0..n {
                self.frame(app, Vec::new());
            }
        }

        fn plot_memory(&self, fig_id: u32) -> egui_plot::PlotMemory {
            egui_plot::PlotMemory::load(&self.ctx, crate::figure::panel_plot_id(fig_id, 0, 0))
                .expect("panel 0 should have plot memory after a frame")
        }

        fn bounds(&self, fig_id: u32) -> egui_plot::PlotBounds {
            *self.plot_memory(fig_id).bounds()
        }

        /// A point inside the panel's plot area, for hover-sensitive input.
        fn plot_center(&self, fig_id: u32) -> egui::Pos2 {
            self.plot_memory(fig_id).transform().frame().center()
        }

        /// Scroll the wheel over the plot for `n` frames. Point units below
        /// egui's 8-point threshold arrive unsmoothed, so the zoom is
        /// deterministic frame to frame.
        fn scroll_over_plot(&self, app: &mut ViewerApp, fig_id: u32, delta_y: f32, n: usize) {
            for _ in 0..n {
                let pos = self.plot_center(fig_id);
                self.frame(
                    app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::Vec2::new(0.0, delta_y),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        }

        /// Press and release the primary button over a widget, by id.
        fn click_widget(&self, app: &mut ViewerApp, id: egui::Id) {
            let rect = self
                .ctx
                .read_response(id)
                .expect("widget should exist after a frame")
                .rect;
            let pos = rect.center();
            self.frame(app, vec![egui::Event::PointerMoved(pos)]);
            self.frame(
                app,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            self.frame(
                app,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            // One more frame so the click's effect is drawn.
            self.frames(app, 1);
        }

        /// Tap a key with the pointer parked over the plot.
        fn key_over_plot(&self, app: &mut ViewerApp, fig_id: u32, key: egui::Key) {
            let pos = self.plot_center(fig_id);
            self.frame(
                app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            self.frames(app, 1);
        }
    }

    /// A single-panel figure with a line and, optionally, script limits.
    fn open_line_figure(
        tx: &mpsc::Sender<ViewerMsg>,
        id: u32,
        limits: Option<(crate::view::AxisLimits, crate::view::AxisLimits)>,
    ) {
        tx.send(ViewerMsg::FigureOpen {
            id,
            rows: 1,
            cols: 1,
            title: "zoom".into(),
        })
        .unwrap();
        tx.send(ViewerMsg::PanelUpdate {
            fig_id: id,
            panel: 0,
            series: vec![line_series()],
        })
        .unwrap();
        if let Some((xlim, ylim)) = limits {
            tx.send(ViewerMsg::PanelLimits {
                fig_id: id,
                panel: 0,
                xlim,
                ylim,
            })
            .unwrap();
        }
        tx.send(ViewerMsg::Redraw { fig_id: id }).unwrap();
    }

    const XLIM: crate::view::AxisLimits = (Some(0.0), Some(2.0));
    const YLIM: crate::view::AxisLimits = (Some(0.0), Some(1.0));

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn script_limits_are_applied_on_first_show() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);
        let id = 1u32;
        open_line_figure(&tx, id, Some((XLIM, YLIM)));

        let h = Harness::new();
        h.frames(&mut app, 2);

        let b = h.bounds(id);
        assert!(
            approx(b.min()[0], 0.0) && approx(b.max()[0], 2.0),
            "x: {b:?}"
        );
        assert!(
            approx(b.min()[1], 0.0) && approx(b.max()[1], 1.0),
            "y: {b:?}"
        );
    }

    /// The bug behind this feature: `set_plot_bounds` ran every frame, so a
    /// scrolled view snapped back to the script's limits one frame later.
    #[test]
    fn scroll_zooms_and_the_zoom_sticks() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);
        let id = 2u32;
        open_line_figure(&tx, id, Some((XLIM, YLIM)));

        let h = Harness::new();
        h.frames(&mut app, 2);
        let before = h.bounds(id);

        h.scroll_over_plot(&mut app, id, 7.0, 3);
        let zoomed = h.bounds(id);
        assert!(
            zoomed.width() < before.width() * 0.99,
            "scrolling up should zoom in: {} → {}",
            before.width(),
            zoomed.width()
        );
        assert!(
            zoomed.height() < before.height() * 0.99,
            "both axes zoom together: {} → {}",
            before.height(),
            zoomed.height()
        );

        // Idle frames (the script keeps redrawing) must not restore the
        // limits over the top of the user's zoom.
        h.frames(&mut app, 3);
        let after = h.bounds(id);
        assert!(
            approx(after.width(), zoomed.width()) && approx(after.height(), zoomed.height()),
            "zoom must survive later frames: {zoomed:?} → {after:?}"
        );
    }

    #[test]
    fn scrolling_down_zooms_out() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);
        let id = 3u32;
        open_line_figure(&tx, id, Some((XLIM, YLIM)));

        let h = Harness::new();
        h.frames(&mut app, 2);
        let before = h.bounds(id);

        h.scroll_over_plot(&mut app, id, -7.0, 3);
        assert!(h.bounds(id).width() > before.width() * 1.01);
    }

    /// A live plot re-sends the same `plot_limits` on every redraw; that
    /// must not count as "new limits" and wipe out the user's zoom.
    #[test]
    fn repeated_identical_limits_do_not_reset_the_view() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);
        let id = 4u32;
        open_line_figure(&tx, id, Some((XLIM, YLIM)));

        let h = Harness::new();
        h.frames(&mut app, 2);
        h.scroll_over_plot(&mut app, id, 7.0, 3);
        let zoomed = h.bounds(id);

        for _ in 0..3 {
            tx.send(ViewerMsg::PanelLimits {
                fig_id: id,
                panel: 0,
                xlim: XLIM,
                ylim: YLIM,
            })
            .unwrap();
            tx.send(ViewerMsg::Redraw { fig_id: id }).unwrap();
            h.frames(&mut app, 1);
        }
        assert!(approx(h.bounds(id).width(), zoomed.width()));
    }

    /// Fresh limits from the script *do* re-frame the panel.
    #[test]
    fn changed_limits_reframe_the_panel() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);
        let id = 5u32;
        open_line_figure(&tx, id, Some((XLIM, YLIM)));

        let h = Harness::new();
        h.frames(&mut app, 2);
        h.scroll_over_plot(&mut app, id, 7.0, 3);

        tx.send(ViewerMsg::PanelLimits {
            fig_id: id,
            panel: 0,
            xlim: (Some(-5.0), Some(5.0)),
            ylim: (Some(-1.0), Some(3.0)),
        })
        .unwrap();
        h.frames(&mut app, 2);

        let b = h.bounds(id);
        assert!(approx(b.min()[0], -5.0) && approx(b.max()[0], 5.0), "{b:?}");
        assert!(approx(b.min()[1], -1.0) && approx(b.max()[1], 3.0), "{b:?}");
    }

    #[test]
    fn home_button_restores_script_limits() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);
        let id = 6u32;
        open_line_figure(&tx, id, Some((XLIM, YLIM)));

        let h = Harness::new();
        h.frames(&mut app, 2);
        h.scroll_over_plot(&mut app, id, 7.0, 3);
        assert!(h.bounds(id).width() < 2.0);

        h.click_widget(&mut app, crate::figure::home_button_id(id, 0, 0));

        let b = h.bounds(id);
        assert!(
            approx(b.min()[0], 0.0) && approx(b.max()[0], 2.0),
            "x: {b:?}"
        );
        assert!(
            approx(b.min()[1], 0.0) && approx(b.max()[1], 1.0),
            "y: {b:?}"
        );
    }

    #[test]
    fn home_key_restores_script_limits() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);
        let id = 7u32;
        open_line_figure(&tx, id, Some((XLIM, YLIM)));

        let h = Harness::new();
        h.frames(&mut app, 2);
        h.scroll_over_plot(&mut app, id, 7.0, 3);
        assert!(h.bounds(id).width() < 2.0);

        h.key_over_plot(&mut app, id, egui::Key::Home);

        let b = h.bounds(id);
        assert!(
            approx(b.min()[0], 0.0) && approx(b.max()[0], 2.0),
            "x: {b:?}"
        );
    }

    /// Without script limits, Home auto-fits the data instead.
    #[test]
    fn home_auto_fits_when_the_script_set_no_limits() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);
        let id = 8u32;
        open_line_figure(&tx, id, None);

        let h = Harness::new();
        h.frames(&mut app, 2);
        let fitted = h.bounds(id);
        assert!(
            h.plot_memory(id).auto_bounds.x,
            "a limit-free panel auto-fits"
        );

        h.scroll_over_plot(&mut app, id, 7.0, 3);
        assert!(h.bounds(id).width() < fitted.width() * 0.99);
        assert!(!h.plot_memory(id).auto_bounds.x, "zooming pins the bounds");

        h.click_widget(&mut app, crate::figure::home_button_id(id, 0, 0));

        assert!(
            h.plot_memory(id).auto_bounds.x,
            "Home hands x back to auto-fit"
        );
        assert!(
            h.plot_memory(id).auto_bounds.y,
            "Home hands y back to auto-fit"
        );
        let restored = h.bounds(id);
        assert!(
            approx(restored.width(), fitted.width()),
            "Home should refit the data: {fitted:?} → {restored:?}"
        );
    }

    /// Every subplot gets its own Home button, and using one leaves the
    /// others alone.
    #[test]
    fn home_is_per_subplot() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);
        let id = 9u32;
        tx.send(ViewerMsg::FigureOpen {
            id,
            rows: 1,
            cols: 2,
            title: String::new(),
        })
        .unwrap();
        for panel in 0..2u16 {
            tx.send(ViewerMsg::PanelUpdate {
                fig_id: id,
                panel,
                series: vec![line_series()],
            })
            .unwrap();
            tx.send(ViewerMsg::PanelLimits {
                fig_id: id,
                panel,
                xlim: XLIM,
                ylim: YLIM,
            })
            .unwrap();
        }
        tx.send(ViewerMsg::Redraw { fig_id: id }).unwrap();

        let h = Harness::new();
        h.frames(&mut app, 2);
        for col in 0..2 {
            assert!(
                h.ctx
                    .read_response(crate::figure::home_button_id(id, 0, col))
                    .is_some(),
                "subplot {col} should have its own Home button"
            );
        }

        // Zoom the right-hand panel by hand (its own plot id), then Home
        // the left one: the right panel must keep its zoom.
        let right_id = crate::figure::panel_plot_id(id, 0, 1);
        let right_center = egui_plot::PlotMemory::load(&h.ctx, right_id)
            .unwrap()
            .transform()
            .frame()
            .center();
        for _ in 0..3 {
            h.frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(right_center),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::Vec2::new(0.0, 7.0),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let right_zoomed = *egui_plot::PlotMemory::load(&h.ctx, right_id)
            .unwrap()
            .bounds();
        assert!(right_zoomed.width() < 2.0);

        h.click_widget(&mut app, crate::figure::home_button_id(id, 0, 0));

        let left = *egui_plot::PlotMemory::load(&h.ctx, crate::figure::panel_plot_id(id, 0, 0))
            .unwrap()
            .bounds();
        assert!(approx(left.width(), 2.0), "left panel reset: {left:?}");
        let right = *egui_plot::PlotMemory::load(&h.ctx, right_id)
            .unwrap()
            .bounds();
        assert!(
            approx(right.width(), right_zoomed.width()),
            "the other subplot keeps its zoom: {right:?}"
        );
    }

    /// 3D surfaces get the same visible Home — equivalent to pressing `R`.
    #[test]
    fn home_button_resets_the_surface_camera() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);
        let id = 10u32;
        tx.send(ViewerMsg::FigureOpen {
            id,
            rows: 1,
            cols: 1,
            title: "surf".into(),
        })
        .unwrap();
        tx.send(ViewerMsg::PanelSurface {
            fig_id: id,
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

        let h = Harness::new();
        h.frames(&mut app, 2);

        // Rotate/zoom away from the default camera.
        {
            let (_, cam) = app.figures.get_mut(&id).unwrap().panels[0]
                .surface
                .as_mut()
                .unwrap();
            cam.yaw = 1.25;
            cam.zoom = 3.0;
        }
        h.frames(&mut app, 1);

        h.click_widget(&mut app, crate::figure::home_button_id(id, 0, 0));

        let (_, cam) = app.figures[&id].panels[0].surface.as_ref().unwrap();
        let default = crate::surface::SurfaceCamera::default();
        assert!((cam.yaw - default.yaw).abs() < 1e-6, "yaw reset");
        assert!((cam.zoom - default.zoom).abs() < 1e-6, "zoom reset");
    }

    /// Drag still pans (the wheel took over zoom, not pan), and the pan
    /// survives later frames for the same reason a zoom does.
    #[test]
    fn drag_pans_and_the_pan_sticks() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);
        let id = 11u32;
        open_line_figure(&tx, id, Some((XLIM, YLIM)));

        let h = Harness::new();
        h.frames(&mut app, 2);
        let before = h.bounds(id);

        let start = h.plot_center(id);
        let end = start + egui::Vec2::new(60.0, 0.0);
        h.frame(&mut app, vec![egui::Event::PointerMoved(start)]);
        h.frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        h.frame(&mut app, vec![egui::Event::PointerMoved(end)]);
        h.frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        h.frames(&mut app, 1);

        let panned = h.bounds(id);
        assert!(
            approx(panned.width(), before.width()),
            "panning must not change the zoom level: {before:?} → {panned:?}"
        );
        assert!(
            panned.min()[0] < before.min()[0] - 1e-3,
            "dragging right moves the view left along x: {before:?} → {panned:?}"
        );

        h.frames(&mut app, 3);
        assert!(approx(h.bounds(id).min()[0], panned.min()[0]), "pan sticks");
    }

    /// egui_plot's built-in double-click reset goes through the same Home
    /// path, so it restores the script's limits rather than auto-fitting.
    #[test]
    fn double_click_restores_script_limits() {
        let (tx, rx) = mpsc::channel();
        let mut app = ViewerApp::new(rx);
        let id = 12u32;
        open_line_figure(&tx, id, Some((XLIM, YLIM)));

        let h = Harness::new();
        h.frames(&mut app, 2);
        h.scroll_over_plot(&mut app, id, 7.0, 3);
        assert!(h.bounds(id).width() < 2.0);

        let pos = h.plot_center(id);
        h.frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
        for _ in 0..2 {
            h.frame(
                &mut app,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            h.frame(
                &mut app,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
        h.frames(&mut app, 1);

        let b = h.bounds(id);
        assert!(
            approx(b.min()[0], 0.0) && approx(b.max()[0], 2.0),
            "x: {b:?}"
        );
    }

}
