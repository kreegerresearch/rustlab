//! Export the current `FIGURE` state to an HTML file using Plotly.js.
//!
//! The page references a sibling `assets/plotly.min.js` (the vendored
//! bundle, written once per output directory) so it opens from `file://`
//! with no network. The script is not inlined: the bundle is about 4.4 MB.

use std::cell::RefCell;
use std::fmt::Write as _;

use crate::error::PlotError;
use crate::figure::{AxisScale, FigureState, LineStyle, PlotKind, SeriesColor, FIGURE};
use crate::theme::ThemeColors;

thread_local! {
    /// When set, every FIGURE mutation re-writes the HTML file at this path.
    static HTML_FIGURE_PATH: RefCell<Option<String>> = RefCell::new(None);
}

/// Set the active HTML figure output path. Subsequent FIGURE mutations
/// will auto-render to this file.
pub fn set_html_figure_path(path: &str) {
    HTML_FIGURE_PATH.with(|p| *p.borrow_mut() = Some(path.to_string()));
}

/// Clear the active HTML figure path (stop auto-rendering).
pub fn clear_html_figure_path() {
    HTML_FIGURE_PATH.with(|p| *p.borrow_mut() = None);
}

/// Returns true when an HTML figure path is active.
pub fn html_figure_active() -> bool {
    HTML_FIGURE_PATH.with(|p| p.borrow().is_some())
}

/// Get the current HTML figure path, if any.
pub fn get_html_figure_path() -> Option<String> {
    HTML_FIGURE_PATH.with(|p| p.borrow().clone())
}

/// If an HTML figure path is active, re-render the current FIGURE to it.
/// No-op when no path is set — safe to call unconditionally.
pub fn sync_html_file() {
    HTML_FIGURE_PATH.with(|p| {
        let p = p.borrow();
        if let Some(path) = p.as_ref() {
            let _ = render_figure_html(path);
        }
    });
}

/// Render the current thread-local FIGURE to an HTML file with Plotly.
pub fn render_figure_html(path: &str) -> Result<(), PlotError> {
    FIGURE.with(|fig| {
        let fig = fig.borrow();
        render_figure_state_html(&fig, path)
    })
}

/// Render a `FigureState` to an HTML file with Plotly (default dark theme).
pub fn render_figure_state_html(fig: &FigureState, path: &str) -> Result<(), PlotError> {
    render_figure_state_html_themed(fig, path, crate::theme::default_theme().colors())
}

/// Render a `FigureState` to an HTML file with Plotly using the given theme.
pub fn render_figure_state_html_themed(
    fig: &FigureState,
    path: &str,
    theme: &ThemeColors,
) -> Result<(), PlotError> {
    crate::vendor::ensure_plotly_asset(path)?;
    let div_content = render_figure_plotly_div(fig, "plot", theme);

    let mut html = String::with_capacity(4096 + div_content.len());
    html.push_str(&format!(
        r##"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>RustLab Plot</title>
<script src="{plotly}"></script>
<style>
  body {{ margin: 0; background: {bg}; }}
  #plot {{ width: 100vw; height: 100vh; }}
</style>
</head>
<body>
"##,
        bg = theme.bg,
        plotly = crate::vendor::PLOTLY_REL_SRC,
    ));
    html.push_str(&div_content);
    html.push_str(
        r##"</body>
</html>
"##,
    );

    std::fs::write(path, html).map_err(|e| PlotError::FileOutput(e.to_string()))
}

/// Paper rectangle of one subplot. `x0..x1` and `y0..y1` are fractions of
/// the plotting area (Plotly's paper coordinates, y growing upward).
struct PanelGeom {
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
}

/// Place panel `idx` in a `rows`×`cols` grid.
///
/// The pad on each side of a cell is the room for that panel's own axis
/// titles. A colour bar steals a strip from the right of its own cell so
/// the next column's labels are not drawn through it.
fn panel_geom(rows: usize, cols: usize, idx: usize, reserve_cbar: bool) -> PanelGeom {
    let rows = rows.max(1);
    let cols = cols.max(1);
    let row = idx / cols;
    let col = idx % cols;
    let col_w = 1.0 / cols as f64;
    let row_h = 1.0 / rows as f64;
    // Multi-column grids need a wider gutter: the right-hand panel's
    // y-axis title and tick labels sit in it. Multi-row grids need a
    // taller gutter for the upper x-axis title and the lower plot title.
    let mut x_pad = if cols > 1 { 0.05 } else { 0.04 };
    let mut y_pad = if rows > 1 { 0.065 } else { 0.04 };
    if x_pad * 2.0 + 0.12 > col_w {
        x_pad = ((col_w - 0.12) / 2.0).max(0.02);
    }
    if y_pad * 2.0 + 0.10 > row_h {
        y_pad = ((row_h - 0.10) / 2.0).max(0.02);
    }
    let x0 = col as f64 * col_w + x_pad;
    let mut x1 = (col + 1) as f64 * col_w - x_pad;
    if reserve_cbar {
        x1 -= ((x1 - x0) * 0.18).min(0.08);
    }
    let y0 = 1.0 - (row + 1) as f64 * row_h + y_pad;
    let y1 = 1.0 - row as f64 * row_h - y_pad;
    PanelGeom { x0, x1, y0, y1 }
}

