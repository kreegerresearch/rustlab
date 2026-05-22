pub mod animation;
pub mod ascii;
pub mod contour;
pub mod error;
pub mod figure;
pub mod file;
pub mod html;
pub mod live;
pub mod quiver;
pub mod smith;
pub mod streamline;
pub mod theme;
#[cfg(feature = "viewer")]
pub mod viewer_client;
#[cfg(feature = "viewer")]
pub mod viewer_live;

pub use animation::{
    clear_figure_traces, clear_frames, clear_notebook_animations, frames_len, push_frame,
    push_notebook_animation_snapshot, render_animation_doc, render_animation_gif,
    render_animation_html, render_animation_inline, take_frames, take_notebook_animations,
    write_animation_gif, NotebookAnimation, NotebookAnimationFormat,
};
pub use ascii::{
    imagesc_terminal, plot_complex, plot_db, plot_histogram, plot_real, push_xy_bar, push_xy_line,
    push_xy_scatter, push_xy_stem, render_figure_terminal, render_heatmap_tui, render_image_tui,
    stem_real, surf_terminal,
};
pub use error::PlotError;
pub use figure::{
    capture_thread_state, clear_notebook_figures, close_all_figures, close_figure, colormap_rgb,
    current_figure_id, current_figure_output, default_axis_y_direction, figure_new,
    figure_new_html, figure_switch, plot_context, push_notebook_figure_snapshot,
    restore_thread_state, set_current_figure_output, set_default_axis_y_direction,
    set_plot_context, take_notebook_figures, AxisYDirection, ContourData, FigureOutput,
    FigureState, HeatmapData, HeatmapKind, HeatmapOrigin, LineStyle, PlotContext, PlotKind, PlotSnapshot,
    QuiverData, Series, SeriesColor, StreamlineData, SubplotState, SurfaceData, FIGURE,
};
pub use file::{
    render_figure_file, render_figure_state_to_file, render_figure_state_to_file_themed,
    render_figure_state_to_rgb_buffer, render_panel_to_rgba,
};
pub use html::{
    clear_html_figure_path, render_figure_html, render_figure_plotly_div, set_html_figure_path,
    sync_html_file,
};
pub use live::LiveFigure;
pub use theme::{Theme, ThemeColors};
#[cfg(feature = "viewer")]
pub use viewer_live::ViewerFigure;
#[cfg(feature = "viewer")]
pub use viewer_live::{
    connect_viewer, connect_viewer_named, disconnect_viewer, sync_viewer, viewer_active,
    viewer_close, viewer_new_figure, viewer_reset,
};

use rustlab_core::{CMatrix, RMatrix, RVector};

/// Sync the current figure to its non-terminal output (HTML file or viewer).
/// Called after FIGURE state mutations that don't go through render_figure_terminal().
pub fn sync_figure_outputs() {
    match current_figure_output() {
        FigureOutput::Html(_) => sync_html_file(),
        #[cfg(feature = "viewer")]
        FigureOutput::Viewer(_) => sync_viewer(),
        FigureOutput::Terminal => {}
    }
}

/// Backend-agnostic interface for live-updating plots.
///
/// Implemented by `LiveFigure` (ratatui terminal) and, when the `viewer`
/// feature is enabled, by `ViewerFigure` (egui via IPC).
pub trait LivePlot: Send + std::fmt::Debug {
    fn update_panel(&mut self, idx: usize, x: Vec<f64>, y: Vec<f64>);
    fn set_panel_labels(&mut self, idx: usize, title: &str, xlabel: &str, ylabel: &str);
    fn set_panel_limits(
        &mut self,
        idx: usize,
        xlim: (Option<f64>, Option<f64>),
        ylim: (Option<f64>, Option<f64>),
    );
    /// Replace the heatmap data on panel `idx` with the values in
    /// `matrix` (rows = vertical axis, cols = horizontal axis). When
    /// `vmin` and `vmax` are `Some`, they pin the colour normalisation
    /// range — otherwise the panel auto-scales from the matrix.
    ///
    /// `origin` controls where row 0 of `matrix` lands in the rendered
    /// image. [`HeatmapOrigin::Lower`] (default in callers' builders) is
    /// the physics / `imagesc` convention used by spectrograms;
    /// [`HeatmapOrigin::Upper`] is used by downward-scrolling waterfalls
    /// where row 0 is the newest sample.
    ///
    /// Default implementation is a no-op so backends that don't
    /// support live heatmaps (e.g. pure line-plot ratatui) compile
    /// without per-call boilerplate.
    fn update_panel_heatmap(
        &mut self,
        _idx: usize,
        _matrix: &RMatrix,
        _colormap: &str,
        _vmin: Option<f64>,
        _vmax: Option<f64>,
        _origin: crate::figure::HeatmapOrigin,
    ) {
    }
    fn redraw(&mut self) -> Result<(), PlotError>;
}

