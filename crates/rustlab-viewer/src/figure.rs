//! Figure and panel state for the viewer application.

use egui_plot::{Plot, PlotBounds, PlotImage};
use rustlab_proto::WireSeries;
use std::sync::Arc;

use crate::render;
use crate::surface::{Surface3dData, SurfaceCamera};

/// Pre-rendered heatmap image ready for egui display.
pub struct HeatmapImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// When true, sample the texture with linear filtering for smooth zoom
    /// (used for pre-rendered figure overlays). When false, use nearest-
    /// neighbour to keep raw data heatmap cell boundaries crisp.
    pub smooth: bool,
    /// Cached egui texture handle; created on first render.
    pub texture: Option<egui::TextureHandle>,
}

/// State for a single subplot panel.
pub struct PanelState {
    pub title: String,
    pub xlabel: String,
    pub ylabel: String,
    pub series: Vec<WireSeries>,
    pub xlim: (Option<f64>, Option<f64>),
    pub ylim: (Option<f64>, Option<f64>),
    pub axis_equal: bool,
    pub heatmap: Option<HeatmapImage>,
    /// 3D surface data + camera. When present, the panel renders a rotatable
    /// surface instead of the 2D egui_plot chart.
    pub surface: Option<(Surface3dData, SurfaceCamera)>,
}

impl PanelState {
    pub fn new() -> Self {
        Self {
            title: String::new(),
            xlabel: String::new(),
            ylabel: String::new(),
            series: Vec::new(),
            xlim: (None, None),
            ylim: (None, None),
            axis_equal: false,
            heatmap: None,
            surface: None,
        }
    }
}

/// A figure window containing a grid of subplot panels.
pub struct FigureWindow {
    pub rows: usize,
    pub cols: usize,
    pub title: String,
    pub panels: Vec<PanelState>,
    /// Set to true when new data arrives; cleared after first redraw.
    pub dirty: bool,
}

impl FigureWindow {
    pub fn new(rows: usize, cols: usize, title: String) -> Self {
        let n = rows * cols;
        let panels = (0..n).map(|_| PanelState::new()).collect();
        Self {
            rows,
            cols,
            title,
            panels,
            dirty: true,
        }
    }

