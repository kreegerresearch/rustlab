//! Software rasterizer for viewer surfaces.
//!
//! The panel used to submit a 2-D triangle mesh and drop every triangle
//! whose screen winding was not strictly positive. A `surf` plot is one
//! sheet, not a closed solid, and egui has no depth buffer, so a face
//! that turned away from the camera became a hole onto the background.
//! Looking up from below (negative elevation, including straight through
//! the bottom pole) discarded the entire sheet. Azimuth just changes
//! which faces point away, so a turn around Z swept those holes across
//! the surface.
//!
//! This path paints both sides of every finite triangle into an RGBA
//! image with a depth buffer. Nearer depth wins regardless of grid order
//! or winding, so a full azimuth sweep, an orbit through ±90° elevation,
//! and a roll about the view axis all keep the sheet opaque. NaN samples
//! stay holes: the cell is skipped and the depth buffer is never poisoned.

use super::{surface_fill, Bounds, Surface3dData, SurfaceCamera};

/// How triangles are shaded. [`Shade::Depth`] is the viewer.
/// [`Shade::CullPainter`] is the old winding cull with painter's order and
/// no depth test, kept so tests can show the frames it wiped out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shade {
    Depth,
    CullPainter,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RasterOpts {
    pub shade: Shade,
    pub axes: bool,
    pub pixels_per_point: f32,
}

impl Default for RasterOpts {
    fn default() -> Self {
        Self {
            shade: Shade::Depth,
            axes: true,
            pixels_per_point: 1.0,
        }
    }
}

pub(crate) struct RasterImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Physical raster size for a panel of `logical` points.
///
/// HiDPI multiplies by `pixels_per_point`. The long edge is capped so a
/// maximized 4K window does not allocate a full-screen float depth buffer
/// for every drag frame; the texture is scaled into the panel.
pub(crate) fn raster_pixel_size(logical: egui::Vec2, pixels_per_point: f32) -> (u32, u32) {
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    let mut w = (logical.x.max(1.0) * ppp).round();
    let mut h = (logical.y.max(1.0) * ppp).round();
    const MAX_EDGE: f32 = 1600.0;
    let edge = w.max(h);
    if edge > MAX_EDGE {
        let s = MAX_EDGE / edge;
        w = (w * s).round();
        h = (h * s).round();
    }
    (w.max(1.0) as u32, h.max(1.0) as u32)
}

/// Fraction of pixels that are not the panel fill. Axis lines count when
/// they were drawn; coverage tests pass `axes: false`.
pub(crate) fn coverage_fraction(image: &RasterImage) -> f64 {
    let bg = fill_rgb();
    let n = image.width as usize * image.height as usize;
    if n == 0 {
        return 0.0;
    }
    let mut hit = 0usize;
    for px in image.rgba.chunks_exact(4) {
        if px[0] != bg[0] || px[1] != bg[1] || px[2] != bg[2] {
            hit += 1;
        }
    }
    hit as f64 / n as f64
}

pub(crate) fn rasterize(
    data: &Surface3dData,
    cam: &SurfaceCamera,
    logical_size: egui::Vec2,
    opts: RasterOpts,
) -> RasterImage {
    let (width, height) = raster_pixel_size(logical_size, opts.pixels_per_point);
    let bg = fill_rgba();
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for px in rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&bg);
    }
    let mut image = RasterImage {
        width,
        height,
        rgba,
    };
    let Some(bounds) = finite_bounds(data) else {
        return image;
    };

    let ppp = if opts.pixels_per_point.is_finite() && opts.pixels_per_point > 0.0 {
        opts.pixels_per_point
    } else {
        1.0
    };
    let size = egui::vec2(width as f32, height as f32);
    let pan = egui::vec2(cam.pan.x * ppp, cam.pan.y * ppp);
    let map = ScreenMap::new(size, cam.zoom, pan);
    let mut zbuf = vec![f32::NEG_INFINITY; width as usize * height as usize];
    // Box edges go in first, biased a hair away from the camera, so a
    // surface sample on the same depth covers the wire and a nearer edge
    // stays visible.
    if opts.axes {
        paint_axis_box(&mut image, &mut zbuf, &bounds, cam, &map);
    }

    let nrows = data.nrows;
    let ncols = data.ncols;
    let mut screen: Vec<Option<ScreenVert>> = Vec::with_capacity(nrows * ncols);
    for r in 0..nrows {
        for c in 0..ncols {
            let z = data.z_at(r, c);
            let x = data.x.get(c).copied().unwrap_or(f64::NAN);
            let y = data.y.get(r).copied().unwrap_or(f64::NAN);
            screen.push(project_screen(x, y, z, &bounds, cam, &map).map(|p| {
                let t =
                    ((z - bounds.zmin) / (bounds.zmax - bounds.zmin).max(1e-12)).clamp(0.0, 1.0);
                let (rr, gg, bb) = super::colormap_rgb(t, &data.colorscale);
                ScreenVert {
                    x: p.x,
                    y: p.y,
                    depth: p.depth,
                    rgb: [rr, gg, bb],
                }
            }));
        }
    }

    let ztest = opts.shade == Shade::Depth;

    let idx = |r: usize, c: usize| r * ncols + c;
    for r in 0..(nrows - 1) {
        for c in 0..(ncols - 1) {
            let i00 = idx(r, c);
            let i10 = idx(r, c + 1);
            let i11 = idx(r + 1, c + 1);
            let i01 = idx(r + 1, c);
            for tri in [[i00, i10, i11], [i00, i11, i01]] {
                let Some(v0) = screen[tri[0]] else { continue };
                let Some(v1) = screen[tri[1]] else { continue };
                let Some(v2) = screen[tri[2]] else { continue };
                if opts.shade == Shade::CullPainter {
                    let area = (v1.x - v0.x) * (v2.y - v0.y) - (v1.y - v0.y) * (v2.x - v0.x);
                    if area <= 0.0 {
                        continue;
                    }
                }
                fill_triangle(
                    &mut image,
                    if ztest { Some(&mut zbuf) } else { None },
                    [v0, v1, v2],
                );
            }
        }
    }
    image
}

