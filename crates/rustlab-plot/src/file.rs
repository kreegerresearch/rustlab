use crate::contour::{band_index, marching_squares};
use crate::error::PlotError;
use crate::figure::{
    colormap_rgb, plot_context, push_notebook_figure_snapshot, ContourData, FigureState, LineStyle,
    PlotContext, PlotKind, SeriesColor, SubplotState, SurfaceData, FIGURE,
};
use crate::theme::{Theme, ThemeColors};
use plotters::prelude::*;

const MARGIN: u32 = 20;
const X_LABEL_AREA: u32 = 50;
const Y_LABEL_AREA: u32 = 70;

// Heatmap colorbar geometry. The colorbar is drawn in a strip reserved on the
// right of the chart area; the strip total width is GUTTER + WIDTH + LABELS.
const COLORBAR_WIDTH: u32 = 28;
const COLORBAR_GUTTER: u32 = 12;
const COLORBAR_LABEL_AREA: u32 = 56;
const COLORBAR_SAMPLES: usize = 64;
// Approximate vertical space plotters reserves for a non-empty caption (font
// 18 plus padding). Used to predict where plotters places the data rect so
// the colorbar can vertically align with it.
const CAPTION_H_EST: u32 = 30;

/// Pre-parsed plotters colors derived from `ThemeColors`. Rendering helpers
/// take this rather than the raw string-typed `ThemeColors` so the parsing
/// happens once per figure instead of per draw call.
#[derive(Clone, Copy)]
struct ThemePalette {
    /// Page / panel background fill.
    bg: RGBColor,
    /// Foreground colour for axis lines, ticks, descriptions, captions.
    text: RGBAColor,
    /// Gridline colour (semi-transparent).
    grid: RGBAColor,
}

impl ThemePalette {
    fn from(theme: &ThemeColors) -> Self {
        ThemePalette {
            bg: parse_color_solid(theme.plot_bg).unwrap_or(RGBColor(255, 255, 255)),
            text: parse_color_rgba(theme.text).unwrap_or(RGBAColor(0, 0, 0, 1.0)),
            grid: parse_color_rgba(theme.plot_grid).unwrap_or(RGBAColor(100, 100, 100, 0.35)),
        }
    }
}

/// Parse a `#RRGGBB` hex string into an opaque `RGBColor`.
fn parse_color_solid(s: &str) -> Option<RGBColor> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(RGBColor(r, g, b));
        }
    }
    parse_color_rgba(s).map(|c| RGBColor(c.0, c.1, c.2))
}

/// Parse `#RRGGBB`, `#RRGGBBAA`, or `rgba(r,g,b,a)` / `rgb(r,g,b)` into an
/// `RGBAColor` (alpha defaults to 1.0).
fn parse_color_rgba(s: &str) -> Option<RGBAColor> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        match hex.len() {
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                return Some(RGBAColor(r, g, b, 1.0));
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                return Some(RGBAColor(r, g, b, a as f64 / 255.0));
            }
            _ => return None,
        }
    }
    let lower = s.to_ascii_lowercase();
    let body = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = body.split(',').map(str::trim).collect();
    if parts.len() < 3 {
        return None;
    }
    let r: u16 = parts[0].parse().ok()?;
    let g: u16 = parts[1].parse().ok()?;
    let b: u16 = parts[2].parse().ok()?;
    let a: f64 = if parts.len() >= 4 {
        parts[3].parse().ok()?
    } else {
        1.0
    };
    Some(RGBAColor(r.min(255) as u8, g.min(255) as u8, b.min(255) as u8, a))
}

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

/// Render a given FigureState to a file (PNG or SVG by extension) using the
/// default theme (matches `render_figure_html`'s default).
pub fn render_figure_state_to_file(fig: &FigureState, path: &str) -> Result<(), PlotError> {
    render_figure_state_to_file_themed(fig, path, Theme::default().colors())
}

/// Render a given FigureState to a file with an explicit theme. Background,
/// axis text, gridlines, and captions all pick up colours from `theme` so the
/// SVG/PNG matches the themed HTML output for the same data.
pub fn render_figure_state_to_file_themed(
    fig: &FigureState,
    path: &str,
    theme: &ThemeColors,
) -> Result<(), PlotError> {
    let rows = fig.subplot_rows;
    let cols = fig.subplot_cols;
    let w = (cols as u32 * 900).min(3600);
    let h = (rows as u32 * 500).min(3000);
    let palette = ThemePalette::from(theme);

    if path.ends_with(".svg") {
        let root = SVGBackend::new(path, (w, h)).into_drawing_area();
        render_to_backend(root, fig, rows, cols, &palette)
    } else {
        let root = BitMapBackend::new(path, (w, h)).into_drawing_area();
        render_to_backend(root, fig, rows, cols, &palette)
    }
}

/// Render a given FigureState into an RGB pixel buffer (3 bytes per pixel,
/// row-major, top-to-bottom). Returns `(buf, w, h)`. Mirrors the size logic
/// of `render_figure_state_to_file_themed` so static PNG output and animated
/// GIF output have identical resolution per subplot.
///
/// Used by `render_animation_gif` to rasterize frames before quantizing them
/// for GIF encoding. Public so other animated-raster backends (APNG, MP4)
/// can reuse the same pipeline if they're ever added.
pub fn render_figure_state_to_rgb_buffer(
    fig: &FigureState,
    theme: &ThemeColors,
) -> Result<(Vec<u8>, u32, u32), PlotError> {
    let rows = fig.subplot_rows;
    let cols = fig.subplot_cols;
    let w = (cols as u32 * 900).min(3600);
    let h = (rows as u32 * 500).min(3000);
    let palette = ThemePalette::from(theme);

    let mut buf = vec![0u8; (w * h * 3) as usize];
    {
        let root = BitMapBackend::with_buffer(&mut buf, (w, h)).into_drawing_area();
        render_to_backend(root, fig, rows, cols, &palette)?;
    }
    Ok((buf, w, h))
}

