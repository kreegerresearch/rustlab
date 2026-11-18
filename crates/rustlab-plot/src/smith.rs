//! Smith-chart grid geometry, labels, and colours.
//!
//! A panel whose [`crate::figure::SubplotState::smith`] is set is a Smith
//! chart. Each backend draws the grid from [`build_grid`] and the value
//! labels from [`grid_labels`], hides the Cartesian axes, and plots the
//! panel's series (the reflection coefficients) on top. The series hold only
//! user data.
//!
//! Every point comes from the bilinear map of normalised impedance
//! `z = r + jx` to the reflection coefficient
//!
//! ```text
//! Γ = (z − 1) / (z + 1)
//! ```
//!
//! which puts the inductive half (`x > 0`) above the real axis, a short at
//! `Γ = −1`, an open at `Γ = +1`, and the matched load at the centre. That is
//! the orientation of MATLAB's `smithplot` and of printed charts.
//!
//! Impedance chart, normalised to Z0:
//!   constant r:  centre (r/(r+1), 0), radius 1/(r+1)
//!   constant x:  centre (1, 1/x), radius 1/|x|, the arc from Γ(jx) on the
//!                unit circle to Γ = 1
//!
//! The admittance chart is the impedance chart turned through 180°
//! (`Γ_y(g + jb) = −Γ_z(g + jb)`), so a capacitive susceptance (`b > 0`)
//! lies in the lower half, where a capacitive reactance also lies.

/// Which grid family to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmithGrid {
    /// Impedance grid (constant-R circles + constant-X arcs). Default.
    Impedance,
    /// Admittance grid (constant-G circles + constant-B arcs).
    Admittance,
    /// Immittance overlay — both Z and Y grids together.
    Immittance,
}

impl SmithGrid {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "Z" | "IMPEDANCE" => Some(SmithGrid::Impedance),
            "Y" | "ADMITTANCE" => Some(SmithGrid::Admittance),
            "ZY" | "YZ" | "IMMITTANCE" => Some(SmithGrid::Immittance),
            _ => None,
        }
    }
}

