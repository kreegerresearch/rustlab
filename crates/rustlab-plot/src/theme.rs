//! Color themes for rendered output (HTML, LaTeX, PDF, SVG plots).
//!
//! Built-in schemes are the four Catppuccin flavors. `Theme::Dark` /
//! `Theme::Light` remain as aliases for Mocha / Latte. Resolve a name with
//! [`theme_colors`] (`"mocha"`, `"macchiato"`, `"frappe"`, `"latte"`, plus
//! aliases `"dark"` / `"light"`).
//!
//! Light vs dark chrome (CSS `color-scheme`, LaTeX `pagecolor`) is derived
//! from background luminance via [`ThemeColors::is_dark`] — not from which
//! static the palette pointer equals.

/// Theme selection for rendered output (HTML, LaTeX, PDF).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// Alias for Catppuccin Mocha.
    Dark,
    /// Alias for Catppuccin Latte.
    Light,
    Mocha,
    Macchiato,
    Frappe,
    Latte,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Dark
    }
}

impl Theme {
    /// Return the color palette for this theme.
    pub fn colors(&self) -> &'static ThemeColors {
        match self {
            Theme::Dark | Theme::Mocha => &MOCHA,
            Theme::Macchiato => &MACCHIATO,
            Theme::Frappe => &FRAPPE,
            Theme::Light | Theme::Latte => &LATTE,
        }
    }

    /// Canonical scheme name (`"mocha"`, …), not the `dark`/`light` alias.
    pub fn name(&self) -> &'static str {
        match self {
            Theme::Dark | Theme::Mocha => "mocha",
            Theme::Macchiato => "macchiato",
            Theme::Frappe => "frappe",
            Theme::Light | Theme::Latte => "latte",
        }
    }
}

/// Resolve a theme name (case-insensitive) to a palette.
///
/// Accepts the four Catppuccin builtins plus aliases `dark` → mocha and
/// `light` → latte. `frappé` (with accent) is accepted as an alias for
/// `frappe`.
pub fn theme_colors(name: &str) -> Option<&'static ThemeColors> {
    match normalize_theme_name(name)?.as_str() {
        "dark" | "mocha" => Some(&MOCHA),
        "macchiato" => Some(&MACCHIATO),
        "frappe" => Some(&FRAPPE),
        "light" | "latte" => Some(&LATTE),
        _ => None,
    }
}

/// Parse a theme name into a [`Theme`] value (aliases map to `Dark`/`Light`).
pub fn parse_theme(name: &str) -> Option<Theme> {
    match normalize_theme_name(name)?.as_str() {
        "dark" => Some(Theme::Dark),
        "mocha" => Some(Theme::Mocha),
        "macchiato" => Some(Theme::Macchiato),
        "frappe" => Some(Theme::Frappe),
        "light" => Some(Theme::Light),
        "latte" => Some(Theme::Latte),
        _ => None,
    }
}

/// Names accepted by [`theme_colors`] / CLI `-t` (builtins first, then aliases).
pub fn builtin_theme_names() -> &'static [&'static str] {
    &[
        "mocha",
        "macchiato",
        "frappe",
        "latte",
        "dark",
        "light",
    ]
}

fn normalize_theme_name(name: &str) -> Option<String> {
    let n = name.trim().to_ascii_lowercase();
    if n.is_empty() {
        return None;
    }
    // NFC-ish: accept the accented flavor name from Catppuccin marketing copy.
    if n == "frappé" || n == "frappe\u{0301}" {
        return Some("frappe".to_string());
    }
    Some(n)
}

/// Complete color palette for rendered output.
#[derive(Debug, Clone, Copy)]
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

impl ThemeColors {
    /// `true` when the page background is closer to black than white
    /// (sRGB relative luminance &lt; 0.5). Used for CSS `color-scheme` and
    /// LaTeX `pagecolor` — not pointer identity against a builtin static.
    pub fn is_dark(&self) -> bool {
        match relative_luminance(self.bg) {
            Some(l) if l >= 0.5 => false,
            _ => true,
        }
    }

    /// CSS `color-scheme` value: `"dark"` or `"light"`.
    pub fn color_scheme(&self) -> &'static str {
        if self.is_dark() {
            "dark"
        } else {
            "light"
        }
    }
}

/// sRGB relative luminance of `#RRGGBB`. `None` on a non-hex swatch.
pub fn relative_luminance(hex: &str) -> Option<f64> {
    let (r, g, b) = parse_hex_rgb(hex)?;
    Some(0.2126 * srgb_lin(r) + 0.7152 * srgb_lin(g) + 0.0722 * srgb_lin(b))
}

/// WCAG contrast ratio of two `#RRGGBB` swatches.
pub fn contrast_ratio(fg: &str, bg: &str) -> Option<f64> {
    let l1 = relative_luminance(fg)?;
    let l2 = relative_luminance(bg)?;
    let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    Some((hi + 0.05) / (lo + 0.05))
}

fn parse_hex_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some((
        ((n >> 16) & 0xff) as u8,
        ((n >> 8) & 0xff) as u8,
        (n & 0xff) as u8,
    ))
}