#[derive(Clone, Copy)]
struct ScreenVert {
    x: f32,
    y: f32,
    depth: f32,
    rgb: [u8; 3],
}

struct ScreenPoint {
    x: f32,
    y: f32,
    depth: f32,
}

struct ScreenMap {
    cx: f32,
    cy: f32,
    scale: f32,
}

impl ScreenMap {
    fn new(size: egui::Vec2, zoom: f32, pan: egui::Vec2) -> Self {
        let zoom = if zoom.is_finite() && zoom > 0.0 {
            zoom
        } else {
            1.0
        };
        let scale = size.x.min(size.y) * 0.42 * zoom;
        Self {
            cx: size.x * 0.5 + pan.x,
            cy: size.y * 0.5 + pan.y,
            scale,
        }
    }

    fn px(&self, sx: f64, sy_up: f64) -> (f32, f32) {
        (
            self.cx + sx as f32 * self.scale,
            self.cy - sy_up as f32 * self.scale,
        )
    }
}

/// Project one world sample to normalized camera-plane coordinates.
///
/// `sx` / `sy_up` are the view-plane axes (y up). `depth` increases toward
/// the camera. Roll spins the view plane and does not change depth, so it
/// cannot invert occlusion. The map is sin/cos only — no atan2 — so a full
/// 360° of yaw and a pass through pitch = ±90° stay finite.
pub(crate) fn project_world(
    x: f64,
    y: f64,
    z: f64,
    bounds: &Bounds,
    cam: &SurfaceCamera,
) -> Option<(f64, f64, f64)> {
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return None;
    }
    let x_span = (bounds.xmax - bounds.xmin).max(1e-12);
    let y_span = (bounds.ymax - bounds.ymin).max(1e-12);
    let z_span = (bounds.zmax - bounds.zmin).max(1e-12);
    let z_scale = if cam.z_scale.is_finite() {
        cam.z_scale as f64
    } else {
        1.0
    };
    let (sy, cy) = (cam.yaw as f64).sin_cos();
    let (sp, cp) = (cam.pitch as f64).sin_cos();
    let (sr, cr) = (cam.roll as f64).sin_cos();
    let nx = 2.0 * (x - bounds.xmin) / x_span - 1.0;
    let ny = 2.0 * (y - bounds.ymin) / y_span - 1.0;
    let nz = (2.0 * (z - bounds.zmin) / z_span - 1.0) * z_scale;
    // Yaw about Z, then pitch about the camera X axis.
    let xr = nx * cy - ny * sy;
    let yr = nx * sy + ny * cy;
    let zr = nz * cp - yr * sp;
    let depth = nz * sp + yr * cp;
    // Roll about the view axis (the depth axis).
    let sx = xr * cr - zr * sr;
    let sy_up = xr * sr + zr * cr;
    if sx.is_finite() && sy_up.is_finite() && depth.is_finite() {
        Some((sx, sy_up, depth))
    } else {
        None
    }
}