/// One polyline of the grid, plus a family tag so backends can colour the
/// frame, Z, and Y lines differently.
#[derive(Debug, Clone)]
pub struct SmithArc {
    pub family: SmithFamily,
    /// Finite (x, y) points inside or on the unit circle.
    pub points: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmithFamily {
    /// Outer unit circle and the real-axis diameter — drawn in every mode.
    Frame,
    /// Z grid (constant-R circles + constant-X arcs).
    Impedance,
    /// Y grid (constant-G circles + constant-B arcs).
    Admittance,
}

/// Grid values of the resistance circles and reactance arcs (and of the
/// conductance circles and susceptance arcs), the values on MATLAB's
/// `smithplot` and on printed charts.
pub const GRID_VALUES: &[f64] = &[0.2, 0.5, 1.0, 2.0, 5.0];

/// Vertices per full turn used by every backend.
pub const ARC_RESOLUTION: usize = 192;

/// Half-width of the square view for backends that cannot measure text
/// (HTML, terminal, viewer). The SVG/PNG backend computes its own margin in
/// pixels; see [`view_half_width`].
pub const VIEW_HALF_WIDTH: f64 = 1.15;

/// Label font size in pixels.
pub const LABEL_FONT_PX: u32 = 11;

/// Gap in pixels between a label and the point it names.
pub const LABEL_GAP_PX: f64 = 4.0;

/// Room in pixels kept around the unit circle for the reactance labels.
pub const LABEL_MARGIN_PX: f64 = 38.0;

/// `Γ = (z − 1) / (z + 1)` for normalised `z = r + jx`.
pub fn z_to_gamma(r: f64, x: f64) -> (f64, f64) {
    let d = (r + 1.0) * (r + 1.0) + x * x;
    ((r * r + x * x - 1.0) / d, 2.0 * x / d)
}

/// `z = (1 + Γ) / (1 − Γ)`, the inverse of [`z_to_gamma`].
pub fn gamma_to_z(gr: f64, gi: f64) -> (f64, f64) {
    let d = (1.0 - gr) * (1.0 - gr) + gi * gi;
    ((1.0 - gr * gr - gi * gi) / d, 2.0 * gi / d)
}

/// Half-width of the view square that leaves `LABEL_MARGIN_PX` pixels
/// around the unit circle when the square is `side_px` pixels wide.
pub fn view_half_width(side_px: f64) -> f64 {
    let usable = side_px - 2.0 * LABEL_MARGIN_PX;
    if usable <= side_px * 0.25 {
        // Too small to keep a pixel margin: fall back to a fixed fraction.
        return 4.0 / 3.0;
    }
    side_px / usable
}

/// Build the grid as a list of polylines.
///
/// `arc_resolution` is the number of vertices per full circle; every arc,
/// however short, gets at least 16 segments.
pub fn build_grid(grid: SmithGrid, arc_resolution: usize) -> Vec<SmithArc> {
    let n = arc_resolution.max(16);
    let mut out: Vec<SmithArc> = Vec::new();

    out.push(SmithArc {
        family: SmithFamily::Frame,
        points: unit_circle(n),
    });
    out.push(SmithArc {
        family: SmithFamily::Frame,
        points: vec![(-1.0, 0.0), (1.0, 0.0)],
    });

    match grid {
        SmithGrid::Impedance => push_family(&mut out, n, SmithFamily::Impedance),
        SmithGrid::Admittance => push_family(&mut out, n, SmithFamily::Admittance),
        SmithGrid::Immittance => {
            push_family(&mut out, n, SmithFamily::Impedance);
            push_family(&mut out, n, SmithFamily::Admittance);
        }
    }
    out
}

fn push_family(out: &mut Vec<SmithArc>, n: usize, family: SmithFamily) {
    let flip = |pts: Vec<(f64, f64)>| -> Vec<(f64, f64)> {
        if family == SmithFamily::Admittance {
            pts.into_iter().map(|(x, y)| (-x, -y)).collect()
        } else {
            pts
        }
    };
    for &r in GRID_VALUES {
        out.push(SmithArc {
            family,
            points: flip(constant_r_circle(r, n)),
        });
    }
    for &x in GRID_VALUES {
        for xv in [x, -x] {
            out.push(SmithArc {
                family,
                points: flip(constant_x_arc(xv, n)),
            });
        }
    }
}

fn unit_circle(n: usize) -> Vec<(f64, f64)> {
    sample_arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU, n)
}

/// Constant-resistance circle `Re(z) = r`, a full circle tangent to the unit
/// circle at `Γ = 1`.
pub fn constant_r_circle(r: f64, n: usize) -> Vec<(f64, f64)> {
    let cx = r / (r + 1.0);
    let radius = 1.0 / (r + 1.0);
    // Start and end at Γ = 1 so the closing point is exact.
    sample_arc(cx, 0.0, radius, 0.0, std::f64::consts::TAU, n)
}

/// Constant-reactance arc `Im(z) = x`: the part of the circle centred
/// `(1, 1/x)` with radius `1/|x|` that lies in the unit disk. It runs from
/// `Γ(jx)` on the unit circle (`r = 0`) to `Γ = 1` (`r → ∞`) and spans
/// `2·atan(|x|)` of its circle.
pub fn constant_x_arc(x: f64, n: usize) -> Vec<(f64, f64)> {
    if x == 0.0 || !x.is_finite() {
        // x = 0 is the real axis, drawn as part of the frame.
        return Vec::new();
    }
    let cx = 1.0;
    let cy = 1.0 / x;
    let radius = 1.0 / x.abs();
    let (sx, sy) = z_to_gamma(0.0, x);
    let start = (sy - cy).atan2(sx - cx);
    let end = (0.0 - cy).atan2(1.0 - cx);
    let mut sweep = end - start;
    // The arc inside the disk is always shorter than a half turn.
    while sweep > std::f64::consts::PI {
        sweep -= std::f64::consts::TAU;
    }
    while sweep <= -std::f64::consts::PI {
        sweep += std::f64::consts::TAU;
    }
    let mut pts = sample_arc(cx, cy, radius, start, sweep, n);
    // Pin the endpoints to their exact values.
    if let Some(first) = pts.first_mut() {
        *first = (sx, sy);
    }
    if let Some(last) = pts.last_mut() {
        *last = (1.0, 0.0);
    }
    pts
}

