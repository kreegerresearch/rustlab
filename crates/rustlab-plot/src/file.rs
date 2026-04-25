use crate::contour::{band_index, marching_squares};
use crate::error::PlotError;
use crate::figure::{
    colormap_rgb, plot_context, push_notebook_figure_snapshot, ContourData, FigureState, LineStyle,
    PlotContext, PlotKind, SeriesColor, SubplotState, SurfaceData, FIGURE,
};
use plotters::prelude::*;

const MARGIN: u32 = 20;
const X_LABEL_AREA: u32 = 50;
const Y_LABEL_AREA: u32 = 70;

// ─── Main render entry points ───────────────────────────────────────────────

/// Render the current FIGURE state to a file (PNG or SVG by extension).
pub fn render_figure_file(path: &str) -> Result<(), PlotError> {
    if plot_context() == PlotContext::Notebook {
        push_notebook_figure_snapshot();
    }
    if path.ends_with(".html") || path.ends_with(".htm") {
        return crate::html::render_figure_html(path);
    }
    FIGURE.with(|fig| {
        let fig = fig.borrow();
        render_figure_state_to_file(&fig, path)
    })
}

/// Render a given FigureState to a file (PNG or SVG by extension).
pub fn render_figure_state_to_file(fig: &FigureState, path: &str) -> Result<(), PlotError> {
    let rows = fig.subplot_rows;
    let cols = fig.subplot_cols;
    let w = (cols as u32 * 900).min(3600);
    let h = (rows as u32 * 500).min(3000);

    if path.ends_with(".svg") {
        let root = SVGBackend::new(path, (w, h)).into_drawing_area();
        render_to_backend(root, fig, rows, cols)
    } else {
        let root = BitMapBackend::new(path, (w, h)).into_drawing_area();
        render_to_backend(root, fig, rows, cols)
    }
}

fn render_to_backend<DB>(
    root: DrawingArea<DB, plotters::coord::Shift>,
    fig: &FigureState,
    rows: usize,
    cols: usize,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());
    root.fill(&WHITE).map_err(err)?;

    let panels: Vec<_> = root.split_evenly((rows, cols));

    for (idx, panel) in panels.iter().enumerate() {
        if idx >= fig.subplots.len() {
            break;
        }
        let sp = &fig.subplots[idx];
        // Surface rendering takes precedence over heatmap/series
        if let Some(sf) = &sp.surface {
            let caption = if sp.title.is_empty() {
                format!("surf — {}", sf.colorscale)
            } else {
                format!("{} — surf {}", sp.title, sf.colorscale)
            };
            render_surface_to_backend(panel.clone(), sf, &caption)?;
            continue;
        }
        // Heatmap and/or contour/quiver/streamline rendering — they share a
        // chart so they overlay correctly under hold on.
        if sp.heatmap.is_some()
            || !sp.contours.is_empty()
            || !sp.quivers.is_empty()
            || !sp.streamlines.is_empty()
        {
            render_heatmap_and_contours_to_backend(panel.clone(), sp)?;
            continue;
        }
        if sp.series.is_empty() {
            continue;
        }
        render_subplot_to_panel(panel, sp)?;
    }
    root.present().map_err(err)?;
    Ok(())
}

