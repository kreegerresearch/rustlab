//! Figure and panel state for the viewer application.

use egui_plot::{Plot, PlotBounds, PlotImage};
use rustlab_proto::WireSeries;
use std::sync::Arc;

use crate::render;
use crate::surface::{Surface3dData, SurfaceCamera};
use crate::view::{bounds_action, zoom_factor_from_scroll, AxisLimits, BoundsAction, PanelView};

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
    pub xlim: AxisLimits,
    pub ylim: AxisLimits,
    pub axis_equal: bool,
    pub heatmap: Option<HeatmapImage>,
    /// 3D surface data + camera. When present, the panel renders a rotatable
    /// surface instead of the 2D egui_plot chart.
    pub surface: Option<(Surface3dData, SurfaceCamera)>,
    /// Reused GPU texture for the software-rasterized surface. Taken out
    /// for the draw call so the rasterizer can update it without aliasing
    /// the surface borrow.
    pub surface_tex: Option<egui::TextureHandle>,
    /// Zoom/pan bookkeeping: whether the script's limits still need to be
    /// pushed into the plot, and whether a Home reset is pending. See
    /// [`crate::view`].
    pub view: PanelView,
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
            surface_tex: None,
            view: PanelView::default(),
        }
    }

    /// Record new axis limits from the script. Only *changed* limits
    /// re-arm the apply latch: live plots re-send the same
    /// `plot_limits` on every redraw, and re-applying them each time
    /// would fight the user's zoom exactly like the old every-frame
    /// `set_plot_bounds` did.
    pub fn set_limits(&mut self, xlim: AxisLimits, ylim: AxisLimits) {
        if self.xlim != xlim || self.ylim != ylim {
            self.xlim = xlim;
            self.ylim = ylim;
            self.view.pending_limits = true;
        }
    }
}

/// Height of the per-panel header strip (title + Home button).
const HEADER_H: f32 = 20.0;

/// Footprint of a panel's Home button.
const HOME_BUTTON_SIZE: egui::Vec2 = egui::Vec2::new(46.0, 18.0);

/// Stable widget id for a panel's egui_plot chart. Explicit (rather than
/// egui's auto-generated id) so headless tests can read the plot's bounds
/// memory back and assert on what zoom and Home actually did.
pub(crate) fn panel_plot_id(fig_id: u32, row: usize, col: usize) -> egui::Id {
    egui::Id::new(("rustlab_panel_plot", fig_id, row, col))
}

/// Stable widget id for a panel's Home button — same reasoning as
/// [`panel_plot_id`]; tests click it by id.
pub(crate) fn home_button_id(fig_id: u32, row: usize, col: usize) -> egui::Id {
    egui::Id::new(("rustlab_panel_home", fig_id, row, col))
}

/// Stable widget id for a panel's Expand / Grid button.
pub(crate) fn expand_button_id(fig_id: u32, row: usize, col: usize) -> egui::Id {
    egui::Id::new(("rustlab_panel_expand", fig_id, row, col))
}

/// Stable widget id for a multi-figure window's Fill / Restore button.
pub(crate) fn fill_window_button_id(fig_id: u32) -> egui::Id {
    egui::Id::new(("rustlab_fill_window", fig_id))
}

/// Footprint of the Expand / Grid button.
const EXPAND_BUTTON_SIZE: egui::Vec2 = egui::Vec2::new(58.0, 18.0);

/// Next expanded-panel index.
///
/// `toggle` is the panel whose Expand button (or title) was activated.
/// Activating the panel that is already expanded returns to the grid.
/// `esc` clears the expansion. An index past `n` is ignored.
pub(crate) fn step_expanded(
    current: Option<usize>,
    toggle: Option<usize>,
    esc: bool,
    n: usize,
) -> Option<usize> {
    if esc || n <= 1 {
        return None;
    }
    let current = current.filter(|&i| i < n);
    match toggle {
        Some(idx) if idx < n => {
            if current == Some(idx) {
                None
            } else {
                Some(idx)
            }
        }
        _ => current,
    }
}

/// Plot area inside one cell of a `rows`×`cols` grid that has `avail`
/// points to spend, after the header strip and the 4 px cell padding.
pub(crate) fn panel_content_size(
    avail: egui::Vec2,
    rows: usize,
    cols: usize,
    header_h: f32,
) -> egui::Vec2 {
    let rows = rows.max(1) as f32;
    let cols = cols.max(1) as f32;
    let cell_w = (avail.x / cols).max(1.0);
    let cell_h = (avail.y / rows).max(1.0);
    egui::vec2((cell_w - 8.0).max(1.0), (cell_h - 8.0 - header_h).max(1.0))
}