/// Logical-pixel position of a world sample inside `rect` (labels, hit tests).
pub(crate) fn project_to_rect(
    x: f64,
    y: f64,
    z: f64,
    bounds: &Bounds,
    cam: &SurfaceCamera,
    rect: egui::Rect,
) -> Option<egui::Pos2> {
    let (sx, sy_up, _) = project_world(x, y, z, bounds, cam)?;
    let map = ScreenMap::new(rect.size(), cam.zoom, cam.pan);
    let (px, py) = map.px(sx, sy_up);
    let px = px + rect.min.x;
    let py = py + rect.min.y;
    if px.is_finite() && py.is_finite() {
        Some(egui::pos2(px, py))
    } else {
        None
    }
}

pub(crate) fn finite_bounds(data: &Surface3dData) -> Option<Bounds> {
    if data.nrows < 2 || data.ncols < 2 {
        return None;
    }
    if data.x.len() < data.ncols || data.y.len() < data.nrows {
        return None;
    }
    if data.z.len() < data.nrows * data.ncols {
        return None;
    }
    let (xmin, xmax) = finite_minmax(&data.x[..data.ncols])?;
    let (ymin, ymax) = finite_minmax(&data.y[..data.nrows])?;
    let mut zmin = f64::INFINITY;
    let mut zmax = f64::NEG_INFINITY;
    let mut any_z = false;
    for &z in &data.z[..data.nrows * data.ncols] {
        if z.is_finite() {
            any_z = true;
            zmin = zmin.min(z);
            zmax = zmax.max(z);
        }
    }
    if !any_z {
        return None;
    }
    Some(Bounds {
        xmin,
        xmax,
        ymin,
        ymax,
        zmin,
        zmax,
    })
}

fn finite_minmax(v: &[f64]) -> Option<(f64, f64)> {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut any = false;
    for &t in v {
        if t.is_finite() {
            any = true;
            min = min.min(t);
            max = max.max(t);
        }
    }
    if any {
        Some((min, max))
    } else {
        None
    }
}

fn project_screen(
    x: f64,
    y: f64,
    z: f64,
    bounds: &Bounds,
    cam: &SurfaceCamera,
    map: &ScreenMap,
) -> Option<ScreenPoint> {
    let (sx, sy_up, depth) = project_world(x, y, z, bounds, cam)?;
    let (px, py) = map.px(sx, sy_up);
    if px.is_finite() && py.is_finite() {
        Some(ScreenPoint {
            x: px,
            y: py,
            depth: depth as f32,
        })
    } else {
        None
    }
}

fn paint_axis_box(
    image: &mut RasterImage,
    zbuf: &mut [f32],
    bounds: &Bounds,
    cam: &SurfaceCamera,
    map: &ScreenMap,
) {
    let b = bounds;
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
    let mut pc = [None; 8];
    for (i, &(x, y, z)) in corners.iter().enumerate() {
        if let Some(p) = project_screen(x, y, z, bounds, cam, map) {
            // Push the box slightly away from the camera so a surface sample
            // that lands on the same depth paints over the wire.
            pc[i] = Some((p.x, p.y, p.depth - 1.0e-3));
        }
    }
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
    let rgb = [90, 90, 110];
    for (a, e) in edges {
        if let (Some(pa), Some(pe)) = (pc[a], pc[e]) {
            draw_line(image, zbuf, pa, pe, rgb);
        }
    }
}

fn draw_line(
    image: &mut RasterImage,
    zbuf: &mut [f32],
    a: (f32, f32, f32),
    b: (f32, f32, f32),
    rgb: [u8; 3],
) {
    if !a.0.is_finite() || !a.1.is_finite() || !b.0.is_finite() || !b.1.is_finite() {
        return;
    }
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil() as i32;
    let steps = steps.max(1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = a.0 + (b.0 - a.0) * t;
        let y = a.1 + (b.1 - a.1) * t;
        let depth = a.2 + (b.2 - a.2) * t;
        put_px(image, Some(zbuf), x, y, depth, rgb);
    }
}