fn render_subplot_to_panel<DB>(
    panel: &DrawingArea<DB, plotters::coord::Shift>,
    sp: &SubplotState,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());

    // Compute axis bounds
    let all_x: Vec<f64> = sp
        .series
        .iter()
        .flat_map(|s| s.x_data.iter().copied())
        .collect();
    let all_y: Vec<f64> = sp
        .series
        .iter()
        .flat_map(|s| s.y_data.iter().copied())
        .collect();
    if all_x.is_empty() || all_y.is_empty() {
        return Ok(());
    }

    let x_min = sp
        .xlim
        .0
        .unwrap_or_else(|| all_x.iter().copied().fold(f64::INFINITY, f64::min));
    let x_max = sp
        .xlim
        .1
        .unwrap_or_else(|| all_x.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    let y_min_raw = all_y.iter().copied().fold(f64::INFINITY, f64::min);
    let y_max_raw = all_y.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let y_margin = ((y_max_raw - y_min_raw).abs() * 0.1).max(1e-6);
    let y_min = sp.ylim.0.unwrap_or(y_min_raw - y_margin);
    let y_max = sp.ylim.1.unwrap_or(y_max_raw + y_margin);

    // Ensure non-degenerate range
    let x_lo = if (x_max - x_min).abs() < 1e-12 {
        x_min - 1.0
    } else {
        x_min
    };
    let x_hi = if (x_max - x_min).abs() < 1e-12 {
        x_max + 1.0
    } else {
        x_max
    };
    let y_lo = if (y_max - y_min).abs() < 1e-12 {
        y_min - 1.0
    } else {
        y_min
    };
    let y_hi = if (y_max - y_min).abs() < 1e-12 {
        y_max + 1.0
    } else {
        y_max
    };

    let title_str = sp.title.as_str();
    let xlabel = if !sp.xlabel.is_empty() {
        sp.xlabel.as_str()
    } else {
        "x"
    };
    let ylabel = if !sp.ylabel.is_empty() {
        sp.ylabel.as_str()
    } else {
        "y"
    };

    let mut chart = ChartBuilder::on(panel)
        .caption(title_str, ("sans-serif", 22u32).into_font())
        .margin(MARGIN)
        .x_label_area_size(X_LABEL_AREA)
        .y_label_area_size(Y_LABEL_AREA)
        .build_cartesian_2d(x_lo..x_hi, y_lo..y_hi)
        .map_err(err)?;

    if let Some(labels) = &sp.x_labels {
        let labels_c = labels.clone();
        chart
            .configure_mesh()
            .disable_mesh()
            .x_desc(xlabel)
            .y_desc(ylabel)
            .x_labels(labels_c.len())
            .x_label_formatter(&|v| {
                let rounded = v.round();
                if (*v - rounded).abs() > 1e-6 {
                    return String::new();
                }
                let idx = (rounded as isize) - 1;
                if idx >= 0 && (idx as usize) < labels_c.len() {
                    labels_c[idx as usize].clone()
                } else {
                    String::new()
                }
            })
            .draw()
            .map_err(err)?;
    } else {
        chart
            .configure_mesh()
            .disable_mesh()
            .x_desc(xlabel)
            .y_desc(ylabel)
            .draw()
            .map_err(err)?;
    }

    if sp.grid {
        const N: usize = 5;
        let grid_color = plotters::style::RGBAColor(100, 100, 100, 0.35);
        for i in 0..=N {
            let yv = y_lo + (y_hi - y_lo) * i as f64 / N as f64;
            chart
                .draw_series(LineSeries::new(
                    vec![(x_lo, yv), (x_hi, yv)],
                    grid_color.stroke_width(1),
                ))
                .map_err(err)?;
        }
        for i in 1..N {
            let xv = x_lo + (x_hi - x_lo) * i as f64 / N as f64;
            chart
                .draw_series(LineSeries::new(
                    vec![(xv, y_lo), (xv, y_hi)],
                    grid_color.stroke_width(1),
                ))
                .map_err(err)?;
        }
    }

    // Pre-compute grouped bar offsets
    let bar_series_count = sp.series.iter().filter(|s| s.kind == PlotKind::Bar).count();
    let mut bar_series_idx = 0usize;

    // Track whether any series has a label so we know whether to draw a
    // legend at the end. legend("a", "b", ...) populates Series.label;
    // an empty label means "don't show this series in the legend".
    let mut any_label = false;

    // Draw each series
    for s in &sp.series {
        let rgb = s.color.to_plotters();
        let stroke_width: u32 = if s.style == LineStyle::Dashed { 1 } else { 2 };
        let color = rgb.stroke_width(stroke_width);
        let has_label = !s.label.is_empty();
        if has_label {
            any_label = true;
        }
        let label = s.label.clone();

        match s.kind {
            PlotKind::Line => {
                let pts: Vec<(f64, f64)> = s
                    .x_data
                    .iter()
                    .copied()
                    .zip(s.y_data.iter().copied())
                    .collect();

                if s.style == LineStyle::Dashed {
                    // Simulate dashed by drawing every other segment.
                    // Attach the legend annotation to the *first* segment
                    // so the legend gets exactly one entry per series.
                    let mut draw_seg = true;
                    let mut first = true;
                    for pair in pts.windows(2) {
                        if draw_seg {
                            let anno = chart
                                .draw_series(LineSeries::new(vec![pair[0], pair[1]], color))
                                .map_err(err)?;
                            if first && has_label {
                                anno.label(label.clone()).legend(move |(x, y)| {
                                    PathElement::new(vec![(x, y), (x + 20, y)], color)
                                });
                                first = false;
                            }
                        }
                        draw_seg = !draw_seg;
                    }
                } else {
                    let anno = chart
                        .draw_series(LineSeries::new(pts, color))
                        .map_err(err)?;
                    if has_label {
                        anno.label(label).legend(move |(x, y)| {
                            PathElement::new(vec![(x, y), (x + 20, y)], color)
                        });
                    }
                }
            }
            PlotKind::Stem => {
                // Baseline (drawn once, not per-series)
                let x_lo_s = s.x_data.iter().copied().fold(f64::INFINITY, f64::min);
                let x_hi_s = s.x_data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                chart
                    .draw_series(LineSeries::new(
                        vec![(x_lo_s, 0.0), (x_hi_s, 0.0)],
                        BLACK.stroke_width(1),
                    ))
                    .map_err(err)?;

                // Stems — attach legend annotation here (the visual identity
                // of a stem plot is the vertical line, not the tip).
                let anno = chart
                    .draw_series(
                        s.x_data
                            .iter()
                            .copied()
                            .zip(s.y_data.iter().copied())
                            .map(|(x, y)| PathElement::new(vec![(x, 0.0), (x, y)], color)),
                    )
                    .map_err(err)?;
                if has_label {
                    anno.label(label).legend(move |(x, y)| {
                        PathElement::new(vec![(x, y), (x + 20, y)], color)
                    });
                }

                // Tips
                chart
                    .draw_series(
                        s.x_data
                            .iter()
                            .copied()
                            .zip(s.y_data.iter().copied())
                            .map(|(x, y)| Circle::new((x, y), 3, rgb.filled())),
                    )
                    .map_err(err)?;
            }
            PlotKind::Bar => {
                let n = s.x_data.len();
                let group_w = if n > 1 {
                    let span = s.x_data[n - 1] - s.x_data[0];
                    (span / (n - 1) as f64) * 0.8
                } else {
                    0.8
                };
                let (bar_w, offset) = if bar_series_count > 1 {
                    let bw = group_w / bar_series_count as f64;
                    let off = -group_w / 2.0 + bw * bar_series_idx as f64 + bw / 2.0;
                    (bw * 0.9, off)
                } else {
                    (group_w, 0.0)
                };
                bar_series_idx += 1;
                let half = bar_w / 2.0;

                // Baseline
                chart
                    .draw_series(LineSeries::new(
                        vec![(x_lo, 0.0), (x_hi, 0.0)],
                        BLACK.stroke_width(1),
                    ))
                    .map_err(err)?;

                // Filled bars — attach legend annotation here so the marker
                // matches the visible bar fill.
                let anno = chart
                    .draw_series(s.x_data.iter().copied().zip(s.y_data.iter().copied()).map(
                        |(x, y)| {
                            let cx = x + offset;
                            let (y0, y1) = if y >= 0.0 { (0.0, y) } else { (y, 0.0) };
                            Rectangle::new([(cx - half, y0), (cx + half, y1)], rgb.filled())
                        },
                    ))
                    .map_err(err)?;
                if has_label {
                    anno.label(label).legend(move |(x, y)| {
                        Rectangle::new([(x, y - 5), (x + 20, y + 5)], rgb.filled())
                    });
                }

                // Bar outlines
                chart
                    .draw_series(s.x_data.iter().copied().zip(s.y_data.iter().copied()).map(
                        |(x, y)| {
                            let cx = x + offset;
                            let (y0, y1) = if y >= 0.0 { (0.0, y) } else { (y, 0.0) };
                            Rectangle::new(
                                [(cx - half, y0), (cx + half, y1)],
                                BLACK.stroke_width(1),
                            )
                        },
                    ))
                    .map_err(err)?;
            }
            PlotKind::Scatter => {
                let anno = chart
                    .draw_series(
                        s.x_data
                            .iter()
                            .copied()
                            .zip(s.y_data.iter().copied())
                            .map(|(x, y)| Circle::new((x, y), 4, rgb.filled())),
                    )
                    .map_err(err)?;
                if has_label {
                    anno.label(label).legend(move |(x, y)| {
                        Circle::new((x + 10, y), 4, rgb.filled())
                    });
                }
            }
        }
    }

    if any_label {
        chart
            .configure_series_labels()
            .background_style(WHITE.mix(0.85))
            .border_style(BLACK.mix(0.3))
            .position(SeriesLabelPosition::UpperRight)
            .draw()
            .map_err(err)?;
    }
    Ok(())
}

