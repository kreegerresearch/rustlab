use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// Named or RGB color for a plot series.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeriesColor {
    Blue,
    Red,
    Green,
    Cyan,
    Magenta,
    Yellow,
    Black,
    White,
    Rgb(u8, u8, u8),
}

impl SeriesColor {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "r" | "red" => Some(Self::Red),
            "g" | "green" => Some(Self::Green),
            "b" | "blue" => Some(Self::Blue),
            "c" | "cyan" => Some(Self::Cyan),
            "m" | "magenta" => Some(Self::Magenta),
            "y" | "yellow" => Some(Self::Yellow),
            "k" | "black" => Some(Self::Black),
            "w" | "white" => Some(Self::White),
            _ => None,
        }
    }
    /// Default color cycle (matplotlib-like).
    pub fn cycle(idx: usize) -> Self {
        match idx % 6 {
            0 => Self::Cyan,
            1 => Self::Yellow,
            2 => Self::Green,
            3 => Self::Magenta,
            4 => Self::Red,
            _ => Self::Blue,
        }
    }
    pub fn to_plotters(&self) -> plotters::style::RGBColor {
        use plotters::style::RGBColor;
        match self {
            Self::Blue => RGBColor(31, 119, 180),
            Self::Red => RGBColor(214, 39, 40),
            Self::Green => RGBColor(44, 160, 44),
            Self::Cyan => RGBColor(23, 190, 207),
            Self::Magenta => RGBColor(148, 103, 189),
            Self::Yellow => RGBColor(188, 189, 34),
            Self::Black => RGBColor(0, 0, 0),
            Self::White => RGBColor(255, 255, 255),
            Self::Rgb(r, g, b) => RGBColor(*r, *g, *b),
        }
    }
    pub fn to_ratatui(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            Self::Blue => Color::Blue,
            Self::Red => Color::Red,
            Self::Green => Color::Green,
            Self::Cyan => Color::Cyan,
            Self::Magenta => Color::Magenta,
            Self::Yellow => Color::Yellow,
            Self::Black => Color::Black,
            Self::White => Color::White,
            Self::Rgb(r, g, b) => Color::Rgb(*r, *g, *b),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LineStyle {
    Solid,
    Dashed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlotKind {
    Line,
    Stem,
    Bar,
    Scatter,
}

/// One data series in a subplot.
#[derive(Debug, Clone)]
pub struct Series {
    pub label: String,
    pub x_data: Vec<f64>,
    pub y_data: Vec<f64>,
    pub color: SeriesColor,
    pub style: LineStyle,
    pub kind: PlotKind,
}

/// Discriminator for the three heatmap-shaped builtins: continuous-value
/// `imagesc`, label-axis `heatmap`, and raw-pixel `image`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeatmapKind {
    /// `imagesc`: continuous values, min/max normalisation, colormap applied.
    Imagesc,
    /// `heatmap`: like `Imagesc` but with categorical axis labels.
    Heatmap,
    /// `image`: raw RGBA pixel data, no normalisation. Uses `rgba`/`rgba_width`/
    /// `rgba_height` instead of `z`/`colorscale`.
    ImageRgba,
}

/// Where row 0 of a heatmap matrix lands in the rendered image.
///
/// Independent of [`AxisYDirection`], which controls axis label orientation.
/// Defaults to [`HeatmapOrigin::Lower`] — physics/MATLAB-`imagesc` convention,
/// matching the existing live-spectrogram pipeline.
///
/// - `Lower` — row 0 of `z` sits at the **bottom** of the rendered image.
///   Used by `imagesc`, `spectrogram`, `scalogram`, and any plot where
///   row index increases with the displayed y-coordinate.
/// - `Upper` — row 0 of `z` sits at the **top** of the rendered image.
///   Used by frequency waterfalls and any downward-scrolling history
///   where row 0 is the newest sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeatmapOrigin {
    /// Row 0 at the bottom. Default — matches `imagesc`, `spectrogram`.
    #[default]
    Lower,
    /// Row 0 at the top. Used by downward-scrolling waterfalls.
    Upper,
}

