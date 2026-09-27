use std::cell::Cell;

/// Theme selection for rendered output (HTML, LaTeX, PDF).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Dark
    }
}

thread_local! {
    /// Per-thread default theme used by un-themed render paths
    /// (`savefig` HTML/SVG/PNG, animation HTML/GIF, viewer pre-render).
    /// Notebooks that pass an explicit theme to the themed APIs are
    /// unaffected. Set at process start from `~/.rustlabrc` `[plot] theme`.
    static DEFAULT_THEME: Cell<Theme> = const { Cell::new(Theme::Dark) };
}

/// Overwrite the per-thread default [`Theme`]. Subsequently un-themed
/// renders pick up this palette.
pub fn set_default_theme(theme: Theme) {
    DEFAULT_THEME.with(|c| c.set(theme));
}

/// Return the current per-thread default [`Theme`].
pub fn default_theme() -> Theme {
    DEFAULT_THEME.with(|c| c.get())
}

impl Theme {
    /// Return the color palette for this theme.
    pub fn colors(&self) -> &'static ThemeColors {
        match self {
            Theme::Dark => &DARK,
            Theme::Light => &LIGHT,
        }
    }
}

/// Complete color palette for rendered output.
pub struct ThemeColors {
    // Page
    pub bg: &'static str,
    pub bg_secondary: &'static str,
    pub text: &'static str,
    pub text_dim: &'static str,
    pub border: &'static str,
    pub border_subtle: &'static str,
    // Headings & accents
    pub accent_primary: &'static str,
    pub accent_secondary: &'static str,
    pub accent_tertiary: &'static str,
    // Code blocks
    pub code_bg: &'static str,
    pub output_bg: &'static str,
    pub inline_code_bg: &'static str,
    // Error
    pub error_bg: &'static str,
    pub error_text: &'static str,
    // Plot
    pub plot_bg: &'static str,
    pub plot_grid: &'static str,
    // Syntax highlighting
    pub syn_keyword: &'static str,
    pub syn_function: &'static str,
    pub syn_number: &'static str,
    pub syn_string: &'static str,
    pub syn_comment: &'static str,
    pub syn_operator: &'static str,
    // Footer
    pub footer_text: &'static str,
}

/// Catppuccin Mocha (dark).
static DARK: ThemeColors = ThemeColors {
    bg: "#1e1e2e",
    bg_secondary: "#181825",
    text: "#cdd6f4",
    text_dim: "#a6adc8",
    border: "#313244",
    border_subtle: "#45475a",
    accent_primary: "#cba6f7",
    accent_secondary: "#89b4fa",
    accent_tertiary: "#74c7ec",
    code_bg: "#11111b",
    output_bg: "#181825",
    inline_code_bg: "#313244",
    error_bg: "#1e0a0a",
    error_text: "#f38ba8",
    plot_bg: "#1e1e2e",
    plot_grid: "rgba(150,150,180,0.3)",
    syn_keyword: "#cba6f7",
    syn_function: "#89b4fa",
    syn_number: "#fab387",
    syn_string: "#a6e3a1",
    syn_comment: "#6c7086",
    syn_operator: "#89dceb",
    footer_text: "#585b70",
};

/// Catppuccin Latte (light).
static LIGHT: ThemeColors = ThemeColors {
    bg: "#eff1f5",
    bg_secondary: "#e6e9ef",
    text: "#4c4f69",
    text_dim: "#6c6f85",
    border: "#ccd0da",
    border_subtle: "#bcc0cc",
    accent_primary: "#8839ef",
    accent_secondary: "#1e66f5",
    accent_tertiary: "#179299",
    code_bg: "#dce0e8",
    output_bg: "#e6e9ef",
    inline_code_bg: "#ccd0da",
    error_bg: "#fce4e4",
    error_text: "#d20f39",
    plot_bg: "#eff1f5",
    plot_grid: "rgba(100,100,120,0.2)",
    syn_keyword: "#8839ef",
    syn_function: "#1e66f5",
    syn_number: "#fe640b",
    syn_string: "#40a02b",
    syn_comment: "#9ca0b0",
    syn_operator: "#179299",
    footer_text: "#9ca0b0",
};