fn fill_triangle(image: &mut RasterImage, mut zbuf: Option<&mut [f32]>, v: [ScreenVert; 3]) {
    if v.iter()
        .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.depth.is_finite())
    {
        return;
    }
    let w = image.width as i32;
    let h = image.height as i32;
    if w <= 0 || h <= 0 {
        return;
    }
    let denom = (v[1].y - v[2].y) * (v[0].x - v[2].x) + (v[2].x - v[1].x) * (v[0].y - v[2].y);
    // Sub-pixel slivers are real triangles; only drop exact degenerates.
    if !denom.is_finite() || denom.abs() < 1e-5 {
        return;
    }
    let min_x = v
        .iter()
        .map(|p| p.x.floor() as i32)
        .fold(i32::MAX, i32::min)
        .max(0);
    let max_x = v
        .iter()
        .map(|p| p.x.ceil() as i32)
        .fold(i32::MIN, i32::max)
        .min(w - 1);
    let min_y = v
        .iter()
        .map(|p| p.y.floor() as i32)
        .fold(i32::MAX, i32::min)
        .max(0);
    let max_y = v
        .iter()
        .map(|p| p.y.ceil() as i32)
        .fold(i32::MIN, i32::max)
        .min(h - 1);
    if min_x > max_x || min_y > max_y {
        return;
    }
    // Negative epsilon closes the cracks between triangles that share an
    // edge. Overlap is harmless: both sides carry almost the same depth.
    const EDGE: f32 = -1.0e-3;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 =
                ((v[1].y - v[2].y) * (px - v[2].x) + (v[2].x - v[1].x) * (py - v[2].y)) / denom;
            let w1 =
                ((v[2].y - v[0].y) * (px - v[2].x) + (v[0].x - v[2].x) * (py - v[2].y)) / denom;
            let w2 = 1.0 - w0 - w1;
            if w0 < EDGE || w1 < EDGE || w2 < EDGE {
                continue;
            }
            let depth = w0 * v[0].depth + w1 * v[1].depth + w2 * v[2].depth;
            if !depth.is_finite() {
                continue;
            }
            let rgb = [
                lerp_u8(v[0].rgb[0], v[1].rgb[0], v[2].rgb[0], w0, w1, w2),
                lerp_u8(v[0].rgb[1], v[1].rgb[1], v[2].rgb[1], w0, w1, w2),
                lerp_u8(v[0].rgb[2], v[1].rgb[2], v[2].rgb[2], w0, w1, w2),
            ];
            put_px(image, zbuf.as_deref_mut(), px, py, depth, rgb);
        }
    }
}