/// Draw a panel's Home button and report whether it was clicked.
///
/// Painted by hand rather than through `ui.add(Button::new(..))` so the
/// widget carries a caller-chosen `Id`.
fn home_button(ui: &mut egui::Ui, id: egui::Id) -> bool {
    let (rect, _) = ui.allocate_exact_size(HOME_BUTTON_SIZE, egui::Sense::hover());
    let response = ui.interact(rect, id, egui::Sense::click());
    let visuals = ui.style().interact(&response);
    let painter = ui.painter();
    painter.rect(
        rect,
        3.0,
        visuals.bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Home",
        egui::FontId::proportional(11.0),
        visuals.fg_stroke.color,
    );
    response
        .on_hover_text(
            "Reset this subplot's view — back to the script's xlim/ylim \
             if it set any, otherwise fit the data (shortcut: Home)",
        )
        .clicked()
}

/// Header button with a caller-chosen id and label (Expand / Grid, Fill).
fn header_text_button(
    ui: &mut egui::Ui,
    id: egui::Id,
    label: &str,
    hover: &str,
    size: egui::Vec2,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let response = ui.interact(rect, id, egui::Sense::click());
    let visuals = ui.style().interact(&response);
    let painter = ui.painter();
    painter.rect(
        rect,
        3.0,
        visuals.bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(11.0),
        visuals.fg_stroke.color,
    );
    response.on_hover_text(hover).clicked()
}

/// Draw the Expand / Grid button. `label` is "Expand" or "Grid".
fn expand_button(ui: &mut egui::Ui, id: egui::Id, label: &str) -> bool {
    let hover = if label == "Grid" {
        "Return to the subplot grid (shortcut: Esc)"
    } else {
        "Show only this subplot. Esc returns to the grid. Double-click the title does the same."
    };
    header_text_button(ui, id, label, hover, EXPAND_BUTTON_SIZE)
}

/// Fill / Restore control for a floating figure window.
pub(crate) fn fill_window_button(ui: &mut egui::Ui, id: egui::Id, label: &str) -> bool {
    let hover = if label == "Restore" {
        "Return this figure to a floating window (shortcut: Esc)"
    } else {
        "Expand this figure to fill the viewer window. Esc restores it."
    };
    header_text_button(ui, id, label, hover, egui::Vec2::new(88.0, 18.0))
}

