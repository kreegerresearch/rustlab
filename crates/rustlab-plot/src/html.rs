//! Export the current `FIGURE` state to a self-contained HTML file using
//! Plotly.js (loaded from CDN).

use std::cell::RefCell;

use crate::error::PlotError;
use crate::figure::{FigureState, LineStyle, PlotKind, SeriesColor, FIGURE};
use crate::theme::{Theme, ThemeColors};

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
    render_figure_state_html_themed(fig, path, Theme::default().colors())
}

/// Render a `FigureState` to an HTML file with Plotly using the given theme.
pub fn render_figure_state_html_themed(
    fig: &FigureState,
    path: &str,
    theme: &ThemeColors,
) -> Result<(), PlotError> {
    let div_content = render_figure_plotly_div(fig, "plot", theme);

    let mut html = String::with_capacity(4096 + div_content.len());
    html.push_str(&format!(
        r##"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>RustLab Plot</title>
<script src="https://cdn.plot.ly/plotly-2.35.0.min.js"></script>
<style>
  body {{ margin: 0; background: {bg}; }}
  #plot {{ width: 100vw; height: 100vh; }}
</style>
</head>
<body>
"##,
        bg = theme.bg
    ));
    html.push_str(&div_content);
    html.push_str(
        r##"</body>
</html>
"##,
    );

    std::fs::write(path, html).map_err(|e| PlotError::FileOutput(e.to_string()))
}