fn lerp_u8(a: u8, b: u8, c: u8, w0: f32, w1: f32, w2: f32) -> u8 {
    (w0 * a as f32 + w1 * b as f32 + w2 * c as f32)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn put_px(
    image: &mut RasterImage,
    zbuf: Option<&mut [f32]>,
    x: f32,
    y: f32,
    depth: f32,
    rgb: [u8; 3],
) {
    if !x.is_finite() || !y.is_finite() || !depth.is_finite() {
        return;
    }
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    if xi < 0 || yi < 0 || xi >= image.width as i32 || yi >= image.height as i32 {
        return;
    }
    let i = yi as usize * image.width as usize + xi as usize;
    if let Some(zbuf) = zbuf {
        if depth <= zbuf[i] {
            return;
        }
        zbuf[i] = depth;
    }
    let o = i * 4;
    image.rgba[o] = rgb[0];
    image.rgba[o + 1] = rgb[1];
    image.rgba[o + 2] = rgb[2];
    image.rgba[o + 3] = 255;
}

fn fill_rgb() -> [u8; 3] {
    let c = surface_fill();
    [c.r(), c.g(), c.b()]
}

fn fill_rgba() -> [u8; 4] {
    let c = surface_fill();
    [c.r(), c.g(), c.b(), 255]
}

/// Screen-space area of the domain rectangle (the four corners). At
/// pitch = ±90° a graph z = f(x, y) projects to exactly this rectangle,
/// so a depth image's coverage should match it from either pole.
pub(crate) fn domain_quad_fraction(
    data: &Surface3dData,
    cam: &SurfaceCamera,
    logical_size: egui::Vec2,
) -> f64 {
    let Some(bounds) = finite_bounds(data) else {
        return 0.0;
    };
    let (width, height) = raster_pixel_size(logical_size, 1.0);
    let map = ScreenMap::new(egui::vec2(width as f32, height as f32), cam.zoom, cam.pan);
    let z = bounds.zmin;
    let corners = [
        (bounds.xmin, bounds.ymin, z),
        (bounds.xmax, bounds.ymin, z),
        (bounds.xmax, bounds.ymax, z),
        (bounds.xmin, bounds.ymax, z),
    ];
    let mut p = [(0.0f32, 0.0f32); 4];
    for (i, &(x, y, zz)) in corners.iter().enumerate() {
        let Some(s) = project_screen(x, y, zz, &bounds, cam, &map) else {
            return 0.0;
        };
        p[i] = (s.x, s.y);
    }
    // Shoelace. Sign follows winding; the area does not.
    let mut acc = 0.0f32;
    for i in 0..4 {
        let j = (i + 1) % 4;
        acc += p[i].0 * p[j].1 - p[j].0 * p[i].1;
    }
    let area = acc.abs() * 0.5;
    let denom = (width as f32) * (height as f32);
    if denom <= 0.0 {
        0.0
    } else {
        (area / denom) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::Surface3dData;

    fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| a + (b - a) * i as f64 / (n - 1) as f64)
            .collect()
    }

    fn surface(n: usize, z_at: impl Fn(f64, f64) -> f64, scale: &str) -> Surface3dData {
        let x = linspace(-3.0, 3.0, n);
        let y = linspace(-3.0, 3.0, n);
        let mut z = Vec::with_capacity(n * n);
        for r in 0..n {
            for c in 0..n {
                z.push(z_at(x[c], y[r]));
            }
        }
        Surface3dData {
            nrows: n,
            ncols: n,
            x,
            y,
            z,
            colorscale: scale.into(),
        }
    }

    fn gaussian(n: usize) -> Surface3dData {
        surface(n, |x, y| (-(x * x + y * y) / 2.0).exp(), "viridis")
    }

    fn plane(n: usize) -> Surface3dData {
        surface(n, |x, y| 0.3 * x + 0.1 * y, "viridis")
    }

    fn ripples(n: usize) -> Surface3dData {
        surface(
            n,
            |x, y| (x * x + y * y).sin() / (x * x + y * y + 0.1),
            "jet",
        )
    }

    fn peaks_with_hole(n: usize) -> Surface3dData {
        let mut s = surface(
            n,
            |x, y| {
                3.0 * (1.0 - x).powi(2) * (-(x * x) - (y + 1.0).powi(2)).exp()
                    - 10.0 * (x / 5.0 - x.powi(3) - y.powi(5)) * (-x * x - y * y).exp()
                    - (1.0 / 3.0) * (-(x + 1.0).powi(2) - y * y).exp()
            },
            "hot",
        );
        // A block of missing samples. Both poles must leave the same hole.
        let r0 = n / 3;
        let r1 = 2 * n / 3;
        for r in r0..r1 {
            for c in r0..r1 {
                s.z[r * n + c] = f64::NAN;
            }
        }
        s
    }

    fn cam(yaw_deg: f32, pitch_deg: f32, roll_deg: f32) -> SurfaceCamera {
        SurfaceCamera {
            yaw: yaw_deg.to_radians(),
            pitch: pitch_deg.to_radians(),
            roll: roll_deg.to_radians(),
            ..SurfaceCamera::default()
        }
    }

    fn paint(data: &Surface3dData, camera: &SurfaceCamera, shade: Shade) -> RasterImage {
        rasterize(
            data,
            camera,
            egui::vec2(64.0, 48.0),
            RasterOpts {
                shade,
                axes: false,
                pixels_per_point: 1.0,
            },
        )
    }

    #[test]
    fn nearer_triangle_wins_regardless_of_order() {
        // Two screen triangles covering the same pixel. Larger depth is closer.
        let far = ScreenVert {
            x: 2.0,
            y: 2.0,
            depth: 0.0,
            rgb: [200, 0, 0],
        };
        let near = ScreenVert {
            x: 2.0,
            y: 2.0,
            depth: 1.0,
            rgb: [0, 0, 220],
        };
        // Build a triangle around pixel (2, 2) by offsetting copies.
        let tri = |v: ScreenVert, dx: f32, dy: f32, depth: f32, rgb: [u8; 3]| ScreenVert {
            x: v.x + dx,
            y: v.y + dy,
            depth,
            rgb,
        };
        let red = [
            tri(far, -2.0, -2.0, 0.0, [200, 0, 0]),
            tri(far, 6.0, -2.0, 0.0, [200, 0, 0]),
            tri(far, -2.0, 6.0, 0.0, [200, 0, 0]),
        ];
        let blue = [
            tri(near, -2.0, -2.0, 1.0, [0, 0, 220]),
            tri(near, 6.0, -2.0, 1.0, [0, 0, 220]),
            tri(near, -2.0, 6.0, 1.0, [0, 0, 220]),
        ];
        for order in [vec![red, blue], vec![blue, red]] {
            let mut image = RasterImage {
                width: 8,
                height: 8,
                rgba: vec![0; 8 * 8 * 4],
            };
            let mut zbuf = vec![f32::NEG_INFINITY; 64];
            for t in order {
                fill_triangle(&mut image, Some(&mut zbuf), t);
            }
            let o = (2 * 8 + 2) * 4;
            assert_eq!(
                &image.rgba[o..o + 3],
                &[0, 0, 220],
                "closer blue covers red"
            );
        }
    }

    #[test]
    fn projection_stays_finite_through_poles_and_full_yaw() {
        let data = gaussian(8);
        let bounds = finite_bounds(&data).unwrap();
        for pitch in [0.0, 30.0, 89.0, 90.0, -30.0, -90.0, 120.0, 180.0] {
            for yaw in (0..360).step_by(15) {
                for roll in [0.0, 45.0, 90.0, 180.0, -90.0] {
                    let camera = cam(yaw as f32, pitch, roll);
                    for &(x, y, z) in &[
                        (bounds.xmin, bounds.ymin, bounds.zmin),
                        (bounds.xmax, bounds.ymax, bounds.zmax),
                        (0.0, 0.0, 0.5 * (bounds.zmin + bounds.zmax)),
                    ] {
                        let p = project_world(x, y, z, &bounds, &camera);
                        let (sx, sy, d) = p.expect("finite projection");
                        assert!(sx.is_finite() && sy.is_finite() && d.is_finite());
                    }
                }
            }
        }
        // Crossing the top pole by a fraction of a degree must not jump.
        let camera_a = cam(20.0, 90.0, 0.0);
        let camera_b = cam(20.0, 90.5, 0.0);
        let a = project_world(1.0, -1.0, 0.2, &bounds, &camera_a).unwrap();
        let b = project_world(1.0, -1.0, 0.2, &bounds, &camera_b).unwrap();
        assert!((a.0 - b.0).abs() < 0.05, "sx jump {}", (a.0 - b.0).abs());
        assert!((a.1 - b.1).abs() < 0.05, "sy jump {}", (a.1 - b.1).abs());
    }

    #[test]
    fn both_poles_match_the_domain_rectangle() {
        let size = egui::vec2(64.0, 48.0);
        for data in [gaussian(16), plane(16), ripples(16)] {
            for yaw in (0..360).step_by(5) {
                for roll in [0.0, 20.0, 45.0, 90.0] {
                    for pitch in [90.0, -90.0] {
                        let camera = cam(yaw as f32, pitch, roll);
                        let quad = domain_quad_fraction(&data, &camera, size);
                        let image = paint(&data, &camera, Shade::Depth);
                        let cov = coverage_fraction(&image);
                        let tol = (quad * 0.18).max(0.02);
                        assert!(
                            (cov - quad).abs() < tol,
                            "pitch {pitch} yaw {yaw} roll {roll}: coverage {cov:.3} vs quad {quad:.3}"
                        );
                        if pitch < 0.0 {
                            let legacy =
                                coverage_fraction(&paint(&data, &camera, Shade::CullPainter));
                            assert!(
                                legacy < 0.01,
                                "old cull still drew the underside at yaw {yaw}: {legacy:.3}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn azimuth_sweep_does_not_open_holes() {
        let pitches = [0.0f32, 30.0, 89.0, 90.0, -30.0, -90.0];
        for (name, data) in [
            ("gaussian", gaussian(14)),
            ("ripples", ripples(14)),
            ("plane", plane(14)),
        ] {
            for pitch in pitches {
                let mut depth_cov = Vec::new();
                let mut legacy_cov = Vec::new();
                for yaw in (0..360).step_by(5) {
                    let camera = cam(yaw as f32, pitch, 0.0);
                    depth_cov.push(coverage_fraction(&paint(&data, &camera, Shade::Depth)));
                    legacy_cov.push(coverage_fraction(&paint(
                        &data,
                        &camera,
                        Shade::CullPainter,
                    )));
                }
                let d_min = depth_cov.iter().copied().fold(f64::INFINITY, f64::min);
                let d_max = depth_cov.iter().copied().fold(0.0, f64::max);
                // A radially symmetric sheet keeps a stable silhouette as it
                // spins about Z. A tilted plane's projected area really does
                // change with azimuth (face-on vs edge-on); there the check
                // is that we never drop pixels the true sheet still covers,
                // which the pole test and the legacy comparison cover.
                if name != "plane" && pitch.abs() >= 30.0 {
                    assert!(
                        d_min > d_max * 0.72,
                        "{name} pitch {pitch}: coverage swung {d_min:.3}..{d_max:.3}"
                    );
                }
                // A tilted plane at a shallow elevation is legitimately
                // edge-on for some azimuths. The underside bug is the
                // sheet vanishing while its projected area is still large:
                // every surface at the bottom pole, and curved sheets at
                // negative elevation.
                let expect_opaque = pitch <= -89.0 || (pitch < 0.0 && name != "plane");
                if expect_opaque {
                    let legacy_max = legacy_cov.iter().copied().fold(0.0, f64::max);
                    // The bottom pole used to discard the whole sheet. At a
                    // moderate negative elevation the old cull still kept
                    // the faces that happened to wind the right way, so the
                    // gap is smaller — but the depth image must still beat
                    // that cull at every azimuth.
                    let factor = if pitch <= -89.0 { 1.4 } else { 1.0 };
                    assert!(
                        d_min > legacy_max * factor && d_min > 0.04,
                        "{name} pitch {pitch}: depth min {d_min:.3}, old cull max {legacy_max:.3}"
                    );
                }
            }
        }
    }

    #[test]
    fn roll_does_not_drop_coverage() {
        let data = gaussian(14);
        for pitch in [30.0, 90.0, -45.0, -90.0] {
            let base = coverage_fraction(&paint(&data, &cam(-45.0, pitch, 0.0), Shade::Depth));
            for roll in [0.0, 30.0, 90.0, 135.0, 180.0, 270.0, -60.0] {
                for yaw in [0.0, 90.0, 180.0, 270.0] {
                    let cov =
                        coverage_fraction(&paint(&data, &cam(yaw, pitch, roll), Shade::Depth));
                    assert!(
                        cov > base * 0.75,
                        "pitch {pitch} yaw {yaw} roll {roll}: {cov:.3} vs {base:.3}"
                    );
                }
            }
        }
    }

    #[test]
    fn nan_hole_is_the_same_from_both_poles() {
        let full = gaussian(18);
        let holed = peaks_with_hole(18);
        for yaw in (0..360).step_by(15) {
            for pitch in [90.0, -90.0] {
                let camera = cam(yaw as f32, pitch, 0.0);
                let a = coverage_fraction(&paint(&holed, &camera, Shade::Depth));
                let b =
                    coverage_fraction(&paint(&holed, &cam(yaw as f32, -pitch, 0.0), Shade::Depth));
                assert!((a - b).abs() < 0.03, "yaw {yaw}: {a:.3} vs {b:.3}");
                let solid = coverage_fraction(&paint(&full, &camera, Shade::Depth));
                assert!(a < solid * 0.92, "hole did not reduce coverage");
                assert!(a > solid * 0.4, "hole ate the sheet");
            }
        }
        // An all-NaN sheet is background, not a smear of non-finite colours.
        let mut blank = gaussian(6);
        for z in &mut blank.z {
            *z = f64::NAN;
        }
        let image = paint(&blank, &cam(10.0, -90.0, 15.0), Shade::Depth);
        assert_eq!(coverage_fraction(&image), 0.0);
        assert!(image.rgba.chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn raster_size_tracks_window_and_hidpi() {
        let (w, h) = raster_pixel_size(egui::vec2(400.0, 300.0), 1.0);
        assert_eq!((w, h), (400, 300));
        let (w2, h2) = raster_pixel_size(egui::vec2(400.0, 300.0), 2.0);
        assert_eq!((w2, h2), (800, 600));
        let (w3, h3) = raster_pixel_size(egui::vec2(800.0, 600.0), 1.0);
        assert!(w3 > w && h3 > h, "a larger window rasterizes more pixels");
        let (cap_w, cap_h) = raster_pixel_size(egui::vec2(4000.0, 2000.0), 2.0);
        assert!(cap_w.max(cap_h) <= 1600);
        assert!(cap_w > 0 && cap_h > 0);
    }

    /// Before/after contact sheets. Set `RUSTLAB_SURF_SHEETS` to a directory.
    #[test]
    fn write_contact_sheets() {
        let Some(dir) = std::env::var_os("RUSTLAB_SURF_SHEETS") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&dir);
        let surfaces = [
            ("gaussian", gaussian(28)),
            ("plane", plane(28)),
            ("ripples", ripples(28)),
            ("peaks+nan", peaks_with_hole(28)),
        ];
        let pitches = [0.0f32, 30.0, 89.0, 90.0, -30.0, -90.0];
        let yaws: Vec<f32> = (0..360).step_by(30).map(|y| y as f32).collect();
        let thumb = egui::vec2(72.0, 54.0);
        for (shade, name) in [(Shade::CullPainter, "before"), (Shade::Depth, "after")] {
            let sheet = contact_sheet(&surfaces, &pitches, &yaws, thumb, shade);
            let path = dir.join(format!("surf_sweep_{name}.png"));
            std::fs::write(&path, encode_png(sheet.0, sheet.1, &sheet.2)).unwrap();
        }
        // Roll strip: one azimuth, several rolls, including through a pole.
        let rolls = [0.0f32, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
        let roll_pitches = [30.0f32, 90.0, -90.0];
        for (shade, name) in [(Shade::CullPainter, "before"), (Shade::Depth, "after")] {
            let sheet = roll_sheet(&gaussian(28), &roll_pitches, &rolls, thumb, shade);
            let path = dir.join(format!("surf_roll_{name}.png"));
            std::fs::write(&path, encode_png(sheet.0, sheet.1, &sheet.2)).unwrap();
        }
    }

    fn contact_sheet(
        surfaces: &[(&str, Surface3dData)],
        pitches: &[f32],
        yaws: &[f32],
        thumb: egui::Vec2,
        shade: Shade,
    ) -> (u32, u32, Vec<u8>) {
        let gap = 3u32;
        let label_w = 8u32;
        let tw = thumb.x as u32;
        let th = thumb.y as u32;
        let cols = yaws.len() as u32;
        let rows = pitches.len() as u32;
        let block_h = rows * th + (rows - 1) * gap;
        let width = label_w + cols * tw + (cols - 1) * gap;
        let height =
            surfaces.len() as u32 * block_h + (surfaces.len().saturating_sub(1) as u32) * (gap * 3);
        let mut rgba = vec![40u8; width as usize * height as usize * 4];
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
        for (s, (_, data)) in surfaces.iter().enumerate() {
            let top = s as u32 * (block_h + gap * 3);
            for (r, &pitch) in pitches.iter().enumerate() {
                for (c, &yaw) in yaws.iter().enumerate() {
                    let image = rasterize(
                        data,
                        &cam(yaw, pitch, 0.0),
                        thumb,
                        RasterOpts {
                            shade,
                            axes: false,
                            pixels_per_point: 1.0,
                        },
                    );
                    blit(
                        &mut rgba,
                        width,
                        &image,
                        label_w + c as u32 * (tw + gap),
                        top + r as u32 * (th + gap),
                    );
                }
            }
        }
        (width, height, rgba)
    }

    fn roll_sheet(
        data: &Surface3dData,
        pitches: &[f32],
        rolls: &[f32],
        thumb: egui::Vec2,
        shade: Shade,
    ) -> (u32, u32, Vec<u8>) {
        let gap = 3u32;
        let tw = thumb.x as u32;
        let th = thumb.y as u32;
        let cols = rolls.len() as u32;
        let rows = pitches.len() as u32;
        let width = cols * tw + (cols - 1) * gap;
        let height = rows * th + (rows - 1) * gap;
        let mut rgba = vec![40u8; width as usize * height as usize * 4];
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
        for (r, &pitch) in pitches.iter().enumerate() {
            for (c, &roll) in rolls.iter().enumerate() {
                let image = rasterize(
                    data,
                    &cam(-45.0, pitch, roll),
                    thumb,
                    RasterOpts {
                        shade,
                        axes: false,
                        pixels_per_point: 1.0,
                    },
                );
                blit(
                    &mut rgba,
                    width,
                    &image,
                    c as u32 * (tw + gap),
                    r as u32 * (th + gap),
                );
            }
        }
        (width, height, rgba)
    }

    fn blit(dst: &mut [u8], dst_w: u32, src: &RasterImage, x: u32, y: u32) {
        for row in 0..src.height {
            for col in 0..src.width {
                let si = (row as usize * src.width as usize + col as usize) * 4;
                let di = ((y + row) as usize * dst_w as usize + (x + col) as usize) * 4;
                dst[di..di + 4].copy_from_slice(&src.rgba[si..si + 4]);
            }
        }
    }

    fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
        let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
        for y in 0..height as usize {
            raw.push(0);
            let start = y * width as usize * 4;
            raw.extend_from_slice(&rgba[start..start + width as usize * 4]);
        }
        let idat = zlib_store(&raw);
        let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        png.extend(chunk(b"IHDR", &ihdr));
        png.extend(chunk(b"IDAT", &idat));
        png.extend(chunk(b"IEND", &[]));
        png
    }

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[4..]);
        out.extend_from_slice(&crc.to_be_bytes());
        out
    }

    fn zlib_store(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x78, 0x01];
        let mut i = 0;
        while i < data.len() {
            let n = (data.len() - i).min(65535);
            let last = i + n == data.len();
            out.push(if last { 1 } else { 0 });
            out.extend_from_slice(&(n as u16).to_le_bytes());
            out.extend_from_slice(&((n as u16) ^ 0xffff).to_le_bytes());
            out.extend_from_slice(&data[i..i + n]);
            i += n;
        }
        out.extend_from_slice(&adler32(data).to_be_bytes());
        out
    }

    fn adler32(data: &[u8]) -> u32 {
        let mut a: u32 = 1;
        let mut b: u32 = 0;
        for &byte in data {
            a = (a + byte as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        !crc
    }
}