/// 2D heatmap data for a subplot (produced by `imagesc`, `heatmap`, `image`).
#[derive(Debug, Clone)]
pub struct HeatmapData {
    /// Row-major matrix values (magnitudes). `z[row][col]`. Empty for `ImageRgba`.
    pub z: Vec<Vec<f64>>,
    /// Colorscale name (rustlab convention: "viridis", "jet", "hot", "gray").
    /// Empty for `ImageRgba`.
    pub colorscale: String,
    /// Which builtin produced this heatmap.
    pub kind: HeatmapKind,
    /// Column labels (`Heatmap` kind). `Some(["Mon","Tue",...])` when set.
    pub x_labels: Option<Vec<String>>,
    /// Row labels (`Heatmap` kind).
    pub y_labels: Option<Vec<String>>,
    /// Pre-rendered RGBA pixels (`ImageRgba` kind), row-major top-to-bottom.
    /// `4 * rgba_width * rgba_height` bytes.
    pub rgba: Option<Vec<u8>>,
    pub rgba_width: u32,
    pub rgba_height: u32,
    /// Fixed colour-mapping lower bound. `None` falls back to the matrix
    /// minimum (auto-scale). Producers that need a stable colour scale
    /// across redraws — live spectrograms, dB-clipped heatmaps — pin this
    /// to a known value (e.g. `vmin_db = -100.0`).
    pub value_min: Option<f64>,
    /// Fixed colour-mapping upper bound. See `value_min` for behaviour
    /// when `None`. Both must be `Some` for the fixed range to take
    /// effect; a single-sided `Some` is treated as auto-scale.
    pub value_max: Option<f64>,
    /// Where row 0 of `z` lands in the rendered image. Defaults to
    /// [`HeatmapOrigin::Lower`] (physics/MATLAB-`imagesc` convention).
    /// Downward-scrolling waterfalls set this to [`HeatmapOrigin::Upper`].
    pub origin: HeatmapOrigin,
}

impl Default for HeatmapData {
    fn default() -> Self {
        Self {
            z: Vec::new(),
            colorscale: String::new(),
            kind: HeatmapKind::Imagesc,
            x_labels: None,
            y_labels: None,
            rgba: None,
            rgba_width: 0,
            rgba_height: 0,
            value_min: None,
            value_max: None,
            origin: HeatmapOrigin::Lower,
        }
    }
}

/// 3D surface data for a subplot (produced by `surf`).
#[derive(Debug, Clone)]
pub struct SurfaceData {
    /// Row-major matrix values. `z[row][col]`; rows = y, cols = x.
    pub z: Vec<Vec<f64>>,
    /// X-axis coordinates, length = ncols.
    pub x: Vec<f64>,
    /// Y-axis coordinates, length = nrows.
    pub y: Vec<f64>,
    /// Colorscale name (rustlab convention: "viridis", "jet", "hot", "gray").
    pub colorscale: String,
}

/// Contour overlay data for a subplot (produced by `contour` / `contourf`).
///
/// Stored additively in `SubplotState::contours` so multiple `contour` calls
/// under `hold on` can stack on top of a heatmap or each other.
#[derive(Debug, Clone)]
pub struct ContourData {
    /// Row-major scalar field, `z[row][col]`. Same convention as `HeatmapData`.
    pub z: Vec<Vec<f64>>,
    /// X-axis coordinates, length = ncols.
    pub x: Vec<f64>,
    /// Y-axis coordinates, length = nrows.
    pub y: Vec<f64>,
    /// Explicit level values (sorted ascending) used by both line and filled
    /// contour rendering.
    pub levels: Vec<f64>,
    /// `true` for `contourf` (filled bands), `false` for `contour` (lines).
    pub filled: bool,
    /// Line color for line contours. `None` falls back to black.
    /// Ignored when `filled` is `true`.
    pub line_color: Option<SeriesColor>,
    /// Colorscale for filled contours (rustlab convention: "viridis", "jet",
    /// "hot", "gray"). Ignored when `filled` is `false`.
    pub colorscale: String,
}

/// Quiver overlay data (produced by `quiver`). Row-major grid with columns
/// indexing `x` and rows indexing `y`. `u[r][c]` is the x-component at
/// `(x[c], y[r])`, `v[r][c]` is the y-component. NaN entries are skipped at
/// render time.
#[derive(Debug, Clone)]
pub struct QuiverData {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub u: Vec<Vec<f64>>,
    pub v: Vec<Vec<f64>>,
    /// User-supplied scale multiplier applied *after* the auto-scale step.
    /// `1.0` is the default (auto-scale sets longest arrow to one cell edge).
    pub scale: f64,
    /// Arrow color. `None` falls back to the next series color.
    pub color: Option<SeriesColor>,
    pub title: Option<String>,
}

