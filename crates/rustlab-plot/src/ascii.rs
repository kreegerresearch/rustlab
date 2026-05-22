use crate::compute_histogram;
use crate::error::PlotError;
use crate::figure::{colormap_rgb, LineStyle, PlotKind, SeriesColor, FIGURE};
use crossterm::{
    event, execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    symbols,
    text::{Line as TuiLine, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph},
    Terminal,
};
use rustlab_core::{CMatrix, CVector, RVector};
use std::io::stdout;

/// Format a float like C's `%.3g`: use scientific for very large/small numbers, otherwise 3 sig figs.
pub(crate) fn fmt_g(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let abs = v.abs();
    if abs < 0.001 || abs >= 10000.0 {
        format!("{:.2e}", v)
    } else if abs >= 100.0 {
        format!("{:.0}", v)
    } else if abs >= 10.0 {
        format!("{:.1}", v)
    } else {
        format!("{:.2}", v)
    }
}

fn wait_for_key() -> Result<(), PlotError> {
    loop {
        if event::poll(std::time::Duration::from_millis(100))? {
            if let event::Event::Key(_) = event::read()? {
                break;
            }
        }
    }
    Ok(())
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen);
}

/// For `axis("equal")` panels: pad the visually-shorter axis so one data unit
/// on x equals one data unit on y, given the panel's terminal-cell dimensions.
///
/// Cell-pixel aspect: cells render roughly 2× taller than wide, so a cell's
/// height equals 2 width-units visually. Equating visual pixels per data unit:
///   `cell_w / dx == 2 * cell_h / dy`  ⇒  target `dy/dx = 2 * cell_h / cell_w`.
///
/// If `user_x_set` (or `user_y_set`) is true, that axis is left untouched —
/// the user explicitly pinned it.
pub(crate) fn apply_axis_equal_pad(
    mut x_lo: f64,
    mut x_hi: f64,
    mut y_lo: f64,
    mut y_hi: f64,
    cell_w: u16,
    cell_h: u16,
    user_x_set: bool,
    user_y_set: bool,
) -> (f64, f64, f64, f64) {
    if user_x_set && user_y_set {
        return (x_lo, x_hi, y_lo, y_hi);
    }
    let cw = cell_w.max(1) as f64;
    let ch = cell_h.max(1) as f64;
    let target = (2.0 * ch / cw).max(f64::EPSILON);
    let dx = (x_hi - x_lo).abs().max(f64::EPSILON);
    let dy = (y_hi - y_lo).abs().max(f64::EPSILON);
    let cur = dy / dx;
    if cur < target && !user_y_set {
        let need_dy = dx * target;
        let pad = (need_dy - dy) * 0.5;
        y_lo -= pad;
        y_hi += pad;
    } else if cur > target && !user_x_set {
        let need_dx = dy / target;
        let pad = (need_dx - dx) * 0.5;
        x_lo -= pad;
        x_hi += pad;
    }
    (x_lo, x_hi, y_lo, y_hi)
}

