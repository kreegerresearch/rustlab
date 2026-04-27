//! Interactive 3D surface renderer for the viewer.
//!
//! Accepts a `Surface3dData` (grid of z values + x/y axes) and paints it
//! inside an egui `Ui` with mouse-driven rotate (left-drag), zoom (scroll),
//! pan (right-drag), and reset (`R` key). Rendering is software: each grid
//! cell becomes a colored quad depth-sorted via painter's algorithm.
//!
//! Per-figure state lives in `PanelState.surface` alongside a `SurfaceCamera`
//! so mouse interaction persists across repaints without a new allocation
//! every frame.

use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};

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

    pub fn bounds(&self) -> Bounds {
        let xmin = self.x.iter().copied().fold(f64::INFINITY, f64::min);
        let xmax = self.x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let ymin = self.y.iter().copied().fold(f64::INFINITY, f64::min);
        let ymax = self.y.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let zmin = self.z.iter().copied().fold(f64::INFINITY, f64::min);
        let zmax = self.z.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Bounds {
            xmin,
            xmax,
            ymin,
            ymax,
            zmin,
            zmax,
        }
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

/// Camera state: yaw (around Z), pitch (around X), zoom (scale factor).
#[derive(Clone, Copy, Debug)]
pub struct SurfaceCamera {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    /// World-space pan offset applied before projection (screen-x, screen-y).
    pub pan: Vec2,
    /// Extra z-axis scale for exaggerated relief (Shift+scroll).
    pub z_scale: f32,
}

impl Default for SurfaceCamera {
    fn default() -> Self {
        Self {
            yaw: -45f32.to_radians(),
            pitch: 30f32.to_radians(),
            zoom: 1.0,
            pan: Vec2::ZERO,
            z_scale: 1.0,
        }
    }
}

/// Colormap lookup (viridis / jet / hot / gray). Keeps the viewer free of
/// a direct rustlab-plot dependency.
fn colormap_rgb(t: f64, name: &str) -> (u8, u8, u8) {
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
/// the panel (minus the title bar).
pub fn draw(ui: &mut Ui, size: Vec2, data: &Surface3dData, cam: &mut SurfaceCamera) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());

    handle_input(ui, &response, cam);

    let painter = ui.painter_at(rect);
    paint_surface(&painter, rect, data, cam);

    response
}