/// Streamline overlay data (produced by `streamplot`). Grid conventions match
/// `QuiverData`. Streamlines are integrated at render time so the raw field
/// is stored here; `seeds` overrides the default uniform grid when supplied.
#[derive(Debug, Clone)]
pub struct StreamlineData {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub u: Vec<Vec<f64>>,
    pub v: Vec<Vec<f64>>,
    /// Seed density multiplier. `1.0` yields ≈ 1 seed per cell area.
    pub density: f64,
    /// Explicit seed points `(x, y)`. When `Some`, overrides the density grid.
    pub seeds: Option<Vec<(f64, f64)>>,
    /// Line color. `None` falls back to the next series color.
    pub color: Option<SeriesColor>,
    pub title: Option<String>,
}

/// Y-axis orientation for a panel that displays a heatmap (`imagesc`,
/// `image`, `heatmap`).
///
/// - `Ij` — image / matrix-pixel convention: row 0 of the matrix sits
///   at the TOP of the chart, y-axis labels read `0` at the top and
///   `nrows` at the bottom. Default. Matches MATLAB / Octave `imagesc`.
/// - `Xy` — physics / meshgrid convention: row 0 at the BOTTOM, y-axis
///   labels read `0` at the bottom and `nrows` at the top, y grows
///   upward. Opt-in via `axis("xy")` per panel, or process-wide via
///   `set_default_axis("xy")`.
///
/// Set per-panel by the `axis("xy" | "ij")` script builtin. The default
/// for newly-created panels comes from the per-process value managed by
/// `set_default_axis_y_direction`.
///
/// The setting is honoured by every backend that draws a heatmap with
/// axis labels (SVG + PNG via plotters, HTML via Plotly). The terminal
/// TUI has no axis labels and renders matrix top-down regardless.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisYDirection {
    /// Row 0 at top, labels reversed. Default — matches MATLAB.
    Ij,
    /// Row 0 at bottom, labels in physics convention.
    Xy,
}

thread_local! {
    /// Per-thread default `AxisYDirection` applied to every new
    /// `SubplotState`. Tools that want physics-y semantics across an
    /// entire notebook / vault set this once at startup (e.g. in a
    /// curriculum preamble) instead of calling `axis("xy")` after every
    /// `imagesc`.
    static DEFAULT_AXIS_Y_DIRECTION: Cell<AxisYDirection> = const { Cell::new(AxisYDirection::Ij) };
}

/// Overwrite the per-thread default `AxisYDirection`. Subsequently
/// created `SubplotState`s start in this orientation.
pub fn set_default_axis_y_direction(dir: AxisYDirection) {
    DEFAULT_AXIS_Y_DIRECTION.with(|c| c.set(dir));
}

/// Return the current per-thread default `AxisYDirection`.
pub fn default_axis_y_direction() -> AxisYDirection {
    DEFAULT_AXIS_Y_DIRECTION.with(|c| c.get())
}

/// State for a single subplot panel.
#[derive(Debug, Clone)]
pub struct SubplotState {
    pub title: String,
    pub xlabel: String,
    pub ylabel: String,
    pub grid: bool,
    pub series: Vec<Series>,
    pub xlim: (Option<f64>, Option<f64>),
    pub ylim: (Option<f64>, Option<f64>),
    /// Lock the visual aspect ratio so one data unit on x equals one data unit on y.
    /// Set by `axis("equal")`; cleared by `axis("auto")`. Honored by all four
    /// rendering backends (SVG, Plotly HTML, ratatui, viewer).
    pub axis_equal: bool,
    /// Y-axis orientation for heatmap-bearing panels. Defaults to the
    /// per-thread `default_axis_y_direction()` at construction time; set
    /// per-panel by the `axis("xy")` / `axis("ij")` script builtin.
    /// Ignored when the panel has no heatmap (line/scatter/contour-only
    /// panels always use the standard ascending axis).
    pub y_axis_direction: AxisYDirection,
    /// Categorical x-axis tick labels (e.g. from string array bar charts).
    pub x_labels: Option<Vec<String>>,
    /// Optional 2D heatmap data (takes precedence over series when present).
    pub heatmap: Option<HeatmapData>,
    /// Optional 3D surface data (takes precedence over heatmap and series when present).
    pub surface: Option<SurfaceData>,
    /// Contour overlays (line and filled). Rendered above the heatmap.
    pub contours: Vec<ContourData>,
    /// Quiver (vector arrow) overlays. Rendered above contours.
    pub quivers: Vec<QuiverData>,
    /// Streamline overlays. Rendered above quivers.
    pub streamlines: Vec<StreamlineData>,
}
impl SubplotState {
    pub fn new() -> Self {
        Self {
            title: String::new(),
            xlabel: String::new(),
            ylabel: String::new(),
            grid: true,
            series: Vec::new(),
            xlim: (None, None),
            ylim: (None, None),
            axis_equal: false,
            y_axis_direction: default_axis_y_direction(),
            x_labels: None,
            heatmap: None,
            surface: None,
            contours: Vec::new(),
            quivers: Vec::new(),
            streamlines: Vec::new(),
        }
    }
}