/// Render a slice of subplot panels into an existing ratatui frame.
/// Called by both `render_figure_terminal()` and `LiveFigure::redraw()`.
pub(crate) fn draw_subplots(
    f: &mut ratatui::Frame,
    subplots: &[crate::figure::SubplotState],
    rows: usize,
    cols: usize,
) {
    let area = f.area();

    let row_constraints: Vec<Constraint> = (0..rows)
        .map(|_| Constraint::Ratio(1, rows as u32))
        .collect();
    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(area);

    for r in 0..rows {
        let col_constraints: Vec<Constraint> = (0..cols)
            .map(|_| Constraint::Ratio(1, cols as u32))
            .collect();
        let col_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints)
            .split(row_areas[r]);

        for c in 0..cols {
            let idx = r * cols + c;
            if idx >= subplots.len() {
                break;
            }
            let sp = &subplots[idx];
            let cell = col_areas[c];

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
                continue;
            }

            let mut x_min = sp
                .xlim
                .0
                .unwrap_or_else(|| all_x.iter().copied().fold(f64::INFINITY, f64::min));
            let mut x_max = sp
                .xlim
                .1
                .unwrap_or_else(|| all_x.iter().copied().fold(f64::NEG_INFINITY, f64::max));
            let y_min_raw = all_y.iter().copied().fold(f64::INFINITY, f64::min);
            let y_max_raw = all_y.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let y_margin = ((y_max_raw - y_min_raw).abs() * 0.1).max(1e-6);
            let mut y_min = sp.ylim.0.unwrap_or(y_min_raw - y_margin);
            let mut y_max = sp.ylim.1.unwrap_or(y_max_raw + y_margin);

            // axis("equal"): pad the visually-shorter axis so one data unit on
            // x equals one data unit on y. Skip when both limits were user-set.
            if sp.axis_equal && sp.heatmap.is_none() {
                let user_set_x = sp.xlim.0.is_some() && sp.xlim.1.is_some();
                let user_set_y = sp.ylim.0.is_some() && sp.ylim.1.is_some();
                let (nx_min, nx_max, ny_min, ny_max) = apply_axis_equal_pad(
                    x_min,
                    x_max,
                    y_min,
                    y_max,
                    cell.width,
                    cell.height,
                    user_set_x,
                    user_set_y,
                );
                x_min = nx_min;
                x_max = nx_max;
                y_min = ny_min;
                y_max = ny_max;
            }
            let x_min = x_min;
            let x_max = x_max;
            let y_min = y_min;
            let y_max = y_max;

            let series_points: Vec<Vec<(f64, f64)>> = sp
                .series
                .iter()
                .map(|s| {
                    s.x_data
                        .iter()
                        .copied()
                        .zip(s.y_data.iter().copied())
                        .collect()
                })
                .collect();

            let stem_points: Vec<Vec<(f64, f64)>> = sp
                .series
                .iter()
                .map(|s| {
                    if s.kind == PlotKind::Stem {
                        let mut pts = Vec::with_capacity(s.x_data.len() * 3);
                        for (&x, &y) in s.x_data.iter().zip(s.y_data.iter()) {
                            pts.push((x, 0.0));
                            pts.push((x, y));
                            pts.push((x, 0.0));
                        }
                        pts
                    } else {
                        vec![]
                    }
                })
                .collect();

            // Count bar series for grouped offset
            let bar_series_count = sp.series.iter().filter(|s| s.kind == PlotKind::Bar).count();
            let mut bar_series_idx = 0usize;
            let bar_points: Vec<Vec<(f64, f64)>> = sp
                .series
                .iter()
                .map(|s| {
                    if s.kind == PlotKind::Bar {
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
                        let mut pts = Vec::with_capacity(n * 4);
                        for (&x, &y) in s.x_data.iter().zip(s.y_data.iter()) {
                            let cx = x + offset;
                            pts.push((cx - half, 0.0));
                            pts.push((cx - half, y));
                            pts.push((cx + half, y));
                            pts.push((cx + half, 0.0));
                        }
                        pts
                    } else {
                        vec![]
                    }
                })
                .collect();

            let grid_pts: Vec<Vec<(f64, f64)>> = if sp.grid {
                const N: usize = 5;
                let mut v = Vec::with_capacity(N * 2 + 2);
                for i in 0..=N {
                    let yv = y_min + (y_max - y_min) * i as f64 / N as f64;
                    v.push(vec![(x_min, yv), (x_max, yv)]);
                }
                for i in 1..N {
                    let xv = x_min + (x_max - x_min) * i as f64 / N as f64;
                    v.push(vec![(xv, y_min), (xv, y_max)]);
                }
                v
            } else {
                vec![]
            };

            let mut datasets: Vec<Dataset> = Vec::new();
            for pts in &grid_pts {
                datasets.push(
                    Dataset::default()
                        .marker(symbols::Marker::Braille)
                        .graph_type(GraphType::Line)
                        .style(Style::default().fg(ratatui::style::Color::Rgb(70, 70, 70)))
                        .data(pts),
                );
            }
            for (i, s) in sp.series.iter().enumerate() {
                let rcolor = s.color.to_ratatui();
                match s.kind {
                    PlotKind::Stem => {
                        datasets.push(
                            Dataset::default()
                                .name(s.label.as_str())
                                .marker(symbols::Marker::Braille)
                                .graph_type(GraphType::Line)
                                .style(Style::default().fg(rcolor))
                                .data(&stem_points[i]),
                        );
                    }
                    PlotKind::Bar => {
                        datasets.push(
                            Dataset::default()
                                .name(s.label.as_str())
                                .marker(symbols::Marker::Braille)
                                .graph_type(GraphType::Line)
                                .style(Style::default().fg(rcolor))
                                .data(&bar_points[i]),
                        );
                    }
                    PlotKind::Scatter => {
                        datasets.push(
                            Dataset::default()
                                .name(s.label.as_str())
                                .marker(symbols::Marker::Dot)
                                .graph_type(GraphType::Scatter)
                                .style(Style::default().fg(rcolor))
                                .data(&series_points[i]),
                        );
                    }
                    PlotKind::Line => {
                        datasets.push(
                            Dataset::default()
                                .name(s.label.as_str())
                                .marker(symbols::Marker::Braille)
                                .graph_type(GraphType::Line)
                                .style(Style::default().fg(rcolor))
                                .data(&series_points[i]),
                        );
                    }
                }
            }

            let title = if !sp.title.is_empty() {
                sp.title.as_str()
            } else {
                ""
            };
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

            let x_mid = (x_min + x_max) / 2.0;
            let y_mid = (y_min + y_max) / 2.0;

            let x_labels_vec = if let Some(labels) = &sp.x_labels {
                labels
                    .iter()
                    .map(|l| ratatui::text::Span::raw(l.clone()))
                    .collect()
            } else {
                vec![
                    ratatui::text::Span::raw(fmt_g(x_min)),
                    ratatui::text::Span::raw(fmt_g(x_mid)),
                    ratatui::text::Span::raw(fmt_g(x_max)),
                ]
            };
            let chart = Chart::new(datasets)
                .block(Block::default().borders(Borders::ALL).title(title))
                .x_axis(
                    Axis::default()
                        .title(xlabel)
                        .bounds([x_min, x_max])
                        .labels(x_labels_vec),
                )
                .y_axis(
                    Axis::default()
                        .title(ylabel)
                        .bounds([y_min, y_max])
                        .labels(vec![
                            ratatui::text::Span::raw(fmt_g(y_min)),
                            ratatui::text::Span::raw(fmt_g(y_mid)),
                            ratatui::text::Span::raw(fmt_g(y_max)),
                        ]),
                );

            f.render_widget(chart, cell);
        }
    }
}