/// Points on the circle `(cx, cy, radius)` from angle `start` through
/// `sweep` radians, with segments no longer than one `n`-th of a full turn
/// and at least 16 of them.
fn sample_arc(cx: f64, cy: f64, radius: f64, start: f64, sweep: f64, n: usize) -> Vec<(f64, f64)> {
    let per_turn = n.max(16) as f64;
    let by_turn = ((sweep.abs() / std::f64::consts::TAU) * per_turn).ceil() as usize;
    // Also bound the chord on the chart, so a short arc of a large circle
    // still reads as a curve.
    let by_length = (radius * sweep.abs() / 0.02).ceil() as usize;
    let segs = by_turn.max(by_length).clamp(16, 4096);
    (0..=segs)
        .map(|k| {
            let t = start + sweep * (k as f64) / (segs as f64);
            (cx + radius * t.cos(), cy + radius * t.sin())
        })
        .collect()
}

/// The part of the circle `(cx, cy, radius)` inside the unit disk, as one
/// polyline from rim to rim (or the whole circle when it lies inside).
/// Empty when the circle is wholly outside the disk.
pub fn circle_in_unit_disk(cx: f64, cy: f64, radius: f64, n: usize) -> Vec<(f64, f64)> {
    let d = (cx * cx + cy * cy).sqrt();
    if radius <= 0.0 || !radius.is_finite() || !d.is_finite() {
        return Vec::new();
    }
    if d + radius <= 1.0 {
        return sample_arc(cx, cy, radius, 0.0, std::f64::consts::TAU, n);
    }
    if d - radius >= 1.0 || radius - d >= 1.0 {
        // Outside the disk, or enclosing it without crossing the rim.
        return Vec::new();
    }
    // |c + ρ e^{jt}| = 1  ⇔  cos(t − arg c) = (1 − d² − ρ²) / (2 ρ d).
    let phase = cy.atan2(cx);
    let k = ((1.0 - d * d - radius * radius) / (2.0 * radius * d)).clamp(-1.0, 1.0);
    let half = k.acos();
    // Points with cos(t − arg c) < k are nearer the origin; they are the
    // arc through t = arg c + π.
    let start = phase + half;
    let sweep = std::f64::consts::TAU - 2.0 * half;
    let mut pts = sample_arc(cx, cy, radius, start, sweep, n);
    // Land the ends exactly on the rim.
    for p in [0, pts.len() - 1] {
        let r = (pts[p].0 * pts[p].0 + pts[p].1 * pts[p].1).sqrt();
        pts[p] = (pts[p].0 / r, pts[p].1 / r);
    }
    pts
}

/// Horizontal anchor of a label: which side of the text box sits at the
/// anchor point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAnchor {
    Left,
    Center,
    Right,
}

/// Vertical anchor of a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAnchor {
    Top,
    Middle,
    Bottom,
}

/// One grid value label. The text box is placed with its `h`/`v` side at
/// the data point `(x, y)` moved by `(dx_px, dy_px)` screen pixels
/// (`dy_px > 0` is up).
#[derive(Debug, Clone, PartialEq)]
pub struct SmithLabel {
    pub x: f64,
    pub y: f64,
    pub dx_px: f64,
    pub dy_px: f64,
    pub h: HAnchor,
    pub v: VAnchor,
    pub text: String,
}