/// Global per-thread figure state shared by all plot builtins.
#[derive(Debug, Clone)]
pub struct FigureState {
    pub hold: bool,
    pub subplot_rows: usize,
    pub subplot_cols: usize,
    pub current_subplot: usize,
    pub subplots: Vec<SubplotState>,
}
impl FigureState {
    pub fn new() -> Self {
        Self {
            hold: false,
            subplot_rows: 1,
            subplot_cols: 1,
            current_subplot: 0,
            subplots: vec![SubplotState::new()],
        }
    }
    pub fn reset(&mut self) {
        *self = Self::new();
    }
    pub fn current(&self) -> &SubplotState {
        let i = self
            .current_subplot
            .min(self.subplots.len().saturating_sub(1));
        &self.subplots[i]
    }
    pub fn current_mut(&mut self) -> &mut SubplotState {
        let i = self
            .current_subplot
            .min(self.subplots.len().saturating_sub(1));
        &mut self.subplots[i]
    }
    /// Switch to subplot (rows×cols, 1-based idx).
    pub fn set_subplot(&mut self, rows: usize, cols: usize, idx: usize) {
        let n = rows * cols;
        if self.subplot_rows != rows || self.subplot_cols != cols {
            self.subplot_rows = rows;
            self.subplot_cols = cols;
            self.subplots = (0..n).map(|_| SubplotState::new()).collect();
        } else {
            while self.subplots.len() < n {
                self.subplots.push(SubplotState::new());
            }
        }
        self.current_subplot = (idx.saturating_sub(1)).min(n.saturating_sub(1));
        self.hold = false;
    }
    /// Color for the next series added to current subplot.
    pub fn next_color(&self) -> SeriesColor {
        SeriesColor::cycle(self.current().series.len())
    }
}

thread_local! {
    pub static FIGURE: RefCell<FigureState> = RefCell::new(FigureState::new());
}

// ─── Process-level plot context ───────────────────────────────────────────

/// Process-level context that controls default output routing.
/// Set once at startup by each binary; cannot be overridden by user code.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlotContext {
    /// Interactive terminal (REPL, `rustlab run`). TUI rendering allowed.
    Terminal,
    /// Notebook batch rendering. No TUI, no viewer. Figures are captured
    /// as FigureState by the notebook executor.
    Notebook,
    /// Headless script run: suppress TUI rendering entirely. `savefig()` still
    /// writes files, viewer output still works if explicitly connected.
    Headless,
}

thread_local! {
    static PLOT_CONTEXT: Cell<PlotContext> = Cell::new(PlotContext::Terminal);
    /// Snapshots captured by `savefig()` calls during a notebook code block.
    /// Drained by the notebook executor at end-of-block.
    static NOTEBOOK_FIGURES: RefCell<Vec<FigureState>> = RefCell::new(Vec::new());
}

/// Set the process-level plot context. Call once at startup.
pub fn set_plot_context(ctx: PlotContext) {
    PLOT_CONTEXT.with(|c| c.set(ctx));
}

/// Get the current plot context.
pub fn plot_context() -> PlotContext {
    PLOT_CONTEXT.with(|c| c.get())
}

/// Push a snapshot of the current FIGURE state onto the notebook capture list.
/// Called from `render_figure_file` when running under `PlotContext::Notebook`.
pub fn push_notebook_figure_snapshot() {
    let snap = FIGURE.with(|f| f.borrow().clone());
    NOTEBOOK_FIGURES.with(|v| v.borrow_mut().push(snap));
}

/// Drain and return all notebook figure snapshots captured since the last call.
pub fn take_notebook_figures() -> Vec<FigureState> {
    NOTEBOOK_FIGURES.with(|v| std::mem::take(&mut *v.borrow_mut()))
}

/// Discard any pending notebook figure snapshots without returning them.
pub fn clear_notebook_figures() {
    NOTEBOOK_FIGURES.with(|v| v.borrow_mut().clear());
}