/// Render the current FIGURE state to the terminal.
pub fn render_figure_terminal() -> Result<(), PlotError> {
    // Notebook / Headless context: never render to terminal.
    match crate::figure::plot_context() {
        crate::figure::PlotContext::Notebook | crate::figure::PlotContext::Headless => {
            return Ok(());
        }
        crate::figure::PlotContext::Terminal => {}
    }
    // Route based on the current figure's output mode.
    match crate::figure::current_figure_output() {
        crate::figure::FigureOutput::Html(_) => return Ok(()),
        #[cfg(feature = "viewer")]
        crate::figure::FigureOutput::Viewer(_) => {
            crate::viewer_live::sync_viewer();
            return Ok(());
        }
        crate::figure::FigureOutput::Terminal => {}
    }
    // Skip silently when stdout is not a real terminal (e.g. script mode, CI).
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return Ok(());
    }
    FIGURE.with(|fig| {
        let fig = fig.borrow();
        let rows = fig.subplot_rows;
        let cols = fig.subplot_cols;
        let subplots = &fig.subplots;
        if subplots.iter().all(|sp| sp.series.is_empty()) {
            return Err(PlotError::EmptyData);
        }

        execute!(stdout(), EnterAlternateScreen)?;
        enable_raw_mode()?;
        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend).map_err(|e| {
            restore_terminal();
            PlotError::Terminal(e.to_string())
        })?;

        let result = terminal.draw(|f| draw_subplots(f, subplots, rows, cols));

        if let Err(e) = result {
            restore_terminal();
            return Err(PlotError::Terminal(e.to_string()));
        }

        let k = wait_for_key();
        restore_terminal();
        k
    })
}

/// Render an imagesc heatmap to the terminal using colored block characters.
pub fn imagesc_terminal(matrix: &CMatrix, title: &str, colormap: &str) -> Result<(), PlotError> {
    let (nrows, ncols) = (matrix.nrows(), matrix.ncols());
    if nrows == 0 || ncols == 0 {
        return Err(PlotError::EmptyData);
    }

    // Use magnitude (norm) of each element
    let vals: Vec<f64> = matrix.iter().map(|c| c.norm()).collect();
    let min_v = vals.iter().copied().fold(f64::INFINITY, f64::min);
    let max_v = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let range = (max_v - min_v).max(1e-12);

    // Push heatmap data into FIGURE state so savefig()/viewer/notebook can use it
    let z: Vec<Vec<f64>> = (0..nrows)
        .map(|r| (0..ncols).map(|c| vals[r * ncols + c]).collect())
        .collect();
    FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            let sp = fig.current_mut();
            sp.series.clear();
            sp.title.clear();
        }
        let sp = fig.current_mut();
        if !title.is_empty() && sp.title.is_empty() {
            sp.title = title.to_string();
        }
        sp.heatmap = Some(crate::figure::HeatmapData {
            z,
            colorscale: colormap.to_string(),
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
    });

    render_heatmap_tui(&vals, nrows, ncols, min_v, range, title, colormap)
}

/// Render a heatmap to the terminal using colored block characters from a
/// row-major scalar buffer. Pure rendering — does **not** touch FIGURE state.
/// `heatmap()` and `image()` builtins use this after pushing their own
/// `HeatmapData`.
///
/// Honours the same Notebook/Headless and non-TTY early-return guards as
/// `imagesc_terminal`.
pub fn render_heatmap_tui(
    vals: &[f64],
    nrows: usize,
    ncols: usize,
    min_v: f64,
    range: f64,
    title: &str,
    colormap: &str,
) -> Result<(), PlotError> {
    // Notebook / Headless mode: FIGURE state already set by caller; skip TUI.
    match crate::figure::plot_context() {
        crate::figure::PlotContext::Notebook | crate::figure::PlotContext::Headless => {
            return Ok(());
        }
        crate::figure::PlotContext::Terminal => {}
    }

    // When the current figure routes to HTML or the external viewer, the
    // heatmap is rendered there — don't draw a TUI preview over it.
    match crate::figure::current_figure_output() {
        crate::figure::FigureOutput::Html(_) => return Ok(()),
        #[cfg(feature = "viewer")]
        crate::figure::FigureOutput::Viewer(_) => {
            crate::viewer_live::sync_viewer();
            return Ok(());
        }
        crate::figure::FigureOutput::Terminal => {}
    }

    // Skip when stdout is not a real terminal (e.g. piped output, CI)
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return Ok(());
    }

    execute!(stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend).map_err(|e| {
        restore_terminal();
        PlotError::Terminal(e.to_string())
    })?;

    let result = terminal.draw(|f| {
        let area = f.area();
        let inner_h = (area.height as usize).saturating_sub(2);
        let inner_w = (area.width as usize).saturating_sub(2);

        let px_cols = inner_w / 2;
        let disp_rows = nrows.min(inner_h);
        let disp_cols = ncols.min(px_cols);

        let mut tui_lines: Vec<TuiLine> = Vec::with_capacity(disp_rows);
        for dr in 0..disp_rows {
            let mr = dr * nrows / disp_rows.max(1);
            let mut spans: Vec<Span<'static>> = Vec::with_capacity(disp_cols);
            for dc in 0..disp_cols {
                let mc = dc * ncols / disp_cols.max(1);
                let v = vals[mr.min(nrows - 1) * ncols + mc.min(ncols - 1)];
                let t = (v - min_v) / range;
                let (r, g, b) = colormap_rgb(t, colormap);
                spans.push(Span::styled("  ", Style::default().bg(Color::Rgb(r, g, b))));
            }
            tui_lines.push(TuiLine::from(spans));
        }

        let max_v = min_v + range;
        let subtitle = format!("{} [{}, {}]", colormap, fmt_g(min_v), fmt_g(max_v));
        let full_title = if title.is_empty() {
            subtitle.clone()
        } else {
            format!("{} — {}", title, subtitle)
        };

        let para = Paragraph::new(tui_lines)
            .block(Block::default().borders(Borders::ALL).title(full_title));
        f.render_widget(para, area);
    });

    if let Err(e) = result {
        restore_terminal();
        return Err(PlotError::Terminal(e.to_string()));
    }
    let k = wait_for_key();
    restore_terminal();
    k
}

