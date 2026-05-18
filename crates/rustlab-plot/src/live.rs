use crate::{
    ascii::draw_subplots,
    error::PlotError,
    figure::{LineStyle, PlotKind, Series, SeriesColor, SubplotState},
};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{stdin, stdout, IsTerminal};
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether a LiveFigure currently owns the terminal.  When true, SIGINT is
/// ignored so the process can exit cleanly via pipe EOF / AudioEof instead
/// of being killed before Drop can restore the terminal.
static LIVE_FIGURE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// A persistent live-updating terminal plot that stays open across multiple
/// `redraw()` calls.  Use `update_panel()` to push new data and `redraw()` to
/// flush it to the screen in one atomic refresh.
///
/// Terminal cleanup (`disable_raw_mode` + `LeaveAlternateScreen`) fires
/// automatically via `Drop` — on explicit `figure_close`, on script end, and
/// on Ctrl-C (Rust runs destructors when unwinding).
pub struct LiveFigure {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    panels: Vec<SubplotState>,
    rows: usize,
    cols: usize,
    raw_mode: bool,
}

impl LiveFigure {
    /// Open the alternate screen and initialise a `rows × cols` live figure.
    /// Returns `Err(PlotError::NotATty)` if stdout is not a real terminal,
    /// or `Err(PlotError::HeadlessDisabled)` when running under `--plot none`.
    pub fn new(rows: usize, cols: usize) -> Result<Self, PlotError> {
        if crate::figure::plot_context() == crate::figure::PlotContext::Headless {
            return Err(PlotError::HeadlessDisabled);
        }
        if !stdout().is_terminal() {
            return Err(PlotError::NotATty);
        }
        execute!(stdout(), EnterAlternateScreen)?;
        // Only enable raw mode when stdin is a real terminal.  When stdin is
        // a pipe (e.g. audio PCM), tcsetattr would fail.  In that case Ctrl-C
        // already works: it kills the upstream process, closing the pipe,
        // which triggers AudioEof for a clean exit.
        let raw_mode = stdin().is_terminal();
        if raw_mode {
            enable_raw_mode()?;
        }
        // Ignore SIGINT so the process isn't killed before Drop can restore
        // the terminal.  When stdin is a pipe, Ctrl-C kills the upstream
        // process (e.g. sox), the pipe closes, and AudioEof triggers a clean
        // exit.  When stdin is a tty, the key-event polling in redraw()
        // handles Ctrl-C explicitly.
        #[cfg(unix)]
        unsafe {
            libc::signal(libc::SIGINT, libc::SIG_IGN);
        }
        LIVE_FIGURE_ACTIVE.store(true, Ordering::SeqCst);
        let backend = CrosstermBackend::new(stdout());
        let terminal = Terminal::new(backend).map_err(|e| PlotError::Terminal(e.to_string()))?;
        let panels = (0..rows * cols).map(|_| SubplotState::new()).collect();
        Ok(Self {
            terminal,
            panels,
            rows,
            cols,
            raw_mode,
        })
    }

    /// Replace the data in panel `idx` (0-based).  Does **not** redraw —
    /// call `redraw()` after updating all panels for one atomic refresh.
    pub fn update_panel(&mut self, idx: usize, x: Vec<f64>, y: Vec<f64>) {
        if idx >= self.panels.len() {
            return;
        }
        let panel = &mut self.panels[idx];
        panel.series.clear();
        panel.series.push(Series {
            label: String::new(),
            x_data: x,
            y_data: y,
            color: SeriesColor::Cyan,
            style: LineStyle::Solid,
            kind: PlotKind::Line,
        });
    }

    /// Set the title and axis labels for a panel (0-based idx).  Optional.
    pub fn set_panel_labels(&mut self, idx: usize, title: &str, xlabel: &str, ylabel: &str) {
        if idx >= self.panels.len() {
            return;
        }
        let p = &mut self.panels[idx];
        p.title = title.to_string();
        p.xlabel = xlabel.to_string();
        p.ylabel = ylabel.to_string();
    }

    /// Set fixed axis limits for a panel (0-based idx).  Pass `None` for auto.
    pub fn set_panel_limits(
        &mut self,
        idx: usize,
        xlim: (Option<f64>, Option<f64>),
        ylim: (Option<f64>, Option<f64>),
    ) {
        if idx >= self.panels.len() {
            return;
        }
        let p = &mut self.panels[idx];
        p.xlim = xlim;
        p.ylim = ylim;
    }