/// Labels for the grid values of `grid`. The immittance overlay is labelled
/// with its impedance values only, so the two families do not print on top
/// of each other.
pub fn grid_labels(grid: SmithGrid) -> Vec<SmithLabel> {
    let sign = match grid {
        SmithGrid::Admittance => -1.0,
        SmithGrid::Impedance | SmithGrid::Immittance => 1.0,
    };
    let mut out = Vec::new();
    for &r in GRID_VALUES {
        let (gx, _) = z_to_gamma(r, 0.0);
        // Above the real axis, on the side away from Γ = 1, so the label
        // sits between this circle and the next larger one.
        // The offset keeps a 4 px marker at the crossing (the matched load
        // sits on the 1.0 label's crossing) clear of the text.
        let (h, dx) = if sign > 0.0 {
            (HAnchor::Right, -LABEL_GAP_PX * 1.25)
        } else {
            (HAnchor::Left, LABEL_GAP_PX * 1.25)
        };
        out.push(SmithLabel {
            x: sign * gx,
            y: 0.0,
            dx_px: dx,
            dy_px: LABEL_GAP_PX * 1.25,
            h,
            v: VAnchor::Bottom,
            text: format_grid_value(r),
        });
    }
    for &x in GRID_VALUES {
        for xv in [x, -x] {
            let (gx, gy) = z_to_gamma(0.0, xv);
            let (gx, gy) = (sign * gx, sign * gy);
            // On the unit circle, pushed straight outward.
            let h = if gx > 0.25 {
                HAnchor::Left
            } else if gx < -0.25 {
                HAnchor::Right
            } else {
                HAnchor::Center
            };
            let v = if gy > 0.0 {
                VAnchor::Bottom
            } else {
                VAnchor::Top
            };
            let text = if xv > 0.0 {
                format!("+j{}", format_grid_value(x))
            } else {
                format!("-j{}", format_grid_value(x))
            };
            out.push(SmithLabel {
                x: gx,
                y: gy,
                dx_px: LABEL_GAP_PX * gx,
                dy_px: LABEL_GAP_PX * gy,
                h,
                v,
                text,
            });
        }
    }
    out
}

/// Chart scale in pixels per unit of Γ below which only the `1.0` and
/// `±j1.0` labels are kept; the grid values are a third of a unit apart on
/// the real axis, too close for three-character labels.
pub const FULL_LABELS_MIN_PX_PER_UNIT: f64 = 72.0;

/// [`grid_labels`] thinned for a chart drawn at `px_per_unit` pixels per
/// unit of Γ.
pub fn grid_labels_at(grid: SmithGrid, px_per_unit: f64) -> Vec<SmithLabel> {
    let all = grid_labels(grid);
    if px_per_unit >= FULL_LABELS_MIN_PX_PER_UNIT {
        return all;
    }
    all.into_iter()
        .filter(|l| matches!(l.text.as_str(), "1.0" | "+j1.0" | "-j1.0"))
        .collect()
}

/// `0.2`, `0.5`, `1.0`, `2.0`, `5.0` — one decimal, as MATLAB prints them.
fn format_grid_value(v: f64) -> String {
    format!("{v:.1}")
}

/// Screen-space box `(left, top, right, bottom)` of `label`, with the data
/// point mapped by `to_px` and text `w` by `h` pixels. Screen y grows down.
pub fn label_box(
    label: &SmithLabel,
    to_px: impl Fn(f64, f64) -> (f64, f64),
    w: f64,
    h: f64,
) -> (f64, f64, f64, f64) {
    let (px, py) = to_px(label.x, label.y);
    let ax = px + label.dx_px;
    let ay = py - label.dy_px;
    let left = match label.h {
        HAnchor::Left => ax,
        HAnchor::Center => ax - w / 2.0,
        HAnchor::Right => ax - w,
    };
    let top = match label.v {
        VAnchor::Top => ay,
        VAnchor::Middle => ay - h / 2.0,
        VAnchor::Bottom => ay - h,
    };
    (left, top, left + w, top + h)
}