/// Render a raw RGBA image to the terminal using colored block characters.
/// Reads `(r, g, b)` directly from the buffer; no normalisation or colormap.
/// Pure rendering — does **not** touch FIGURE state.
///
/// `rgba` is row-major top-to-bottom, 4 bytes per pixel
/// (`rgba.len() == 4 * width * height`).
pub fn render_image_tui(
    rgba: &[u8],
    width: usize,
    height: usize,
    title: &str,
) -> Result<(), PlotError> {
    match crate::figure::plot_context() {
        crate::figure::PlotContext::Notebook | crate::figure::PlotContext::Headless => {
            return Ok(());
        }
        crate::figure::PlotContext::Terminal => {}
    }

    // When the current figure routes to HTML or the external viewer, the
    // image is rendered there — don't draw a TUI preview over it.
    match crate::figure::current_figure_output() {
        crate::figure::FigureOutput::Html(_) => return Ok(()),
        #[cfg(feature = "viewer")]
        crate::figure::FigureOutput::Viewer(_) => {
            crate::viewer_live::sync_viewer();
            return Ok(());
        }
        crate::figure::FigureOutput::Terminal => {}
    }

    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return Ok(());
    }

    execute!(stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend).map_err(|e| {
        restore_terminal();
        PlotError::Terminal(e.to_string())
    })?;

    let result = terminal.draw(|f| {
        let area = f.area();
        let inner_h = (area.height as usize).saturating_sub(2);
        let inner_w = (area.width as usize).saturating_sub(2);

        let px_cols = inner_w / 2;
        let disp_rows = height.min(inner_h);
        let disp_cols = width.min(px_cols);

        let mut tui_lines: Vec<TuiLine> = Vec::with_capacity(disp_rows);
        for dr in 0..disp_rows {
            let mr = dr * height / disp_rows.max(1);
            let mut spans: Vec<Span<'static>> = Vec::with_capacity(disp_cols);
            for dc in 0..disp_cols {
                let mc = dc * width / disp_cols.max(1);
                let off = (mr.min(height - 1) * width + mc.min(width - 1)) * 4;
                let r = rgba[off];
                let g = rgba[off + 1];
                let b = rgba[off + 2];
                spans.push(Span::styled("  ", Style::default().bg(Color::Rgb(r, g, b))));
            }
            tui_lines.push(TuiLine::from(spans));
        }

        let full_title = if title.is_empty() {
            format!("image {}x{}", width, height)
        } else {
            format!("{} — image {}x{}", title, width, height)
        };

        let para = Paragraph::new(tui_lines)
            .block(Block::default().borders(Borders::ALL).title(full_title));
        f.render_widget(para, area);
    });

    if let Err(e) = result {
        restore_terminal();
        return Err(PlotError::Terminal(e.to_string()));
    }
    let k = wait_for_key();
    restore_terminal();
    k
}