/// Render a `FigureState` as a Plotly `<div>` + `<script>` fragment.
/// The `div_id` is used as the element ID for `Plotly.newPlot()`.
/// This is the shared building block for both single-file HTML export
/// and multi-figure report generation.
pub fn render_figure_plotly_div(fig: &FigureState, div_id: &str, theme: &ThemeColors) -> String {
    let rows = fig.subplot_rows;
    let cols = fig.subplot_cols;
    let n_panels = rows * cols;

    let mut traces = String::new();
    let mut layout_axes = String::new();
    let mut scenes = String::new();
    let mut annotations = String::new();

    for (idx, panel) in fig.subplots.iter().enumerate().take(n_panels) {
        let row = idx / cols;
        let col = idx % cols;

        // Plotly subplot axis naming: xaxis, xaxis2, xaxis3, ...
        let axis_suffix = if idx == 0 {
            String::new()
        } else {
            format!("{}", idx + 1)
        };
        let xaxis_ref = format!("x{}", axis_suffix);
        let yaxis_ref = format!("y{}", axis_suffix);

        // Domain computation for subplot positioning
        let col_width = 1.0 / cols as f64;
        let row_height = 1.0 / rows as f64;
        let gap = 0.08;
        let x_start = col as f64 * col_width + gap / 2.0;
        let x_end = (col + 1) as f64 * col_width - gap / 2.0;
        // Plotly y-axis goes bottom-to-top, but we want row 0 at top
        let y_start = 1.0 - (row + 1) as f64 * row_height + gap / 2.0;
        let y_end = 1.0 - row as f64 * row_height - gap / 2.0;

        // Axis layout
        let show_grid = if panel.grid { "true" } else { "false" };
        // Categorical x-axis: switch Plotly into category mode and preserve
        // the user-provided label order. Traces below emit their x values as
        // the label strings directly, so tickvals/ticktext are not needed.
        let xtick_extra = if let Some(labels) = &panel.x_labels {
            let category_array: Vec<String> = labels
                .iter()
                .map(|l| format!("\"{}\"", escape_js(l)))
                .collect();
            format!(
                r#", type: "category", categoryorder: "array", categoryarray: [{}]"#,
                category_array.join(","),
            )
        } else {
            String::new()
        };
        // Square aspect ratio for heatmaps. For `heatmap()` and `image()` also
        // reverse the y-axis so row 0 lands at the top (standard image/data
        // orientation). `imagesc` HTML output is left unflipped to preserve
        // existing behaviour; the SVG/HTML divergence on imagesc is tracked
        // separately.
        let yaxis_extra = if let Some(hm) = &panel.heatmap {
            let anchor = if axis_suffix.is_empty() {
                "x".to_string()
            } else {
                format!("x{axis_suffix}")
            };
            let reversed = matches!(
                hm.kind,
                crate::figure::HeatmapKind::Heatmap | crate::figure::HeatmapKind::ImageRgba
            );
            if reversed {
                format!(r#", scaleanchor: "{anchor}", autorange: "reversed""#)
            } else {
                format!(r#", scaleanchor: "{anchor}""#)
            }
        } else if panel.axis_equal && panel.surface.is_none() {
            // axis("equal") for line/scatter panels — same scaleanchor pattern
            // as heatmaps, no autorange override.
            let anchor = if axis_suffix.is_empty() {
                "x".to_string()
            } else {
                format!("x{axis_suffix}")
            };
            format!(r#", scaleanchor: "{anchor}""#)
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
                r#"{scene_key}: {{ domain: {{ x: [{x0:.4}, {x1:.4}], y: [{y0:.4}, {y1:.4}] }}, xaxis: {{ title: {{ text: "{xlabel}" }}, color: "{text}" }}, yaxis: {{ title: {{ text: "{ylabel}" }}, color: "{text}" }}, zaxis: {{ color: "{text}" }}, bgcolor: "{plot_bg}" }},
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
            layout_axes.push_str(&format!(
                r#"xaxis{ax}: {{ domain: [{x0:.4}, {x1:.4}], title: {{ text: "{xlabel}" }}{xrange}, showgrid: {grid}, gridcolor: "{plot_grid}"{xtick} }},
yaxis{ax}: {{ domain: [{y0:.4}, {y1:.4}], title: {{ text: "{ylabel}" }}{yrange}, showgrid: {grid}, gridcolor: "{plot_grid}"{yextra} }},
"#,
                ax = axis_suffix,
                x0 = x_start, x1 = x_end,
                y0 = y_start, y1 = y_end,
                grid = show_grid,
                plot_grid = theme.plot_grid,
                xlabel = escape_js(&panel.xlabel),
                ylabel = escape_js(&panel.ylabel),
                xrange = format_range(panel.xlim),
                yrange = format_range(panel.ylim),
                xtick = xtick_extra,
                yextra = yaxis_extra,
            ));
        }

        // Title as annotation
        if !panel.title.is_empty() {
            annotations.push_str(&format!(
                r#"{{ text: "{title}", xref: "paper", yref: "paper", x: {cx:.4}, y: {ty:.4}, showarrow: false, font: {{ size: 14 }} }},
"#,
                title = escape_js(&panel.title),
                cx = (x_start + x_end) / 2.0,
                ty = y_end + 0.03,
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
                r#"{{ type: "surface", z: {z}, x: {x}, y: {y}, colorscale: "{cmap}", showscale: true, scene: "{scene}" }},
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
                    traces.push_str(&format!(
                        r#"{{ z: {z}, type: "heatmap", colorscale: "{cmap}", showscale: true, xaxis: "{xa}", yaxis: "{ya}" }},
"#,
                        z = z_json,
                        cmap = plotly_cmap,
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
                    let x_field = if let Some(labels) = &hm.x_labels {
                        let parts: Vec<String> = labels
                            .iter()
                            .map(|l| format!("\"{}\"", escape_js(l)))
                            .collect();
                        format!(", x: [{}]", parts.join(","))
                    } else {
                        String::new()
                    };
                    let y_field = if let Some(labels) = &hm.y_labels {
                        let parts: Vec<String> = labels
                            .iter()
                            .map(|l| format!("\"{}\"", escape_js(l)))
                            .collect();
                        format!(", y: [{}]", parts.join(","))
                    } else {
                        String::new()
                    };
                    traces.push_str(&format!(
                        r#"{{ z: {z}{xfield}{yfield}, type: "heatmap", colorscale: "{cmap}", showscale: true, xaxis: "{xa}", yaxis: "{ya}" }},
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
                    r#"{{ z: {z}, x: {x}, y: {y}, type: "contour", contours: {{ coloring: "fill", start: {s}, end: {e}, size: {step} }}, colorscale: "{cmap}", showscale: true, xaxis: "{xa}", yaxis: "{ya}" }},
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
            if arrows.is_empty() { continue; }
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
            if seeds.is_empty() { continue; }
            let step = crate::streamline::default_step(&sd.x, &sd.y);
            let ref_len = crate::quiver::cell_distance(&sd.x, &sd.y) * 0.5;
            let mut paths: Vec<Vec<(f64, f64)>> = Vec::new();
            let mut arrowheads: Vec<crate::quiver::Arrow> = Vec::new();
            for (sx, sy) in seeds {
                let pts = crate::streamline::integrate(
                    &sd.u, &sd.v, &sd.x, &sd.y, sx, sy, step, 400, 1e-10,
                );
                if pts.len() < 2 { continue; }
                if let Some(a) = crate::quiver::midpoint_arrow(&pts, ref_len) {
                    arrowheads.push(a);
                }
                paths.push(pts);
            }
            if paths.is_empty() { continue; }
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
                    traces.push_str(&format!(
                        r#"{{ x: {x}, y: {y}, type: "{stype}", mode: "lines", name: "{name}", line: {{ color: "{color}", dash: "{dash}" }}, xaxis: "{xa}", yaxis: "{ya}" }},
"#,
                        stype = scatter_type,
                        x = json_f64_array(&series.x_data),
                        y = json_f64_array(&series.y_data),
                        name = escape_js(&series.label),
                        color = color_str,
                        dash = dash,
                        xa = xaxis_ref,
                        ya = yaxis_ref,
                    ));
                }
                PlotKind::Scatter => {
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
                PlotKind::Bar => {
                    // Categorical bar: when the subplot has x_labels that
                    // match this series 1:1, feed the labels in as x so
                    // Plotly's type="category" axis renders them correctly.
                    let x_json = match &panel.x_labels {
                        Some(labels) if labels.len() == series.x_data.len() => {
                            let items: Vec<String> = labels
                                .iter()
                                .map(|l| format!("\"{}\"", escape_js(l)))
                                .collect();
                            format!("[{}]", items.join(","))
                        }
                        _ => json_f64_array(&series.x_data),
                    };
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
                    let mut sx = Vec::new();
                    let mut sy = Vec::new();
                    for (&xi, &yi) in series.x_data.iter().zip(series.y_data.iter()) {
                        sx.push(format!("{}", xi));
                        sx.push(format!("{}", xi));
                        sx.push("null".to_string());
                        sy.push("0".to_string());
                        sy.push(format!("{}", yi));
                        sy.push("null".to_string());
                    }
                    // Stem lines
                    traces.push_str(&format!(
                        r#"{{ x: [{sx}], y: [{sy}], type: "{stype}", mode: "lines", name: "{name}", line: {{ color: "{color}" }}, xaxis: "{xa}", yaxis: "{ya}", showlegend: false }},
"#,
                        stype = scatter_type,
                        sx = sx.join(","),
                        sy = sy.join(","),
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
  margin: {{ t: 60, b: 60, l: 70, r: 30 }},
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

fn color_to_css(c: &SeriesColor) -> String {
    match c {
        SeriesColor::Blue => "rgb(31,119,180)".into(),
        SeriesColor::Red => "rgb(214,39,40)".into(),
        SeriesColor::Green => "rgb(44,160,44)".into(),
        SeriesColor::Cyan => "rgb(23,190,207)".into(),
        SeriesColor::Magenta => "rgb(148,103,189)".into(),
        SeriesColor::Yellow => "rgb(188,189,34)".into(),
        SeriesColor::Black => "rgb(0,0,0)".into(),
        SeriesColor::White => "rgb(255,255,255)".into(),
        SeriesColor::Rgb(r, g, b) => format!("rgb({},{},{})", r, g, b),
    }
}

/// Flatten a slice of arrow glyphs into a pair of Plotly JSON arrays
/// (`x`, `y`) with `null` separators between arrows. Each arrow is emitted
/// as `tail, tip, leftBarb, tip, rightBarb, null`.
fn arrows_to_polyline(arrows: &[crate::quiver::Arrow]) -> (String, String) {
    let mut out_x = String::from("[");
    let mut out_y = String::from("[");
    let mut first = true;
    for a in arrows {
        let pts: [(f64, f64); 5] = [
            a.shaft.0, a.shaft.1, a.head[0], a.head[1], a.head[2],
        ];
        if !first {
            out_x.push_str(",null");
            out_y.push_str(",null");
        }
        for (i, p) in pts.iter().enumerate() {
            if i > 0 || !first {
                out_x.push(',');
                out_y.push(',');
            }
            out_x.push_str(&format!("{}", p.0));
            out_y.push_str(&format!("{}", p.1));
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
        if path.is_empty() { continue; }
        if !first {
            out_x.push_str(",null");
            out_y.push_str(",null");
        }
        for (i, p) in path.iter().enumerate() {
            if i > 0 || !first {
                out_x.push(',');
                out_y.push(',');
            }
            out_x.push_str(&format!("{}", p.0));
            out_y.push_str(&format!("{}", p.1));
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
            s.push_str(&format!("{}", v));
        } else {
            s.push_str("null");
        }
    }
    s.push(']');
    s
}

fn escape_js(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn format_range(lim: (Option<f64>, Option<f64>)) -> String {
    match lim {
        (Some(lo), Some(hi)) => format!(", range: [{}, {}]", lo, hi),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::figure::{ContourData, FigureState, HeatmapData, PlotKind, Series};
    use crate::{LineStyle, SeriesColor, Theme};

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
        assert!(
            div.contains(r#"categoryarray: ["|00>","|01>","|10>","|11>"]"#),
            "categoryarray should list labels in order"
        );
        // Trace x should carry the label strings, not numeric indices.
        assert!(
            div.contains(r#"x: ["|00>","|01>","|10>","|11>"]"#),
            "bar trace x should be the label strings; got:\n{}",
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
}