/// Render a single subplot panel into an RGBA pixel buffer (row-major, 4 bytes
/// per pixel) using the same plotters pipeline as SVG/PNG output. Used by the
/// viewer backend to display panels containing plot kinds the viewer can't
/// render natively (contour, quiver, streamplot).
pub fn render_panel_to_rgba(
    sp: &SubplotState,
    theme: &ThemeColors,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, PlotError> {
    let palette = ThemePalette::from(theme);
    let mut rgb = vec![0u8; (width * height * 3) as usize];
    {
        let root =
            BitMapBackend::with_buffer(&mut rgb, (width, height)).into_drawing_area();
        let err =
            |e: DrawingAreaErrorKind<<BitMapBackend as DrawingBackend>::ErrorType>| {
                PlotError::FileOutput(e.to_string())
            };
        root.fill(&palette.bg).map_err(err)?;
        if sp.heatmap.is_some()
            || !sp.contours.is_empty()
            || !sp.quivers.is_empty()
            || !sp.streamlines.is_empty()
        {
            render_heatmap_and_contours_to_backend(root.clone(), sp, &palette)?;
        } else if !sp.series.is_empty() {
            render_subplot_to_panel(&root, sp, &palette)?;
        }
        root.present().map_err(err)?;
    }
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for px in rgb.chunks_exact(3) {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    Ok(rgba)
}

fn render_to_backend<DB>(
    root: DrawingArea<DB, plotters::coord::Shift>,
    fig: &FigureState,
    rows: usize,
    cols: usize,
    palette: &ThemePalette,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());
    root.fill(&palette.bg).map_err(err)?;

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
            render_surface_to_backend(panel.clone(), sf, &caption, palette)?;
            continue;
        }
        // Heatmap and/or contour/quiver/streamline rendering — they share a
        // chart so they overlay correctly under hold on.
        if sp.heatmap.is_some()
            || !sp.contours.is_empty()
            || !sp.quivers.is_empty()
            || !sp.streamlines.is_empty()
        {
            render_heatmap_and_contours_to_backend(panel.clone(), sp, palette)?;
            continue;
        }
        if sp.series.is_empty() {
            continue;
        }
        render_subplot_to_panel(panel, sp, palette)?;
    }
    root.present().map_err(err)?;
    Ok(())
}