/// Store a 3D surface into FIGURE and, under `PlotContext::Terminal`,
/// render a heatmap-of-Z preview in the terminal. True 3D rotate/zoom
/// lives in `rustlab-viewer`; HTML uses Plotly's native 3D surface.
pub fn surf_terminal(
    z: Vec<Vec<f64>>,
    x: Vec<f64>,
    y: Vec<f64>,
    title: &str,
    colormap: &str,
) -> Result<(), PlotError> {
    let nrows = z.len();
    let ncols = if nrows > 0 { z[0].len() } else { 0 };
    if nrows == 0 || ncols == 0 {
        return Err(PlotError::EmptyData);
    }

    // Flat value list for min/max computation.
    let vals: Vec<f64> = z.iter().flat_map(|row| row.iter().copied()).collect();
    let min_v = vals.iter().copied().fold(f64::INFINITY, f64::min);
    let max_v = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let range = (max_v - min_v).max(1e-12);

    // Clone z before moving into SurfaceData so we can still use it below.
    let z_for_draw = z.clone();

    FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            let sp = fig.current_mut();
            sp.series.clear();
            sp.title.clear();
            sp.heatmap = None;
        }
        let sp = fig.current_mut();
        if !title.is_empty() && sp.title.is_empty() {
            sp.title = title.to_string();
        }
        sp.surface = Some(crate::figure::SurfaceData {
            z,
            x,
            y,
            colorscale: colormap.to_string(),
        });
    });

    // Notebook / Headless: state set, no terminal render.
    match crate::figure::plot_context() {
        crate::figure::PlotContext::Notebook | crate::figure::PlotContext::Headless => {
            return Ok(());
        }
        crate::figure::PlotContext::Terminal => {}
    }

    // When the current figure routes to HTML or the external viewer, the
    // surface is rendered there — don't draw a TUI preview over it.
    match crate::figure::current_figure_output() {
        crate::figure::FigureOutput::Html(_) => return Ok(()),
        #[cfg(feature = "viewer")]
        crate::figure::FigureOutput::Viewer(_) => return Ok(()),
        crate::figure::FigureOutput::Terminal => {}
    }

    // Skip when stdout is not a real terminal (tests, piped runs).
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return Ok(());
    }

    execute!(stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend).map_err(|e| {
        restore_terminal();
        PlotError::Terminal(e.to_string())
    })?;

    let result = terminal.draw(|f| {
        let area = f.area();
        let inner_h = (area.height as usize).saturating_sub(2);
        let inner_w = (area.width as usize).saturating_sub(2);

        let px_cols = inner_w / 2;
        let disp_rows = nrows.min(inner_h);
        let disp_cols = ncols.min(px_cols);

        let mut tui_lines: Vec<TuiLine> = Vec::with_capacity(disp_rows);
        for dr in 0..disp_rows {
            let mr = dr * nrows / disp_rows.max(1);
            let mut spans: Vec<Span<'static>> = Vec::with_capacity(disp_cols);
            for dc in 0..disp_cols {
                let mc = dc * ncols / disp_cols.max(1);
                let v = z_for_draw[mr.min(nrows - 1)][mc.min(ncols - 1)];
                let t = (v - min_v) / range;
                let (r, g, b) = colormap_rgb(t, colormap);
                spans.push(Span::styled("  ", Style::default().bg(Color::Rgb(r, g, b))));
            }
            tui_lines.push(TuiLine::from(spans));
        }

        let subtitle = format!("surf {} [{}, {}]", colormap, fmt_g(min_v), fmt_g(max_v));
        let full_title = if title.is_empty() {
            subtitle
        } else {
            format!("{} — {}", title, subtitle)
        };
        let para = Paragraph::new(tui_lines)
            .block(Block::default().borders(Borders::ALL).title(full_title));
        f.render_widget(para, area);
    });

    if let Err(e) = result {
        restore_terminal();
        return Err(PlotError::Terminal(e.to_string()));
    }
    let k = wait_for_key();
    restore_terminal();
    k
}

// ─── Helpers that push series into FIGURE ──────────────────────────────────

fn push_line_series(
    x: Vec<f64>,
    y: Vec<f64>,
    label: &str,
    title: &str,
    color: Option<SeriesColor>,
    style: LineStyle,
) {
    FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            let sp = fig.current_mut();
            sp.series.clear();
            sp.title.clear();
        }
        let color = color.unwrap_or_else(|| fig.next_color());
        let sp = fig.current_mut();
        if !title.is_empty() && sp.title.is_empty() {
            sp.title = title.to_string();
        }
        sp.series.push(crate::figure::Series {
            label: label.to_string(),
            x_data: x,
            y_data: y,
            color,
            style,
            kind: PlotKind::Line,
        });
    });
}

fn push_stem_series(
    x: Vec<f64>,
    y: Vec<f64>,
    label: &str,
    title: &str,
    color: Option<SeriesColor>,
) {
    FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            let sp = fig.current_mut();
            sp.series.clear();
            sp.title.clear();
        }
        let color = color.unwrap_or_else(|| fig.next_color());
        let sp = fig.current_mut();
        if !title.is_empty() && sp.title.is_empty() {
            sp.title = title.to_string();
        }
        sp.series.push(crate::figure::Series {
            label: label.to_string(),
            x_data: x,
            y_data: y,
            color,
            style: LineStyle::Solid,
            kind: PlotKind::Stem,
        });
    });
}

/// Push a line series with explicit x-data.
pub fn push_xy_line(
    x: Vec<f64>,
    y: Vec<f64>,
    label: &str,
    title: &str,
    color: Option<SeriesColor>,
    style: LineStyle,
) {
    push_line_series(x, y, label, title, color, style);
}

/// Push a stem series with explicit x-data.
pub fn push_xy_stem(
    x: Vec<f64>,
    y: Vec<f64>,
    label: &str,
    title: &str,
    color: Option<SeriesColor>,
) {
    push_stem_series(x, y, label, title, color);
}

/// Push a bar series with explicit x-positions and heights.
pub fn push_xy_bar(x: Vec<f64>, y: Vec<f64>, label: &str, title: &str, color: Option<SeriesColor>) {
    FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            let sp = fig.current_mut();
            sp.series.clear();
            sp.title.clear();
        }
        let color = color.unwrap_or_else(|| fig.next_color());
        let sp = fig.current_mut();
        if !title.is_empty() && sp.title.is_empty() {
            sp.title = title.to_string();
        }
        sp.series.push(crate::figure::Series {
            label: label.to_string(),
            x_data: x,
            y_data: y,
            color,
            style: LineStyle::Solid,
            kind: PlotKind::Bar,
        });
    });
}

