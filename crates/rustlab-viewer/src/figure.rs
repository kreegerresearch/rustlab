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
    /// Data-coordinate placement on the x-axis. `Some((lo, hi))` makes
    /// the viewer paint the texture into `[lo, hi]` so tick labels read
    /// in user units (seconds, mm, etc.). `None` falls back to pixel-
    /// index extents and to the panel's `xlim` if that is set.
    pub x_extent: Option<(f64, f64)>,
    /// Data-coordinate placement on the y-axis. Same semantics as
    /// `x_extent`. For a spectrogram this is typically
    /// `Some((0.0, fs / 2.0))`.
    pub y_extent: Option<(f64, f64)>,
    /// Colormap value range used by the sender when rasterising the
    /// RGBA. Required for drawing a colorbar legend; `None` disables
    /// the legend.
    pub value_min: Option<f64>,
    pub value_max: Option<f64>,
    /// Colormap name. Used by the legend gradient strip. Empty string
    /// means "viridis".
    pub colorscale: String,
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

                        // Reserve a gutter on the right for the colorbar
                        // legend when the panel has a heatmap with a known
                        // colour range (sender pinned `value_min` /
                        // `value_max`). The colorbar is drawn in a sibling
                        // horizontal layout after `plot.show(...)`.
                        let (cbar_vmin, cbar_vmax, cbar_colorscale) = panel
                            .heatmap
                            .as_ref()
                            .and_then(|hm| match (hm.value_min, hm.value_max) {
                                (Some(a), Some(b)) if b > a => {
                                    Some((a, b, hm.colorscale.clone()))
                                }
                                _ => None,
                            })
                            .map(|(a, b, c)| (Some(a), Some(b), c))
                            .unwrap_or((None, None, String::new()));
                        let cbar_w = if cbar_vmin.is_some() { 56.0_f32 } else { 0.0 };
                        let plot_width = (cell_w - 8.0 - cbar_w).max(40.0);
                        let plot_height = cell_h - 8.0 - title_h;

                        let plot_id = format!("fig_{}_panel_{}_{}", fig_id, row, col);
                        let mut plot = Plot::new(&plot_id)
                            .width(plot_width)
                            .height(plot_height)
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

                        // Heatmap y-axis labels read the plot-coord
                        // directly (0 at the bottom, height at the top).
                        // The sender (`viewer_live::update_panel_heatmap`
                        // and the static `sync_viewer` heatmap path)
                        // builds the RGBA in physics convention — source
                        // row 0 lands at the bottom of the texture — so
                        // no further label flip is needed here.
                        //
                        // (A prior version of this code applied a
                        // `height - mark.value` formatter that assumed
                        // image convention; that broke the live
                        // spectrogram, which has been physics-convention
                        // since `figure_live`'s introduction. The
                        // formatter was redundant for static viewer
                        // heatmaps anyway — `render_panel_to_rgba` bakes
                        // its own axes into the texture.)

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

                        // Collect texture info plus the data-coord extent
                        // before the closure borrows panel immutably. The
                        // extent priority is: heatmap-supplied (`x_extent`
                        // / `y_extent` on the wire message) wins, then the
                        // panel's `xlim` / `ylim` from `plot_limits`, then
                        // a fallback of pixel-index coords. This is what
                        // makes the live spectrogram's y-axis read in Hz
                        // instead of bin indices — the script calls
                        // `plot_limits(fig, 1, [0, time_span], [0, fs/2])`
                        // and the image stretches to fit those bounds.
                        let panel_x = panel.xlim;
                        let panel_y = panel.ylim;
                        let hm_info = panel.heatmap.as_ref().and_then(|hm| {
                            hm.texture.as_ref().map(|tex| {
                                let (xl, xh) = match (hm.x_extent, panel_x) {
                                    (Some(e), _) => e,
                                    (None, (Some(a), Some(b))) => (a, b),
                                    _ => (0.0, hm.width as f64),
                                };
                                let (yl, yh) = match (hm.y_extent, panel_y) {
                                    (Some(e), _) => e,
                                    (None, (Some(a), Some(b))) => (a, b),
                                    _ => (0.0, hm.height as f64),
                                };
                                (tex.id(), xl, xh, yl, yh)
                            })
                        });

                        ui.horizontal(|ui| {
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

                            // Render heatmap as a texture image, placed
                            // in data coords so the egui_plot axis ticks
                            // read in the same units (Hz, sec, m, …).
                            if let Some((tex_id, xl, xh, yl, yh)) = hm_info {
                                let center = egui_plot::PlotPoint::new(
                                    (xl + xh) * 0.5,
                                    (yl + yh) * 0.5,
                                );
                                let size = egui::Vec2::new(
                                    (xh - xl) as f32,
                                    (yh - yl) as f32,
                                );
                                plot_ui.image(PlotImage::new(tex_id, center, size));
                            }

                            for series in &panel.series {
                                render::render_series(plot_ui, series);
                            }
                        });

                        // Colorbar legend: thin gradient strip to the
                        // right of the plot, painted only when the sender
                        // pinned an explicit colour range. The gradient
                        // is computed via `crate::surface::colormap_rgb`
                        // so it matches the cell colours exactly.
                        if let (Some(lo), Some(hi)) = (cbar_vmin, cbar_vmax) {
                            draw_colorbar(
                                ui,
                                cbar_w,
                                plot_height,
                                lo,
                                hi,
                                &cbar_colorscale,
                            );
                        }
                        }); // close ui.horizontal
                    }); // close ui.vertical
                }
            });
        }

        self.dirty = false;
    }
}