fn srgb_lin(c: u8) -> f64 {
    let x = f64::from(c) / 255.0;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

// ── Catppuccin → ThemeColors mapping ──────────────────────────────────────
// base→bg, mantle→bg_secondary, text→text, subtext0→text_dim,
// surface0→border / inline_code_bg, surface1→border_subtle,
// mauve→accent_primary / syn_keyword, blue→accent_secondary / syn_function,
// sapphire→accent_tertiary, crust→code_bg, mantle→output_bg,
// red→error_text, peach→syn_number, green→syn_string, overlay0→syn_comment,
// sky→syn_operator, surface2→footer_text. error_bg / plot_grid are local
// tints (not named Catppuccin tokens). See docs/notebooks.md.

/// Catppuccin Mocha (dark) — also the `dark` alias.
static MOCHA: ThemeColors = ThemeColors {
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

/// Catppuccin Macchiato (dark).
static MACCHIATO: ThemeColors = ThemeColors {
    bg: "#24273a",
    bg_secondary: "#1e2030",
    text: "#cad3f5",
    text_dim: "#a5adcb",
    border: "#363a4f",
    border_subtle: "#494d64",
    accent_primary: "#c6a0f6",
    accent_secondary: "#8aadf4",
    accent_tertiary: "#7dc4e4",
    code_bg: "#181926",
    output_bg: "#1e2030",
    inline_code_bg: "#363a4f",
    error_bg: "#1e0a0a",
    error_text: "#ed8796",
    plot_bg: "#24273a",
    plot_grid: "rgba(150,150,180,0.3)",
    syn_keyword: "#c6a0f6",
    syn_function: "#8aadf4",
    syn_number: "#f5a97f",
    syn_string: "#a6da95",
    syn_comment: "#6e738d",
    syn_operator: "#91d7e3",
    footer_text: "#5b6078",
};

/// Catppuccin Frappé (dark).
static FRAPPE: ThemeColors = ThemeColors {
    bg: "#303446",
    bg_secondary: "#292c3c",
    text: "#c6d0f5",
    text_dim: "#a5adce",
    border: "#414559",
    border_subtle: "#51576d",
    accent_primary: "#ca9ee6",
    accent_secondary: "#8caaee",
    accent_tertiary: "#85c1dc",
    code_bg: "#232634",
    output_bg: "#292c3c",
    inline_code_bg: "#414559",
    error_bg: "#1e0a0a",
    error_text: "#e78284",
    plot_bg: "#303446",
    plot_grid: "rgba(150,150,180,0.3)",
    syn_keyword: "#ca9ee6",
    syn_function: "#8caaee",
    syn_number: "#ef9f76",
    syn_string: "#a6d189",
    syn_comment: "#737994",
    syn_operator: "#99d1db",
    footer_text: "#626880",
};

/// Catppuccin Latte (light) — also the `light` alias.
static LATTE: ThemeColors = ThemeColors {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mocha_and_dark_alias_are_dark() {
        assert!(MOCHA.is_dark());
        assert_eq!(MOCHA.color_scheme(), "dark");
        assert!(Theme::Dark.colors().is_dark());
        assert!(Theme::Mocha.colors().is_dark());
        assert!(std::ptr::eq(Theme::Dark.colors(), Theme::Mocha.colors()));
    }

    #[test]
    fn macchiato_and_frappe_are_dark() {
        assert!(MACCHIATO.is_dark());
        assert_eq!(MACCHIATO.color_scheme(), "dark");
        assert!(FRAPPE.is_dark());
        assert_eq!(FRAPPE.color_scheme(), "dark");
    }

    #[test]
    fn latte_and_light_alias_are_light() {
        assert!(!LATTE.is_dark());
        assert_eq!(LATTE.color_scheme(), "light");
        assert!(!Theme::Light.colors().is_dark());
        assert!(std::ptr::eq(Theme::Light.colors(), Theme::Latte.colors()));
    }

    #[test]
    fn synthetic_dark_bg_is_dark() {
        let custom = ThemeColors {
            bg: "#111111",
            ..MOCHA
        };
        assert!(custom.is_dark());
        assert_eq!(custom.color_scheme(), "dark");
        // Not the Mocha static — luminance must not rely on pointer identity.
        assert!(!std::ptr::eq(&custom, &MOCHA));
    }

    #[test]
    fn theme_colors_resolves_names_and_aliases() {
        assert!(std::ptr::eq(theme_colors("mocha").unwrap(), &MOCHA));
        assert!(std::ptr::eq(theme_colors("DARK").unwrap(), &MOCHA));
        assert!(std::ptr::eq(theme_colors("Macchiato").unwrap(), &MACCHIATO));
        assert!(std::ptr::eq(theme_colors("frappe").unwrap(), &FRAPPE));
        assert!(std::ptr::eq(theme_colors("frappé").unwrap(), &FRAPPE));
        assert!(std::ptr::eq(theme_colors("latte").unwrap(), &LATTE));
        assert!(std::ptr::eq(theme_colors("light").unwrap(), &LATTE));
        assert!(theme_colors("nope").is_none());
        assert!(theme_colors("").is_none());
    }

    #[test]
    fn dark_builtin_accents_meet_wcag_aa() {
        for (name, colors) in [
            ("mocha", &MOCHA),
            ("macchiato", &MACCHIATO),
            ("frappe", &FRAPPE),
        ] {
            let u = contrast_ratio(colors.accent_secondary, colors.bg).unwrap();
            let v = contrast_ratio(colors.accent_primary, colors.bg).unwrap();
            assert!(
                u >= 4.5,
                "{name} accent_secondary contrast {u:.2} < 4.5"
            );
            assert!(
                v >= 4.5,
                "{name} accent_primary contrast {v:.2} < 4.5"
            );
        }
    }
}