/// Word-wrap `title` to the width of one column, keeping explicit newlines.
fn wrap_title_lines(title: &str, cols: usize) -> Vec<String> {
    if title.is_empty() {
        return Vec::new();
    }
    let width = (56 / cols.max(1)).max(20);
    let mut lines = Vec::new();
    for para in title.split('\n') {
        let words: Vec<&str> = para.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }
        let mut cur = String::new();
        for w in words {
            if cur.is_empty() {
                cur = w.to_string();
            } else if cur.len() + 1 + w.len() > width {
                lines.push(std::mem::take(&mut cur));
                cur = w.to_string();
            } else {
                cur.push(' ');
                cur.push_str(w);
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
    }
    lines
}

/// Plotly annotation text. Newlines and wrapped lines become `<br>` after
/// each line is escaped, so a literal `<` in the title cannot close the
/// surrounding `<script>`.
fn annotation_text(title: &str, cols: usize) -> String {
    wrap_title_lines(title, cols)
        .into_iter()
        .map(|line| escape_js(&line))
        .collect::<Vec<_>>()
        .join("<br>")
}

fn panel_has_colorbar(panel: &crate::figure::SubplotState) -> bool {
    if panel.surface.is_some() {
        return true;
    }
    if matches!(
        panel.heatmap.as_ref().map(|h| &h.kind),
        Some(crate::figure::HeatmapKind::Imagesc) | Some(crate::figure::HeatmapKind::Heatmap)
    ) {
        return true;
    }
    panel.contours.iter().any(|c| c.filled)
}

/// Colour-bar slot for this panel, in paper coordinates, so a bar stays
/// beside its own subplot instead of spanning the whole figure.
fn colorbar_attr(g: &PanelGeom, text: &str) -> String {
    let x = (g.x1 + 0.012).min(1.02);
    let y = (g.y0 + g.y1) / 2.0;
    let len = ((g.y1 - g.y0) * 0.86).max(0.05);
    format!(
        r#", colorbar: {{ x: {x:.4}, xref: "paper", y: {y:.4}, yref: "paper", len: {len:.4}, thickness: 12, outlinewidth: 0, tickfont: {{ color: "{text}", size: 11 }} }}"#
    )
}

/// Render a `FigureState` as a Plotly `<div>` + `<script>` fragment.
/// The `div_id` is used as the element ID for `Plotly.newPlot()`.
/// This is the shared building block for both single-file HTML export
/// and multi-figure report generation.
pub fn render_figure_plotly_div(fig: &FigureState, div_id: &str, theme: &ThemeColors) -> String {
    let rows = fig.subplot_rows.max(1);
    let cols = fig.subplot_cols.max(1);
    let n_panels = rows * cols;

    let mut traces = String::new();
    let mut layout_axes = String::new();
    let mut scenes = String::new();
    let mut annotations = String::new();

    // Top-row titles are drawn upward into the top margin. Lower-row titles
    // sit in the gutter between rows (see `panel_geom`).
    let top_lines = fig
        .subplots
        .iter()
        .take(cols)
        .map(|p| wrap_title_lines(&p.title, cols).len())
        .max()
        .unwrap_or(0);
    let any_cbar = fig.subplots.iter().take(n_panels).any(panel_has_colorbar);
    let margin_t = if top_lines == 0 {
        28
    } else {
        28 + 18 * top_lines
    };
    let margin_b = 64;
    let margin_l = 78;
    let margin_r = if any_cbar { 100 } else { 36 };

    for (idx, panel) in fig.subplots.iter().enumerate().take(n_panels) {
        // Plotly subplot axis naming: xaxis, xaxis2, xaxis3, ...
        let axis_suffix = if idx == 0 {
            String::new()
        } else {
            format!("{}", idx + 1)
        };
        let xaxis_ref = format!("x{}", axis_suffix);
        let yaxis_ref = format!("y{}", axis_suffix);
        // Bind each x axis to its own y axis (and the reverse). Without
        // this, Plotly anchors every extra axis to `x`/`y`, so a lower
        // panel's x-axis title is drawn on the first row — on top of the
        // next panel's title — and a right-hand y-axis title is drawn on
        // top of the left-hand one.
        let geom = panel_geom(rows, cols, idx, panel_has_colorbar(panel));
        let x_start = geom.x0;
        let x_end = geom.x1;
        let y_start = geom.y0;
        let y_end = geom.y1;
        let cbar = colorbar_attr(&geom, theme.text);

        // Axis layout
        let show_grid = if panel.grid { "true" } else { "false" };
        // Categorical x-axis: switch Plotly into category mode and preserve
        // the user-provided label order. Traces below emit their x values as
        // the label strings directly, so tickvals/ticktext are not needed.
        // The quoted/escaped label list is built once per panel and reused by
        // every label-matched Line/Bar series.
        let x_label_items: Option<(usize, String)> = panel
            .x_labels
            .as_ref()
            .map(|labels| (labels.len(), json_label_items(labels)));
        let xtick_extra = match &x_label_items {
            Some((_, items)) => {
                format!(r#", type: "category", categoryorder: "array", categoryarray: [{items}]"#)
            }
            None => String::new(),
        };
        // Categorical x for a series: when the panel has x_labels that match
        // the series 1:1, Plotly's category axis wants the label strings as
        // the x values.
        let series_x_json = |x_data: &[f64]| match &x_label_items {
            Some((n, items)) if *n == x_data.len() => format!("[{items}]"),
            _ => json_f64_array(x_data),
        };
        // Square aspect ratio for all heatmap kinds. The y-axis is
        // reversed (autorange: "reversed") so row 0 lands at the TOP of
        // the chart — image convention. Fixed across all three kinds
        // 2026-05-16; prior to that, `Imagesc` was the only kind
        // without the flip and its HTML output disagreed with its
        // own SVG output.
        let yaxis_extra = if let Some(hm) = &panel.heatmap {
            let anchor = if axis_suffix.is_empty() {
                "x".to_string()
            } else {
                format!("x{axis_suffix}")
            };
            // Y-axis orientation honours the per-panel
            // `y_axis_direction`. Default `Ij` reverses the axis for
            // image convention (matches SVG backend and MATLAB `imagesc`);
            // `Xy` keeps Plotly's standard ascending y-axis so the
            // physics convention shines through for users who opted in.
            let _ = hm;
            let reversed = panel.y_axis_direction == crate::figure::AxisYDirection::Ij;
            if reversed {
                format!(r#", scaleanchor: "{anchor}", autorange: "reversed""#)
            } else {
                format!(r#", scaleanchor: "{anchor}""#)
            }
        } else if panel.axis_equal && panel.surface.is_none() {
            // axis("equal") for line/scatter panels — same scaleanchor pattern
            // as heatmaps. `constrain: "domain"` keeps Plotly honouring the
            // explicit xlim/ylim instead of auto-expanding to fit overflow
            // (the Smith-chart case: stability/gain circles routinely lie
            // outside the unit disk and would otherwise squish the chart).
            let anchor = if axis_suffix.is_empty() {
                "x".to_string()
            } else {
                format!("x{axis_suffix}")
            };
            format!(r#", scaleanchor: "{anchor}", constrain: "domain""#)
        } else {
            String::new()
        };
        let has_surface = panel.surface.is_some();
        if has_surface {
            // 3D surface: position a Plotly `scene` in the subplot's domain.
            // Scenes are independent of xaxis/yaxis, so skip 2D axis layout.
            let scene_key = if axis_suffix.is_empty() {
                "scene".to_string()
            } else {
                format!("scene{}", idx + 1)
            };
            scenes.push_str(&format!(
                r#"{scene_key}: {{ domain: {{ x: [{x0:.4}, {x1:.4}], y: [{y0:.4}, {y1:.4}] }}, xaxis: {{ title: {{ text: "{xlabel}", font: {{ color: "{text}", size: 13 }} }}, color: "{text}" }}, yaxis: {{ title: {{ text: "{ylabel}", font: {{ color: "{text}", size: 13 }} }}, color: "{text}" }}, zaxis: {{ color: "{text}" }}, bgcolor: "{plot_bg}" }},
"#,
                scene_key = scene_key,
                x0 = x_start, x1 = x_end,
                y0 = y_start, y1 = y_end,
                xlabel = escape_js(&panel.xlabel),
                ylabel = escape_js(&panel.ylabel),
                text = theme.text,
                plot_bg = theme.plot_bg,
            ));
        } else {
            // Log-scaled axes (semilogx/semilogy/loglog): Plotly renders real
            // data on a `type: "log"` axis with decade ticks natively. An
            // explicit range must be given in log10 units for a log axis.
            let x_log = panel.x_scale == AxisScale::Log;
            let y_log = panel.y_scale == AxisScale::Log;
            let log10_lim =
                |lim: (Option<f64>, Option<f64>)| (lim.0.map(f64::log10), lim.1.map(f64::log10));
            let xtype = if x_log { r#", type: "log""# } else { "" };
            let ytype = if y_log { r#", type: "log""# } else { "" };
            let xrange = if x_log {
                format_range(log10_lim(panel.xlim))
            } else {
                format_range(panel.xlim)
            };
            let yrange = if y_log {
                format_range(log10_lim(panel.ylim))
            } else {
                format_range(panel.ylim)
            };
            layout_axes.push_str(&format!(
                r#"xaxis{ax}: {{ domain: [{x0:.4}, {x1:.4}], anchor: "{y_id}", automargin: true, title: {{ text: "{xlabel}", standoff: 8, font: {{ color: "{text}", size: 13 }} }}{xrange}{xtype}, showgrid: {grid}, gridcolor: "{plot_grid}"{xtick} }},
yaxis{ax}: {{ domain: [{y0:.4}, {y1:.4}], anchor: "{x_id}", automargin: true, title: {{ text: "{ylabel}", standoff: 8, font: {{ color: "{text}", size: 13 }} }}{yrange}{ytype}, showgrid: {grid}, gridcolor: "{plot_grid}"{yextra} }},
"#,
                ax = axis_suffix,
                x_id = xaxis_ref,
                y_id = yaxis_ref,
                x0 = x_start, x1 = x_end,
                y0 = y_start, y1 = y_end,
                grid = show_grid,
                plot_grid = theme.plot_grid,
                text = theme.text,
                xlabel = escape_js(&panel.xlabel),
                ylabel = escape_js(&panel.ylabel),
                xtick = xtick_extra,
                yextra = yaxis_extra,
            ));
        }

        // Title sits on the top edge of this panel and grows upward into
        // the gutter (or the top margin), so it cannot land on the axes.
        if !panel.title.is_empty() {
            annotations.push_str(&format!(
                r#"{{ text: "{title}", xref: "paper", yref: "paper", x: {cx:.4}, y: {ty:.4}, xanchor: "center", yanchor: "bottom", yshift: 6, showarrow: false, font: {{ size: 14, color: "{color}" }} }},
"#,
                title = annotation_text(&panel.title, cols),
                cx = (x_start + x_end) / 2.0,
                ty = y_end,
                color = theme.text,
            ));
        }

        // 3D surface trace (takes precedence over heatmap and series)
        if let Some(sf) = &panel.surface {
            let plotly_cmap = match sf.colorscale.as_str() {
                "jet" => "Jet",
                "hot" => "Hot",
                "gray" => "Greys",
                _ => "Viridis",
            };
            let scene_key = if axis_suffix.is_empty() {
                "scene".to_string()
            } else {
                format!("scene{}", idx + 1)
            };
            let z_rows: Vec<String> = sf.z.iter().map(|row| json_f64_array(row)).collect();
            let z_json = format!("[{}]", z_rows.join(","));
            let x_json = json_f64_array(&sf.x);
            let y_json = json_f64_array(&sf.y);
            traces.push_str(&format!(
                r#"{{ type: "surface", z: {z}, x: {x}, y: {y}, colorscale: "{cmap}", showscale: true{cbar}, scene: "{scene}" }},
"#,
                z = z_json,
                x = x_json,
                y = y_json,
                cmap = plotly_cmap,
                scene = scene_key,
            ));
            continue;
        }

        // Heatmap trace (takes precedence when present)
        if let Some(hm) = &panel.heatmap {
            match hm.kind {
                crate::figure::HeatmapKind::Imagesc => {
                    let plotly_cmap = match hm.colorscale.as_str() {
                        "jet" => "Jet",
                        "hot" => "Hot",
                        "gray" => "Greys",
                        _ => "Viridis",
                    };
                    let z_rows: Vec<String> = hm.z.iter().map(|row| json_f64_array(row)).collect();
                    let z_json = format!("[{}]", z_rows.join(","));
                    // Fixed colour range, when set on the HeatmapData, becomes
                    // Plotly's zmin/zmax so the colour mapping matches the SVG
                    // path. Without this Plotly auto-scales per render, which
                    // gives a different colour stretch than the static backend.
                    let zrange = match (hm.value_min, hm.value_max) {
                        (Some(a), Some(b)) if a < b => format!(", zmin: {a}, zmax: {b}"),
                        _ => String::new(),
                    };
                    traces.push_str(&format!(
                        r#"{{ z: {z}, type: "heatmap", colorscale: "{cmap}", showscale: true{cbar}{zr}, xaxis: "{xa}", yaxis: "{ya}" }},
"#,
                        z = z_json,
                        cmap = plotly_cmap,
                        zr = zrange,
                        xa = xaxis_ref,
                        ya = yaxis_ref,
                    ));
                }
                crate::figure::HeatmapKind::Heatmap => {
                    let plotly_cmap = match hm.colorscale.as_str() {
                        "jet" => "Jet",
                        "hot" => "Hot",
                        "gray" => "Greys",
                        _ => "Viridis",
                    };
                    let z_rows: Vec<String> = hm.z.iter().map(|row| json_f64_array(row)).collect();
                    let z_json = format!("[{}]", z_rows.join(","));
                    let x_field = match &hm.x_labels {
                        Some(labels) => format!(", x: [{}]", json_label_items(labels)),
                        None => String::new(),
                    };
                    let y_field = match &hm.y_labels {
                        Some(labels) => format!(", y: [{}]", json_label_items(labels)),
                        None => String::new(),
                    };
                    traces.push_str(&format!(
                        r#"{{ z: {z}{xfield}{yfield}, type: "heatmap", colorscale: "{cmap}", showscale: true{cbar}, xaxis: "{xa}", yaxis: "{ya}" }},
"#,
                        z = z_json,
                        xfield = x_field,
                        yfield = y_field,
                        cmap = plotly_cmap,
                        xa = xaxis_ref,
                        ya = yaxis_ref,
                    ));
                }
                crate::figure::HeatmapKind::ImageRgba => {
                    if let Some(rgba) = &hm.rgba {
                        let w = hm.rgba_width as usize;
                        let h = hm.rgba_height as usize;
                        let mut z_json = String::from("[");
                        for r in 0..h {
                            if r > 0 {
                                z_json.push(',');
                            }
                            z_json.push('[');
                            for c in 0..w {
                                if c > 0 {
                                    z_json.push(',');
                                }
                                let off = (r * w + c) * 4;
                                z_json.push_str(&format!(
                                    "[{},{},{},{}]",
                                    rgba[off],
                                    rgba[off + 1],
                                    rgba[off + 2],
                                    rgba[off + 3]
                                ));
                            }
                            z_json.push(']');
                        }
                        z_json.push(']');
                        traces.push_str(&format!(
                            r#"{{ z: {z}, type: "image", xaxis: "{xa}", yaxis: "{ya}" }},
"#,
                            z = z_json,
                            xa = xaxis_ref,
                            ya = yaxis_ref,
                        ));
                    }
                }
            }
        }

        // Contour overlays (rendered above heatmap, below series).
        for cd in &panel.contours {
            let z_rows: Vec<String> = cd.z.iter().map(|row| json_f64_array(row)).collect();
            let z_json = format!("[{}]", z_rows.join(","));
            let x_json = json_f64_array(&cd.x);
            let y_json = json_f64_array(&cd.y);
            // Choose start/end/size from the levels vector. For uniform
            // levels (the common case from auto_levels) Plotly draws them
            // exactly; for non-uniform user-supplied levels Plotly will draw
            // uniformly between start and end.
            let (start, end, size) = if cd.levels.is_empty() {
                (0.0, 0.0, 1.0)
            } else if cd.levels.len() == 1 {
                (cd.levels[0], cd.levels[0], 1.0)
            } else {
                let s = cd.levels[0];
                let e = cd.levels[cd.levels.len() - 1];
                let step = (e - s) / (cd.levels.len() as f64 - 1.0);
                (s, e, step.max(f64::MIN_POSITIVE))
            };
            if cd.filled {
                let plotly_cmap = match cd.colorscale.as_str() {
                    "jet" => "Jet",
                    "hot" => "Hot",
                    "gray" => "Greys",
                    _ => "Viridis",
                };
                traces.push_str(&format!(
                    r#"{{ z: {z}, x: {x}, y: {y}, type: "contour", contours: {{ coloring: "fill", start: {s}, end: {e}, size: {step} }}, colorscale: "{cmap}", showscale: true{cbar}, xaxis: "{xa}", yaxis: "{ya}" }},
"#,
                    z = z_json,
                    x = x_json,
                    y = y_json,
                    s = start,
                    e = end,
                    step = size,
                    cmap = plotly_cmap,
                    xa = xaxis_ref,
                    ya = yaxis_ref,
                ));
            } else {
                // When the user didn't pick a colour, follow the theme
                // foreground so dark-theme contours don't render as invisible
                // black-on-near-black. (`theme.text` is already a CSS hex
                // string in `ThemeColors`.)
                let line_color = match &cd.line_color {
                    Some(c) => color_to_css(c),
                    None => theme.text.to_string(),
                };
                traces.push_str(&format!(
                    r#"{{ z: {z}, x: {x}, y: {y}, type: "contour", contours: {{ coloring: "none", showlines: true, start: {s}, end: {e}, size: {step} }}, line: {{ color: "{color}", width: 1.5 }}, showscale: false, hoverinfo: "skip", xaxis: "{xa}", yaxis: "{ya}" }},
"#,
                    z = z_json,
                    x = x_json,
                    y = y_json,
                    s = start,
                    e = end,
                    step = size,
                    color = line_color,
                    xa = xaxis_ref,
                    ya = yaxis_ref,
                ));
            }
        }

        // Quiver overlays: emit one scatter(lines) trace per QuiverData,
        // with `null` separators between arrows so Plotly draws each as an
        // independent polyline (shaft + arrowhead).
        for qd in &panel.quivers {
            let arrows = crate::quiver::build_arrows(&qd.u, &qd.v, &qd.x, &qd.y, qd.scale);
            if arrows.is_empty() {
                continue;
            }
            let color = color_to_css(qd.color.as_ref().unwrap_or(&SeriesColor::Cyan));
            let (xs, ys) = arrows_to_polyline(&arrows);
            traces.push_str(&format!(
                r#"{{ x: {x}, y: {y}, type: "scatter", mode: "lines", connectgaps: false, line: {{ color: "{color}", width: 1.25 }}, showlegend: false, hoverinfo: "skip", xaxis: "{xa}", yaxis: "{ya}" }},
"#,
                x = xs,
                y = ys,
                color = color,
                xa = xaxis_ref,
                ya = yaxis_ref,
            ));
        }

        // Streamline overlays: integrate each seed and emit one scatter(lines)
        // trace per StreamlineData with null-separated polylines.
        for sd in &panel.streamlines {
            let seeds = match &sd.seeds {
                Some(s) => s.clone(),
                None => crate::streamline::default_seeds(&sd.x, &sd.y, sd.density),
            };
            if seeds.is_empty() {
                continue;
            }
            let step = crate::streamline::default_step(&sd.x, &sd.y);
            let ref_len = crate::quiver::cell_distance(&sd.x, &sd.y) * 0.5;
            let mut paths: Vec<Vec<(f64, f64)>> = Vec::new();
            let mut arrowheads: Vec<crate::quiver::Arrow> = Vec::new();
            for (sx, sy) in seeds {
                let pts = crate::streamline::integrate(
                    &sd.u, &sd.v, &sd.x, &sd.y, sx, sy, step, 400, 1e-10,
                );
                if pts.len() < 2 {
                    continue;
                }
                if let Some(a) = crate::quiver::midpoint_arrow(&pts, ref_len) {
                    arrowheads.push(a);
                }
                paths.push(pts);
            }
            if paths.is_empty() {
                continue;
            }
            let color = color_to_css(sd.color.as_ref().unwrap_or(&SeriesColor::Cyan));
            let (xs, ys) = paths_to_polyline(&paths);
            traces.push_str(&format!(
                r#"{{ x: {x}, y: {y}, type: "scatter", mode: "lines", connectgaps: false, line: {{ color: "{color}", width: 1.25 }}, showlegend: false, hoverinfo: "skip", xaxis: "{xa}", yaxis: "{ya}" }},
"#,
                x = xs,
                y = ys,
                color = color,
                xa = xaxis_ref,
                ya = yaxis_ref,
            ));
            if !arrowheads.is_empty() {
                let (hxs, hys) = arrows_to_polyline(&arrowheads);
                traces.push_str(&format!(
                    r#"{{ x: {x}, y: {y}, type: "scatter", mode: "lines", connectgaps: false, line: {{ color: "{color}", width: 1.25 }}, showlegend: false, hoverinfo: "skip", xaxis: "{xa}", yaxis: "{ya}" }},
"#,
                    x = hxs,
                    y = hys,
                    color = color,
                    xa = xaxis_ref,
                    ya = yaxis_ref,
                ));
            }
        }

        // Traces for each series
        for series in &panel.series {
            let color_str = color_to_css(&series.color);
            // Use WebGL backend for large traces (>10k points)
            let scatter_type = if series.x_data.len() > 10_000 {
                "scattergl"
            } else {
                "scatter"
            };
            match series.kind {
                PlotKind::Line => {
                    let dash = match series.style {
                        LineStyle::Solid => "solid",
                        LineStyle::Dashed => "dash",
                    };
                    // Categorical line: when the subplot has x_labels that
                    // match this series 1:1, feed the labels in as x so
                    // Plotly's type="category" axis renders them correctly.
                    let x_json = series_x_json(&series.x_data);
                    let legend_attr = if series.label.is_empty() {
                        ", showlegend: false"
                    } else {
                        ""
                    };
                    traces.push_str(&format!(
                        r#"{{ x: {x}, y: {y}, type: "{stype}", mode: "lines", name: "{name}", line: {{ color: "{color}", dash: "{dash}" }}, xaxis: "{xa}", yaxis: "{ya}"{legend} }},
"#,
                        stype = scatter_type,
                        x = x_json,
                        y = json_f64_array(&series.y_data),
                        name = escape_js(&series.label),
                        color = color_str,
                        dash = dash,
                        xa = xaxis_ref,
                        ya = yaxis_ref,
                        legend = legend_attr,
                    ));
                }
                PlotKind::Scatter => {
                    let legend_attr = if series.label.is_empty() {
                        ", showlegend: false"
                    } else {
                        ""
                    };
                    traces.push_str(&format!(
                        r#"{{ x: {x}, y: {y}, type: "{stype}", mode: "markers", name: "{name}", marker: {{ color: "{color}", size: 6 }}, xaxis: "{xa}", yaxis: "{ya}"{legend} }},
"#,
                        stype = scatter_type,
                        x = json_f64_array(&series.x_data),
                        y = json_f64_array(&series.y_data),
                        name = escape_js(&series.label),
                        color = color_str,
                        xa = xaxis_ref,
                        ya = yaxis_ref,
                        legend = legend_attr,
                    ));
                }
                PlotKind::Bar => {
                    // Categorical bar: when the subplot has x_labels that
                    // match this series 1:1, feed the labels in as x so
                    // Plotly's type="category" axis renders them correctly.
                    let x_json = series_x_json(&series.x_data);
                    traces.push_str(&format!(
                        r#"{{ x: {x}, y: {y}, type: "bar", name: "{name}", marker: {{ color: "{color}" }}, xaxis: "{xa}", yaxis: "{ya}" }},
"#,
                        x = x_json,
                        y = json_f64_array(&series.y_data),
                        name = escape_js(&series.label),
                        color = color_str,
                        xa = xaxis_ref,
                        ya = yaxis_ref,
                    ));
                }
                PlotKind::Stem => {
                    // Stems: vertical lines from y=0 to each point
                    let mut sx = String::with_capacity(series.x_data.len() * 16);
                    let mut sy = String::with_capacity(series.y_data.len() * 16);
                    for (i, (&xi, &yi)) in
                        series.x_data.iter().zip(series.y_data.iter()).enumerate()
                    {
                        if i > 0 {
                            sx.push(',');
                            sy.push(',');
                        }
                        let _ = write!(sx, "{xi},{xi},null");
                        let _ = write!(sy, "0,{yi},null");
                    }
                    // Stem lines
                    traces.push_str(&format!(
                        r#"{{ x: [{sx}], y: [{sy}], type: "{stype}", mode: "lines", name: "{name}", line: {{ color: "{color}" }}, xaxis: "{xa}", yaxis: "{ya}", showlegend: false }},
"#,
                        stype = scatter_type,
                        sx = sx,
                        sy = sy,
                        name = escape_js(&series.label),
                        color = color_str,
                        xa = xaxis_ref,
                        ya = yaxis_ref,
                    ));
                    // Marker tips
                    traces.push_str(&format!(
                        r#"{{ x: {x}, y: {y}, type: "{stype}", mode: "markers", name: "{name}", marker: {{ color: "{color}", size: 6 }}, xaxis: "{xa}", yaxis: "{ya}" }},
"#,
                        stype = scatter_type,
                        x = json_f64_array(&series.x_data),
                        y = json_f64_array(&series.y_data),
                        name = escape_js(&series.label),
                        color = color_str,
                        xa = xaxis_ref,
                        ya = yaxis_ref,
                    ));
                }
            }
        }
    }

    // JS variable names can't contain hyphens, so replace with underscores
    let js_var = div_id.replace('-', "_");

    let mut out = String::with_capacity(4096 + traces.len());
    out.push_str(&format!(
        r#"<div id="{div_id}"></div>
<script>
var data_{js_var} = ["#,
        div_id = div_id,
        js_var = js_var
    ));
    out.push_str(&traces);
    out.push_str(&format!(
        r##"];
var layout_{js_var} = {{
  paper_bgcolor: "{plot_bg}",
  plot_bgcolor: "{plot_bg}",
  font: {{ color: "{text}" }},
  "##,
        js_var = js_var,
        plot_bg = theme.plot_bg,
        text = theme.text
    ));
    out.push_str(&layout_axes);
    out.push_str(&scenes);
    out.push_str("  annotations: [");
    out.push_str(&annotations);
    out.push_str(&format!(
        r##"],
  margin: {{ t: {margin_t}, b: {margin_b}, l: {margin_l}, r: {margin_r} }},
  barmode: "group",
}};
Plotly.newPlot("{div_id}", data_{js_var}, layout_{js_var}, {{ responsive: true }});
</script>
"##,
        div_id = div_id,
        js_var = js_var
    ));

    out
}

/// Delegates to [`SeriesColor::to_rgb`] so the Plotly backend can't drift from
/// the SVG one — the two tables used to be maintained by hand, side by side.
fn color_to_css(c: &SeriesColor) -> String {
    let (r, g, b) = c.to_rgb();
    format!("rgb({},{},{})", r, g, b)
}

/// Flatten a slice of arrow glyphs into a pair of Plotly JSON arrays
/// (`x`, `y`) with `null` separators between arrows. Each arrow is emitted
/// as `tail, tip, leftBarb, tip, rightBarb, null`.
fn arrows_to_polyline(arrows: &[crate::quiver::Arrow]) -> (String, String) {
    let mut out_x = String::from("[");
    let mut out_y = String::from("[");
    let mut first = true;
    for a in arrows {
        let pts: [(f64, f64); 5] = [a.shaft.0, a.shaft.1, a.head[0], a.head[1], a.head[2]];
        if !first {
            out_x.push_str(",null");
            out_y.push_str(",null");
        }
        for (i, p) in pts.iter().enumerate() {
            if i > 0 || !first {
                out_x.push(',');
                out_y.push(',');
            }
            let _ = write!(out_x, "{}", p.0);
            let _ = write!(out_y, "{}", p.1);
        }
        first = false;
    }
    out_x.push(']');
    out_y.push(']');
    (out_x, out_y)
}

/// Flatten a slice of polylines into Plotly JSON arrays with `null`
/// separators between polylines.
fn paths_to_polyline(paths: &[Vec<(f64, f64)>]) -> (String, String) {
    let mut out_x = String::from("[");
    let mut out_y = String::from("[");
    let mut first = true;
    for path in paths {
        if path.is_empty() {
            continue;
        }
        if !first {
            out_x.push_str(",null");
            out_y.push_str(",null");
        }
        for (i, p) in path.iter().enumerate() {
            if i > 0 || !first {
                out_x.push(',');
                out_y.push(',');
            }
            let _ = write!(out_x, "{}", p.0);
            let _ = write!(out_y, "{}", p.1);
        }
        first = false;
    }
    out_x.push(']');
    out_y.push(']');
    (out_x, out_y)
}

fn json_f64_array(data: &[f64]) -> String {
    let mut s = String::with_capacity(data.len() * 10);
    s.push('[');
    for (i, v) in data.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        if v.is_finite() {
            let _ = write!(s, "{v}");
        } else {
            s.push_str("null");
        }
    }
    s.push(']');
    s
}

/// Comma-joined list of quoted, JS-escaped label strings: `"a","b","c"`.
fn json_label_items(labels: &[String]) -> String {
    let mut s = String::with_capacity(labels.len() * 12);
    for (i, l) in labels.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push('"');
        s.push_str(&escape_js(l));
        s.push('"');
    }
    s
}

fn escape_js(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        // Escape `<` so a literal "</script>" in a user-supplied
        // title/label/legend can't terminate the inline <script> block (which
        // would break the chart and is a stored-injection vector). `>`/`&`
        // aren't special inside a <script> JS string, so leave them alone to
        // avoid churning legitimate labels (e.g. the "|00>" ket strings).
        .replace('<', "\\u003c")
}

fn format_range(lim: (Option<f64>, Option<f64>)) -> String {
    match lim {
        // `autorange: false` together with the explicit range disables Plotly's
        // tendency to silently expand the axis to fit data — important for
        // Smith charts whose stability/gain-circle overlays reach far outside
        // the unit disk and would otherwise stretch the chart.
        (Some(lo), Some(hi)) => format!(", range: [{}, {}], autorange: false", lo, hi),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::figure::{ContourData, FigureState, HeatmapData, PlotKind, Series};
    use crate::{LineStyle, SeriesColor, Theme};

    #[test]
    fn imagesc_html_axis_xy_does_not_reverse_y_axis() {
        // Per-panel `axis("xy")` opts out of image convention; the
        // Plotly trace's y-axis stays in standard ascending order
        // (no `autorange: "reversed"`).
        let mut fig = FigureState::new();
        let sp = fig.current_mut();
        sp.y_axis_direction = crate::figure::AxisYDirection::Xy;
        sp.heatmap = Some(HeatmapData {
            z: vec![vec![1.0, 0.0], vec![0.0, 0.0]],
            colorscale: "viridis".to_string(),
            kind: crate::figure::HeatmapKind::Imagesc,
            x_labels: None,
            y_labels: None,
            rgba: None,
            rgba_width: 0,
            rgba_height: 0,
            value_min: None,
            value_max: None,
            origin: crate::figure::HeatmapOrigin::Lower,
        });

        let div = render_figure_plotly_div(&fig, "plot", Theme::default().colors());
        assert!(
            !div.contains(r#"autorange: "reversed""#),
            "axis-xy must NOT reverse the Plotly y-axis; got:\n{div}",
        );
    }

    #[test]
    fn imagesc_html_emits_reversed_y_axis_for_image_convention() {
        // Regression for 2026-05-16: imagesc HTML output previously did
        // not reverse the y-axis. With Plotly's default heatmap
        // orientation (z[0] at the bottom), this rendered row 0 at the
        // bottom of the chart — opposite of image convention and
        // opposite of the SVG backend's render. Fix: include `Imagesc`
        // in the autorange-reversed set so row 0 appears at the top in
        // Plotly too.
        let mut fig = FigureState::new();
        fig.current_mut().heatmap = Some(HeatmapData {
            z: vec![vec![1.0, 0.0], vec![0.0, 0.0]],
            colorscale: "viridis".to_string(),
            kind: crate::figure::HeatmapKind::Imagesc,
            x_labels: None,
            y_labels: None,
            rgba: None,
            rgba_width: 0,
            rgba_height: 0,
            value_min: None,
            value_max: None,
            origin: crate::figure::HeatmapOrigin::Lower,
        });

        let div = render_figure_plotly_div(&fig, "plot", Theme::default().colors());

        assert!(
            div.contains(r#"autorange: "reversed""#),
            "Imagesc must use a reversed y-axis (image convention); got:\n{div}",
        );
    }

    #[test]
    fn categorical_bar_plotly_emits_category_axis_and_label_xs() {
        // Regression: bar(labels, y) used to emit numeric x=[1,2,3,4] on a
        // default (linear) axis with tickvals/ticktext that Plotly ignored,
        // so the label strings never appeared. Now we set type:"category"
        // and feed the label strings as x values.
        let mut fig = FigureState::new();
        let labels = vec![
            "|00>".to_string(),
            "|01>".to_string(),
            "|10>".to_string(),
            "|11>".to_string(),
        ];
        let sp = fig.current_mut();
        sp.x_labels = Some(labels.clone());
        sp.series.push(Series {
            label: "bar".to_string(),
            x_data: vec![1.0, 2.0, 3.0, 4.0],
            y_data: vec![0.25, 0.12, 0.48, 0.15],
            color: SeriesColor::Cyan,
            style: LineStyle::Solid,
            kind: PlotKind::Bar,
        });

        let div = render_figure_plotly_div(&fig, "plot", Theme::default().colors());

        assert!(
            div.contains(r#"type: "category""#),
            "x-axis should be category type; got:\n{}",
            div
        );
        assert!(
            div.contains(r#"categoryorder: "array""#),
            "category order should be 'array' to preserve user order"
        );
        // `>` is left un-escaped (only `<` is, to block "</script>"), so the
        // ket labels round-trip verbatim.
        assert!(
            div.contains(r#"categoryarray: ["|00>","|01>","|10>","|11>"]"#),
            "categoryarray should list labels in order; got:\n{}",
            div
        );
        // Trace x should carry the label strings, not numeric indices.
        assert!(
            div.contains(r#"x: ["|00>","|01>","|10>","|11>"]"#),
            "bar trace x should be the label strings; got:\n{}",
            div
        );
    }

    #[test]
    fn categorical_plot_plotly_emits_category_axis_and_label_xs() {
        // Regression for Phase 3 of notebook_future.md: plot(labels, y) must
        // emit a Plotly category axis and feed the label strings as the Line
        // trace's x values, the same way bar(labels, y) does.
        let mut fig = FigureState::new();
        let labels = vec![
            "Mon".to_string(),
            "Tue".to_string(),
            "Wed".to_string(),
            "Thu".to_string(),
            "Fri".to_string(),
        ];
        let sp = fig.current_mut();
        sp.x_labels = Some(labels.clone());
        sp.series.push(Series {
            label: "value".to_string(),
            x_data: vec![1.0, 2.0, 3.0, 4.0, 5.0],
            y_data: vec![12.0, 19.0, 14.0, 22.0, 18.0],
            color: SeriesColor::Cyan,
            style: LineStyle::Solid,
            kind: PlotKind::Line,
        });

        let div = render_figure_plotly_div(&fig, "plot", Theme::default().colors());

        assert!(
            div.contains(r#"type: "category""#),
            "x-axis should be category type; got:\n{}",
            div
        );
        assert!(
            div.contains(r#"categoryarray: ["Mon","Tue","Wed","Thu","Fri"]"#),
            "categoryarray should list labels in order"
        );
        assert!(
            div.contains(r#"x: ["Mon","Tue","Wed","Thu","Fri"]"#),
            "line trace x should carry the label strings, not numeric indices; got:\n{}",
            div
        );
    }

    #[test]
    fn axis_equal_html_emits_scaleanchor() {
        // axis("equal") on a line/scatter panel must emit Plotly's
        // `scaleanchor: "x"` on the y-axis layout — same mechanism heatmaps
        // already use, just generalised to non-heatmap panels.
        let mut fig = FigureState::new();
        let sp = fig.current_mut();
        sp.axis_equal = true;
        sp.series.push(Series {
            label: "circle".to_string(),
            x_data: (0..32)
                .map(|i| (i as f64 / 32.0 * std::f64::consts::TAU).cos())
                .collect(),
            y_data: (0..32)
                .map(|i| (i as f64 / 32.0 * std::f64::consts::TAU).sin())
                .collect(),
            color: SeriesColor::Cyan,
            style: LineStyle::Solid,
            kind: PlotKind::Line,
        });

        let div = render_figure_plotly_div(&fig, "plot", Theme::default().colors());
        assert!(
            div.contains(r#"scaleanchor: "x""#),
            "axis_equal panel should emit scaleanchor on y-axis; got:\n{}",
            div
        );
        // Also pin `constrain: "domain"` — without it, Plotly auto-expands
        // the explicit xlim/ylim to fit data that overflows, which squashes
        // Smith charts when stability/gain circles lie outside the unit disk.
        assert!(
            div.contains(r#"constrain: "domain""#),
            "axis_equal panel must emit constrain: domain so explicit ranges hold; got:\n{}",
            div
        );

        // Sanity: with axis_equal off, no scaleanchor on a non-heatmap panel.
        let mut fig2 = FigureState::new();
        let sp2 = fig2.current_mut();
        sp2.series.push(Series {
            label: "circle".to_string(),
            x_data: vec![0.0, 1.0],
            y_data: vec![0.0, 1.0],
            color: SeriesColor::Cyan,
            style: LineStyle::Solid,
            kind: PlotKind::Line,
        });
        let div2 = render_figure_plotly_div(&fig2, "plot", Theme::default().colors());
        assert!(
            !div2.contains("scaleanchor"),
            "non-heatmap panel without axis_equal must not emit scaleanchor"
        );
    }

    #[test]
    fn subplot_axis_titles_anchor_to_their_own_panel() {
        // A lower panel's x-axis used to be anchored to the first y-axis, so
        // its title was drawn on the row above, through that row's title.
        let mut fig = FigureState::new();
        fig.set_subplot(2, 2, 1);
        for (idx, name) in [(1, "TL"), (2, "TR"), (3, "BL"), (4, "BR")] {
            fig.set_subplot(2, 2, idx);
            let sp = fig.current_mut();
            sp.title = format!("Title {name}");
            sp.xlabel = format!("X {name}");
            sp.ylabel = format!("Y {name}");
            sp.series.push(Series {
                label: String::new(),
                x_data: vec![0.0, 1.0],
                y_data: vec![0.0, 1.0],
                color: SeriesColor::Cyan,
                style: LineStyle::Solid,
                kind: PlotKind::Line,
            });
        }
        let div = render_figure_plotly_div(&fig, "plot", Theme::default().colors());
        for (x, y) in [("x", "y"), ("x2", "y2"), ("x3", "y3"), ("x4", "y4")] {
            assert!(
                div.contains(&format!(r#"anchor: "{y}""#)),
                "{x} must anchor to {y}"
            );
            assert!(
                div.contains(&format!(r#"anchor: "{x}""#)),
                "{y} must anchor to {x}"
            );
        }
        assert!(
            div.contains(r#"yanchor: "bottom""#),
            "panel titles must sit above the axes, growing upward"
        );
        assert!(
            div.contains("automargin: true"),
            "long tick labels must be allowed to grow the margin"
        );
    }

    #[test]
    fn multiline_title_becomes_a_break_and_keeps_theme_color() {
        let mut fig = FigureState::new();
        fig.current_mut().title = "First line of the title\nSecond line stays clear".into();
        fig.current_mut().series.push(Series {
            label: String::new(),
            x_data: vec![0.0, 1.0],
            y_data: vec![1.0, 2.0],
            color: SeriesColor::Cyan,
            style: LineStyle::Solid,
            kind: PlotKind::Line,
        });
        let theme = Theme::default().colors();
        let div = render_figure_plotly_div(&fig, "plot", theme);
        assert!(
            div.contains("First line of the title<br>Second line stays clear"),
            "newline in a title must be a Plotly line break; got:\n{div}"
        );
        assert!(
            div.contains(&format!(r#"color: "{}""#, theme.text)),
            "title text must use the theme foreground"
        );
    }

    #[test]
    fn line_contour_emits_plotly_contour_trace() {
        let mut fig = FigureState::new();
        let z = vec![
            vec![0.0, 0.5, 1.0],
            vec![0.5, 1.0, 1.5],
            vec![1.0, 1.5, 2.0],
        ];
        fig.current_mut().contours.push(ContourData {
            z,
            x: vec![0.0, 1.0, 2.0],
            y: vec![0.0, 1.0, 2.0],
            levels: vec![0.5, 1.0, 1.5],
            filled: false,
            line_color: Some(SeriesColor::Black),
            colorscale: "viridis".to_string(),
        });
        let div = render_figure_plotly_div(&fig, "plot", Theme::default().colors());
        assert!(
            div.contains(r#"type: "contour""#),
            "expected a contour trace; got:\n{div}"
        );
        assert!(
            div.contains(r#"showlines: true"#),
            "line contour must request showlines"
        );
        assert!(
            div.contains(r#"coloring: "none""#),
            "line contour must use coloring:'none' so line.color is honoured"
        );
    }

    #[test]
    fn filled_contour_emits_fill_coloring() {
        let mut fig = FigureState::new();
        let z = vec![vec![0.0, 1.0], vec![2.0, 3.0]];
        fig.current_mut().contours.push(ContourData {
            z,
            x: vec![0.0, 1.0],
            y: vec![0.0, 1.0],
            levels: vec![0.5, 1.5, 2.5],
            filled: true,
            line_color: None,
            colorscale: "viridis".to_string(),
        });
        let div = render_figure_plotly_div(&fig, "plot", Theme::default().colors());
        assert!(
            div.contains(r#"coloring: "fill""#),
            "filled contour must set coloring:'fill'; got:\n{div}"
        );
        assert!(div.contains(r#"colorscale: "Viridis""#));
    }

    #[test]
    fn heatmap_with_contour_emits_both_traces() {
        let mut fig = FigureState::new();
        let z = vec![vec![0.0, 1.0], vec![2.0, 3.0]];
        fig.current_mut().heatmap = Some(HeatmapData {
            z: z.clone(),
            colorscale: "viridis".to_string(),
            ..Default::default()
        });
        fig.current_mut().contours.push(ContourData {
            z,
            x: vec![0.0, 1.0],
            y: vec![0.0, 1.0],
            levels: vec![0.5, 1.5],
            filled: false,
            line_color: Some(SeriesColor::Black),
            colorscale: "viridis".to_string(),
        });
        let div = render_figure_plotly_div(&fig, "plot", Theme::default().colors());
        assert!(div.contains(r#"type: "heatmap""#), "heatmap missing");
        assert!(div.contains(r#"type: "contour""#), "contour missing");
    }

    fn line_series(n: usize) -> Series {
        Series {
            label: "s".to_string(),
            x_data: (0..n).map(|i| i as f64).collect(),
            y_data: (0..n).map(|i| (i as f64).sin()).collect(),
            color: SeriesColor::Cyan,
            style: LineStyle::Solid,
            kind: PlotKind::Line,
        }
    }

    #[test]
    fn series_over_10k_points_uses_webgl_scattergl_trace() {
        // Traces beyond 10k points must switch to Plotly's WebGL backend
        // (type "scattergl") to stay responsive in the browser.
        let mut fig = FigureState::new();
        fig.current_mut().series.push(line_series(10_001));
        let div = render_figure_plotly_div(&fig, "plot", Theme::default().colors());
        assert!(
            div.contains(r#"type: "scattergl""#),
            "10_001-point series should emit a scattergl trace; got:\n{}",
            &div[..div.len().min(600)]
        );
    }

    #[test]
    fn series_at_or_below_10k_points_uses_dom_scatter_trace() {
        // At exactly the threshold (10k points) the DOM renderer is kept.
        let mut fig = FigureState::new();
        fig.current_mut().series.push(line_series(10_000));
        let div = render_figure_plotly_div(&fig, "plot", Theme::default().colors());
        assert!(div.contains(r#"type: "scatter""#), "scatter trace missing");
        assert!(
            !div.contains("scattergl"),
            "a 10_000-point series must NOT fall back to WebGL"
        );
    }

    #[test]
    fn json_f64_array_formats_values_and_maps_nonfinite_to_null() {
        assert_eq!(json_f64_array(&[]), "[]");
        assert_eq!(json_f64_array(&[1.5, 2.0, -0.25]), "[1.5,2,-0.25]");
        // NaN and infinities are not valid JSON numbers; they become null so
        // Plotly renders a gap instead of throwing a parse error.
        assert_eq!(
            json_f64_array(&[f64::NAN, 3.0, f64::INFINITY, f64::NEG_INFINITY]),
            "[null,3,null,null]"
        );
    }

    #[test]
    fn arrows_to_polyline_emits_tail_tip_barbs_with_null_separators() {
        use crate::quiver::Arrow;
        assert_eq!(
            arrows_to_polyline(&[]),
            ("[]".to_string(), "[]".to_string())
        );

        let arrows = [
            Arrow {
                shaft: ((0.0, 0.0), (1.0, 0.0)),
                head: [(0.8, 0.1), (1.0, 0.0), (0.8, -0.1)],
            },
            Arrow {
                shaft: ((2.0, 1.0), (3.0, 1.0)),
                head: [(2.8, 1.1), (3.0, 1.0), (2.8, 0.9)],
            },
        ];
        let (xs, ys) = arrows_to_polyline(&arrows);
        // Per arrow: tail, tip, head[0], head[1], head[2]; arrows separated
        // by a null so Plotly draws disconnected glyphs in one trace.
        assert_eq!(xs, "[0,1,0.8,1,0.8,null,2,3,2.8,3,2.8]");
        assert_eq!(ys, "[0,0,0.1,0,-0.1,null,1,1,1.1,1,0.9]");
    }

    #[test]
    fn paths_to_polyline_separates_paths_with_null_and_skips_empty() {
        assert_eq!(paths_to_polyline(&[]), ("[]".to_string(), "[]".to_string()));

        let paths = vec![
            vec![], // empty: skipped, no leading null
            vec![(0.0, 0.0), (1.0, 2.0)],
            vec![], // empty in the middle: skipped
            vec![(3.0, 4.5)],
        ];
        let (xs, ys) = paths_to_polyline(&paths);
        assert_eq!(xs, "[0,1,null,3]");
        assert_eq!(ys, "[0,2,null,4.5]");
    }

    #[test]
    fn savefig_html_uses_sibling_plotly_not_a_cdn() {
        let dir = std::env::temp_dir().join(format!("rustlab-savefig-html-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("figure.html");
        let path_str = path.to_str().unwrap();
        render_figure_state_html(&FigureState::new(), path_str).unwrap();
        let html = std::fs::read_to_string(&path).unwrap();
        assert!(
            !crate::vendor::html_loads_remote_script_or_style(&html),
            "savefig HTML loaded a remote script or stylesheet:\n{html}"
        );
        assert!(html.contains("src=\"assets/plotly.min.js\""));
        let js = dir.join("assets/plotly.min.js");
        let len = std::fs::metadata(&js).unwrap().len();
        assert!(len > 1_000_000, "sibling plotly bundle too small: {len}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