// ─── Thread-local capture / restore (for notebook output cache) ────────────

/// Captured snapshot of every persistent thread-local in this crate.
/// Used by the notebook output cache to roll back to a prior render's
/// final state without re-executing code blocks.
///
/// Excludes the per-block-drained buffers (`NOTEBOOK_FIGURES`,
/// `NOTEBOOK_ANIMATIONS`) and the per-process `PLOT_CONTEXT` (immutable
/// after startup).
#[derive(Clone)]
pub struct PlotSnapshot {
    figure: FigureState,
    store: FigureStore,
    frames: Vec<FigureState>,
    html_path: Option<String>,
}

/// Capture every persistent thread-local in this crate into a snapshot.
pub fn capture_thread_state() -> PlotSnapshot {
    PlotSnapshot {
        figure: FIGURE.with(|f| f.borrow().clone()),
        store: STORE.with(|s| s.borrow().clone()),
        frames: crate::animation::frames_snapshot(),
        html_path: crate::html::get_html_figure_path(),
    }
}

/// Restore a previously captured snapshot, overwriting every persistent
/// thread-local. The per-block-drained buffers are also cleared so a
/// subsequent block executor starts in the same shape it would after a
/// fresh block boundary.
pub fn restore_thread_state(snap: &PlotSnapshot) {
    FIGURE.with(|f| *f.borrow_mut() = snap.figure.clone());
    STORE.with(|s| *s.borrow_mut() = snap.store.clone());
    crate::animation::restore_frames(snap.frames.clone());
    match &snap.html_path {
        Some(p) => crate::html::set_html_figure_path(p),
        None => crate::html::clear_html_figure_path(),
    }
    clear_notebook_figures();
    crate::animation::clear_notebook_animations();
}

// ─── Multi-figure store (figure handles) ──────────────────────────────────

/// Output routing mode for a figure.
#[derive(Debug, Clone)]
pub enum FigureOutput {
    Terminal,
    Html(String),
    #[cfg(feature = "viewer")]
    Viewer(u32),
}

#[derive(Clone)]
struct StoredFigure {
    state: FigureState,
    output: FigureOutput,
}

#[derive(Clone)]
struct FigureStore {
    figures: HashMap<u32, StoredFigure>,
    /// ID of the active figure. 0 = anonymous (no `figure()` called yet).
    current_id: u32,
    /// Next auto-assigned ID.
    next_id: u32,
    /// Output mode of the active figure (kept in sync with thread-locals).
    current_output: FigureOutput,
}

thread_local! {
    static STORE: RefCell<FigureStore> = RefCell::new(FigureStore {
        figures: HashMap::new(),
        current_id: 0,
        next_id: 1,
        current_output: FigureOutput::Terminal,
    });
}

/// Snapshot the active workspace (FIGURE + output mode) into a StoredFigure.
fn snapshot_current() -> StoredFigure {
    let state = FIGURE.with(|f| f.borrow().clone());
    let output = STORE.with(|s| s.borrow().current_output.clone());
    StoredFigure { state, output }
}

/// Restore a StoredFigure into the active workspace thread-locals.
fn restore(stored: StoredFigure) {
    FIGURE.with(|f| *f.borrow_mut() = stored.state);
    match &stored.output {
        FigureOutput::Terminal => {
            crate::html::clear_html_figure_path();
            #[cfg(feature = "viewer")]
            if crate::viewer_live::viewer_active() {
                // Viewer is connected but this figure renders to terminal —
                // we don't touch VIEWER_CONN, just mark output as terminal.
            }
        }
        FigureOutput::Html(path) => {
            crate::html::set_html_figure_path(path);
        }
        #[cfg(feature = "viewer")]
        FigureOutput::Viewer(fig_id) => {
            crate::html::clear_html_figure_path();
            crate::viewer_live::set_viewer_fig_id(*fig_id);
        }
    }
    STORE.with(|s| s.borrow_mut().current_output = stored.output);
}

/// Save the current figure into the store (assigns ID if anonymous).
fn save_current() {
    STORE.with(|s| {
        let mut store = s.borrow_mut();
        let id = if store.current_id == 0 {
            let id = store.next_id;
            store.next_id += 1;
            store.current_id = id;
            id
        } else {
            store.current_id
        };
        drop(store); // release borrow before snapshot_current reads STORE
        let snap = snapshot_current();
        s.borrow_mut().figures.insert(id, snap);
    });
}

