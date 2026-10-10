//! Interactive 3D surface renderer for the viewer.
//!
//! Accepts a `Surface3dData` (grid of z values + x/y axes) and paints it
//! inside an egui `Ui` with mouse-driven rotate (left-drag), zoom (scroll),
//! pan (right-drag), and reset (the panel's Home button, the `Home` key, or
//! the `R` key — all three restore the default camera). Rendering is software: each grid
//! cell is rasterized with a depth buffer (see [`raster`]) and shown as one
//! texture that fills the panel. Both sides of the sheet are drawn, so a
//! turn around Z or a pass through the poles does not open holes.
//!
//! Per-figure state lives in `PanelState.surface` alongside a `SurfaceCamera`
//! so mouse interaction persists across repaints. The texture handle is
//! reused; it is not allocated again each frame.

mod raster;

use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};

/// Raw 3D surface grid shipped across the viewer IPC.
#[derive(Clone, Debug)]
pub struct Surface3dData {
    pub nrows: usize,
    pub ncols: usize,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>, // row-major, length = nrows * ncols
    pub colorscale: String,
}

impl Surface3dData {
    pub fn z_at(&self, r: usize, c: usize) -> f64 {
        self.z[r * self.ncols + c]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub xmin: f64,
    pub xmax: f64,
    pub ymin: f64,
    pub ymax: f64,
    pub zmin: f64,
    pub zmax: f64,
}

/// Camera state. Yaw spins around Z, pitch raises and lowers the eye
/// (positive looks down, negative looks up, ±90° are the poles), and roll
/// spins the view plane after projection. Roll does not change depth.
#[derive(Clone, Copy, Debug)]
pub struct SurfaceCamera {
    pub yaw: f32,
    pub pitch: f32,
    /// Rotation about the view axis, in radians. Ctrl/Cmd+left-drag.
    pub roll: f32,
    pub zoom: f32,
    /// Screen-space pan offset in logical points (screen-x, screen-y).
    pub pan: Vec2,
    /// Extra z-axis scale for exaggerated relief (Shift+scroll).
    pub z_scale: f32,
}

impl Default for SurfaceCamera {
    fn default() -> Self {
        Self {
            yaw: -45f32.to_radians(),
            pitch: 30f32.to_radians(),
            roll: 0.0,
            zoom: 1.0,
            pan: Vec2::ZERO,
            z_scale: 1.0,
        }
    }
}

/// Colormap lookup (viridis / jet / hot / gray). Keeps the viewer free of
/// a direct rustlab-plot dependency.
pub(crate) fn colormap_rgb(t: f64, name: &str) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    type Pts = &'static [(f64, (u8, u8, u8))];
    let pts: Pts = match name {
        "jet" => &[
            (0.00, (0, 0, 128)),
            (0.25, (0, 128, 255)),
            (0.50, (0, 255, 128)),
            (0.75, (255, 255, 0)),
            (1.00, (128, 0, 0)),
        ],
        "hot" => &[
            (0.00, (0, 0, 0)),
            (0.33, (255, 0, 0)),
            (0.67, (255, 255, 0)),
            (1.00, (255, 255, 255)),
        ],
        "gray" => &[(0.00, (0, 0, 0)), (1.00, (255, 255, 255))],
        _ => &[
            (0.00, (68, 1, 84)),
            (0.25, (59, 82, 139)),
            (0.50, (33, 145, 140)),
            (0.75, (94, 201, 98)),
            (1.00, (253, 231, 37)),
        ],
    };
    for w in pts.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t >= t0 && t <= t1 {
            let s = (t - t0) / (t1 - t0);
            let lerp = |a: u8, b: u8| (a as f64 * (1.0 - s) + b as f64 * s).round() as u8;
            return (lerp(c0.0, c1.0), lerp(c0.1, c1.1), lerp(c0.2, c1.2));
        }
    }
    pts.last().map(|(_, c)| *c).unwrap_or((0, 0, 0))
}

/// Draw the surface inside the given `Ui`. `size` is the area allocated for
/// the panel (minus the title bar). `texture` is reused across frames.
pub fn draw(
    ui: &mut Ui,
    size: Vec2,
    data: &Surface3dData,
    cam: &mut SurfaceCamera,
    texture: &mut Option<egui::TextureHandle>,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());

    handle_input(ui, &response, cam);
    // Double-click is fullscreen only on the 3D panel. A 2D panel's
    // double-click is still Home. A drag that moves the pointer does not
    // count as a double-click, so rotate / pan / zoom are unchanged.
    if response.double_clicked() {
        let on = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!on));
    }

    let painter = ui.painter_at(rect);
    paint_surface(
        ui.ctx(),
        &painter,
        rect,
        data,
        cam,
        texture,
        ui.ctx().pixels_per_point(),
    );

    response
}