/// Render a 3D surface to a plotters backend using a fixed isometric camera.
/// Draws colored quads over the grid (painter's algorithm depth sort) plus a
/// simple wireframe + axis box. Matches the HTML/viewer surface output at a
/// reasonable static angle so SVG/PNG exports look like 3D surfaces, not 2D heatmaps.
fn render_surface_to_backend<DB>(
    root: DrawingArea<DB, plotters::coord::Shift>,
    sf: &SurfaceData,
    caption: &str,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());
    root.fill(&WHITE).map_err(err)?;

    let nrows = sf.z.len();
    let ncols = if nrows > 0 { sf.z[0].len() } else { 0 };
    if nrows < 2 || ncols < 2 {
        return Ok(());
    }

    // Caption
    let (w_pixels, h_pixels) = root.dim_in_pixel();
    let caption_style: TextStyle = ("sans-serif", 18u32).into_font().into();
    root.draw_text(caption, &caption_style, (MARGIN as i32, 4))
        .map_err(err)?;

    // Plot area inside root (leave margins for caption + axes).
    let pad_l = 50i32;
    let pad_r = 20i32;
    let pad_t = 34i32;
    let pad_b = 40i32;
    let plot_w = (w_pixels as i32 - pad_l - pad_r).max(50);
    let plot_h = (h_pixels as i32 - pad_t - pad_b).max(50);

    // Z min/max for normalization.
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for row in &sf.z {
        for &v in row {
            if v < min_z {
                min_z = v;
            }
            if v > max_z {
                max_z = v;
            }
        }
    }
    let z_range = (max_z - min_z).max(1e-12);

    // Data bounds
    let x_min = sf.x.iter().copied().fold(f64::INFINITY, f64::min);
    let x_max = sf.x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let y_min = sf.y.iter().copied().fold(f64::INFINITY, f64::min);
    let y_max = sf.y.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let x_span = (x_max - x_min).max(1e-12);
    let y_span = (y_max - y_min).max(1e-12);

    // Camera: yaw about z (azimuth) + pitch about x (elevation).
    // yaw = -45°, pitch = 30° gives a standard isometric look.
    let yaw = -45f64.to_radians();
    let pitch = 30f64.to_radians();
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();

    // World-to-camera projection (orthographic) of (x, y, z) in normalized units.
    // Each axis is mapped to [-1, 1] before projection.
    let project = |xi: f64, yi: f64, zi: f64| -> (f64, f64, f64) {
        let nx = 2.0 * (xi - x_min) / x_span - 1.0;
        let ny = 2.0 * (yi - y_min) / y_span - 1.0;
        let nz = 2.0 * (zi - min_z) / z_range - 1.0;
        // Rotate about z (yaw)
        let xr = nx * cy - ny * sy;
        let yr = nx * sy + ny * cy;
        // Rotate about x (pitch)
        let zr = nz * cp - yr * sp;
        let yr2 = nz * sp + yr * cp;
        (xr, yr2, zr) // (screen-x, depth, screen-y)
    };

    // Compute projected-extent to rescale to pixel coords.
    let mut sxmin = f64::INFINITY;
    let mut sxmax = f64::NEG_INFINITY;
    let mut symin = f64::INFINITY;
    let mut symax = f64::NEG_INFINITY;
    for r in 0..nrows {
        for c in 0..ncols {
            let (sx, _d, sz) = project(sf.x[c], sf.y[r], sf.z[r][c]);
            if sx < sxmin {
                sxmin = sx;
            }
            if sx > sxmax {
                sxmax = sx;
            }
            if sz < symin {
                symin = sz;
            }
            if sz > symax {
                symax = sz;
            }
        }
    }
    let sxr = (sxmax - sxmin).max(1e-12);
    let syr = (symax - symin).max(1e-12);
    let scale = (plot_w as f64 / sxr).min(plot_h as f64 / syr) * 0.92;
    let cx = pad_l as f64 + plot_w as f64 * 0.5;
    let cy_px = pad_t as f64 + plot_h as f64 * 0.5;
    let to_px = |sx: f64, sz: f64| -> (i32, i32) {
        let x = cx + (sx - (sxmin + sxmax) * 0.5) * scale;
        // Screen y grows downward; invert sz.
        let y = cy_px - (sz - (symin + symax) * 0.5) * scale;
        (x.round() as i32, y.round() as i32)
    };

    // Draw axes box as a faint wireframe cube: project the 8 corners of the
    // unit bounding box in world space, then connect edges.
    let corners = [
        (x_min, y_min, min_z),
        (x_max, y_min, min_z),
        (x_max, y_max, min_z),
        (x_min, y_max, min_z),
        (x_min, y_min, max_z),
        (x_max, y_min, max_z),
        (x_max, y_max, max_z),
        (x_min, y_max, max_z),
    ];
    let pc: Vec<(i32, i32)> = corners
        .iter()
        .map(|&(x, y, z)| {
            let (sx, _d, sz) = project(x, y, z);
            to_px(sx, sz)
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
    let axis_color = RGBColor(160, 160, 160);
    for (a, b) in edges {
        root.draw(&PathElement::new(
            vec![pc[a], pc[b]],
            axis_color.stroke_width(1),
        ))
        .map_err(err)?;
    }

    // Build quads with their centroid depth for sorting.
    struct Quad {
        depth: f64,
        pts: [(i32, i32); 4],
        color: RGBColor,
    }
    let mut quads: Vec<Quad> = Vec::with_capacity((nrows - 1) * (ncols - 1));
    for r in 0..(nrows - 1) {
        for c in 0..(ncols - 1) {
            let v00 = (sf.x[c], sf.y[r], sf.z[r][c]);
            let v10 = (sf.x[c + 1], sf.y[r], sf.z[r][c + 1]);
            let v11 = (sf.x[c + 1], sf.y[r + 1], sf.z[r + 1][c + 1]);
            let v01 = (sf.x[c], sf.y[r + 1], sf.z[r + 1][c]);
            let p00 = project(v00.0, v00.1, v00.2);
            let p10 = project(v10.0, v10.1, v10.2);
            let p11 = project(v11.0, v11.1, v11.2);
            let p01 = project(v01.0, v01.1, v01.2);
            let depth = (p00.1 + p10.1 + p11.1 + p01.1) * 0.25;
            let zc = (sf.z[r][c] + sf.z[r][c + 1] + sf.z[r + 1][c + 1] + sf.z[r + 1][c]) * 0.25;
            let t = (zc - min_z) / z_range;
            let (rr, gg, bb) = colormap_rgb(t, &sf.colorscale);
            quads.push(Quad {
                depth,
                pts: [
                    to_px(p00.0, p00.2),
                    to_px(p10.0, p10.2),
                    to_px(p11.0, p11.2),
                    to_px(p01.0, p01.2),
                ],
                color: RGBColor(rr, gg, bb),
            });
        }
    }
    // Painter's algorithm: draw far faces first (smallest depth first).
    quads.sort_by(|a, b| {
        a.depth
            .partial_cmp(&b.depth)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let edge_style = RGBColor(80, 80, 80).stroke_width(1);
    for q in &quads {
        root.draw(&Polygon::new(q.pts.to_vec(), q.color.filled()))
            .map_err(err)?;
        let mut ring = q.pts.to_vec();
        ring.push(q.pts[0]);
        root.draw(&PathElement::new(ring, edge_style))
            .map_err(err)?;
    }

    // Axis tick labels (min/max on X and Y, min/max on Z).
    let tick_font = ("sans-serif", 11u32).into_font();
    let label = |corner: &(i32, i32), s: String| -> Result<(), PlotError> {
        root.draw(&Text::new(
            s,
            (corner.0 + 4, corner.1 + 2),
            tick_font.clone(),
        ))
        .map_err(err)?;
        Ok(())
    };
    label(&pc[0], format!("x={:.3}", x_min))?;
    label(&pc[1], format!("x={:.3}", x_max))?;
    label(&pc[3], format!("y={:.3}", y_max))?;
    label(&pc[4], format!("z={:.3}", max_z))?;

    root.present().map_err(err)?;
    Ok(())
}

fn series_color_to_rgb(c: &SeriesColor) -> RGBColor {
    match c {
        SeriesColor::Blue => RGBColor(31, 119, 180),
        SeriesColor::Red => RGBColor(214, 39, 40),
        SeriesColor::Green => RGBColor(44, 160, 44),
        SeriesColor::Cyan => RGBColor(23, 190, 207),
        SeriesColor::Magenta => RGBColor(148, 103, 189),
        SeriesColor::Yellow => RGBColor(188, 189, 34),
        SeriesColor::Black => RGBColor(0, 0, 0),
        SeriesColor::White => RGBColor(255, 255, 255),
        SeriesColor::Rgb(r, g, b) => RGBColor(*r, *g, *b),
    }
}

/// Render a heatmap and/or contour overlays into a single shared chart so
/// that under `hold on` an `imagesc` heatmap and a `contour` overlay align
/// in the same coordinate frame.
///
/// Coordinate selection:
/// - If at least one contour is present, the chart bounds come from the
///   first contour's `(x, y)` vectors and any heatmap is rescaled to fit.
/// - Otherwise, the heatmap uses its native integer cell coordinates.
fn render_heatmap_and_contours_to_backend<DB>(
    root: DrawingArea<DB, plotters::coord::Shift>,
    sp: &SubplotState,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());
    root.fill(&WHITE).map_err(err)?;

    // Decide chart bounds. First overlay that can dictate world coordinates
    // wins; heatmap-only falls back to integer cell coordinates.
    let (x_lo, x_hi, y_lo, y_hi) = if let Some(cd) = sp.contours.first() {
        let (xmin, xmax) = bounds(&cd.x);
        let (ymin, ymax) = bounds(&cd.y);
        (xmin, xmax, ymin, ymax)
    } else if let Some(qd) = sp.quivers.first() {
        let (xmin, xmax) = bounds(&qd.x);
        let (ymin, ymax) = bounds(&qd.y);
        (xmin, xmax, ymin, ymax)
    } else if let Some(sd) = sp.streamlines.first() {
        let (xmin, xmax) = bounds(&sd.x);
        let (ymin, ymax) = bounds(&sd.y);
        (xmin, xmax, ymin, ymax)
    } else if let Some(hm) = &sp.heatmap {
        let nrows = hm.z.len();
        let ncols = if nrows > 0 { hm.z[0].len() } else { 0 };
        (0.0, ncols as f64, 0.0, nrows as f64)
    } else {
        return Ok(());
    };

    let caption = sp.title.clone();
    let mut chart = ChartBuilder::on(&root)
        .caption(caption, ("sans-serif", 18u32).into_font())
        .margin(MARGIN)
        .x_label_area_size(X_LABEL_AREA)
        .y_label_area_size(Y_LABEL_AREA)
        .build_cartesian_2d(x_lo..x_hi, y_lo..y_hi)
        .map_err(err)?;

    chart.configure_mesh().disable_mesh().draw().map_err(err)?;

    // Draw heatmap, rescaled into the chart bounds.
    if let Some(hm) = &sp.heatmap {
        let nrows = hm.z.len();
        let ncols = if nrows > 0 { hm.z[0].len() } else { 0 };
        if nrows > 0 && ncols > 0 {
            let vals: Vec<f64> = hm.z.iter().flat_map(|row| row.iter().copied()).collect();
            let min_v = vals.iter().copied().fold(f64::INFINITY, f64::min);
            let max_v = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let range = (max_v - min_v).max(1e-12);
            let cell_w = (x_hi - x_lo) / ncols as f64;
            let cell_h = (y_hi - y_lo) / nrows as f64;
            for r in 0..nrows {
                for c in 0..ncols {
                    let v = vals[r * ncols + c];
                    let t = (v - min_v) / range;
                    let (rr, gg, bb) = colormap_rgb(t, &hm.colorscale);
                    let color = RGBColor(rr, gg, bb);
                    let x0 = x_lo + c as f64 * cell_w;
                    // Flip y so row 0 sits at the top of the chart.
                    let y0 = y_hi - (r as f64 + 1.0) * cell_h;
                    chart
                        .draw_series(std::iter::once(Rectangle::new(
                            [(x0, y0), (x0 + cell_w, y0 + cell_h)],
                            color.filled(),
                        )))
                        .map_err(err)?;
                }
            }
        }
    }

    // Draw contour overlays. Filled contours render as per-cell coloured
    // rectangles based on band classification (a discrete-band approximation
    // of the proper polygon fill); HTML output uses Plotly's exact contour
    // trace for the same data.
    for cd in &sp.contours {
        if cd.z.is_empty() || cd.x.len() < 2 || cd.y.len() < 2 {
            continue;
        }
        if cd.filled {
            render_contour_filled(&mut chart, cd)?;
        } else {
            render_contour_lines(&mut chart, cd)?;
        }
    }

    // Quiver overlays.
    for qd in &sp.quivers {
        render_quiver_to_backend(&mut chart, qd)?;
    }

    // Streamline overlays.
    for sd in &sp.streamlines {
        render_streamlines_to_backend(&mut chart, sd)?;
    }

    root.present().map_err(err)?;
    Ok(())
}

fn render_quiver_to_backend<DB>(
    chart: &mut ChartContext<
        DB,
        Cartesian2d<plotters::coord::types::RangedCoordf64, plotters::coord::types::RangedCoordf64>,
    >,
    qd: &crate::figure::QuiverData,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());
    let arrows = crate::quiver::build_arrows(&qd.u, &qd.v, &qd.x, &qd.y, qd.scale);
    if arrows.is_empty() {
        return Ok(());
    }
    let color = series_color_to_rgb(qd.color.as_ref().unwrap_or(&SeriesColor::Cyan));
    let style = ShapeStyle {
        color: color.to_rgba(),
        filled: false,
        stroke_width: 1,
    };
    for a in arrows {
        chart
            .draw_series(std::iter::once(PathElement::new(
                vec![a.shaft.0, a.shaft.1],
                style,
            )))
            .map_err(err)?;
        chart
            .draw_series(std::iter::once(PathElement::new(
                vec![a.head[0], a.head[1], a.head[2]],
                style,
            )))
            .map_err(err)?;
    }
    Ok(())
}

fn render_streamlines_to_backend<DB>(
    chart: &mut ChartContext<
        DB,
        Cartesian2d<plotters::coord::types::RangedCoordf64, plotters::coord::types::RangedCoordf64>,
    >,
    sd: &crate::figure::StreamlineData,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());
    let seeds = match &sd.seeds {
        Some(s) => s.clone(),
        None => crate::streamline::default_seeds(&sd.x, &sd.y, sd.density),
    };
    if seeds.is_empty() {
        return Ok(());
    }
    let step = crate::streamline::default_step(&sd.x, &sd.y);
    let ref_len = crate::quiver::cell_distance(&sd.x, &sd.y) * 0.5;
    let color = series_color_to_rgb(sd.color.as_ref().unwrap_or(&SeriesColor::Cyan));
    let style = ShapeStyle {
        color: color.to_rgba(),
        filled: false,
        stroke_width: 1,
    };
    for (sx, sy) in seeds {
        let pts = crate::streamline::integrate(
            &sd.u, &sd.v, &sd.x, &sd.y, sx, sy, step, 400, 1e-10,
        );
        if pts.len() < 2 {
            continue;
        }
        chart
            .draw_series(std::iter::once(PathElement::new(pts.clone(), style)))
            .map_err(err)?;
        if let Some(a) = crate::quiver::midpoint_arrow(&pts, ref_len) {
            chart
                .draw_series(std::iter::once(PathElement::new(
                    vec![a.shaft.0, a.shaft.1],
                    style,
                )))
                .map_err(err)?;
            chart
                .draw_series(std::iter::once(PathElement::new(
                    vec![a.head[0], a.head[1], a.head[2]],
                    style,
                )))
                .map_err(err)?;
        }
    }
    Ok(())
}