/// Determine the default output mode for a new figure.
fn default_new_output() -> FigureOutput {
    if plot_context() == PlotContext::Notebook {
        return FigureOutput::Html(String::new());
    }
    #[cfg(feature = "viewer")]
    if crate::viewer_live::viewer_active() {
        let fig_id = crate::viewer_live::allocate_viewer_fig_id();
        return FigureOutput::Viewer(fig_id);
    }
    FigureOutput::Terminal
}

/// Create a new figure, save the current one to the store. Returns the new ID.
pub fn figure_new() -> u32 {
    save_current();
    let output = default_new_output();
    FIGURE.with(|f| f.borrow_mut().reset());
    crate::animation::clear_frames();
    crate::html::clear_html_figure_path();
    let id = STORE.with(|s| {
        let mut store = s.borrow_mut();
        let id = store.next_id;
        store.next_id += 1;
        store.current_id = id;
        store.current_output = output.clone();
        id
    });
    // Apply viewer fig_id if needed
    #[cfg(feature = "viewer")]
    if let FigureOutput::Viewer(fig_id) = &STORE.with(|s| s.borrow().current_output.clone()) {
        crate::viewer_live::set_viewer_fig_id(*fig_id);
    }
    id
}

/// Create a new figure in HTML mode. Returns the new ID.
pub fn figure_new_html(path: &str) -> u32 {
    save_current();
    FIGURE.with(|f| f.borrow_mut().reset());
    crate::animation::clear_frames();
    crate::html::set_html_figure_path(path);
    let id = STORE.with(|s| {
        let mut store = s.borrow_mut();
        let id = store.next_id;
        store.next_id += 1;
        store.current_id = id;
        store.current_output = FigureOutput::Html(path.to_string());
        id
    });
    id
}

/// Switch to figure `id`. Creates a fresh figure if `id` doesn't exist.
/// Returns the ID.
pub fn figure_switch(id: u32) -> Result<u32, crate::PlotError> {
    // If already the current figure, nothing to do.
    let current = STORE.with(|s| s.borrow().current_id);
    if current == id && current != 0 {
        return Ok(id);
    }

    save_current();
    crate::animation::clear_frames();

    let stored = STORE.with(|s| s.borrow_mut().figures.remove(&id));
    if let Some(stored) = stored {
        restore(stored);
    } else {
        // Create a fresh figure with this ID
        FIGURE.with(|f| f.borrow_mut().reset());
        crate::html::clear_html_figure_path();
        let output = default_new_output();
        #[cfg(feature = "viewer")]
        if let FigureOutput::Viewer(fig_id) = &output {
            crate::viewer_live::set_viewer_fig_id(*fig_id);
        }
        STORE.with(|s| {
            let mut store = s.borrow_mut();
            store.current_output = output;
        });
    }

    STORE.with(|s| {
        let mut store = s.borrow_mut();
        store.current_id = id;
        // Ensure next_id stays ahead
        if id >= store.next_id {
            store.next_id = id + 1;
        }
    });

    Ok(id)
}

/// Get the current figure's numeric ID (0 if no figure() has been called).
pub fn current_figure_id() -> u32 {
    STORE.with(|s| s.borrow().current_id)
}

/// Get the current figure's output mode.
pub fn current_figure_output() -> FigureOutput {
    STORE.with(|s| s.borrow().current_output.clone())
}

/// Set the current figure's output mode (used by `viewer on`/`viewer off`).
pub fn set_current_figure_output(output: FigureOutput) {
    STORE.with(|s| s.borrow_mut().current_output = output);
}