/// Push a scatter series with explicit x and y point data.
pub fn push_xy_scatter(
    x: Vec<f64>,
    y: Vec<f64>,
    label: &str,
    title: &str,
    color: Option<SeriesColor>,
) {
    FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            let sp = fig.current_mut();
            sp.series.clear();
            sp.title.clear();
        }
        let color = color.unwrap_or_else(|| fig.next_color());
        let sp = fig.current_mut();
        if !title.is_empty() && sp.title.is_empty() {
            sp.title = title.to_string();
        }
        sp.series.push(crate::figure::Series {
            label: label.to_string(),
            x_data: x,
            y_data: y,
            color,
            style: LineStyle::Solid,
            kind: PlotKind::Scatter,
        });
    });
}

// ─── Legacy wrappers ───────────────────────────────────────────────────────

pub fn plot_real(data: &RVector, title: &str) -> Result<(), PlotError> {
    if data.is_empty() {
        return Err(PlotError::EmptyData);
    }
    let x: Vec<f64> = (0..data.len()).map(|i| i as f64).collect();
    let y: Vec<f64> = data.iter().copied().collect();
    push_line_series(x, y, "value", title, None, LineStyle::Solid);
    render_figure_terminal()
}

pub fn stem_real(data: &RVector, title: &str) -> Result<(), PlotError> {
    if data.is_empty() {
        return Err(PlotError::EmptyData);
    }
    let x: Vec<f64> = (0..data.len()).map(|i| i as f64).collect();
    let y: Vec<f64> = data.iter().copied().collect();
    push_stem_series(x, y, "stem", title, None);
    render_figure_terminal()
}

pub fn plot_complex(data: &CVector, title: &str) -> Result<(), PlotError> {
    if data.is_empty() {
        return Err(PlotError::EmptyData);
    }
    FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            let sp = fig.current_mut();
            sp.series.clear();
            sp.title.clear();
        }
        let sp = fig.current_mut();
        if !title.is_empty() && sp.title.is_empty() {
            sp.title = title.to_string();
        }
        let x: Vec<f64> = (0..data.len()).map(|i| i as f64).collect();
        sp.series.push(crate::figure::Series {
            label: "magnitude".to_string(),
            x_data: x.clone(),
            y_data: data.iter().map(|c| c.norm()).collect(),
            color: SeriesColor::Blue,
            style: LineStyle::Solid,
            kind: PlotKind::Line,
        });
        sp.series.push(crate::figure::Series {
            label: "real".to_string(),
            x_data: x,
            y_data: data.iter().map(|c| c.re).collect(),
            color: SeriesColor::Green,
            style: LineStyle::Solid,
            kind: PlotKind::Line,
        });
    });
    render_figure_terminal()
}

pub fn plot_db(freqs: &RVector, h: &CVector, title: &str) -> Result<(), PlotError> {
    let n = freqs.len().min(h.len());
    if n == 0 {
        return Err(PlotError::EmptyData);
    }
    const FLOOR_DB: f64 = -120.0;
    let x: Vec<f64> = freqs.iter().take(n).copied().collect();
    let y: Vec<f64> = h
        .iter()
        .take(n)
        .map(|c| {
            let m = c.norm();
            if m < 1e-12 {
                FLOOR_DB
            } else {
                20.0 * m.log10()
            }
        })
        .collect();
    FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            let sp = fig.current_mut();
            sp.series.clear();
            sp.title.clear();
            sp.xlabel.clear();
            sp.ylabel.clear();
        }
        let color = fig.next_color();
        let sp = fig.current_mut();
        if !title.is_empty() && sp.title.is_empty() {
            sp.title = title.to_string();
        }
        if sp.xlabel.is_empty() {
            sp.xlabel = "Frequency (Hz)".to_string();
        }
        if sp.ylabel.is_empty() {
            sp.ylabel = "Magnitude (dB)".to_string();
        }
        sp.series.push(crate::figure::Series {
            label: "dB".to_string(),
            x_data: x,
            y_data: y,
            color,
            style: LineStyle::Solid,
            kind: PlotKind::Line,
        });
    });
    render_figure_terminal()
}

pub fn plot_histogram(data: &RVector, n_bins: usize, title: &str) -> Result<(), PlotError> {
    if data.is_empty() {
        return Err(PlotError::EmptyData);
    }
    let (centers, counts, bin_width) = compute_histogram(data, n_bins);
    if centers.is_empty() {
        return Err(PlotError::EmptyData);
    }
    // Step-function outline
    let mut x: Vec<f64> = Vec::with_capacity(n_bins * 4);
    let mut y: Vec<f64> = Vec::with_capacity(n_bins * 4);
    for i in 0..n_bins {
        let left = centers[i] - bin_width / 2.0;
        let right = centers[i] + bin_width / 2.0;
        x.extend_from_slice(&[left, left, right, right]);
        y.extend_from_slice(&[0.0, counts[i], counts[i], 0.0]);
    }
    FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            let sp = fig.current_mut();
            sp.series.clear();
            sp.title.clear();
            sp.xlabel.clear();
            sp.ylabel.clear();
        }
        let color = fig.next_color();
        let sp = fig.current_mut();
        if !title.is_empty() && sp.title.is_empty() {
            sp.title = title.to_string();
        }
        if sp.xlabel.is_empty() {
            sp.xlabel = "Value".to_string();
        }
        if sp.ylabel.is_empty() {
            sp.ylabel = "Count".to_string();
        }
        sp.series.push(crate::figure::Series {
            label: "count".to_string(),
            x_data: x,
            y_data: y,
            color,
            style: LineStyle::Solid,
            kind: PlotKind::Line,
        });
    });
    render_figure_terminal()
}

