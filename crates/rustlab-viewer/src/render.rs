//! Convert wire protocol types to egui_plot elements.

use egui::Color32;
use egui_plot::{Bar, BarChart, Line, PlotPoints, Points};
use rustlab_proto::{WireColor, WireLineStyle, WirePlotKind, WireSeries};

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
            let mut line = Line::new(PlotPoints::new(points))
                .color(color)
                .name(&series.label);
            if matches!(series.style, WireLineStyle::Dashed) {
                line = line.style(egui_plot::LineStyle::dashed_dense());
            }
            ui.line(line);
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