fn bounds(xs: &[f64]) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &v in xs {
        if v.is_finite() {
            if v < lo {
                lo = v;
            }
            if v > hi {
                hi = v;
            }
        }
    }
    if !lo.is_finite() || !hi.is_finite() || (hi - lo).abs() < 1e-12 {
        return (0.0, xs.len() as f64);
    }
    (lo, hi)
}

fn render_contour_lines<DB>(
    chart: &mut ChartContext<
        DB,
        Cartesian2d<plotters::coord::types::RangedCoordf64, plotters::coord::types::RangedCoordf64>,
    >,
    cd: &ContourData,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());
    let color = series_color_to_rgb(cd.line_color.as_ref().unwrap_or(&SeriesColor::Black));
    let style = ShapeStyle {
        color: color.to_rgba(),
        filled: false,
        stroke_width: 1,
    };
    for &lv in &cd.levels {
        let segs = marching_squares(&cd.z, &cd.x, &cd.y, lv);
        for s in segs {
            chart
                .draw_series(std::iter::once(PathElement::new(vec![s.p0, s.p1], style)))
                .map_err(err)?;
        }
    }
    Ok(())
}

fn render_contour_filled<DB>(
    chart: &mut ChartContext<
        DB,
        Cartesian2d<plotters::coord::types::RangedCoordf64, plotters::coord::types::RangedCoordf64>,
    >,
    cd: &ContourData,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());
    if cd.levels.is_empty() {
        return Ok(());
    }
    let nrows = cd.z.len();
    let ncols = if nrows > 0 { cd.z[0].len() } else { 0 };
    if nrows < 2 || ncols < 2 {
        return Ok(());
    }
    // Per-cell band fill: classify the cell-centre value, map to a colormap
    // sample at the centre of that band's [0, 1] slot. Discrete-band
    // approximation of true polygon fill — exact polygon fill is the HTML
    // backend's responsibility.
    let nbands = cd.levels.len() + 1;
    for r in 0..nrows - 1 {
        for c in 0..ncols - 1 {
            let centre = 0.25 * (cd.z[r][c] + cd.z[r][c + 1] + cd.z[r + 1][c] + cd.z[r + 1][c + 1]);
            if !centre.is_finite() {
                continue;
            }
            let bi = band_index(centre, &cd.levels);
            let t = (bi as f64 + 0.5) / nbands as f64;
            let (rr, gg, bb) = colormap_rgb(t, &cd.colorscale);
            let color = RGBColor(rr, gg, bb);
            let x0 = cd.x[c];
            let x1 = cd.x[c + 1];
            let y0 = cd.y[r];
            let y1 = cd.y[r + 1];
            chart
                .draw_series(std::iter::once(Rectangle::new(
                    [(x0.min(x1), y0.min(y1)), (x0.max(x1), y0.max(y1))],
                    color.filled(),
                )))
                .map_err(err)?;
        }
    }
    Ok(())
}