/// Close figure `id`. Removes it from the store and, when its output is
/// `Viewer`, tells the connected viewer to close its corresponding window.
/// If `id` is the active figure, the active slot is reset (next-most-recent
/// stored figure if any, otherwise an empty anonymous figure).
///
/// Returns `true` when a figure was actually closed, `false` when `id` was
/// neither stored nor active.
pub fn close_figure(id: u32) -> bool {
    if id == 0 {
        return false;
    }
    let was_current = STORE.with(|s| s.borrow().current_id == id);

    // Determine the wire-level fig_id (Viewer output carries its own id
    // distinct from the FigureStore key) and remove the entry.
    let stored_output = if was_current {
        Some(STORE.with(|s| s.borrow().current_output.clone()))
    } else {
        STORE
            .with(|s| s.borrow_mut().figures.remove(&id))
            .map(|st| st.output)
    };

    let mut closed = false;
    if let Some(output) = stored_output {
        closed = true;
        #[cfg(feature = "viewer")]
        if let FigureOutput::Viewer(fig_id) = output {
            crate::viewer_live::viewer_close(fig_id);
        }
        #[cfg(not(feature = "viewer"))]
        {
            let _ = output;
        }
    }

    if was_current {
        // Pick the highest remaining stored ID as the new active figure.
        let next = STORE.with(|s| s.borrow().figures.keys().copied().max());
        if let Some(next_id) = next {
            let stored = STORE.with(|s| s.borrow_mut().figures.remove(&next_id));
            if let Some(stored) = stored {
                restore(stored);
                STORE.with(|s| s.borrow_mut().current_id = next_id);
            }
        } else {
            // Store is empty — reset to an anonymous figure routed to the
            // current default backend (viewer, notebook HTML, or terminal).
            // Matches `figure_new()` so `close` followed by an implicit plot
            // doesn't silently flip routing off the viewer.
            FIGURE.with(|f| f.borrow_mut().reset());
            crate::animation::clear_frames();
            crate::html::clear_html_figure_path();
            let output = default_new_output();
            #[cfg(feature = "viewer")]
            if let FigureOutput::Viewer(fig_id) = &output {
                crate::viewer_live::set_viewer_fig_id(*fig_id);
            }
            STORE.with(|s| {
                let mut store = s.borrow_mut();
                store.current_id = 0;
                store.current_output = output;
            });
        }
    }
    closed
}

/// Close every figure: empty the store, reset the active workspace, and
/// (when connected) tell the viewer to drop all figure windows in one
/// `Reset` message. The viewer connection itself stays open — the active
/// figure's output mode is recomputed via `default_new_output()` so the
/// next plot continues to route to the viewer (or notebook HTML) instead
/// of silently snapping back to the terminal. Idempotent.
pub fn close_all_figures() {
    FIGURE.with(|f| f.borrow_mut().reset());
    crate::animation::clear_frames();
    crate::html::clear_html_figure_path();
    #[cfg(feature = "viewer")]
    crate::viewer_live::viewer_reset();
    let output = default_new_output();
    #[cfg(feature = "viewer")]
    if let FigureOutput::Viewer(fig_id) = &output {
        crate::viewer_live::set_viewer_fig_id(*fig_id);
    }
    STORE.with(|s| {
        let mut store = s.borrow_mut();
        store.figures.clear();
        store.current_id = 0;
        store.current_output = output;
    });
}

#[cfg(test)]
mod close_tests {
    use super::*;

    /// Reset thread-local state so tests don't bleed into one another.
    /// Each `#[test]` runs on its own thread, but cargo can reuse threads
    /// across tests within a test binary, so explicit cleanup is safer.
    fn reset_state() {
        close_all_figures();
        STORE.with(|s| {
            let mut store = s.borrow_mut();
            store.next_id = 1;
        });
    }

    #[test]
    fn close_current_with_no_figures_is_noop() {
        reset_state();
        // No figure() called yet — current_id == 0.
        assert_eq!(current_figure_id(), 0);
        assert!(!close_figure(0)); // 0 is reserved for "no figure", refuses
        assert!(!close_figure(99)); // unknown id
        assert_eq!(current_figure_id(), 0);
    }

    #[test]
    fn close_current_falls_back_to_most_recent() {
        reset_state();
        // The first figure_new() call promotes the anonymous current figure
        // (id 0) to a real id, then allocates a new one — so we collect every
        // real id by walking the store after building three figures.
        let a = figure_new();
        let b = figure_new();
        let c = figure_new();
        assert_eq!(current_figure_id(), c);

        // Close the active figure repeatedly. Each close must yield a
        // strictly smaller current id (highest remaining picked first), and
        // current_output must remain a valid variant. Eventually current_id
        // reaches 0 and the workspace falls back to Terminal.
        let mut prev = c;
        loop {
            assert!(close_figure(prev));
            let next = current_figure_id();
            assert!(next < prev, "current id should shrink: was {}, now {}", prev, next);
            if next == 0 {
                break;
            }
            prev = next;
        }
        assert_eq!(current_figure_id(), 0);
        assert!(matches!(current_figure_output(), FigureOutput::Terminal));
        // a, b were intermediate fallback IDs; reference them so the
        // compiler doesn't warn about unused locals.
        let _ = (a, b);
    }