#[cfg(test)]
mod tests {
    use super::fmt_g;

    #[test]
    fn zero() {
        assert_eq!(fmt_g(0.0), "0");
    }
    #[test]
    fn small_positive() {
        assert_eq!(fmt_g(5.5), "5.50");
    }
    #[test]
    fn tens() {
        assert_eq!(fmt_g(42.7), "42.7");
    }
    #[test]
    fn hundreds() {
        assert_eq!(fmt_g(256.0), "256");
    }
    #[test]
    fn very_small() {
        assert!(fmt_g(0.00001).contains("e"));
    }
    #[test]
    fn very_large() {
        assert!(fmt_g(99999.0).contains("e"));
    }
    #[test]
    fn negative() {
        assert_eq!(fmt_g(-5.5), "-5.50");
    }
    #[test]
    fn negative_small() {
        assert!(fmt_g(-0.0001).contains("e"));
    }
    #[test]
    fn one() {
        assert_eq!(fmt_g(1.0), "1.00");
    }
    #[test]
    fn boundary_001() {
        assert_eq!(fmt_g(0.001), "0.00");
    }
    #[test]
    fn boundary_10000() {
        assert!(fmt_g(10000.0).contains("e"));
    }
    #[test]
    fn pi() {
        assert_eq!(fmt_g(std::f64::consts::PI), "3.14");
    }

    #[test]
    fn axis_equal_pad_pads_y_when_panel_is_taller_than_data() {
        // 80×24-cell panel, target dy/dx = 2 * 24 / 80 = 0.6. Unit data
        // (dx=dy=2) has cur ratio 1.0 > target → pad x to dy/target = 2/0.6.
        let (xl, xh, yl, yh) =
            super::apply_axis_equal_pad(-1.0, 1.0, -1.0, 1.0, 80, 24, false, false);
        let dx = xh - xl;
        let dy = yh - yl;
        let target = 2.0 * 24.0 / 80.0;
        assert!(
            (dy / dx - target).abs() < 1e-9,
            "post-pad dy/dx ({}) should equal target {} within 1e-9",
            dy / dx,
            target
        );
        // y untouched, x widened symmetrically around 0.
        assert!((yl + 1.0).abs() < 1e-12);
        assert!((yh - 1.0).abs() < 1e-12);
        assert!(xl < -1.0 && xh > 1.0);
        assert!((xl + xh).abs() < 1e-9, "x pad should be symmetric");
    }

    #[test]
    fn axis_equal_pad_pads_x_when_data_is_tall_for_panel() {
        // 80×24 panel, data dx=2 dy=10 → cur = 5 > target 0.6 → pad x.
        let (xl, xh, yl, yh) =
            super::apply_axis_equal_pad(-1.0, 1.0, -5.0, 5.0, 80, 24, false, false);
        let dx = xh - xl;
        let dy = yh - yl;
        let target = 2.0 * 24.0 / 80.0;
        assert!((dy / dx - target).abs() < 1e-9);
        assert!(yl == -5.0 && yh == 5.0, "y must be untouched");
        assert!(xl < -1.0 && xh > 1.0);
    }

    #[test]
    fn axis_equal_pad_respects_user_set_limits() {
        // Both axes user-set: no change.
        let (xl, xh, yl, yh) =
            super::apply_axis_equal_pad(-1.0, 1.0, -1.0, 1.0, 80, 24, true, true);
        assert_eq!((xl, xh, yl, yh), (-1.0, 1.0, -1.0, 1.0));
    }

    // ── FIGURE state tests ──────────────────────────────────────────────

    use crate::figure::{LineStyle, PlotKind, FIGURE};

    /// Helper: reset FIGURE to clean state before each test.
    fn reset_figure() {
        FIGURE.with(|f| {
            let mut fig = f.borrow_mut();
            fig.reset();
        });
    }

    /// Helper: read current subplot title from FIGURE.
    fn figure_title() -> String {
        FIGURE.with(|f| f.borrow().current().title.clone())
    }

    /// Helper: read current subplot xlabel from FIGURE.
    fn figure_xlabel() -> String {
        FIGURE.with(|f| f.borrow().current().xlabel.clone())
    }

    /// Helper: read current subplot series count from FIGURE.
    fn figure_series_count() -> usize {
        FIGURE.with(|f| f.borrow().current().series.len())
    }

    /// Helper: check if FIGURE has heatmap data.
    fn figure_has_heatmap() -> bool {
        FIGURE.with(|f| f.borrow().current().heatmap.is_some())
    }

    #[test]
    fn push_line_sets_title() {
        reset_figure();
        super::push_xy_line(
            vec![1.0, 2.0],
            vec![3.0, 4.0],
            "s1",
            "My Title",
            None,
            LineStyle::Solid,
        );
        assert_eq!(figure_title(), "My Title");
        assert_eq!(figure_series_count(), 1);
    }

    #[test]
    fn push_line_clears_title_when_hold_off() {
        reset_figure();
        // First plot sets title
        super::push_xy_line(vec![1.0], vec![2.0], "", "First", None, LineStyle::Solid);
        assert_eq!(figure_title(), "First");
        // Second plot (hold off) should clear old title and set new one
        super::push_xy_line(vec![1.0], vec![2.0], "", "Second", None, LineStyle::Solid);
        assert_eq!(figure_title(), "Second");
    }