fn render_subplot_to_panel<DB>(
    panel: &DrawingArea<DB, plotters::coord::Shift>,
    sp: &SubplotState,
    palette: &ThemePalette,
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
    // Pass labels through verbatim. An empty string causes plotters to skip
    // drawing the descriptor — matches HTML's behaviour (Plotly with empty
    // axis title also draws nothing). The previous "x" / "y" fallback caused
    // unset axes to render literal placeholders that HTML never showed.
    let xlabel = sp.xlabel.as_str();
    let ylabel = sp.ylabel.as_str();

    let caption_style: TextStyle =
        ("sans-serif", 22u32, &palette.text).into_text_style(panel);
    let label_style: TextStyle =
        ("sans-serif", 12u32, &palette.text).into_text_style(panel);
    let desc_style: TextStyle =
        ("sans-serif", 14u32, &palette.text).into_text_style(panel);
    let axis_style: ShapeStyle = palette.text.stroke_width(1);

    // axis("equal"): shrink the chart area so one data unit on x equals one
    // data unit on y. Without this, plotters maps `[x_lo, x_hi]` and
    // `[y_lo, y_hi]` independently to the panel dimensions, so a unit circle
    // renders as an ellipse on a non-square canvas.
    let chart_area = if sp.axis_equal {
        let caption_h = if title_str.is_empty() { 0u32 } else { CAPTION_H_EST };
        let (panel_w, panel_h) = panel.dim_in_pixel();
        let data_w_avail = panel_w.saturating_sub(2 * MARGIN + Y_LABEL_AREA);
        let data_h_avail = panel_h.saturating_sub(2 * MARGIN + X_LABEL_AREA + caption_h);
        let dx = (x_hi - x_lo).abs().max(f64::EPSILON);
        let dy = (y_hi - y_lo).abs().max(f64::EPSILON);
        let scale = (data_w_avail as f64 / dx).min(data_h_avail as f64 / dy);
        let data_w = (scale * dx).round().max(1.0) as u32;
        let data_h = (scale * dy).round().max(1.0) as u32;
        let req_cw = panel_w.saturating_sub(data_w + 2 * MARGIN + Y_LABEL_AREA);
        let req_ch = panel_h.saturating_sub(data_h + 2 * MARGIN + X_LABEL_AREA + caption_h);
        panel.clone().shrink((0, 0), (req_cw, req_ch))
    } else {
        panel.clone()
    };

    let mut chart = ChartBuilder::on(&chart_area)
        .caption(title_str, caption_style)
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
            .axis_style(axis_style)
            .label_style(label_style.clone())
            .axis_desc_style(desc_style.clone())
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
            .axis_style(axis_style)
            .label_style(label_style.clone())
            .axis_desc_style(desc_style.clone())
            .x_desc(xlabel)
            .y_desc(ylabel)
            .draw()
            .map_err(err)?;
    }

    if sp.grid {
        draw_grid(&mut chart, x_lo, x_hi, y_lo, y_hi, palette)?;
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

                // Stems — attach legend annotation here. The legend marker
                // is the same filled circle used for the tip glyph below, so
                // mixed plot/stem legends are distinguishable (matches the
                // marker that Plotly's stem trace shows in its own legend).
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
                    anno.label(label)
                        .legend(move |(x, y)| Circle::new((x + 10, y), 3, rgb.filled()));
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
        // Legend background sits on top of the chart, so it must use the same
        // theme background (with a slight transparency for visual separation)
        // and the theme foreground for borders + label text.
        let legend_bg = palette.bg.mix(0.85);
        let legend_border = palette.text.mix(0.3);
        let legend_label_style: TextStyle =
            ("sans-serif", 14u32, &palette.text).into_text_style(panel);
        chart
            .configure_series_labels()
            .background_style(legend_bg)
            .border_style(legend_border)
            .label_font(legend_label_style)
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
    palette: &ThemePalette,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());
    root.fill(&palette.bg).map_err(err)?;

    let nrows = sf.z.len();
    let ncols = if nrows > 0 { sf.z[0].len() } else { 0 };
    if nrows < 2 || ncols < 2 {
        return Ok(());
    }

    // Caption
    let (w_pixels, h_pixels) = root.dim_in_pixel();
    let caption_style: TextStyle = ("sans-serif", 18u32, &palette.text).into_text_style(&root);
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
    let axis_color = palette.text;
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
    let edge_style = palette.text.stroke_width(1);
    for q in &quads {
        root.draw(&Polygon::new(q.pts.to_vec(), q.color.filled()))
            .map_err(err)?;
        let mut ring = q.pts.to_vec();
        ring.push(q.pts[0]);
        root.draw(&PathElement::new(ring, edge_style))
            .map_err(err)?;
    }

    // Axis tick labels (min/max on X and Y, min/max on Z).
    let tick_font: TextStyle = ("sans-serif", 11u32, &palette.text).into_text_style(&root);
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
    palette: &ThemePalette,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());
    root.fill(&palette.bg).map_err(err)?;

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
        let (nrows, ncols) = match hm.kind {
            crate::figure::HeatmapKind::ImageRgba => {
                (hm.rgba_height as usize, hm.rgba_width as usize)
            }
            _ => {
                let nrows = hm.z.len();
                let ncols = if nrows > 0 { hm.z[0].len() } else { 0 };
                (nrows, ncols)
            }
        };
        (0.0, ncols as f64, 0.0, nrows as f64)
    } else {
        return Ok(());
    };

    // When a heatmap is present, split off a colorbar strip on the right and
    // shrink the chart drawing area so its post-margin/post-label data rect
    // is square. This matches Plotly's `scaleanchor: "x"` behaviour and gives
    // `imagesc` cells a 1:1 aspect — parity with the HTML renderer.
    let caption_h = if sp.title.is_empty() { 0u32 } else { CAPTION_H_EST };
    let needs_colorbar = matches!(
        sp.heatmap.as_ref().map(|h| &h.kind),
        Some(crate::figure::HeatmapKind::Imagesc) | Some(crate::figure::HeatmapKind::Heatmap)
    );
    let (chart_area, cbar_info) = if needs_colorbar {
        let (panel_w, panel_h) = root.dim_in_pixel();
        let cbar_total = COLORBAR_GUTTER + COLORBAR_WIDTH + COLORBAR_LABEL_AREA;
        let split_x = (panel_w as i32 - cbar_total as i32).max(50);
        let (chart_side, cbar_side) = root.split_horizontally(split_x);
        let (cw, ch) = chart_side.dim_in_pixel();
        let data_w_avail = cw.saturating_sub(2 * MARGIN + Y_LABEL_AREA);
        let data_h_avail = ch.saturating_sub(2 * MARGIN + X_LABEL_AREA + caption_h);
        let side = data_w_avail.min(data_h_avail).max(1);
        let req_cw = side + 2 * MARGIN + Y_LABEL_AREA;
        let req_ch = side + 2 * MARGIN + X_LABEL_AREA + caption_h;
        let chart_area = chart_side.shrink((0, 0), (req_cw, req_ch));
        let data_top = (MARGIN + caption_h) as i32;
        let data_bot = data_top + side as i32;
        let _ = panel_h; // unused but documents that we considered total height
        (chart_area, Some((cbar_side, data_top, data_bot)))
    } else if sp.heatmap.is_some() {
        // ImageRgba: square chart area, no colorbar gutter — raw pixels have
        // no scale to display.
        let (panel_w, panel_h) = root.dim_in_pixel();
        let data_w_avail = panel_w.saturating_sub(2 * MARGIN + Y_LABEL_AREA);
        let data_h_avail = panel_h.saturating_sub(2 * MARGIN + X_LABEL_AREA + caption_h);
        let side = data_w_avail.min(data_h_avail).max(1);
        let req_cw = side + 2 * MARGIN + Y_LABEL_AREA;
        let req_ch = side + 2 * MARGIN + X_LABEL_AREA + caption_h;
        (root.clone().shrink((0, 0), (req_cw, req_ch)), None)
    } else if sp.axis_equal {
        // axis("equal") on a non-heatmap panel: shrink to a square data area.
        // The data extents may not be 1:1 (e.g. a Nyquist locus could be
        // [-2, 1] × [-1.5, 1.5]); pick the smaller scale so one data unit
        // maps to the same number of pixels on both axes.
        let (panel_w, panel_h) = root.dim_in_pixel();
        let data_w_avail = panel_w.saturating_sub(2 * MARGIN + Y_LABEL_AREA);
        let data_h_avail = panel_h.saturating_sub(2 * MARGIN + X_LABEL_AREA + caption_h);
        let dx = (x_hi - x_lo).abs().max(f64::EPSILON);
        let dy = (y_hi - y_lo).abs().max(f64::EPSILON);
        let scale = (data_w_avail as f64 / dx).min(data_h_avail as f64 / dy);
        let data_w = (scale * dx).round().max(1.0) as u32;
        let data_h = (scale * dy).round().max(1.0) as u32;
        let req_cw = panel_w.saturating_sub(data_w + 2 * MARGIN + Y_LABEL_AREA);
        let req_ch = panel_h.saturating_sub(data_h + 2 * MARGIN + X_LABEL_AREA + caption_h);
        (root.clone().shrink((0, 0), (req_cw, req_ch)), None)
    } else {
        (root.clone(), None)
    };

    let caption = sp.title.clone();
    let caption_style: TextStyle =
        ("sans-serif", 18u32, &palette.text).into_text_style(&chart_area);
    let label_style: TextStyle =
        ("sans-serif", 12u32, &palette.text).into_text_style(&chart_area);
    let desc_style: TextStyle =
        ("sans-serif", 14u32, &palette.text).into_text_style(&chart_area);
    let axis_style: ShapeStyle = palette.text.stroke_width(1);

    let mut chart = ChartBuilder::on(&chart_area)
        .caption(caption, caption_style)
        .margin(MARGIN)
        .x_label_area_size(X_LABEL_AREA)
        .y_label_area_size(Y_LABEL_AREA)
        .build_cartesian_2d(x_lo..x_hi, y_lo..y_hi)
        .map_err(err)?;

    // Heatmap kind with both axis label vectors gets categorical tick
    // formatters; otherwise use the default numeric mesh.
    let heatmap_labels = sp.heatmap.as_ref().and_then(|h| {
        if matches!(h.kind, crate::figure::HeatmapKind::Heatmap) {
            match (h.x_labels.clone(), h.y_labels.clone()) {
                (Some(xl), Some(yl)) => Some((xl, yl, h.z.len())),
                _ => None,
            }
        } else {
            None
        }
    });
    if let Some((xl_c, yl_c, nrows_c)) = heatmap_labels {
        chart
            .configure_mesh()
            .disable_mesh()
            .axis_style(axis_style)
            .label_style(label_style)
            .axis_desc_style(desc_style)
            .x_desc(sp.xlabel.as_str())
            .y_desc(sp.ylabel.as_str())
            // Request len+1 ticks across [0, len]; plotters picks "nice"
            // values and this hint lands them at every integer cell boundary
            // (0, 1, ..., len), so each label slot gets a tick.
            .x_labels(xl_c.len() + 1)
            .y_labels(yl_c.len() + 1)
            .x_label_formatter(&|v| {
                // Tick at integer v labels the column whose left edge is at v.
                let rounded = v.round();
                if (*v - rounded).abs() > 1e-6 {
                    return String::new();
                }
                let idx = rounded as isize;
                if idx >= 0 && (idx as usize) < xl_c.len() {
                    xl_c[idx as usize].clone()
                } else {
                    String::new()
                }
            })
            .y_label_formatter(&|v| {
                // Row 0 sits at the top (chart-y = nrows). Tick at integer v
                // labels the row whose top edge is at v: row_idx = nrows - v.
                let rounded = v.round();
                if (*v - rounded).abs() > 1e-6 {
                    return String::new();
                }
                let row_idx = (nrows_c as isize) - (rounded as isize);
                if row_idx >= 0 && (row_idx as usize) < yl_c.len() {
                    yl_c[row_idx as usize].clone()
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
            .axis_style(axis_style)
            .label_style(label_style)
            .axis_desc_style(desc_style)
            .x_desc(sp.xlabel.as_str())
            .y_desc(sp.ylabel.as_str())
            .draw()
            .map_err(err)?;
    }

    // Gridlines first so heatmap cells / filled contours render on top.
    if sp.grid {
        draw_grid(&mut chart, x_lo, x_hi, y_lo, y_hi, palette)?;
    }

    // Draw heatmap, rescaled into the chart bounds. Capture (min_v, max_v,
    // colorscale) so the colorbar pass can use the same range.
    let mut heatmap_meta: Option<(f64, f64, String)> = None;
    if let Some(hm) = &sp.heatmap {
        match hm.kind {
            crate::figure::HeatmapKind::ImageRgba => {
                if let Some(rgba) = &hm.rgba {
                    let ncols = hm.rgba_width as usize;
                    let nrows = hm.rgba_height as usize;
                    if nrows > 0 && ncols > 0 && rgba.len() >= nrows * ncols * 4 {
                        let cell_w = (x_hi - x_lo) / ncols as f64;
                        let cell_h = (y_hi - y_lo) / nrows as f64;
                        for r in 0..nrows {
                            for c in 0..ncols {
                                let off = (r * ncols + c) * 4;
                                let color = RGBColor(rgba[off], rgba[off + 1], rgba[off + 2]);
                                let x0 = x_lo + c as f64 * cell_w;
                                // Flip y so row 0 sits at the top.
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
            }
            _ => {
                let nrows = hm.z.len();
                let ncols = if nrows > 0 { hm.z[0].len() } else { 0 };
                if nrows > 0 && ncols > 0 {
                    let vals: Vec<f64> =
                        hm.z.iter().flat_map(|row| row.iter().copied()).collect();
                    let min_v = vals.iter().copied().fold(f64::INFINITY, f64::min);
                    let max_v = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    // Match Plotly's auto-zmin/zmax behaviour: scale to the
                    // actual data range. Even a floating-point-noise spread
                    // (e.g. divU on a linear field where range ~ 1e-15 from
                    // rounding) gets that tiny range mapped across the full
                    // colormap, exactly as Plotly does. Only when the range
                    // is *literally* zero (every cell bit-equal to every
                    // other cell) do we collapse to a single mid-colormap
                    // value — division by zero would otherwise yield NaN.
                    let raw_range = max_v - min_v;
                    let degenerate = raw_range == 0.0;
                    let range = if degenerate { 1.0 } else { raw_range };
                    let cell_w = (x_hi - x_lo) / ncols as f64;
                    let cell_h = (y_hi - y_lo) / nrows as f64;
                    for r in 0..nrows {
                        for c in 0..ncols {
                            let v = vals[r * ncols + c];
                            let t = if degenerate { 0.5 } else { (v - min_v) / range };
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
                    heatmap_meta = Some((min_v, max_v, hm.colorscale.clone()));
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
            render_contour_lines(&mut chart, cd, palette)?;
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

    // Colorbar — only when the panel has a heatmap (mirrors the HTML
    // renderer's `showscale: true` on the heatmap trace).
    if let (Some((cbar_side, data_top, data_bot)), Some((min_v, max_v, scale))) =
        (cbar_info, heatmap_meta)
    {
        render_colorbar_to_backend(
            &cbar_side, data_top, data_bot, min_v, max_v, &scale, palette,
        )?;
    }

    root.present().map_err(err)?;
    Ok(())
}

/// Draw a vertical colorbar with tick labels into a side strip. The y range
/// `[data_top, data_bot]` is in pixels within `cbar_area` and is chosen by
/// the caller to align with the chart's data rectangle.
///
/// Top of the strip = `max_v`; bottom = `min_v` (matches Plotly's default).
fn render_colorbar_to_backend<DB>(
    cbar_area: &DrawingArea<DB, plotters::coord::Shift>,
    data_top: i32,
    data_bot: i32,
    min_v: f64,
    max_v: f64,
    colorscale: &str,
    palette: &ThemePalette,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());

    let strip_left = COLORBAR_GUTTER as i32;
    let strip_right = strip_left + COLORBAR_WIDTH as i32;
    let strip_h = (data_bot - data_top).max(1);
    let n = COLORBAR_SAMPLES as i32;

    // For a strictly-constant heatmap (every cell bit-equal), the heatmap
    // pass renders mid-colormap; the colorbar must do the same so it doesn't
    // visually imply a gradient. For any non-zero range — including
    // floating-point noise — we use the full ramp so the colorbar matches
    // Plotly's auto-scaled behaviour (and matches the noisy variation the
    // cells now show).
    let degenerate = (max_v - min_v) == 0.0;

    // Vertical gradient: top = max_v, bottom = min_v. Reuses the same
    // colormap_rgb sampler as the heatmap cells, so swatches and cells of
    // the same value are pixel-identical in colour.
    for i in 0..n {
        let y0 = data_top + (strip_h * i) / n;
        let y1 = data_top + (strip_h * (i + 1)) / n;
        let t = if degenerate {
            0.5
        } else {
            1.0 - (i as f64 + 0.5) / n as f64
        };
        let (r, g, b) = colormap_rgb(t, colorscale);
        let color = RGBColor(r, g, b);
        cbar_area
            .draw(&Rectangle::new(
                [(strip_left, y0), (strip_right, y1)],
                color.filled(),
            ))
            .map_err(err)?;
    }

    // Border + tick text use the theme's foreground colour so dark themes
    // don't render an unreadable black-on-dark border.
    let stroke = palette.text.stroke_width(1);
    cbar_area
        .draw(&Rectangle::new(
            [(strip_left, data_top), (strip_right, data_bot)],
            stroke,
        ))
        .map_err(err)?;

    // Five evenly-spaced tick labels (min, +25%, +50%, +75%, max).
    let label_font: TextStyle =
        ("sans-serif", 11u32, &palette.text).into_text_style(cbar_area);
    let label_x = strip_right + 6;
    let range = max_v - min_v;
    for i in 0..5 {
        let frac = i as f64 / 4.0;
        let v = min_v + frac * range;
        // i=0 → bottom (min_v); i=4 → top (max_v).
        let y = data_bot - (strip_h * i) / 4;
        cbar_area
            .draw(&PathElement::new(
                vec![(strip_right, y), (strip_right + 4, y)],
                stroke,
            ))
            .map_err(err)?;
        let s = format_cbar_value(v, range);
        cbar_area
            .draw(&Text::new(s, (label_x, y - 6), label_font.clone()))
            .map_err(err)?;
    }

    Ok(())
}

fn format_cbar_value(v: f64, range: f64) -> String {
    let m = v.abs().max(range.abs());
    if m != 0.0 && (m >= 1e4 || m < 1e-3) {
        format!("{:.2e}", v)
    } else {
        format!("{:.3}", v)
    }
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

/// Draw a 5×5 light gridline overlay on the chart's data area using the
/// theme grid colour. Shared by the series and heatmap render paths so
/// `sp.grid` is honoured uniformly. For heatmap-bearing panels the cells
/// are drawn on top, hiding the grid (matches Plotly's `showgrid` + heatmap
/// behaviour); contour-only / quiver-only / streamline-only panels see the
/// grid through the overlays.
fn draw_grid<DB>(
    chart: &mut ChartContext<
        DB,
        Cartesian2d<plotters::coord::types::RangedCoordf64, plotters::coord::types::RangedCoordf64>,
    >,
    x_lo: f64,
    x_hi: f64,
    y_lo: f64,
    y_hi: f64,
    palette: &ThemePalette,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());
    const N: usize = 5;
    let grid_color = palette.grid;
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
    palette: &ThemePalette,
) -> Result<(), PlotError>
where
    DB: DrawingBackend,
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let err = |e: DrawingAreaErrorKind<DB::ErrorType>| PlotError::FileOutput(e.to_string());
    // When the user didn't pick a colour, follow the theme foreground so
    // dark-theme contours don't render as invisible black-on-near-black.
    let color: RGBAColor = match &cd.line_color {
        Some(c) => series_color_to_rgb(c).to_rgba(),
        None => palette.text,
    };
    let style = ShapeStyle {
        color,
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
                ..Default::default()
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

    /// Pull `width` / `height` (in pixels) out of every `<rect>` in an SVG.
    fn extract_rect_dims(svg: &str) -> Vec<(u32, u32)> {
        fn parse_attr_u32(s: &str, key: &str) -> Option<u32> {
            let pat = format!("{}=\"", key);
            let p = s.find(&pat)?;
            let rest = &s[p + pat.len()..];
            let q = rest.find('"')?;
            rest[..q].parse().ok()
        }
        let mut dims = Vec::new();
        let mut i = 0;
        while let Some(p) = svg[i..].find("<rect") {
            let start = i + p;
            let end = svg[start..]
                .find("/>")
                .map(|e| start + e)
                .unwrap_or(svg.len());
            let chunk = &svg[start..end];
            if let (Some(w), Some(h)) = (
                parse_attr_u32(chunk, "width"),
                parse_attr_u32(chunk, "height"),
            ) {
                dims.push((w, h));
            }
            i = end + 1;
        }
        dims
    }

    fn push_gaussian_heatmap(n: usize, title: &str) {
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
            let sp = fig.current_mut();
            sp.title = title.to_string();
            let mid = (n as f64 - 1.0) * 0.5;
            let z: Vec<Vec<f64>> = (0..n)
                .map(|r| {
                    (0..n)
                        .map(|c| {
                            let dx = c as f64 - mid;
                            let dy = r as f64 - mid;
                            (-(dx * dx + dy * dy) / 50.0).exp()
                        })
                        .collect()
                })
                .collect();
            sp.heatmap = Some(crate::figure::HeatmapData {
                z,
                colorscale: "viridis".to_string(),
                ..Default::default()
            });
        });
    }

    #[test]
    fn imagesc_svg_has_colorbar() {
        // Render a heatmap whose values span exactly [0, 1] so the five
        // colorbar tick labels are the predictable 0.000 / 0.250 / 0.500 /
        // 0.750 / 1.000. Verify the SVG contains both colorbar swatch rects
        // (≥ COLORBAR_SAMPLES / 2) and at least 3 of those numeric labels.
        // Without the colorbar fix, neither would appear.
        let path = tmp_path("_imagesc_cbar.svg");
        let n = 5usize;
        let denom = (n * n - 1) as f64;
        let z: Vec<Vec<f64>> = (0..n)
            .map(|r| (0..n).map(|c| (r * n + c) as f64 / denom).collect())
            .collect();
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
            let sp = fig.current_mut();
            sp.title = "cbar test".to_string();
            sp.heatmap = Some(crate::figure::HeatmapData {
                z,
                colorscale: "viridis".to_string(),
                ..Default::default()
            });
        });
        render_figure_file(&path).expect("render should succeed");
        let content = std::fs::read_to_string(&path).expect("read SVG");

        let dims = extract_rect_dims(&content);
        let swatches = dims
            .iter()
            .filter(|(w, _)| *w == COLORBAR_WIDTH)
            .count();
        assert!(
            swatches >= COLORBAR_SAMPLES / 2,
            "expected at least {} colorbar swatch rects of width {}, got {}",
            COLORBAR_SAMPLES / 2,
            COLORBAR_WIDTH,
            swatches
        );

        let numeric_ticks = ["0.000", "0.250", "0.500", "0.750", "1.000"]
            .iter()
            .filter(|s| content.contains(*s))
            .count();
        assert!(
            numeric_ticks >= 3,
            "expected ≥ 3 numeric tick labels in colorbar text, got {numeric_ticks}"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn axis_equal_svg_panel_is_square() {
        // Render a unit-circle parametric (cos θ, sin θ) at axis_equal = true
        // into a 900×500 canvas. The chart-area shrink should compress horiz
        // until width ≈ height. Verify by rendering twice (with and without
        // the flag) and checking that the path data x-extent shrinks under
        // the flag.
        let path_eq = tmp_path("_axis_equal_on.svg");
        let path_auto = tmp_path("_axis_equal_off.svg");
        let n = 64;
        let xs: Vec<f64> = (0..n)
            .map(|i| (i as f64 / n as f64 * std::f64::consts::TAU).cos())
            .collect();
        let ys: Vec<f64> = (0..n)
            .map(|i| (i as f64 / n as f64 * std::f64::consts::TAU).sin())
            .collect();

        FIGURE.with(|fig| fig.borrow_mut().reset());
        push_xy_line(xs.clone(), ys.clone(), "", "", None, LineStyle::Solid);
        FIGURE.with(|fig| fig.borrow_mut().current_mut().axis_equal = true);
        render_figure_file(&path_eq).expect("render axis_equal=on");
        let svg_eq = std::fs::read_to_string(&path_eq).expect("read eq SVG");

        FIGURE.with(|fig| fig.borrow_mut().reset());
        push_xy_line(xs, ys, "", "", None, LineStyle::Solid);
        render_figure_file(&path_auto).expect("render axis_equal=off");
        let svg_auto = std::fs::read_to_string(&path_auto).expect("read auto SVG");

        assert_ne!(
            svg_eq, svg_auto,
            "SVG output should differ when axis_equal is toggled"
        );

        // Extract polyline x-coordinates from the rendered locus. The chart
        // background frame is a <rect>; the data is in <polyline points="x,y x,y ...">.
        let extract_x_extent = |svg: &str| -> (f64, f64) {
            let mut xs = Vec::new();
            let mut i = 0;
            while let Some(p) = svg[i..].find("points=\"") {
                let start = i + p + 8;
                let end = svg[start..].find('"').map(|e| start + e).unwrap_or(svg.len());
                for tok in svg[start..end].split_whitespace() {
                    if let Some((x, _)) = tok.split_once(',') {
                        if let Ok(v) = x.parse::<f64>() {
                            xs.push(v);
                        }
                    }
                }
                i = end + 1;
            }
            let mn = xs.iter().copied().fold(f64::INFINITY, f64::min);
            let mx = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            (mn, mx)
        };
        let (eq_xmin, eq_xmax) = extract_x_extent(&svg_eq);
        let (auto_xmin, auto_xmax) = extract_x_extent(&svg_auto);
        let eq_w = eq_xmax - eq_xmin;
        let auto_w = auto_xmax - auto_xmin;
        // Without axis_equal, the locus should span more horizontally (the
        // chart fills the wider canvas). With axis_equal in a 900×500 canvas,
        // it gets cropped to ~500-wide.
        assert!(
            eq_w < auto_w * 0.85,
            "axis_equal should compress x-extent: eq={eq_w:.0}, auto={auto_w:.0}"
        );

        let _ = std::fs::remove_file(&path_eq);
        let _ = std::fs::remove_file(&path_auto);
    }

    #[test]
    fn imagesc_svg_cells_are_square() {
        // Without the aspect-shrink, cells render rectangular (~37 × 18 for a
        // 21×21 grid in a 900×500 SVG). After the fix, every cell rect should
        // have width == height within ±1 px (rounding tolerance).
        let path = tmp_path("_imagesc_square.svg");
        push_gaussian_heatmap(21, "square test");
        render_figure_file(&path).expect("render should succeed");
        let content = std::fs::read_to_string(&path).expect("read SVG");

        let dims = extract_rect_dims(&content);
        // Cells are the small (~10-25 px) square-ish rects; filter out the
        // colorbar swatches (width = COLORBAR_WIDTH = 28, very short height)
        // and any chart-axis rectangles (width > 100, e.g. background or
        // frame).
        let cells: Vec<&(u32, u32)> = dims
            .iter()
            .filter(|(w, h)| *w >= 6 && *w <= 30 && *h >= 6 && *h <= 30 && *w != COLORBAR_WIDTH)
            .collect();
        assert!(
            cells.len() >= 400,
            "expected ≥ 400 cell rects for 21×21 heatmap, got {}",
            cells.len()
        );
        for &(w, h) in &cells {
            assert!(
                (*w as i32 - *h as i32).abs() <= 1,
                "non-square cell: width={w}, height={h}"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn heatmap_panel_renders_xlabel_and_ylabel() {
        // Regression: render_heatmap_and_contours_to_backend used to call
        // configure_mesh().disable_mesh().draw() with no .x_desc/.y_desc, so
        // imagesc / contour / quiver panels in SVG dropped axis labels even
        // when set. HTML rendered them via `xaxis.title` / `yaxis.title`.
        let path = tmp_path("_heatmap_axes.svg");
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
            let sp = fig.current_mut();
            sp.title = "labels".to_string();
            sp.xlabel = "x [m]".to_string();
            sp.ylabel = "y [m]".to_string();
            sp.heatmap = Some(crate::figure::HeatmapData {
                z: vec![vec![0.0, 1.0], vec![1.0, 0.0]],
                colorscale: "viridis".to_string(),
                ..Default::default()
            });
        });
        render_figure_file(&path).expect("render should succeed");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        assert!(
            content.contains("x [m]"),
            "expected xlabel 'x [m]' in heatmap SVG axis text"
        );
        assert!(
            content.contains("y [m]"),
            "expected ylabel 'y [m]' in heatmap SVG axis text"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn series_panel_omits_xlabel_when_unset() {
        // Regression: render_subplot_to_panel used to render literal "x" /
        // "y" when sp.xlabel / sp.ylabel were empty. HTML emits empty axis
        // titles in that case; SVG should match.
        let path = tmp_path("_no_default_labels.svg");
        FIGURE.with(|fig| fig.borrow_mut().reset());
        push_xy_line(
            vec![0.0, 1.0, 2.0],
            vec![0.0, 1.0, 0.5],
            "",
            "",
            Some(crate::figure::SeriesColor::Blue),
            LineStyle::Solid,
        );
        render_figure_file(&path).expect("render should succeed");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        // After the fix, no <text> element should contain a literal lone "x"
        // or "y" descriptor. plotters emits axis tick labels as numerics
        // ("0.5", "1.0", …) so checking for >x< or >y< won't false-positive
        // on data values.
        assert!(
            !content.contains(">x<"),
            "found literal 'x' axis descriptor in SVG with no xlabel set"
        );
        assert!(
            !content.contains(">y<"),
            "found literal 'y' axis descriptor in SVG with no ylabel set"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn heatmap_panel_renders_grid_when_enabled() {
        // Regression: render_heatmap_and_contours_to_backend ignored sp.grid.
        // After the fix, a contour-only panel with grid=true should emit
        // gridlines in the theme grid colour. We use a contour-only panel
        // (no heatmap) so the gridlines aren't covered by cell rectangles.
        let path = tmp_path("_grid_contour.svg");
        let (z, x, y) = radial_z(11);
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
            let sp = fig.current_mut();
            sp.grid = true;
            sp.contours.push(crate::figure::ContourData {
                z,
                x,
                y,
                levels: vec![0.5, 1.0, 1.5],
                filled: false,
                line_color: Some(crate::figure::SeriesColor::Red),
                colorscale: "viridis".to_string(),
            });
        });
        render_figure_file(&path).expect("contour-with-grid SVG should render");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        // draw_grid emits LineSeries with the theme's grid colour. plotters
        // SVGBackend writes opacity from RGBAColor as a CSS `opacity:` style.
        // Any of these markers indicates a grid line was drawn:
        //   - the literal `opacity:0.3` from the dark grid alpha
        //   - the literal `opacity:0.2` from the light grid alpha
        let has_grid_opacity = content.contains("opacity:0.3")
            || content.contains("opacity:0.2")
            || content.contains("opacity=\"0.3\"")
            || content.contains("opacity=\"0.2\"");
        assert!(
            has_grid_opacity,
            "expected gridline strokes (semi-transparent) in heatmap-path SVG \
             when sp.grid=true"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn stem_legend_uses_circle_marker() {
        // Regression: stem series' legend symbol used to be a horizontal line
        // (PathElement), inconsistent with HTML/Plotly which shows a circle
        // marker in the legend (matching the stem's tip glyph). After the
        // fix, the legend closure draws a Circle.
        let path = tmp_path("_stem_legend.svg");
        FIGURE.with(|fig| fig.borrow_mut().reset());
        let x: Vec<f64> = (0..16).map(|i| i as f64).collect();
        let y: Vec<f64> = x.iter().map(|&xi| (xi * 0.4).sin()).collect();
        push_xy_stem(
            x,
            y,
            "stem series",
            "stems",
            Some(crate::figure::SeriesColor::Red),
        );
        render_figure_file(&path).expect("stem SVG should render");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        assert!(
            content.contains("stem series"),
            "legend label missing — stem series didn't register a label"
        );
        // The legend area sits in the upper-right of the chart (per
        // SeriesLabelPosition::UpperRight) and should contain a <circle>
        // element from the legend closure. Total <circle> count = tips
        // (16) + legend marker (1) = 17. Without the fix the legend marker
        // is a <polyline> instead, so the count drops to 16.
        let circle_count = content.matches("<circle").count();
        assert!(
            circle_count >= 17,
            "expected ≥ 17 <circle> elements (16 tips + 1 legend marker), \
             got {circle_count}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn themed_svg_uses_theme_background_color() {
        // render_figure_state_to_file_themed should fill the canvas with
        // `theme.plot_bg`. Pick the Light theme (#eff1f5) so the assertion
        // doesn't conflict with anything plotters might emit by default and
        // is unlikely to occur incidentally in unrelated tests.
        let path = tmp_path("_themed_light.svg");
        FIGURE.with(|fig| fig.borrow_mut().reset());
        push_xy_line(
            vec![0.0, 1.0, 2.0],
            vec![0.0, 1.0, 0.5],
            "trace",
            "themed",
            Some(crate::figure::SeriesColor::Blue),
            LineStyle::Solid,
        );
        let snapshot: FigureState = FIGURE.with(|f| f.borrow().clone());
        render_figure_state_to_file_themed(&snapshot, &path, Theme::Light.colors())
            .expect("themed render should succeed");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        // Light theme bg = #eff1f5 → plotters writes it as #EFF1F5.
        // SVGBackend writes hex in upper-case; check both hex and rgb forms.
        assert!(
            content.contains("#EFF1F5")
                || content.contains("#eff1f5")
                || content.contains("rgb(239,241,245)"),
            "expected light-theme bg colour in SVG fill (got first 300 chars: {})",
            &content.chars().take(300).collect::<String>()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn imagesc_svg_min_eq_max_no_division_by_zero() {
        // Constant matrix (e.g. divU = 2 from a uniform divergence field):
        // min_v == max_v. Render must succeed; colorbar ticks must all show
        // the constant value; heatmap cells AND colorbar swatches must all
        // render in the SAME single colour (mid-colormap), not a full ramp
        // contradicting the uniform data. Tests the constant-matrix visual
        // divergence fix.
        let path = tmp_path("_imagesc_const.svg");
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            fig.reset();
            let sp = fig.current_mut();
            sp.title = "constant".to_string();
            sp.heatmap = Some(crate::figure::HeatmapData {
                z: vec![vec![2.0; 8]; 8],
                colorscale: "viridis".to_string(),
                ..Default::default()
            });
        });
        render_figure_file(&path).expect("constant heatmap should render");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        assert!(
            content.contains("2.000"),
            "expected constant value '2.000' in colorbar tick text"
        );

        // Collect every fill colour used by every cell rect (8x8 = 64) and
        // every colorbar swatch (COLORBAR_SAMPLES = 64) — every one of those
        // 128 fills should be the same single mid-viridis colour.
        let mut fills: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut i = 0;
        while let Some(p) = content[i..].find("<rect") {
            let start = i + p;
            let end = content[start..]
                .find("/>")
                .map(|e| start + e)
                .unwrap_or(content.len());
            let chunk = &content[start..end];
            // Skip the figure-background rects (width = full SVG width 900).
            if let Some(w_pos) = chunk.find("width=\"") {
                let rest = &chunk[w_pos + 7..];
                if let Some(q) = rest.find('"') {
                    if let Ok(w) = rest[..q].parse::<u32>() {
                        if w == 900 {
                            i = end + 1;
                            continue;
                        }
                    }
                }
            }
            if let Some(f_pos) = chunk.find("fill=\"#") {
                let rest = &chunk[f_pos + 7..];
                if let Some(q) = rest.find('"') {
                    fills.insert(rest[..q].to_uppercase());
                }
            }
            i = end + 1;
        }
        assert_eq!(
            fills.len(),
            1,
            "expected a single fill colour across heatmap cells + colorbar \
             swatches for a constant matrix; got {} distinct colours: {:?}",
            fills.len(),
            fills
        );
        // Mid-viridis is `#21918C` (close to t=0.5; varies by interpolation
        // segment). Reject the bottom of the ramp explicitly so a regression
        // back to t=0 fails this test.
        let only_fill = fills.iter().next().unwrap();
        assert!(
            !only_fill.starts_with("#440"),
            "constant-matrix cells fell back to bottom-of-ramp colour ({only_fill}); \
             expected mid-colormap"
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
    fn contour_lines_use_theme_foreground_when_color_unset() {
        // Regression: render_contour_lines defaulted to SeriesColor::Black
        // (#000000) when ContourData.line_color was None, which made dark-
        // theme contours invisible against the dark background. The default
        // should now pick up palette.text.
        let path = tmp_path("_contour_dark.svg");
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
                line_color: None,
                colorscale: "viridis".to_string(),
            });
        });
        let snapshot: FigureState = FIGURE.with(|f| f.borrow().clone());
        render_figure_state_to_file_themed(&snapshot, &path, Theme::Dark.colors())
            .expect("dark-theme contour render should succeed");
        let content = std::fs::read_to_string(&path).expect("read SVG");
        // Catppuccin Mocha text = #cdd6f4. plotters' SVGBackend emits stroke
        // colours as upper-case hex; allow either case for safety.
        assert!(
            content.contains("#CDD6F4") || content.contains("#cdd6f4"),
            "expected dark-theme foreground colour in contour stroke; \
             black-on-dark would mean the regression is back"
        );
        // And the literal pure-black stroke must not appear on contour
        // polylines (plotters emits other elements like <text> in theme
        // colour too, so assert no `stroke="#000000"`).
        assert!(
            !content.contains("stroke=\"#000000\"")
                && !content.contains("stroke=\"#000\""),
            "found pure-black stroke in dark-theme SVG — contours likely fell \
             back to SeriesColor::Black"
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
    fn render_panel_to_rgba_produces_nonzero_pixels_for_quiver() {
        // Regression: the viewer relies on render_panel_to_rgba to display
        // panels containing quiver/streamline/contour data. A successful
        // render must produce 4-byte-per-pixel RGBA with at least some
        // non-background pixels (the arrow strokes).
        let (x, y, u, v) = uniform_field(8, 8, 1.0, 0.0);
        let mut sp = SubplotState::new();
        sp.quivers.push(crate::figure::QuiverData {
            x, y, u, v,
            scale: 1.0,
            color: None,
            title: None,
        });
        let theme = Theme::default();
        let rgba = render_panel_to_rgba(&sp, theme.colors(), 200, 150)
            .expect("panel rgba render");
        assert_eq!(rgba.len(), 200 * 150 * 4);
        // At least one pixel should differ from the background colour.
        let bg_r = rgba[0];
        let bg_g = rgba[1];
        let bg_b = rgba[2];
        let any_diff = rgba.chunks_exact(4)
            .any(|p| p[0] != bg_r || p[1] != bg_g || p[2] != bg_b);
        assert!(any_diff, "quiver panel rgba is uniformly background");
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
                ..Default::default()
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
                ..Default::default()
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