fn handle_input(ui: &Ui, response: &Response, cam: &mut SurfaceCamera) {
    // Left-drag: rotate. Right-drag: pan. Scroll: zoom (Shift = z-scale).
    if response.dragged_by(egui::PointerButton::Primary) {
        let delta = response.drag_delta();
        cam.yaw -= delta.x * 0.01;
        cam.pitch += delta.y * 0.01;
        cam.pitch = cam.pitch.clamp(-1.55, 1.55);
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
    // Press R to reset.
    if response.hovered() && ui.ctx().input(|i| i.key_pressed(egui::Key::R)) {
        *cam = SurfaceCamera::default();
    }
}

fn paint_surface(painter: &Painter, rect: Rect, data: &Surface3dData, cam: &SurfaceCamera) {
    painter.rect(
        rect,
        0.0,
        Color32::from_rgb(18, 18, 22),
        Stroke::new(1.0, Color32::from_rgb(60, 60, 70)),
        StrokeKind::Inside,
    );

    if data.nrows < 2 || data.ncols < 2 {
        return;
    }
    let b = data.bounds();
    let x_span = (b.xmax - b.xmin).max(1e-12);
    let y_span = (b.ymax - b.ymin).max(1e-12);
    let z_span = (b.zmax - b.zmin).max(1e-12);

    let (sy, cy) = (cam.yaw.sin() as f64, cam.yaw.cos() as f64);
    let (sp, cp) = (cam.pitch.sin() as f64, cam.pitch.cos() as f64);
    let z_scale = cam.z_scale as f64;

    // World → camera → screen. Coordinates are normalized to [-1, 1] per axis
    // so the camera math is independent of data scale.
    let project = |xi: f64, yi: f64, zi: f64| -> (f64, f64, f64) {
        let nx = 2.0 * (xi - b.xmin) / x_span - 1.0;
        let ny = 2.0 * (yi - b.ymin) / y_span - 1.0;
        let nz = (2.0 * (zi - b.zmin) / z_span - 1.0) * z_scale;
        let xr = nx * cy - ny * sy;
        let yr = nx * sy + ny * cy;
        let zr = nz * cp - yr * sp;
        let yr2 = nz * sp + yr * cp;
        (xr, yr2, zr) // (screen-x before scale, depth, screen-y before scale)
    };

    let base_scale = (rect.width().min(rect.height()) as f64) * 0.42 * (cam.zoom as f64);
    let cx = rect.center().x as f64 + cam.pan.x as f64;
    let cy_px = rect.center().y as f64 + cam.pan.y as f64;
    let to_screen = |sx: f64, sz: f64| -> Pos2 {
        Pos2::new(
            (cx + sx * base_scale) as f32,
            (cy_px - sz * base_scale) as f32,
        )
    };

    // Draw axis box (behind).
    let corners = [
        (b.xmin, b.ymin, b.zmin),
        (b.xmax, b.ymin, b.zmin),
        (b.xmax, b.ymax, b.zmin),
        (b.xmin, b.ymax, b.zmin),
        (b.xmin, b.ymin, b.zmax),
        (b.xmax, b.ymin, b.zmax),
        (b.xmax, b.ymax, b.zmax),
        (b.xmin, b.ymax, b.zmax),
    ];
    let pc: Vec<(Pos2, f64)> = corners
        .iter()
        .map(|&(x, y, z)| {
            let (sx, d, sz) = project(x, y, z);
            (to_screen(sx, sz), d)
        })
        .collect();
    let edges = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    let axis_color = Color32::from_rgb(90, 90, 110);
    for (a, e) in edges {
        painter.line_segment([pc[a].0, pc[e].0], Stroke::new(1.0, axis_color));
    }

    // Each grid cell becomes **two triangles** (split along the p00↔p11
    // diagonal). A world-space rectangle can project to a *non-convex*
    // (bowtie) quadrilateral in screen space on surfaces with steep
    // features at certain yaw / pitch angles; emitting individual
    // triangles instead of quads avoids that.
    //
    // **Single-mesh rendering.** Triangles are aggregated into one
    // `egui::Mesh`. Adjacent triangles share grid-corner *vertices by
    // index* — that's the trick. egui's tessellator anti-aliases each
    // shape's perimeter, so emitting each triangle as its own
    // `Shape::convex_polygon` would feather the edge alpha at every
    // shared cell border, leaving a 1-pixel darker band that looks like
    // an unwanted grid line. Within a single Mesh, AA is applied only
    // to the silhouette where the mesh ends — internal edges between
    // shared-vertex triangles render seam-free.
    //
    // **Back-face culling.** For a height-field surface viewed from
    // above, triangles whose projected 2-D winding is CCW are
    // back-faces (the underside / the far side of a ridge) and should
    // be hidden — otherwise the painter's algorithm lets the saddle's
    // valley floor or the ripples' underside bleed through the front
    // of the surface as large translucent planes. World-space
    // triangles are built CCW in (x, y); `to_screen` flips y (egui y
    // points down), so front-facing triangles end up CW in screen
    // space (negative 2-D cross product). Cull where the 2-D cross
    // product is non-positive.
    fn signed_area_2x(a: Pos2, b: Pos2, c: Pos2) -> f32 {
        (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
    }
    let nrows = data.nrows;
    let ncols = data.ncols;

    // Build the projected vertex grid once. Index `r * ncols + c` is
    // the corner at (data.x[c], data.y[r], z(r, c)).
    let mut verts: Vec<(Pos2, Color32)> = Vec::with_capacity(nrows * ncols);
    for r in 0..nrows {
        for c in 0..ncols {
            let z = data.z_at(r, c);
            let p = project(data.x[c], data.y[r], z);
            // Per-vertex colour from z (smoother shading than per-cell
            // average; collapses to per-cell when neighbours have
            // similar z).
            let t = ((z - b.zmin) / z_span).clamp(0.0, 1.0);
            let (rr, gg, bb) = colormap_rgb(t, &data.colorscale);
            verts.push((to_screen(p.0, p.2), Color32::from_rgb(rr, gg, bb)));
        }
    }
    let idx = |r: usize, c: usize| -> u32 { (r * ncols + c) as u32 };

    let mut mesh = egui::epaint::Mesh::default();
    mesh.vertices.reserve(nrows * ncols);
    for &(pos, color) in &verts {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: egui::epaint::WHITE_UV,
            color,
        });
    }
    for r in 0..(nrows - 1) {
        for c in 0..(ncols - 1) {
            // Triangle A: (p00, p10, p11) — CCW in world x/y.
            let i00 = idx(r, c);
            let i10 = idx(r, c + 1);
            let i11 = idx(r + 1, c + 1);
            let i01 = idx(r + 1, c);
            let p00 = verts[i00 as usize].0;
            let p10 = verts[i10 as usize].0;
            let p11 = verts[i11 as usize].0;
            let p01 = verts[i01 as usize].0;

            if signed_area_2x(p00, p10, p11) > 0.0 {
                mesh.indices.extend_from_slice(&[i00, i10, i11]);
            }
            if signed_area_2x(p00, p11, p01) > 0.0 {
                mesh.indices.extend_from_slice(&[i00, i11, i01]);
            }
        }
    }
    painter.add(Shape::mesh(mesh));

    // Axis tick labels: min/max on each axis.
    let label_color = Color32::from_rgb(200, 200, 210);
    let font = egui::FontId::proportional(11.0);
    let label = |p: Pos2, s: String| {
        painter.text(
            p + Vec2::new(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            s,
            font.clone(),
            label_color,
        );
    };
    label(pc[0].0, format!("x={:.3}", b.xmin));
    label(pc[1].0, format!("x={:.3}", b.xmax));
    label(pc[3].0, format!("y={:.3}", b.ymax));
    label(pc[4].0, format!("z={:.3}", b.zmax));

    // On-screen hint (top-left, tiny).
    let hint = "drag=rotate  scroll=zoom  shift+scroll=z  right-drag=pan  R=reset";
    painter.text(
        rect.left_top() + Vec2::new(6.0, 4.0),
        egui::Align2::LEFT_TOP,
        hint,
        egui::FontId::proportional(10.0),
        Color32::from_rgb(140, 140, 155),
    );
}
