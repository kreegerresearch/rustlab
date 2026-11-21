//! Convert wire protocol types to egui_plot elements.

use egui::Color32;
use egui_plot::{Bar, BarChart, Line, PlotPoints, Points};
use rustlab_proto::{WireColor, WireLineStyle, WirePlotKind, WireSeries, WireText};

/// Convert a `WireColor` to an egui `Color32`.
pub fn wire_color_to_egui(c: &WireColor) -> Color32 {
    match c {
        WireColor::Named(name) => match name.as_str() {
            "blue" => Color32::from_rgb(31, 119, 180),
            "red" => Color32::from_rgb(214, 39, 40),
            "green" => Color32::from_rgb(44, 160, 44),
            "cyan" => Color32::from_rgb(23, 190, 207),
            "magenta" => Color32::from_rgb(148, 103, 189),
            "yellow" => Color32::from_rgb(188, 189, 34),
            "black" => Color32::from_rgb(0, 0, 0),
            "white" => Color32::from_rgb(255, 255, 255),
            _ => Color32::from_rgb(200, 200, 200),
        },
        WireColor::Rgb(r, g, b) => Color32::from_rgb(*r, *g, *b),
    }
}

/// Render a `WireSeries` into egui_plot items added to a `PlotUi`.
pub fn render_series(ui: &mut egui_plot::PlotUi, series: &WireSeries) {
    let color = wire_color_to_egui(&series.color);
    let points: Vec<[f64; 2]> = series
        .x
        .iter()
        .copied()
        .zip(series.y.iter().copied())
        .map(|(x, y)| [x, y])
        .collect();

    match series.kind {
        WirePlotKind::Line => {
            // One line per finite run: a NaN breaks the line, as in MATLAB.
            // Runs share the name, so the legend shows one entry.
            for run in finite_runs(&points) {
                let mut line = Line::new(PlotPoints::new(run))
                    .color(color)
                    .name(&series.label);
                if matches!(series.style, WireLineStyle::Dashed) {
                    line = line.style(egui_plot::LineStyle::dashed_dense());
                }
                ui.line(line);
            }
        }
        WirePlotKind::Scatter => {
            let pts = Points::new(PlotPoints::new(points))
                .color(color)
                .radius(3.0_f32)
                .name(&series.label);
            ui.points(pts);
        }
        WirePlotKind::Bar => {
            let bars: Vec<Bar> = series
                .x
                .iter()
                .copied()
                .zip(series.y.iter().copied())
                .map(|(x, y)| Bar::new(x, y).fill(color))
                .collect();
            let chart = BarChart::new(bars).name(&series.label);
            ui.bar_chart(chart);
        }
        WirePlotKind::Stem => {
            // Vertical lines from y=0 to each point
            for (&xi, &yi) in series.x.iter().zip(series.y.iter()) {
                let stem = Line::new(PlotPoints::new(vec![[xi, 0.0], [xi, yi]])).color(color);
                ui.line(stem);
            }
            // Marker tips
            let pts = Points::new(PlotPoints::new(points))
                .color(color)
                .radius(3.0_f32)
                .name(&series.label);
            ui.points(pts);
        }
    }
}

/// Split a polyline at non-finite points.
pub fn finite_runs(points: &[[f64; 2]]) -> Vec<Vec<[f64; 2]>> {
    let mut runs = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    for p in points {
        if p[0].is_finite() && p[1].is_finite() {
            cur.push(*p);
        } else if !cur.is_empty() {
            runs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    runs
}

/// Draw a label. `sx` / `sy` are screen points per data unit (`sy < 0`
/// because screen y grows down), used to apply the label's point offset.
pub fn render_text(ui: &mut egui_plot::PlotUi, t: &WireText, sx: f64, sy: f64) {
    let ox = if sx != 0.0 { t.dx as f64 / sx } else { 0.0 };
    let oy = if sy != 0.0 { -(t.dy as f64) / sy } else { 0.0 };
    let anchor = egui::Align2([
        match t.h {
            -1 => egui::Align::Min,
            1 => egui::Align::Max,
            _ => egui::Align::Center,
        },
        // Align2's y runs down the screen: Min is the top of the box.
        match t.v {
            -1 => egui::Align::Max,
            1 => egui::Align::Min,
            _ => egui::Align::Center,
        },
    ]);
    let text = egui::RichText::new(&t.text)
        .size(t.size)
        .color(wire_color_to_egui(&t.color));
    ui.text(
        egui_plot::Text::new(egui_plot::PlotPoint::new(t.x + ox, t.y + oy), text).anchor(anchor),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_runs_split_at_nan_and_drop_empty_runs() {
        let nan = f64::NAN;
        let runs = finite_runs(&[[0.0, 0.0], [1.0, 1.0], [nan, nan], [nan, 0.0], [2.0, 2.0]]);
        assert_eq!(runs, vec![vec![[0.0, 0.0], [1.0, 1.0]], vec![[2.0, 2.0]]]);
    }
}