/// A figure window containing a grid of subplot panels.
pub struct FigureWindow {
    pub rows: usize,
    pub cols: usize,
    pub title: String,
    pub panels: Vec<PanelState>,
    /// Set to true when new data arrives; cleared after first redraw.
    pub dirty: bool,
    /// When set, and this figure has more than one panel, only that panel
    /// is drawn, filling the figure. Esc clears it.
    pub expanded_panel: Option<usize>,
    /// Multi-figure mode: this floating window is pinned to the viewer
    /// window. Esc clears it.
    pub filled: bool,
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
            expanded_panel: None,
            filled: false,
        }
    }

    /// Render this figure's subplot grid into the given `Ui`.
    /// `fig_id` is used to generate unique egui widget IDs across figures.
    pub fn render(&mut self, ui: &mut egui::Ui, fig_id: u32) {
        let multi = self.rows * self.cols > 1;
        let expanded = self
            .expanded_panel
            .filter(|&i| i < self.panels.len() && multi);
        let grid_rows = if expanded.is_some() {
            1
        } else {
            self.rows.max(1)
        };
        let grid_cols = if expanded.is_some() {
            1
        } else {
            self.cols.max(1)
        };
        let avail = ui.available_size();
        let cell_w = (avail.x / grid_cols as f32).max(1.0);
        let cell_h = (avail.y / grid_rows as f32).max(1.0);
        let mut toggle_expand: Option<usize> = None;
        let cols_n = self.cols.max(1);

        for row in 0..grid_rows {
            ui.horizontal(|ui| {
                for col in 0..grid_cols {
                    let idx = expanded.unwrap_or(row * cols_n + col);
                    if idx >= self.panels.len() {
                        continue;
                    }
                    let src_row = idx / cols_n;
                    let src_col = idx % cols_n;
                    let panel = &mut self.panels[idx];

                    // Every panel gets a header strip, titled or not, so
                    // the Home button sits in the same place on all of
                    // them (one per subplot, not one per figure).
                    let header_h = HEADER_H;

                    ui.vertical(|ui| {
                        let mut home_clicked = false;
                        let header_w = (cell_w - 8.0).max(HOME_BUTTON_SIZE.x);
                        ui.allocate_ui_with_layout(
                            egui::Vec2::new(header_w, header_h),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                home_clicked =
                                    home_button(ui, home_button_id(fig_id, src_row, src_col));
                                if multi {
                                    let label = if expanded == Some(idx) {
                                        "Grid"
                                    } else {
                                        "Expand"
                                    };
                                    if expand_button(
                                        ui,
                                        expand_button_id(fig_id, src_row, src_col),
                                        label,
                                    ) {
                                        toggle_expand = Some(idx);
                                    }
                                }
                                if !panel.title.is_empty() {
                                    ui.vertical_centered(|ui| {
                                        let sense = if multi {
                                            egui::Sense::click()
                                        } else {
                                            egui::Sense::hover()
                                        };
                                        let resp = ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&panel.title)
                                                    .strong()
                                                    .size(14.0),
                                            )
                                            .sense(sense),
                                        );
                                        if multi && resp.double_clicked() {
                                            toggle_expand = Some(idx);
                                        }
                                    });
                                }
                            },
                        );
                        if home_clicked {
                            panel.view.home_requested = true;
                        }

                        // 3D surface panel: software raster instead of egui_plot.
                        // Home matches the `R` key (default camera). Take the
                        // latch and the texture before borrowing the surface;
                        // a 2-D panel puts both back.
                        let surface_home = std::mem::take(&mut panel.view.home_requested);
                        let mut tex = panel.surface_tex.take();
                        if let Some((data, cam)) = panel.surface.as_mut() {
                            if surface_home {
                                *cam = SurfaceCamera::default();
                            }
                            // One cell of the (possibly expanded) grid. The
                            // helper is the same size the 2-D path uses, so a
                            // window resize and a subplot expand share one
                            // formula.
                            let size =
                                panel_content_size(egui::vec2(cell_w, cell_h), 1, 1, header_h);
                            crate::surface::draw(ui, size, data, cam, &mut tex);
                            panel.surface_tex = tex;
                            return;
                        }
                        if surface_home {
                            panel.view.home_requested = true;
                        }
                        panel.surface_tex = tex;

                        // Reserve a gutter on the right for the colorbar
                        // legend when the panel has a heatmap with a known
                        // colour range (sender pinned `value_min` /
                        // `value_max`). The colorbar is drawn in a sibling
                        // horizontal layout after `plot.show(...)`.
                        let (cbar_vmin, cbar_vmax, cbar_colorscale) = panel
                            .heatmap
                            .as_ref()
                            .and_then(|hm| match (hm.value_min, hm.value_max) {
                                (Some(a), Some(b)) if b > a => Some((a, b, hm.colorscale.clone())),
                                _ => None,
                            })
                            .map(|(a, b, c)| (Some(a), Some(b), c))
                            .unwrap_or((None, None, String::new()));
                        let cbar_w = if cbar_vmin.is_some() { 56.0_f32 } else { 0.0 };
                        let plot_width = (cell_w - 8.0 - cbar_w).max(40.0);
                        let plot_height = (cell_h - 8.0 - header_h).max(1.0);

                        let plot_id = format!("fig_{}_panel_{}_{}", fig_id, src_row, src_col);
                        let mut plot = Plot::new(&plot_id)
                            .id(panel_plot_id(fig_id, src_row, src_col))
                            .width(plot_width)
                            .height(plot_height)
                            .show_axes([true, true])
                            .show_grid([true, true])
                            .allow_zoom(true)
                            .allow_drag(true)
                            // Plain scroll zooms (handled below), so
                            // egui_plot's own scroll-to-pan is off —
                            // leaving it on would pan and zoom at once.
                            .allow_scroll(false)
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

                        // Whether the script's limits get pushed this
                        // frame. Once applied they are *not* re-applied
                        // every frame — that is what used to snap a
                        // scrolled/dragged view straight back.
                        let action = bounds_action(panel.xlim, panel.ylim, panel.view);

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
                                hm.texture = Some(ui.ctx().load_texture("heatmap", image, opts));
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
                                let hovered = plot_ui.response().hovered();
                                // The Home key is the keyboard twin of the
                                // panel's Home button (and of `R` on 3D
                                // surfaces). egui_plot's built-in double-click
                                // reset is routed through the same path so it
                                // restores the script's limits too, instead of
                                // always auto-fitting.
                                let home_gesture = (hovered
                                    && plot_ui.ctx().input(|i| i.key_pressed(egui::Key::Home)))
                                    || plot_ui.response().double_clicked();
                                let action = if home_gesture {
                                    bounds_action(
                                        panel.xlim,
                                        panel.ylim,
                                        PanelView {
                                            home_requested: true,
                                            ..panel.view
                                        },
                                    )
                                } else {
                                    action
                                };

                                // Apply explicit bounds (x and y independently).
                                // Axes the script left open are handed back to
                                // auto-fit rather than frozen at whatever the
                                // previous frame happened to show.
                                let cur = plot_ui.plot_bounds();
                                match action {
                                    BoundsAction::Keep => {}
                                    BoundsAction::AutoFit => plot_ui.set_auto_bounds(true),
                                    BoundsAction::Apply { auto_x, auto_y } => {
                                        let (x0, x1) = match panel.xlim {
                                            (Some(a), Some(b)) => (a, b),
                                            _ => (*cur.range_x().start(), *cur.range_x().end()),
                                        };
                                        let (y0, y1) = match panel.ylim {
                                            (Some(a), Some(b)) => (a, b),
                                            _ => (*cur.range_y().start(), *cur.range_y().end()),
                                        };
                                        plot_ui.set_plot_bounds(PlotBounds::from_min_max(
                                            [x0, y0],
                                            [x1, y1],
                                        ));
                                        if auto_x || auto_y {
                                            plot_ui.set_auto_bounds([auto_x, auto_y]);
                                        }
                                    }
                                }

                                // Plain scroll wheel = zoom about the pointer,
                                // both axes together. Drag still pans, and
                                // ctrl+scroll / pinch keep working through
                                // egui_plot's own zoom path.
                                if hovered {
                                    let scroll = plot_ui.ctx().input(|i| i.smooth_scroll_delta.y);
                                    let factor = zoom_factor_from_scroll(scroll);
                                    if factor != 1.0 {
                                        plot_ui
                                            .zoom_bounds_around_hovered(egui::Vec2::splat(factor));
                                    }
                                }

                                // Render heatmap as a texture image, placed
                                // in data coords so the egui_plot axis ticks
                                // read in the same units (Hz, sec, m, …).
                                if let Some((tex_id, xl, xh, yl, yh)) = hm_info {
                                    let center =
                                        egui_plot::PlotPoint::new((xl + xh) * 0.5, (yl + yh) * 0.5);
                                    let size = egui::Vec2::new((xh - xl) as f32, (yh - yl) as f32);
                                    plot_ui.image(PlotImage::new(tex_id, center, size));
                                }

                                for series in &panel.series {
                                    render::render_series(plot_ui, series);
                                }
                            });

                            // Latches cleared once the frame has drawn: the
                            // limits now live in egui_plot's bounds memory, so
                            // from here on the panel is the user's to zoom.
                            panel.view.pending_limits = false;
                            panel.view.home_requested = false;

                            // Colorbar legend: thin gradient strip to the
                            // right of the plot, painted only when the sender
                            // pinned an explicit colour range. The gradient
                            // is computed via `crate::surface::colormap_rgb`
                            // so it matches the cell colours exactly.
                            if let (Some(lo), Some(hi)) = (cbar_vmin, cbar_vmax) {
                                draw_colorbar(ui, cbar_w, plot_height, lo, hi, &cbar_colorscale);
                            }
                        }); // close ui.horizontal
                    }); // close ui.vertical
                }
            });
        }

        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc {
            self.expanded_panel = None;
            self.filled = false;
        } else if let Some(idx) = toggle_expand {
            self.expanded_panel =
                step_expanded(self.expanded_panel, Some(idx), false, self.panels.len());
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

    let (rect, _resp) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
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
            Rect::from_min_max(Pos2::new(grad_left, y), Pos2::new(grad_right, y + 1.0)),
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

#[cfg(test)]
mod layout_tests {
    use super::{panel_content_size, step_expanded};

    #[test]
    fn expand_toggles_one_panel_and_esc_clears_it() {
        assert_eq!(step_expanded(None, Some(1), false, 4), Some(1));
        assert_eq!(step_expanded(Some(1), Some(1), false, 4), None);
        assert_eq!(step_expanded(Some(1), Some(0), false, 4), Some(0));
        assert_eq!(step_expanded(Some(1), None, true, 4), None);
        assert_eq!(step_expanded(Some(1), Some(0), true, 4), None);
        assert_eq!(step_expanded(Some(3), None, false, 2), None);
        assert_eq!(step_expanded(Some(0), Some(0), false, 1), None);
    }

    #[test]
    fn panel_content_grows_with_the_window_and_stays_positive() {
        let small = panel_content_size(egui::vec2(400.0, 300.0), 2, 2, 20.0);
        let big = panel_content_size(egui::vec2(1200.0, 800.0), 2, 2, 20.0);
        assert!(big.x > small.x && big.y > small.y);
        let cramped = panel_content_size(egui::vec2(40.0, 30.0), 2, 2, 20.0);
        assert!(cramped.x >= 1.0 && cramped.y >= 1.0);
        let solo = panel_content_size(egui::vec2(800.0, 600.0), 1, 1, 20.0);
        assert!(solo.x > 700.0 && solo.y > 500.0);
    }
}