/// Colours of the frame, the grid lines, and the labels for a theme with
/// background `bg` and text colour `text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmithColors {
    pub frame: (u8, u8, u8),
    pub impedance: (u8, u8, u8),
    pub admittance: (u8, u8, u8),
    pub label: (u8, u8, u8),
}

impl SmithColors {
    pub fn for_theme(bg: (u8, u8, u8), text: (u8, u8, u8)) -> Self {
        // A light background needs the grid nearer the text colour for the
        // same contrast as on a dark one.
        let light = relative_luminance(bg) > 0.5;
        let (frame_t, grid_t, label_t) = if light {
            (0.70, 0.40, 0.85)
        } else {
            (0.62, 0.27, 0.80)
        };
        let teal = if light {
            (23, 146, 153)
        } else {
            (148, 226, 213)
        };
        SmithColors {
            frame: mix(bg, text, frame_t),
            impedance: mix(bg, text, grid_t),
            admittance: mix(bg, teal, grid_t + 0.1),
            label: mix(bg, text, label_t),
        }
    }

    pub fn of(&self, family: SmithFamily) -> (u8, u8, u8) {
        match family {
            SmithFamily::Frame => self.frame,
            SmithFamily::Impedance => self.impedance,
            SmithFamily::Admittance => self.admittance,
        }
    }
}

fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f64) -> (u8, u8, u8) {
    let m = |x: u8, y: u8| {
        (x as f64 + (y as f64 - x as f64) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// WCAG relative luminance of an sRGB colour.
pub fn relative_luminance(c: (u8, u8, u8)) -> f64 {
    let lin = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

/// WCAG contrast ratio between two colours.
pub fn contrast_ratio(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Parse `#rrggbb`.
pub fn parse_hex_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// Colours for a theme given as `ThemeColors` hex strings.
pub fn colors_for(theme: &crate::theme::ThemeColors) -> SmithColors {
    let bg = parse_hex_rgb(theme.plot_bg.as_ref()).unwrap_or((255, 255, 255));
    let text = parse_hex_rgb(theme.text.as_ref()).unwrap_or((0, 0, 0));
    SmithColors::for_theme(bg, text)
}

/// `#rrggbb` for an RGB triple.
pub fn hex(c: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-12;

    fn close(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
        (a.0 - b.0).abs() <= tol && (a.1 - b.1).abs() <= tol
    }

    #[test]
    fn cardinal_points_follow_gamma_of_z() {
        // Matched load at the centre, short at −1, open at +1.
        assert!(close(z_to_gamma(1.0, 0.0), (0.0, 0.0), EPS));
        assert!(close(z_to_gamma(0.0, 0.0), (-1.0, 0.0), EPS));
        assert!(close(z_to_gamma(1e15, 0.0), (1.0, 0.0), 1e-12));
        // z = 2: Γ = 1/3. z = 0.5: Γ = −1/3.
        assert!(close(z_to_gamma(2.0, 0.0), (1.0 / 3.0, 0.0), EPS));
        assert!(close(z_to_gamma(0.5, 0.0), (-1.0 / 3.0, 0.0), EPS));
        // z = ±j: Γ = ±j (top and bottom of the chart; inductive is up).
        assert!(close(z_to_gamma(0.0, 1.0), (0.0, 1.0), EPS));
        assert!(close(z_to_gamma(0.0, -1.0), (0.0, -1.0), EPS));
        // z = 0.5 + 0.5j: Γ = (−0.5 + 0.5j)/(1.5 + 0.5j) = −0.2 + 0.4j.
        assert!(close(z_to_gamma(0.5, 0.5), (-0.2, 0.4), EPS));
        // z = 1 + j: Γ = j/(2 + j) = 0.2 + 0.4j.
        assert!(close(z_to_gamma(1.0, 1.0), (0.2, 0.4), EPS));
    }

    #[test]
    fn pure_reactance_lies_on_the_unit_circle() {
        for x in [-5.0, -1.0, -0.3, 0.1, 0.7, 3.0, 40.0] {
            let (gr, gi) = z_to_gamma(0.0, x);
            assert!(((gr * gr + gi * gi).sqrt() - 1.0).abs() < EPS, "x = {x}");
            assert_eq!(gi > 0.0, x > 0.0, "inductive must be the upper half");
        }
    }

    #[test]
    fn gamma_to_z_inverts_z_to_gamma() {
        for &(r, x) in &[(0.2, 0.0), (1.0, 1.0), (2.0, -0.5), (0.0, 0.3), (5.0, 5.0)] {
            let (gr, gi) = z_to_gamma(r, x);
            let (r2, x2) = gamma_to_z(gr, gi);
            assert!((r - r2).abs() < 1e-9 && (x - x2).abs() < 1e-9, "{r} {x}");
        }
    }

    #[test]
    fn grid_includes_frame_and_arcs() {
        let g = build_grid(SmithGrid::Impedance, 64);
        // 2 frame items + 5 R circles + 10 X arcs = 17
        assert_eq!(g.len(), 17);
        assert!(matches!(g[0].family, SmithFamily::Frame));
        assert!(matches!(g[1].family, SmithFamily::Frame));
    }

    #[test]
    fn every_grid_point_is_finite() {
        for mode in [
            SmithGrid::Impedance,
            SmithGrid::Admittance,
            SmithGrid::Immittance,
        ] {
            for arc in build_grid(mode, ARC_RESOLUTION) {
                assert!(arc.points.len() >= 2);
                for (x, y) in arc.points {
                    assert!(x.is_finite() && y.is_finite(), "{mode:?}");
                    assert!(x * x + y * y <= 1.0 + 1e-9, "({x}, {y}) outside the disk");
                }
            }
        }
    }

    #[test]
    fn constant_r_circle_points_have_real_part_r() {
        for &r in GRID_VALUES {
            let c = constant_r_circle(r, ARC_RESOLUTION);
            for &(gx, gy) in &c {
                // Γ = 1 is the tangent point, where z is infinite.
                if (gx - 1.0).abs() < 1e-6 && gy.abs() < 1e-6 {
                    continue;
                }
                let (rr, _) = gamma_to_z(gx, gy);
                assert!((rr - r).abs() < 1e-6 * (1.0 + r), "r = {r}: got {rr}");
            }
            // It crosses the real axis at Γ(r) on the left.
            let (lx, _) = z_to_gamma(r, 0.0);
            let min_x = c.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            assert!((min_x - lx).abs() < 1e-3, "r = {r}");
        }
    }

    #[test]
    fn constant_x_arc_runs_exactly_from_the_unit_circle_to_gamma_one() {
        for &x in GRID_VALUES {
            for xv in [x, -x] {
                let arc = constant_x_arc(xv, ARC_RESOLUTION);
                let first = arc[0];
                let last = *arc.last().unwrap();
                assert!(
                    close(first, z_to_gamma(0.0, xv), 1e-12),
                    "x = {xv}: start {first:?}"
                );
                assert!(close(last, (1.0, 0.0), 1e-12), "x = {xv}: end {last:?}");
                for &(gx, gy) in &arc[..arc.len() - 1] {
                    let (r, xx) = gamma_to_z(gx, gy);
                    assert!(r >= -1e-9, "x = {xv}: r = {r}");
                    assert!(
                        (xx - xv).abs() < 1e-6 * (1.0 + xv.abs()),
                        "x = {xv}: got {xx}"
                    );
                }
            }
        }
    }

    #[test]
    fn constant_x_arc_is_smooth() {
        // The x = 0.2 arc spans only 22.6° of a radius-5 circle. Every
        // segment must stay short on the chart, so the arc reads as a curve.
        for &x in GRID_VALUES {
            let arc = constant_x_arc(x, ARC_RESOLUTION);
            assert!(arc.len() >= 17, "x = {x}: {} points", arc.len());
            for w in arc.windows(2) {
                let d = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
                assert!(d < 0.05, "x = {x}: segment of {d}");
            }
        }
    }

    #[test]
    fn admittance_grid_is_the_impedance_grid_turned_half_way() {
        // Γ_y(y) = −Γ_z(y): the g = 1 circle is centred at (−0.5, 0), and the
        // b = +1 (capacitive) arc ends at −j at the bottom of the chart.
        let y = build_grid(SmithGrid::Admittance, ARC_RESOLUTION);
        let fam: Vec<&SmithArc> = y
            .iter()
            .filter(|a| a.family == SmithFamily::Admittance)
            .collect();
        assert_eq!(fam.len(), 15);
        let g1 = fam[2];
        let mean_x = g1.points.iter().map(|p| p.0).sum::<f64>() / g1.points.len() as f64;
        assert!((mean_x + 0.5).abs() < 0.01, "g = 1 centre at {mean_x}");
        // Arcs follow the circles: b = +0.2, −0.2, +0.5, −0.5, +1, ...
        let b1 = fam[5 + 4];
        assert!(
            close(b1.points[0], (0.0, -1.0), 1e-12),
            "{:?}",
            b1.points[0]
        );
        assert!(close(*b1.points.last().unwrap(), (-1.0, 0.0), 1e-12));
    }

    #[test]
    fn immittance_has_both_families() {
        let g = build_grid(SmithGrid::Immittance, 64);
        assert!(g.iter().any(|a| matches!(a.family, SmithFamily::Impedance)));
        assert!(g
            .iter()
            .any(|a| matches!(a.family, SmithFamily::Admittance)));
    }

    #[test]
    fn constant_r0_circle_is_unit_circle() {
        let c = constant_r_circle(0.0, 96);
        for (x, y) in c {
            let r = (x * x + y * y).sqrt();
            assert!((r - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn parse_string_to_grid_mode() {
        assert_eq!(SmithGrid::parse("Z"), Some(SmithGrid::Impedance));
        assert_eq!(SmithGrid::parse("y"), Some(SmithGrid::Admittance));
        assert_eq!(SmithGrid::parse("ZY"), Some(SmithGrid::Immittance));
        assert_eq!(SmithGrid::parse("impedance"), Some(SmithGrid::Impedance));
        assert_eq!(SmithGrid::parse("bogus"), None);
    }

    #[test]
    fn circle_in_unit_disk_keeps_only_the_inside_arc() {
        // Inside: whole circle.
        let c = circle_in_unit_disk(0.2, 0.1, 0.3, ARC_RESOLUTION);
        assert!(c.len() > 100);
        // Outside or enclosing: nothing.
        assert!(circle_in_unit_disk(3.0, 0.0, 0.5, ARC_RESOLUTION).is_empty());
        assert!(circle_in_unit_disk(0.0, 0.0, 5.0, ARC_RESOLUTION).is_empty());
        // Crossing: the centre (1, 0) radius 1 circle meets the rim at
        // Γ = 0.5 ± j√3/2 and passes through the origin.
        let a = circle_in_unit_disk(1.0, 0.0, 1.0, ARC_RESOLUTION);
        let s3 = 3f64.sqrt() / 2.0;
        let (f, l) = (a[0], *a.last().unwrap());
        assert!(
            close(f, (0.5, s3), 1e-9) || close(f, (0.5, -s3), 1e-9),
            "{f:?}"
        );
        assert!(
            close(l, (0.5, s3), 1e-9) || close(l, (0.5, -s3), 1e-9),
            "{l:?}"
        );
        assert!(
            a.iter().any(|p| close(*p, (0.0, 0.0), 0.02)),
            "passes the origin"
        );
        for (x, y) in &a {
            assert!(x * x + y * y <= 1.0 + 1e-9);
            assert!(((x - 1.0).powi(2) + y * y - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn labels_sit_where_their_circles_and_arcs_meet_the_axis_and_rim() {
        let labels = grid_labels(SmithGrid::Impedance);
        assert_eq!(labels.len(), 15);
        let find = |t: &str| labels.iter().find(|l| l.text == t).unwrap().clone();
        let r1 = find("1.0");
        assert!(close((r1.x, r1.y), (0.0, 0.0), EPS));
        let r2 = find("2.0");
        assert!(close((r2.x, r2.y), (1.0 / 3.0, 0.0), EPS));
        let x1 = find("+j1.0");
        assert!(close((x1.x, x1.y), (0.0, 1.0), EPS));
        assert_eq!((x1.h, x1.v), (HAnchor::Center, VAnchor::Bottom));
        let xm1 = find("-j1.0");
        assert!(close((xm1.x, xm1.y), (0.0, -1.0), EPS));
        assert_eq!(xm1.v, VAnchor::Top);
        let x02 = find("+j0.2");
        assert!(close((x02.x, x02.y), z_to_gamma(0.0, 0.2), EPS));
        assert_eq!(x02.h, HAnchor::Right);
        let x5 = find("+j5.0");
        assert_eq!(x5.h, HAnchor::Left);
    }

    #[test]
    fn admittance_labels_are_mirrored() {
        let labels = grid_labels(SmithGrid::Admittance);
        let g2 = labels.iter().find(|l| l.text == "2.0").unwrap();
        assert!(close((g2.x, g2.y), (-1.0 / 3.0, 0.0), EPS));
        let b1 = labels.iter().find(|l| l.text == "+j1.0").unwrap();
        assert!(close((b1.x, b1.y), (0.0, -1.0), EPS));
    }

    /// Approximate Liberation Sans advance at `LABEL_FONT_PX`, generous so
    /// the overlap check errs on the safe side.
    fn text_w(s: &str) -> f64 {
        s.chars().count() as f64 * LABEL_FONT_PX as f64 * 0.62
    }

    #[test]
    fn labels_do_not_overlap_and_fit_the_view() {
        // From a 2×3 subplot grid up to a full-page chart.
        for side in [120.0, 150.0, 190.0, 220.0, 300.0, 480.0, 800.0] {
            let half = view_half_width(side);
            let scale = side / (2.0 * half);
            let to_px = |x: f64, y: f64| (side / 2.0 + x * scale, side / 2.0 - y * scale);
            for mode in [
                SmithGrid::Impedance,
                SmithGrid::Admittance,
                SmithGrid::Immittance,
            ] {
                let boxes: Vec<_> = grid_labels_at(mode, scale)
                    .iter()
                    .map(|l| {
                        (
                            l.text.clone(),
                            label_box(l, to_px, text_w(&l.text), LABEL_FONT_PX as f64),
                        )
                    })
                    .collect();
                for (t, b) in &boxes {
                    assert!(
                        b.0 >= 0.0 && b.1 >= 0.0 && b.2 <= side && b.3 <= side,
                        "side {side}: {t} {b:?} leaves the view"
                    );
                }
                for i in 0..boxes.len() {
                    for j in i + 1..boxes.len() {
                        let (a, b) = (&boxes[i].1, &boxes[j].1);
                        let overlap = a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3;
                        assert!(
                            !overlap,
                            "side {side}: {} {a:?} overlaps {} {b:?}",
                            boxes[i].0, boxes[j].0
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn colours_keep_contrast_in_every_theme() {
        for theme in [
            crate::theme::Theme::Mocha,
            crate::theme::Theme::Macchiato,
            crate::theme::Theme::Frappe,
            crate::theme::Theme::Latte,
        ] {
            let t = theme.colors();
            let bg = parse_hex_rgb(t.plot_bg.as_ref()).unwrap();
            let c = colors_for(&t);
            assert!(contrast_ratio(c.label, bg) >= 4.5, "{theme:?} label");
            assert!(contrast_ratio(c.frame, bg) >= 3.0, "{theme:?} frame");
            for g in [c.impedance, c.admittance] {
                let k = contrast_ratio(g, bg);
                assert!((1.5..3.0).contains(&k), "{theme:?} grid contrast {k}");
            }
        }
    }
}
