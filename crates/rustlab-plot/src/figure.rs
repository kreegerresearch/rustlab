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

/// 2D heatmap data for a subplot (produced by `imagesc`).
#[derive(Debug, Clone)]
pub struct HeatmapData {
    /// Row-major matrix values (magnitudes). `z[row][col]`.
    pub z: Vec<Vec<f64>>,
    /// Colorscale name (rustlab convention: "viridis", "jet", "hot", "gray").
    pub colorscale: String,
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

// ─── Multi-figure store (figure handles) ──────────────────────────────────

/// Output routing mode for a figure.
#[derive(Debug, Clone)]
pub enum FigureOutput {
    Terminal,
    Html(String),
    #[cfg(feature = "viewer")]
    Viewer(u32),
}

struct StoredFigure {
    state: FigureState,
    output: FigureOutput,
}

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