    #[test]
    fn close_inactive_id_leaves_current_alone() {
        reset_state();
        let a = figure_new();
        let b = figure_new();
        assert_eq!(current_figure_id(), b);
        // Close a (the inactive one). b stays current.
        assert!(close_figure(a));
        assert_eq!(current_figure_id(), b);
        // Re-closing a is a no-op.
        assert!(!close_figure(a));
    }

    #[test]
    fn close_all_empties_store_and_resets_active() {
        reset_state();
        let _ = figure_new();
        let _ = figure_new();
        let _ = figure_new();
        close_all_figures();
        assert_eq!(current_figure_id(), 0);
        assert!(matches!(current_figure_output(), FigureOutput::Terminal));
        let count = STORE.with(|s| s.borrow().figures.len());
        assert_eq!(count, 0);
    }

    #[test]
    fn close_all_is_idempotent() {
        reset_state();
        close_all_figures();
        close_all_figures();
        assert_eq!(current_figure_id(), 0);
    }

    #[test]
    fn close_all_under_notebook_context_keeps_html_routing() {
        // Regression: `close all` used to hardcode FigureOutput::Terminal,
        // which silently flipped routing off the viewer (and off notebook
        // HTML capture). It must instead recompute via default_new_output()
        // so the surrounding plot context is preserved. The Notebook context
        // is the testable proxy for this — it doesn't need a live IPC peer
        // the way the viewer path does.
        reset_state();
        set_plot_context(PlotContext::Notebook);
        let _ = figure_new();
        close_all_figures();
        match current_figure_output() {
            FigureOutput::Html(_) => { /* expected */ }
            other => panic!(
                "close all under Notebook context should preserve HTML routing, got {:?}",
                other
            ),
        }
        // Restore so other tests on the same thread aren't affected.
        set_plot_context(PlotContext::Terminal);
        clear_notebook_figures();
    }

    #[test]
    fn close_figure_empty_store_branch_recomputes_default_output() {
        // Regression for the empty-store branch of close_figure(): it used
        // to hardcode FigureOutput::Terminal, which silently flipped
        // routing off the viewer (and off notebook HTML capture). Construct
        // a "single active figure, no stored siblings" state directly so we
        // exercise that branch without fighting the ghost-figure dance that
        // figure_new() performs on its first call.
        reset_state();
        set_plot_context(PlotContext::Notebook);
        STORE.with(|s| {
            let mut store = s.borrow_mut();
            store.current_id = 5;
            store.current_output = FigureOutput::Html(String::new());
            store.next_id = 6;
            // figures map intentionally empty — the only figure is the
            // active one, so closing it must fall through to the
            // empty-store branch.
        });
        close_figure(5);
        match current_figure_output() {
            FigureOutput::Html(_) => { /* expected */ }
            other => panic!(
                "empty-store close branch under Notebook context should produce HTML routing, got {:?}",
                other
            ),
        }
        set_plot_context(PlotContext::Terminal);
        clear_notebook_figures();
    }
}

// ─── Colormap ──────────────────────────────────────────────────────────────

/// Interpolate a colormap at normalised position t ∈ [0,1].
/// Supported names: "viridis" (default), "jet", "hot", "gray".
pub fn colormap_rgb(t: f64, name: &str) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    type Pts = &'static [(f64, (u8, u8, u8))];
    let pts: Pts = match name {
        "jet" => &[
            (0.00, (0, 0, 128)),
            (0.25, (0, 128, 255)),
            (0.50, (0, 255, 128)),
            (0.75, (255, 255, 0)),
            (1.00, (128, 0, 0)),
        ],
        "hot" => &[
            (0.00, (0, 0, 0)),
            (0.33, (255, 0, 0)),
            (0.67, (255, 255, 0)),
            (1.00, (255, 255, 255)),
        ],
        "gray" => &[(0.00, (0, 0, 0)), (1.00, (255, 255, 255))],
        _ => &[
            // viridis
            (0.00, (68, 1, 84)),
            (0.25, (59, 82, 139)),
            (0.50, (33, 145, 140)),
            (0.75, (94, 201, 98)),
            (1.00, (253, 231, 37)),
        ],
    };
    for w in pts.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t >= t0 && t <= t1 {
            let s = (t - t0) / (t1 - t0);
            let lerp = |a: u8, b: u8| (a as f64 * (1.0 - s) + b as f64 * s).round() as u8;
            return (lerp(c0.0, c1.0), lerp(c0.1, c1.1), lerp(c0.2, c1.2));
        }
    }
    pts.last().map(|(_, c)| *c).unwrap_or((0, 0, 0))
}