    /// Render this figure's subplot grid into the given `Ui`.
    /// `fig_id` is used to generate unique egui widget IDs across figures.
    pub fn render(&mut self, ui: &mut egui::Ui, fig_id: u32) {
        let avail = ui.available_size();
        let cell_w = avail.x / self.cols as f32;
        let cell_h = avail.y / self.rows as f32;

        for row in 0..self.rows {
            ui.horizontal(|ui| {
                for col in 0..self.cols {
                    let idx = row * self.cols + col;
                    if idx >= self.panels.len() {
                        continue;
                    }
                    let panel = &mut self.panels[idx];

                    let title_h = if panel.title.is_empty() { 0.0 } else { 20.0 };

                    ui.vertical(|ui| {
                        if !panel.title.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new(&panel.title).strong().size(14.0));
                            });
                        }

                        // 3D surface panel: render via the custom software
                        // renderer instead of egui_plot so users can rotate,
                        // tilt, and zoom.
                        if let Some((data, cam)) = panel.surface.as_mut() {
                            let size = egui::Vec2::new(cell_w - 8.0, cell_h - 8.0 - title_h);
                            crate::surface::draw(ui, size, data, cam);
                            return;
                        }

                        let plot_id = format!("fig_{}_panel_{}_{}", fig_id, row, col);
                        let mut plot = Plot::new(&plot_id)
                            .width(cell_w - 8.0)
                            .height(cell_h - 8.0 - title_h)
                            .show_axes([true, true])
                            .show_grid([true, true])
                            .allow_zoom(true)
                            .allow_drag(true)
                            .allow_scroll(true)
                            .x_axis_label(&panel.xlabel)
                            .y_axis_label(&panel.ylabel)
                            .label_formatter(|name, value| {
                                if name.is_empty() {
                                    format!("x: {:.4}\ny: {:.4}", value.x, value.y)
                                } else {
                                    format!("{}\nx: {:.4}\ny: {:.4}", name, value.x, value.y)
                                }
                            });
                        if panel.axis_equal {
                            plot = plot.data_aspect(1.0);
                        }

                        // Apply categorical x-axis labels if present
                        let cat_labels: Option<Arc<Vec<(f64, String)>>> = panel
                            .series
                            .iter()
                            .find_map(|s| s.x_labels.as_ref())
                            .map(|labels| {
                                Arc::new(
                                    labels
                                        .iter()
                                        .enumerate()
                                        .map(|(i, l)| (i as f64, l.clone()))
                                        .collect(),
                                )
                            });
                        if let Some(labels) = cat_labels {
                            plot = plot.x_axis_formatter(move |mark, _range| {
                                let idx = mark.value.round() as usize;
                                labels
                                    .iter()
                                    .find(|(x, _)| (*x - mark.value).abs() < 0.5)
                                    .map(|(_, l)| l.clone())
                                    .unwrap_or_else(|| {
                                        if idx < labels.len() {
                                            String::new()
                                        } else {
                                            String::new()
                                        }
                                    })
                            });
                        }

                        // Heatmap y-axis: image convention matching the
                        // SVG/HTML backends. The texture's row 0 sits at
                        // the TOP of the plot (egui draws textures with
                        // their top pixel at the top of the bounding
                        // box, and our plot bounds run y ∈ [0, height]
                        // so the top of the bounding box is at plot-y =
                        // height). Without this formatter, the default
                        // y-axis ticks would read 0 at the bottom and N
                        // at the top — physics-convention labels
                        // disagreeing with the image-convention render,
                        // the exact bug fixed in SVG/HTML on 2026-05-16.
                        // Reverse the labels so the top tick reads `0`
                        // and the bottom reads N.
                        if let Some(ref hm) = panel.heatmap {
                            let height = hm.height as f64;
                            plot = plot.y_axis_formatter(move |mark, _range| {
                                let displayed = height - mark.value;
                                let rounded = displayed.round();
                                if (displayed - rounded).abs() < 1e-6 {
                                    format!("{}", rounded as i64)
                                } else {
                                    format!("{:.2}", displayed)
                                }
                            });
                        }

                        // Set fixed bounds when limits are specified
                        let has_bounds = panel.xlim.0.is_some()
                            || panel.xlim.1.is_some()
                            || panel.ylim.0.is_some()
                            || panel.ylim.1.is_some();
                        if has_bounds {
                            // Auto-fit is disabled when explicit bounds are set
                            plot = plot.auto_bounds([
                                panel.xlim.0.is_none().into(),
                                panel.ylim.0.is_none().into(),
                            ]);
                        }

                        // Ensure heatmap texture is created before entering plot closure
                        if let Some(ref mut hm) = panel.heatmap {
                            if hm.texture.is_none() && !hm.rgba.is_empty() {
                                let image = egui::ColorImage::from_rgba_unmultiplied(
                                    [hm.width as usize, hm.height as usize],
                                    &hm.rgba,
                                );
                                let opts = if hm.smooth {
                                    egui::TextureOptions::LINEAR
                                } else {
                                    egui::TextureOptions::NEAREST
                                };
                                hm.texture =
                                    Some(ui.ctx().load_texture("heatmap", image, opts));
                            }
                        }

                        // Collect texture info before the closure borrows panel immutably
                        let hm_info = panel.heatmap.as_ref().and_then(|hm| {
                            hm.texture
                                .as_ref()
                                .map(|tex| (tex.id(), hm.width as f64, hm.height as f64))
                        });

                        plot.show(ui, |plot_ui| {
                            // Apply explicit bounds (x and y independently)
                            let cur = plot_ui.plot_bounds();
                            match (panel.xlim, panel.ylim) {
                                ((Some(x0), Some(x1)), (Some(y0), Some(y1))) => {
                                    plot_ui.set_plot_bounds(PlotBounds::from_min_max(
                                        [x0, y0],
                                        [x1, y1],
                                    ));
                                }
                                ((Some(x0), Some(x1)), _) => {
                                    plot_ui.set_plot_bounds(PlotBounds::from_min_max(
                                        [x0, *cur.range_y().start()],
                                        [x1, *cur.range_y().end()],
                                    ));
                                }
                                (_, (Some(y0), Some(y1))) => {
                                    plot_ui.set_plot_bounds(PlotBounds::from_min_max(
                                        [*cur.range_x().start(), y0],
                                        [*cur.range_x().end(), y1],
                                    ));
                                }
                                _ => {}
                            }

                            // Render heatmap as a texture image if present
                            if let Some((tex_id, w, h)) = hm_info {
                                let center = egui_plot::PlotPoint::new(w / 2.0, h / 2.0);
                                let size = egui::Vec2::new(w as f32, h as f32);
                                plot_ui.image(PlotImage::new(tex_id, center, size));
                            }

                            for series in &panel.series {
                                render::render_series(plot_ui, series);
                            }
                        });
                    }); // close ui.vertical
                }
            });
        }

        self.dirty = false;
    }
}