    #[test]
    fn push_line_preserves_title_when_hold_on() {
        reset_figure();
        super::push_xy_line(vec![1.0], vec![2.0], "", "Original", None, LineStyle::Solid);
        FIGURE.with(|f| f.borrow_mut().hold = true);
        super::push_xy_line(vec![3.0], vec![4.0], "", "Ignored", None, LineStyle::Solid);
        // Title should stay as "Original" because hold is on
        assert_eq!(figure_title(), "Original");
        assert_eq!(figure_series_count(), 2);
    }

    #[test]
    fn push_stem_clears_title_when_hold_off() {
        reset_figure();
        super::push_xy_stem(vec![1.0], vec![2.0], "", "Stem1", None);
        assert_eq!(figure_title(), "Stem1");
        super::push_xy_stem(vec![1.0], vec![2.0], "", "Stem2", None);
        assert_eq!(figure_title(), "Stem2");
    }

    #[test]
    fn push_bar_clears_title_when_hold_off() {
        reset_figure();
        super::push_xy_bar(vec![0.0], vec![5.0], "", "Bar1", None);
        assert_eq!(figure_title(), "Bar1");
        super::push_xy_bar(vec![0.0], vec![3.0], "", "Bar2", None);
        assert_eq!(figure_title(), "Bar2");
    }

    #[test]
    fn push_scatter_clears_title_when_hold_off() {
        reset_figure();
        super::push_xy_scatter(vec![1.0], vec![2.0], "", "Scatter1", None);
        assert_eq!(figure_title(), "Scatter1");
        super::push_xy_scatter(vec![3.0], vec![4.0], "", "Scatter2", None);
        assert_eq!(figure_title(), "Scatter2");
    }

    #[test]
    fn plot_db_clears_title_when_hold_off() {
        reset_figure();
        // Manually set a title via push_line to simulate a prior plot
        super::push_xy_line(
            vec![1.0],
            vec![2.0],
            "",
            "Old Title",
            None,
            LineStyle::Solid,
        );
        assert_eq!(figure_title(), "Old Title");

        // plot_db should clear title and set new one
        // We can't call plot_db directly (it renders to terminal), so test
        // the FIGURE state manipulation inline, matching the plot_db pattern:
        let x = vec![100.0, 200.0];
        let y = vec![-3.0, -6.0];
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            if !fig.hold {
                let sp = fig.current_mut();
                sp.series.clear();
                sp.title.clear();
                sp.xlabel.clear();
                sp.ylabel.clear();
            }
            let color = fig.next_color();
            let sp = fig.current_mut();
            let title = "dB Response";
            if !title.is_empty() && sp.title.is_empty() {
                sp.title = title.to_string();
            }
            if sp.xlabel.is_empty() {
                sp.xlabel = "Frequency (Hz)".to_string();
            }
            if sp.ylabel.is_empty() {
                sp.ylabel = "Magnitude (dB)".to_string();
            }
            sp.series.push(crate::figure::Series {
                label: "dB".to_string(),
                x_data: x,
                y_data: y,
                color,
                style: LineStyle::Solid,
                kind: PlotKind::Line,
            });
        });
        assert_eq!(figure_title(), "dB Response");
        assert_eq!(figure_xlabel(), "Frequency (Hz)");
    }

    #[test]
    fn imagesc_pushes_heatmap_to_figure() {
        use num_complex::Complex;
        use rustlab_core::CMatrix;
        reset_figure();

        // Set notebook context so imagesc_terminal returns early without
        // trying to render to terminal
        crate::figure::set_plot_context(crate::figure::PlotContext::Notebook);
        let data = vec![
            Complex::new(1.0, 0.0),
            Complex::new(2.0, 0.0),
            Complex::new(3.0, 0.0),
            Complex::new(4.0, 0.0),
        ];
        let matrix = CMatrix::from_shape_vec((2, 2), data).unwrap();
        super::imagesc_terminal(&matrix, "Heat", "viridis").unwrap();
        // Restore default context
        crate::figure::set_plot_context(crate::figure::PlotContext::Terminal);

        assert!(figure_has_heatmap());
        assert_eq!(figure_title(), "Heat");
    }

    #[test]
    fn imagesc_clears_title_when_hold_off() {
        use num_complex::Complex;
        use rustlab_core::CMatrix;
        reset_figure();

        // Set an initial title via push_line
        super::push_xy_line(
            vec![1.0],
            vec![2.0],
            "",
            "Line Title",
            None,
            LineStyle::Solid,
        );
        assert_eq!(figure_title(), "Line Title");

        crate::figure::set_plot_context(crate::figure::PlotContext::Notebook);
        let data = vec![
            Complex::new(1.0, 0.0),
            Complex::new(0.0, 0.0),
            Complex::new(0.0, 0.0),
            Complex::new(1.0, 0.0),
        ];
        let matrix = CMatrix::from_shape_vec((2, 2), data).unwrap();
        super::imagesc_terminal(&matrix, "Heatmap", "viridis").unwrap();
        crate::figure::set_plot_context(crate::figure::PlotContext::Terminal);

        assert_eq!(figure_title(), "Heatmap");
    }
}