fn handle_input(ui: &Ui, response: &Response, cam: &mut SurfaceCamera) {
    // Left-drag: orbit. Ctrl/Cmd+left-drag: roll. Right-drag: pan.
    // Scroll: zoom (Shift = z-scale). Pitch is not clamped, so the orbit
    // passes through the poles.
    if response.dragged_by(egui::PointerButton::Primary) {
        let delta = response.drag_delta();
        let roll_mod = ui.ctx().input(|i| i.modifiers.command || i.modifiers.ctrl);
        if roll_mod {
            cam.roll += delta.x * 0.01;
        } else {
            cam.yaw -= delta.x * 0.01;
            cam.pitch += delta.y * 0.01;
        }
    }
    if response.dragged_by(egui::PointerButton::Secondary) {
        cam.pan += response.drag_delta();
    }
    if response.hovered() {
        let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 {
            let shift = ui.ctx().input(|i| i.modifiers.shift);
            if shift {
                let factor = (1.0 + scroll * 0.005).clamp(0.5, 1.5);
                cam.z_scale = (cam.z_scale * factor).clamp(0.05, 50.0);
            } else {
                let factor = (1.0 + scroll * 0.005).clamp(0.5, 1.5);
                cam.zoom = (cam.zoom * factor).clamp(0.05, 50.0);
            }
        }
    }
    // Press R (or Home, matching the 2D panels' Home button) to reset.
    if response.hovered()
        && ui
            .ctx()
            .input(|i| i.key_pressed(egui::Key::R) || i.key_pressed(egui::Key::Home))
    {
        *cam = SurfaceCamera::default();
    }
}

/// 3-D panel fill. Tick labels and the hint are drawn on this color.
fn surface_fill() -> Color32 {
    Color32::from_rgb(18, 18, 22)
}

/// Axis tick labels on [`surface_fill`].
fn surface_label_color() -> Color32 {
    Color32::from_rgb(200, 200, 210)
}

/// The one-line interaction hint on [`surface_fill`].
fn surface_hint_color() -> Color32 {
    Color32::from_rgb(140, 140, 155)
}

fn paint_surface(
    ctx: &egui::Context,
    painter: &Painter,
    rect: Rect,
    data: &Surface3dData,
    cam: &SurfaceCamera,
    texture: &mut Option<egui::TextureHandle>,
    pixels_per_point: f32,
) {
    // Fill first so a failed raster still has the panel color. The border
    // is stroked after the texture so it stays visible.
    painter.rect_filled(rect, 0.0, surface_fill());

    if data.nrows >= 2 && data.ncols >= 2 {
        let image = raster::rasterize(
            data,
            cam,
            rect.size(),
            raster::RasterOpts {
                shade: raster::Shade::Depth,
                axes: true,
                pixels_per_point,
            },
        );
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [image.width as usize, image.height as usize],
            &image.rgba,
        );
        if let Some(tex) = texture.as_mut() {
            tex.set(color, egui::TextureOptions::LINEAR);
        } else {
            *texture = Some(ctx.load_texture("rustlab-surf", color, egui::TextureOptions::LINEAR));
        }
        if let Some(tex) = texture.as_ref() {
            painter.image(
                tex.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, Color32::from_rgb(60, 60, 70)),
        StrokeKind::Inside,
    );

    let label_color = surface_label_color();
    let font = egui::FontId::proportional(11.0);
    if let Some(bounds) = raster::finite_bounds(data) {
        let label = |x: f64, y: f64, z: f64, s: String| {
            if let Some(p) = raster::project_to_rect(x, y, z, &bounds, cam, rect) {
                painter.text(
                    p + Vec2::new(4.0, 2.0),
                    egui::Align2::LEFT_TOP,
                    s,
                    font.clone(),
                    label_color,
                );
            }
        };
        label(
            bounds.xmin,
            bounds.ymin,
            bounds.zmin,
            format!("x={:.3}", bounds.xmin),
        );
        label(
            bounds.xmax,
            bounds.ymin,
            bounds.zmin,
            format!("x={:.3}", bounds.xmax),
        );
        label(
            bounds.xmin,
            bounds.ymax,
            bounds.zmin,
            format!("y={:.3}", bounds.ymax),
        );
        label(
            bounds.xmin,
            bounds.ymin,
            bounds.zmax,
            format!("z={:.3}", bounds.zmax),
        );
    }

    let hint = "drag=rotate  ctrl-drag=roll  scroll=zoom  shift+scroll=z  right-drag=pan  dbl-click=fullscreen  Home/R=reset";
    painter.text(
        rect.left_top() + Vec2::new(6.0, 4.0),
        egui::Align2::LEFT_TOP,
        hint,
        egui::FontId::proportional(10.0),
        surface_hint_color(),
    );
}

#[cfg(test)]
mod contrast_tests {
    use super::{surface_fill, surface_hint_color, surface_label_color};

    fn lin(c: u8) -> f64 {
        let x = f64::from(c) / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    }

    fn contrast(fg: egui::Color32, bg: egui::Color32) -> f64 {
        let lum =
            |c: egui::Color32| 0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b());
        let (hi, lo) = {
            let a = lum(fg);
            let b = lum(bg);
            if a > b {
                (a, b)
            } else {
                (b, a)
            }
        };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn surface_labels_meet_wcag_aa_on_the_panel() {
        let bg = surface_fill();
        let labels = contrast(surface_label_color(), bg);
        let hint = contrast(surface_hint_color(), bg);
        assert!(labels >= 4.5, "tick labels {labels:.2}:1 on the 3D panel");
        assert!(hint >= 4.5, "hint {hint:.2}:1 on the 3D panel");
        // Dark blue on this near-black fill is the named failure. It must
        // stay failing so this guard cannot pass by accident.
        let blue = contrast(egui::Color32::from_rgb(0, 0, 0xee), bg);
        assert!(
            blue < 4.5,
            "UA blue on the 3D panel contrast {blue:.2} is no longer a failure"
        );
    }
}