/// Convert a complex matrix to dB magnitudes (`20·log10(|x|)`) and clip
/// to a floor below the global maximum.
///
/// Used by `spectrogram`, `scalogram`, and any future heatmap display
/// that wants the canonical "wide-dynamic-range image" look. Returns
/// `(matrix_db, vmin, vmax)` where `vmax` is the matrix maximum in dB
/// and `vmin = vmax - floor_db`. All matrix entries below `vmin` are
/// clipped to `vmin`.
///
/// `floor_db` is the displayed dynamic range — typically 80 dB. Use
/// `f64::INFINITY` to disable clipping.
///
/// For an all-zero matrix the maximum is set to `0 dB`, the floor to
/// `-floor_db dB`, and every cell is clipped to the floor.
pub fn db_clip(magnitude: &CMatrix, floor_db: f64) -> (RMatrix, f64, f64) {
    use ndarray::Array2;
    let (rows, cols) = (magnitude.nrows(), magnitude.ncols());
    let mut out = Array2::<f64>::zeros((rows, cols));
    let mut max_mag = 0.0f64;
    for r in 0..rows {
        for c in 0..cols {
            let mag = magnitude[(r, c)].norm();
            if mag > max_mag {
                max_mag = mag;
            }
            out[(r, c)] = mag; // store magnitudes temporarily
        }
    }
    let vmax = if max_mag > 0.0 {
        20.0 * max_mag.log10()
    } else {
        0.0 // all-zero input: synthesise a 0 dB ceiling
    };
    let vmin = vmax - floor_db.max(0.0);
    // Convert magnitudes to dB and clip.
    for v in out.iter_mut() {
        // 1e-300 floor keeps log10 finite for zero entries; the clip
        // below brings them up to vmin regardless.
        let db = 20.0 * (*v).max(1e-300).log10();
        *v = if db < vmin { vmin } else { db };
    }
    (out, vmin, vmax)
}

/// Compute histogram bin centers and counts.
/// Returns `(centers, counts, bin_width)`.
/// The last bin is closed on the right so the maximum value falls in it.
pub fn compute_histogram(data: &RVector, n_bins: usize) -> (Vec<f64>, Vec<f64>, f64) {
    if data.is_empty() || n_bins == 0 {
        return (vec![], vec![], 0.0);
    }
    let min = data.iter().copied().fold(f64::INFINITY, f64::min);
    let max = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let range = max - min;
    let bin_width = if range < 1e-300 {
        1.0
    } else {
        range / n_bins as f64
    };
    let mut counts = vec![0.0f64; n_bins];
    for &x in data.iter() {
        let idx = ((x - min) / bin_width) as usize;
        counts[idx.min(n_bins - 1)] += 1.0;
    }
    let centers: Vec<f64> = (0..n_bins)
        .map(|i| min + (i as f64 + 0.5) * bin_width)
        .collect();
    (centers, counts, bin_width)
}

/// Build a 2-row ndarray matrix from histogram output: row 0 = centers, row 1 = counts.
pub fn histogram_matrix(centers: &[f64], counts: &[f64]) -> rustlab_core::CMatrix {
    use ndarray::Array2;
    use num_complex::Complex;
    let n = centers.len();
    let mut m = Array2::zeros((2, n));
    for i in 0..n {
        m[(0, i)] = Complex::new(centers[i], 0.0);
        m[(1, i)] = Complex::new(counts[i], 0.0);
    }
    m
}

#[cfg(test)]
mod db_clip_tests {
    use super::db_clip;
    use ndarray::Array2;
    use num_complex::Complex;

    fn cmat(rows: usize, cols: usize, vals: &[f64]) -> rustlab_core::CMatrix {
        let mut m = Array2::zeros((rows, cols));
        for (i, &v) in vals.iter().enumerate() {
            let r = i / cols;
            let c = i % cols;
            m[(r, c)] = Complex::new(v, 0.0);
        }
        m
    }

    #[test]
    fn vmax_is_max_db() {
        // |x| = 10 -> 20 log10(10) = 20 dB
        let m = cmat(1, 3, &[1.0, 10.0, 5.0]);
        let (_, _, vmax) = db_clip(&m, 80.0);
        assert!((vmax - 20.0).abs() < 1e-9, "vmax = {vmax}");
    }

    #[test]
    fn vmin_is_vmax_minus_floor() {
        let m = cmat(1, 2, &[1.0, 100.0]);
        let (_, vmin, vmax) = db_clip(&m, 60.0);
        // vmax = 40 dB (from 100), vmin = -20 dB
        assert!((vmax - 40.0).abs() < 1e-9);
        assert!((vmin + 20.0).abs() < 1e-9);
    }

    #[test]
    fn small_values_clip_to_vmin() {
        // 100 -> 40 dB; 1e-6 -> -120 dB. With floor 60 dB, vmin = -20 dB,
        // so the 1e-6 entry should be clamped to -20.
        let m = cmat(1, 2, &[1e-6, 100.0]);
        let (db, vmin, _) = db_clip(&m, 60.0);
        assert!((db[(0, 0)] - vmin).abs() < 1e-9);
        // Big value unchanged.
        assert!((db[(0, 1)] - 40.0).abs() < 1e-9);
    }

    #[test]
    fn all_zero_matrix_is_safe() {
        let m = cmat(2, 2, &[0.0, 0.0, 0.0, 0.0]);
        let (db, vmin, vmax) = db_clip(&m, 80.0);
        assert!(vmax.is_finite());
        assert!(vmin.is_finite());
        // Every cell clipped to the floor.
        for v in db.iter() {
            assert!((*v - vmin).abs() < 1e-9);
        }
    }

    #[test]
    fn complex_magnitude_used() {
        // |3 + 4j| = 5 -> 20 log10(5) ≈ 13.979 dB
        let mut m = Array2::zeros((1, 1));
        m[(0, 0)] = Complex::new(3.0, 4.0);
        let (_, _, vmax) = db_clip(&m, 80.0);
        let expected = 20.0 * 5.0_f64.log10();
        assert!((vmax - expected).abs() < 1e-9);
    }
}
