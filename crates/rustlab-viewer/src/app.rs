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
        self.process_messages(ctx);

        // Request periodic repaint so we pick up new messages promptly.
        // When idle this costs almost nothing on modern GPUs.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        // Dark theme
        ctx.set_visuals(egui::Visuals::dark());

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
}