/// Draw a vertical colorbar legend strip with `vmin` at the bottom and
/// `vmax` at the top, alongside a thin gradient column using the same
/// `colormap_rgb` lookup as the heatmap texture. `width` and `height` are
/// the total footprint reserved for the legend (gradient + labels +
/// padding); the gradient column itself is fixed at ~14 px wide.
fn draw_colorbar(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    vmin: f64,
    vmax: f64,
    colorscale: &str,
) {
    use egui::{Color32, Pos2, Rect, Stroke, Vec2};

    let (rect, _resp) = ui.allocate_exact_size(
        Vec2::new(width, height),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);

    // Reserve room at the top and bottom for the numeric labels and a
    // few pixels of breathing room. Labels are 12pt text → ~14 px tall.
    let label_pad = 14.0;
    let grad_top = rect.top() + label_pad;
    let grad_bottom = rect.bottom() - label_pad;
    let grad_h = (grad_bottom - grad_top).max(1.0);
    let grad_w = 14.0_f32.min(width - 4.0);
    let grad_left = rect.left() + 4.0;
    let grad_right = grad_left + grad_w;

    // Paint the gradient as a column of 1-pixel-tall horizontal lines,
    // top = vmax, bottom = vmin. Matches the orientation the heatmap
    // texture uses (physics convention: high values toward the top).
    let n_steps = grad_h.ceil() as usize;
    for i in 0..n_steps {
        let t = 1.0 - (i as f64) / (n_steps as f64).max(1.0);
        let (r, g, b) = crate::surface::colormap_rgb(t, colorscale);
        let color = Color32::from_rgb(r, g, b);
        let y = grad_top + i as f32;
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(grad_left, y),
                Pos2::new(grad_right, y + 1.0),
            ),
            0.0,
            color,
        );
    }
    // Border around the gradient so it reads as a distinct legend.
    painter.rect_stroke(
        Rect::from_min_max(
            Pos2::new(grad_left, grad_top),
            Pos2::new(grad_right, grad_bottom),
        ),
        0.0,
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color),
        egui::StrokeKind::Inside,
    );

    // Numeric labels. Two decimals is plenty for dB ranges; the live
    // spectrogram uses `vmin_db = -100`, `vmax_db = 0` so the labels
    // read e.g. "0.0" and "-100.0".
    let text_color = ui.visuals().text_color();
    let label_x = grad_right + 4.0;
    let font_id = egui::FontId::proportional(11.0);
    painter.text(
        Pos2::new(label_x, grad_top),
        egui::Align2::LEFT_TOP,
        format!("{:.1}", vmax),
        font_id.clone(),
        text_color,
    );
    painter.text(
        Pos2::new(label_x, grad_bottom),
        egui::Align2::LEFT_BOTTOM,
        format!("{:.1}", vmin),
        font_id,
        text_color,
    );
}