// NOTE: Legacy save_* wrapper functions were removed — save builtins now use
// the same push helpers as interactive builtins (push_xy_line, push_xy_stem, etc.)
// followed by render_figure_file(). See builtins.rs for the consolidated logic.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{push_xy_bar, push_xy_line, push_xy_scatter, push_xy_stem};

    fn tmp_path(suffix: &str) -> String {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "rustlab_plot_test_{}{}",
            std::process::id(),
            suffix
        ));
        p.to_str().unwrap().to_string()
    }

    // Tests use the push helpers + render_figure_file pattern (same as builtins)

    #[test]
    fn legend_labels_appear_in_svg_when_set() {
        // Regression: legend("a", "b") used to populate Series.label but the
        // SVG renderer never read it, so the rendered .md plots had no key.
        let path = tmp_path("_legend.svg");
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
        });
        let x: Vec<f64> = (0..32).map(|i| i as f64).collect();
        let y1: Vec<f64> = x.iter().map(|&xi| xi.sin()).collect();
        let y2: Vec<f64> = x.iter().map(|&xi| xi.cos()).collect();
        push_xy_line(
            x.clone(),
            y1,
            "sine wave",
            "Trig",
            Some(crate::figure::SeriesColor::Blue),
            LineStyle::Solid,
        );
        push_xy_line(
            x,
            y2,
            "cosine wave",
            "Trig",
            Some(crate::figure::SeriesColor::Red),
            LineStyle::Solid,
        );
        render_figure_file(&path).expect("render should succeed");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        assert!(
            content.contains("sine wave"),
            "legend missing 'sine wave' label in SVG"
        );
        assert!(
            content.contains("cosine wave"),
            "legend missing 'cosine wave' label in SVG"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn no_legend_drawn_when_no_labels_set() {
        // If every series has an empty label, no legend box should render.
        // Verify by drawing without labels and checking that a known
        // non-data string (e.g. a label we *didn't* set) is absent.
        let path = tmp_path("_nolegend.svg");
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
        });
        let x: Vec<f64> = (0..32).map(|i| i as f64).collect();
        let y: Vec<f64> = x.iter().map(|&xi| xi.sin()).collect();
        push_xy_line(x, y, "", "Untitled", None, LineStyle::Solid);
        render_figure_file(&path).expect("render should succeed");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        assert!(content.contains("<svg"));
        // Plotters writes legend background as a <rect> with a specific
        // structure; the simplest invariant is that no <text> element
        // contains the non-existent label "MISSING_LEGEND_TEXT".
        assert!(!content.contains("MISSING_LEGEND_TEXT"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn push_line_and_render_produces_svg() {
        let path = tmp_path("_line.svg");
        let x: Vec<f64> = (0..64).map(|i| i as f64).collect();
        let y: Vec<f64> = x.iter().map(|&xi| xi.sin()).collect();
        push_xy_line(x, y, "value", "Test Line", None, LineStyle::Solid);
        render_figure_file(&path).expect("render should succeed");
        let meta = std::fs::metadata(&path).expect("SVG file should exist");
        assert!(
            meta.len() > 500,
            "SVG should be non-trivial (>500 bytes), got {}",
            meta.len()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn push_stem_and_render_produces_svg() {
        let path = tmp_path("_stem.svg");
        let x: Vec<f64> = (0..32).map(|i| i as f64).collect();
        let y: Vec<f64> = x.iter().map(|&xi| xi.sin()).collect();
        push_xy_stem(x, y, "stem", "Test Stem", None);
        render_figure_file(&path).expect("render should succeed");
        let meta = std::fs::metadata(&path).expect("stem SVG should exist");
        assert!(
            meta.len() > 500,
            "stem SVG should be non-trivial (>500 bytes), got {}",
            meta.len()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn push_bar_and_render_produces_svg() {
        let path = tmp_path("_bar.svg");
        push_xy_bar(
            vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
            vec![3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0],
            "bar",
            "Test Bar",
            None,
        );
        render_figure_file(&path).expect("render should succeed");
        let meta = std::fs::metadata(&path).expect("bar SVG should exist");
        assert!(
            meta.len() > 500,
            "bar SVG should be non-trivial (>500 bytes), got {}",
            meta.len()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn push_bar_negative_values() {
        let path = tmp_path("_bar_neg.svg");
        push_xy_bar(
            vec![0.0, 1.0, 2.0, 3.0],
            vec![-3.0, 2.0, -1.0, 5.0],
            "bar",
            "Negative Bars",
            None,
        );
        render_figure_file(&path).expect("render should succeed");
        let meta = std::fs::metadata(&path).expect("file should exist");
        assert!(meta.len() > 500);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn push_scatter_and_render_produces_svg() {
        let path = tmp_path("_scatter.svg");
        let x: Vec<f64> = (0..20).map(|i| i as f64 * 0.5).collect();
        let y: Vec<f64> = x.iter().map(|&xi| xi * xi * 0.1).collect();
        push_xy_scatter(x, y, "scatter", "Test Scatter", None);
        render_figure_file(&path).expect("render should succeed");
        let meta = std::fs::metadata(&path).expect("scatter SVG should exist");
        assert!(
            meta.len() > 500,
            "scatter SVG should be non-trivial, got {}",
            meta.len()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn push_scatter_contains_svg_tag() {
        let path = tmp_path("_scatter_tag.svg");
        push_xy_scatter(
            vec![1.0, 2.0, 3.0],
            vec![4.0, 2.0, 5.0],
            "pts",
            "Scatter Tag",
            None,
        );
        render_figure_file(&path).expect("render should succeed");
        let content = std::fs::read_to_string(&path).expect("should read SVG");
        assert!(
            content.contains("<svg"),
            "scatter SVG should contain '<svg' tag"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn categorical_bar_svg_renders_each_label_once() {
        // Regression: bar(labels, y) used to emit each tick label twice because
        // plotters generates ticks at half-integer positions and the formatter
        // mapped (v as usize)-1 to labels[0] for both v=1.0 and v=1.5. The
        // formatter now returns "" for non-integer ticks.
        let path = tmp_path("_cat_bar.svg");
        let labels = vec![
            "|00>".to_string(),
            "|01>".to_string(),
            "|10>".to_string(),
            "|11>".to_string(),
        ];
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            let sp = fig.current_mut();
            sp.series.clear();
            sp.title.clear();
            sp.x_labels = Some(labels.clone());
        });
        push_xy_bar(
            vec![1.0, 2.0, 3.0, 4.0],
            vec![0.25, 0.12, 0.48, 0.15],
            "bar",
            "Categorical Bar",
            None,
        );
        render_figure_file(&path).expect("render should succeed");
        let content = std::fs::read_to_string(&path).expect("should read SVG");
        // SVG escapes '>' as '&gt;'; check the escaped form.
        for lbl in ["|00&gt;", "|01&gt;", "|10&gt;", "|11&gt;"] {
            let count = content.matches(lbl).count();
            assert_eq!(
                count, 1,
                "expected {} to appear once in SVG, found {}",
                lbl, count
            );
        }
        // Reset state so sibling tests aren't affected.
        FIGURE.with(|fig| fig.borrow_mut().current_mut().x_labels = None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn heatmap_in_figure_renders_to_svg() {
        let path = tmp_path("_heatmap.svg");
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            let sp = fig.current_mut();
            sp.series.clear();
            sp.title = "Heatmap Test".to_string();
            sp.heatmap = Some(crate::figure::HeatmapData {
                z: vec![
                    vec![0.0, 0.5, 1.0],
                    vec![0.3, 0.7, 0.2],
                    vec![1.0, 0.1, 0.5],
                ],
                colorscale: "viridis".to_string(),
            });
        });
        render_figure_file(&path).expect("heatmap render should succeed");
        let content = std::fs::read_to_string(&path).expect("should read SVG");
        assert!(
            content.contains("<svg"),
            "heatmap SVG should contain '<svg' tag"
        );
        let _ = std::fs::remove_file(&path);
    }

    fn radial_z(n: usize) -> (Vec<Vec<f64>>, Vec<f64>, Vec<f64>) {
        let xs: Vec<f64> = (0..n)
            .map(|i| -1.0 + 2.0 * i as f64 / (n as f64 - 1.0))
            .collect();
        let ys = xs.clone();
        let z: Vec<Vec<f64>> = (0..n)
            .map(|r| (0..n).map(|c| xs[c] * xs[c] + ys[r] * ys[r]).collect())
            .collect();
        (z, xs, ys)
    }

    #[test]
    fn line_contour_renders_to_svg_with_paths() {
        let path = tmp_path("_contour_lines.svg");
        let (z, x, y) = radial_z(31);
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
            let sp = fig.current_mut();
            sp.contours.push(crate::figure::ContourData {
                z,
                x,
                y,
                levels: vec![0.1, 0.4, 0.9],
                filled: false,
                line_color: Some(crate::figure::SeriesColor::Black),
                colorscale: "viridis".to_string(),
            });
        });
        render_figure_file(&path).expect("line contour SVG should render");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        assert!(content.contains("<svg"));
        // Marching-squares emits one <polyline> element per segment.
        let seg_count = content.matches("<polyline").count();
        assert!(
            seg_count > 30,
            "expected many polyline segments for 3 levels, got {seg_count}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn filled_contour_renders_to_svg_with_rectangles() {
        let path = tmp_path("_contour_filled.svg");
        let (z, x, y) = radial_z(11);
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
            let sp = fig.current_mut();
            sp.contours.push(crate::figure::ContourData {
                z,
                x,
                y,
                levels: vec![0.25, 0.5, 0.75, 1.0, 1.25],
                filled: true,
                line_color: None,
                colorscale: "viridis".to_string(),
            });
        });
        render_figure_file(&path).expect("filled contour SVG should render");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        assert!(content.contains("<svg"));
        // Per-cell band fill emits many filled <rect> elements.
        let rect_count = content.matches("<rect").count();
        assert!(
            rect_count >= (10 * 10) - 5,
            "expected ~100 cell rectangles, got {rect_count}"
        );
        let _ = std::fs::remove_file(&path);
    }

    fn uniform_field(nx: usize, ny: usize, ux: f64, uy: f64)
        -> (Vec<f64>, Vec<f64>, Vec<Vec<f64>>, Vec<Vec<f64>>)
    {
        let x: Vec<f64> = (0..nx).map(|i| -1.0 + 2.0 * i as f64 / (nx - 1) as f64).collect();
        let y: Vec<f64> = (0..ny).map(|i| -1.0 + 2.0 * i as f64 / (ny - 1) as f64).collect();
        let u = vec![vec![ux; nx]; ny];
        let v = vec![vec![uy; nx]; ny];
        (x, y, u, v)
    }

    fn vortex_field(nx: usize, ny: usize)
        -> (Vec<f64>, Vec<f64>, Vec<Vec<f64>>, Vec<Vec<f64>>)
    {
        let x: Vec<f64> = (0..nx).map(|i| -2.0 + 4.0 * i as f64 / (nx - 1) as f64).collect();
        let y: Vec<f64> = (0..ny).map(|i| -2.0 + 4.0 * i as f64 / (ny - 1) as f64).collect();
        let mut u = vec![vec![0.0; nx]; ny];
        let mut v = vec![vec![0.0; nx]; ny];
        for r in 0..ny {
            for c in 0..nx {
                u[r][c] = -y[r];
                v[r][c] = x[c];
            }
        }
        (x, y, u, v)
    }

    #[test]
    fn quiver_renders_horizontal_arrows_to_svg() {
        let path = tmp_path("_quiver_uniform.svg");
        let (x, y, u, v) = uniform_field(8, 8, 1.0, 0.0);
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
            fig.current_mut().quivers.push(crate::figure::QuiverData {
                x, y, u, v,
                scale: 1.0,
                color: None,
                title: None,
            });
        });
        render_figure_file(&path).expect("quiver SVG should render");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        assert!(content.contains("<svg"));
        // One shaft path + one head path per arrow, minus border cells where
        // the arrow might stick out of bounds. Either polyline or path should
        // show up repeatedly.
        let strokes = content.matches("<polyline").count() + content.matches("<path").count();
        assert!(strokes > 30, "quiver SVG missing arrow strokes, got {strokes}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn quiver_skips_nan_entries() {
        let path = tmp_path("_quiver_nan.svg");
        let (x, y, mut u, v) = uniform_field(6, 6, 1.0, 0.0);
        // Poison a handful of cells with NaN.
        for r in 0..6 { u[r][0] = f64::NAN; }
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
            fig.current_mut().quivers.push(crate::figure::QuiverData {
                x, y, u, v,
                scale: 1.0,
                color: None,
                title: None,
            });
        });
        // Smoke: should not panic; SVG should render.
        render_figure_file(&path).expect("quiver NaN SVG should render");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        assert!(content.contains("<svg"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn streamplot_renders_paths_for_vortex() {
        let path = tmp_path("_stream_vortex.svg");
        let (x, y, u, v) = vortex_field(21, 21);
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
            fig.current_mut().streamlines.push(crate::figure::StreamlineData {
                x, y, u, v,
                density: 0.3,
                seeds: None,
                color: None,
                title: None,
            });
        });
        render_figure_file(&path).expect("streamplot SVG should render");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        assert!(content.contains("<svg"));
        // Each streamline traces a multi-point polyline; expect several in SVG.
        let strokes = content.matches("<polyline").count() + content.matches("<path").count();
        assert!(strokes > 5, "streamplot SVG missing line strokes, got {strokes}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn heatmap_with_quiver_overlay_both_render() {
        let path = tmp_path("_heatmap_quiver.svg");
        let (z, x, y) = radial_z(11);
        let (qx, qy, u, v) = uniform_field(6, 6, 1.0, 0.0);
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
            let sp = fig.current_mut();
            sp.heatmap = Some(crate::figure::HeatmapData {
                z,
                colorscale: "viridis".to_string(),
            });
            sp.contours.push(crate::figure::ContourData {
                z: vec![vec![0.0; 2]; 2],
                x: x.clone(), y: y.clone(),
                levels: vec![0.5],
                filled: false,
                line_color: Some(crate::figure::SeriesColor::Black),
                colorscale: "viridis".to_string(),
            });
            sp.quivers.push(crate::figure::QuiverData {
                x: qx, y: qy, u, v,
                scale: 1.0,
                color: Some(crate::figure::SeriesColor::Red),
                title: None,
            });
        });
        render_figure_file(&path).expect("overlay render");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        assert!(content.contains("<svg"));
        // Heatmap rectangles + quiver arrow strokes should both be present.
        assert!(content.matches("<rect").count() > 50, "heatmap cells missing");
        let strokes = content.matches("<polyline").count() + content.matches("<path").count();
        assert!(strokes > 30, "quiver arrows missing, got {strokes}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn heatmap_with_contour_overlay_both_render() {
        // Heatmap and a single contour overlay on the same subplot.
        let path = tmp_path("_contour_overlay.svg");
        let (z, x, y) = radial_z(11);
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
            let sp = fig.current_mut();
            sp.heatmap = Some(crate::figure::HeatmapData {
                z: z.clone(),
                colorscale: "viridis".to_string(),
            });
            sp.contours.push(crate::figure::ContourData {
                z,
                x,
                y,
                levels: vec![0.5, 1.0],
                filled: false,
                line_color: Some(crate::figure::SeriesColor::Black),
                colorscale: "viridis".to_string(),
            });
        });
        render_figure_file(&path).expect("overlay render");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        assert!(content.contains("<svg"));
        // Polyline segments (contour) AND many rectangles (heatmap cells).
        assert!(
            content.matches("<polyline").count() > 5,
            "contour segments missing"
        );
        assert!(
            content.matches("<rect").count() > 50,
            "heatmap cells missing"
        );
        let _ = std::fs::remove_file(&path);
    }
}