    /// Render all panels to the terminal.  Returns immediately after the draw
    /// call — no keypress wait.
    ///
    /// Also drains any pending key events.  Returns `Err(PlotError::Interrupted)`
    /// if Ctrl-C or 'q' is pressed, so the caller can exit cleanly.
    pub fn redraw(&mut self) -> Result<(), PlotError> {
        // Clear before each draw to force a full repaint.  Without this,
        // ratatui's double-buffer diff can miss updates when only chart data
        // (not the widget layout) changes between frames.
        self.terminal
            .clear()
            .map_err(|e| PlotError::Terminal(e.to_string()))?;
        let panels = &self.panels;
        let rows = self.rows;
        let cols = self.cols;
        self.terminal
            .draw(|f| draw_subplots(f, panels, rows, cols))
            .map_err(|e| PlotError::Terminal(e.to_string()))?;

        // Drain pending key events when raw mode is active (stdin is a tty).
        // When stdin is a pipe, key polling is not available — Ctrl-C works
        // via SIGINT killing the upstream process instead.
        if self.raw_mode {
            while event::poll(std::time::Duration::ZERO)
                .map_err(|e| PlotError::Terminal(e.to_string()))?
            {
                if let Event::Key(key) =
                    event::read().map_err(|e| PlotError::Terminal(e.to_string()))?
                {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        return Err(PlotError::Interrupted);
                    }
                    if key.code == KeyCode::Char('q') {
                        return Err(PlotError::Interrupted);
                    }
                }
            }
        }
        Ok(())
    }
}

impl crate::LivePlot for LiveFigure {
    fn update_panel(&mut self, idx: usize, x: Vec<f64>, y: Vec<f64>) {
        self.update_panel(idx, x, y);
    }
    fn set_panel_labels(&mut self, idx: usize, title: &str, xlabel: &str, ylabel: &str) {
        self.set_panel_labels(idx, title, xlabel, ylabel);
    }
    fn set_panel_limits(
        &mut self,
        idx: usize,
        xlim: (Option<f64>, Option<f64>),
        ylim: (Option<f64>, Option<f64>),
    ) {
        self.set_panel_limits(idx, xlim, ylim);
    }
    fn update_panel_heatmap(
        &mut self,
        idx: usize,
        matrix: &rustlab_core::RMatrix,
        colormap: &str,
        _vmin: Option<f64>,
        _vmax: Option<f64>,
    ) {
        if idx >= self.panels.len() {
            return;
        }
        let (nrows, ncols) = (matrix.nrows(), matrix.ncols());
        if nrows == 0 || ncols == 0 {
            return;
        }
        let z: Vec<Vec<f64>> = (0..nrows)
            .map(|r| (0..ncols).map(|c| matrix[(r, c)]).collect())
            .collect();
        let panel = &mut self.panels[idx];
        // axis("xy") for live heatmaps: physics convention with row 0 at
        // the bottom, matching the spectrogram / scalogram builtin output.
        panel.y_axis_direction = crate::figure::AxisYDirection::Xy;
        panel.heatmap = Some(crate::figure::HeatmapData {
            z,
            colorscale: colormap.to_string(),
            kind: crate::figure::HeatmapKind::Imagesc,
            x_labels: None,
            y_labels: None,
            rgba: None,
            rgba_width: 0,
            rgba_height: 0,
        });
    }
    fn redraw(&mut self) -> Result<(), crate::PlotError> {
        self.redraw()
    }
}

impl std::fmt::Debug for LiveFigure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LiveFigure({}x{})", self.rows, self.cols)
    }
}

impl Drop for LiveFigure {
    fn drop(&mut self) {
        if self.raw_mode {
            let _ = disable_raw_mode();
        }
        let _ = execute!(stdout(), crossterm::cursor::Show, LeaveAlternateScreen);
        // Restore default SIGINT handling.
        #[cfg(unix)]
        unsafe {
            libc::signal(libc::SIGINT, libc::SIG_DFL);
        }
        LIVE_FIGURE_ACTIVE.store(false, Ordering::SeqCst);
    }
}
