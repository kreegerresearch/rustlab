//! Color themes for rendered output (HTML, LaTeX, PDF, SVG plots).
//!
//! Built-in schemes are the four Catppuccin flavors. `Theme::Dark` /
//! `Theme::Light` remain as aliases for Mocha / Latte. Resolve a name with
//! [`theme_colors`] (`"mocha"`, `"macchiato"`, `"frappe"`, `"latte"`, plus
//! aliases `"dark"` / `"light"`).
//!
//! HTML light vs dark chrome (CSS `color-scheme`) is derived from background
//! luminance via [`ThemeColors::is_dark`] — not from which static the palette
//! pointer equals. LaTeX and PDF do not use that switch: they are always
//! Catppuccin Latte on white paper, with no `pagecolor`.
//!
//! HTML emitters call [`ThemeColors::css_custom_properties`] for `:root`
//! `--rl-*` tokens and [`ThemeColors::css_var`] for `var(--rl-…, literal)`
//! fallbacks in the page stylesheet.

use std::cell::Cell;

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
    &["mocha", "macchiato", "frappe", "latte", "dark", "light"]
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
    // Notebook highlights and margin notes. `cm_mark_bg` is Catppuccin
    // yellow blended over `bg` so body text stays at least 4.5:1.
    pub cm_mark_bg: &'static str,
    pub cm_note_bg: &'static str,
    pub cm_note_border: &'static str,
}

impl ThemeColors {
    /// `true` when the page background is closer to black than white
    /// (sRGB relative luminance &lt; 0.5). Used for CSS `color-scheme` —
    /// not pointer identity against a builtin static. LaTeX and PDF do
    /// not consult this; they are always Latte on white paper, with no
    /// `pagecolor`.
    pub fn is_dark(&self) -> bool {
        !matches!(relative_luminance(self.bg), Some(l) if l >= 0.5)
    }

    /// CSS `color-scheme` value: `"dark"` or `"light"`.
    pub fn color_scheme(&self) -> &'static str {
        if self.is_dark() {
            "dark"
        } else {
            "light"
        }
    }

    /// Stable `--rl-*` custom properties derived from this palette.
    ///
    /// Names are kebab-case of the [`ThemeColors`] fields (`bg_secondary`
    /// → `--rl-bg-secondary`). Order is the struct field order so HTML
    /// tests and future theme files can pin a contract.
    pub fn css_tokens(&self) -> [(&'static str, &'static str); 26] {
        [
            ("--rl-bg", self.bg),
            ("--rl-bg-secondary", self.bg_secondary),
            ("--rl-text", self.text),
            ("--rl-text-dim", self.text_dim),
            ("--rl-border", self.border),
            ("--rl-border-subtle", self.border_subtle),
            ("--rl-accent-primary", self.accent_primary),
            ("--rl-accent-secondary", self.accent_secondary),
            ("--rl-accent-tertiary", self.accent_tertiary),
            ("--rl-code-bg", self.code_bg),
            ("--rl-output-bg", self.output_bg),
            ("--rl-inline-code-bg", self.inline_code_bg),
            ("--rl-error-bg", self.error_bg),
            ("--rl-error-text", self.error_text),
            ("--rl-plot-bg", self.plot_bg),
            ("--rl-plot-grid", self.plot_grid),
            ("--rl-syn-keyword", self.syn_keyword),
            ("--rl-syn-function", self.syn_function),
            ("--rl-syn-number", self.syn_number),
            ("--rl-syn-string", self.syn_string),
            ("--rl-syn-comment", self.syn_comment),
            ("--rl-syn-operator", self.syn_operator),
            ("--rl-footer-text", self.footer_text),
            ("--rl-cm-mark-bg", self.cm_mark_bg),
            ("--rl-cm-note-bg", self.cm_note_bg),
            ("--rl-cm-note-border", self.cm_note_border),
        ]
    }

    /// Indented `--rl-*` declarations for a `:root` block (no wrapping braces).
    pub fn css_custom_properties(&self) -> String {
        self.css_tokens()
            .iter()
            .map(|(name, value)| format!("    {name}: {value};"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `var(--rl-<role>, <literal>)` using this palette's value as the fallback.
    ///
    /// `role` is the kebab-case field name (`"bg-secondary"`, `"accent-primary"`).
    /// Unknown roles are a programmer error and return a token with no fallback.
    pub fn css_var(&self, role: &str) -> String {
        let name = format!("--rl-{role}");
        match self.css_tokens().into_iter().find(|(n, _)| *n == name) {
            Some((_, value)) => format!("var({name}, {value})"),
            None => {
                debug_assert!(false, "unknown ThemeColors CSS role `{role}`");
                format!("var({name})")
            }
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
// Backgrounds stay the stock tokens: base→bg / plot_bg, mantle→bg_secondary
// / output_bg, surface0→border / inline_code_bg, surface1→border_subtle,
// crust→code_bg. error_bg / plot_grid are local tints. See docs/notebooks.md.
//
// Font roles start from the same swatches (text, subtext0, mauve, blue,
// sapphire, red, peach, green, overlay0, sky, surface2) but a role whose
// stock pair is under WCAG AA 4.5:1 on a surface it is actually painted on
// is lightness-nudged, hue kept. Two roles that share a stock token can
// therefore diverge: Frappé mauve is lighter as an accent (it also sits on
// the border) than as a keyword (code panel only), and Latte headings are
// darker than the code-panel tokens of the same hue.

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
    // overlay0 #6c7086 is 3.84:1 on crust; surface2 #585b70 is 2.46:1 on base.
    syn_comment: "#787c92",
    syn_operator: "#89dceb",
    footer_text: "#81859c",
    // yellow #f9e2af at 25% over base. Text contrast ~5.55.
    cm_mark_bg: "#554f4e",
    cm_note_bg: "#181825",
    cm_note_border: "#89b4fa",
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
    // overlay0 #6e738d is 3.73:1 on crust; surface2 #5b6078 is 2.38:1 on base.
    syn_comment: "#7c8199",
    syn_operator: "#91d7e3",
    footer_text: "#898ea5",
    // yellow #eed49f at 25% over base. Text contrast ~5.19.
    cm_mark_bg: "#565253",
    cm_note_bg: "#1e2030",
    cm_note_border: "#8aadf4",
};

/// Catppuccin Frappé (dark).
static FRAPPE: ThemeColors = ThemeColors {
    bg: "#303446",
    bg_secondary: "#292c3c",
    text: "#c6d0f5",
    // subtext0 #a5adce is 4.26:1 on surface0 (sidebar / table head).
    text_dim: "#aab2d1",
    border: "#414559",
    border_subtle: "#51576d",
    // mauve #ca9ee6 is 4.30:1 and blue #8caaee is 4.10:1 on surface0.
    // Keywords stay the stock mauve: they are only drawn on crust.
    accent_primary: "#cda4e7",
    accent_secondary: "#98b3f0",
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
    // overlay0 #737994 is 3.50:1 on crust; surface2 #626880 is 2.23:1 on base.
    syn_comment: "#868ca3",
    syn_operator: "#99d1db",
    footer_text: "#979caf",
    // yellow #e5c890 at 20% over base. 25% sits on the 4.5 threshold.
    cm_mark_bg: "#545255",
    cm_note_bg: "#292c3c",
    cm_note_border: "#98b3f0",
};

/// Catppuccin Latte (light) — also the `light` alias.
static LATTE: ThemeColors = ThemeColors {
    bg: "#eff1f5",
    bg_secondary: "#e6e9ef",
    text: "#4c4f69",
    // subtext0 #6c6f85 is 3.20:1 on surface0 and on the white PDF page.
    text_dim: "#56586a",
    border: "#ccd0da",
    border_subtle: "#bcc0cc",
    // Headings and links also sit on surface0 and on white paper.
    // Stock mauve #8839ef is 3.51:1 there, blue #1e66f5 is 3.18:1,
    // sapphire #179299 is 3.08:1 on mantle.
    accent_primary: "#7113ec",
    accent_secondary: "#094dd3",
    accent_tertiary: "#12747a",
    code_bg: "#dce0e8",
    output_bg: "#e6e9ef",
    inline_code_bg: "#ccd0da",
    error_bg: "#fce4e4",
    // Stock red #d20f39 is 4.46:1 on mantle and 4.10:1 on the code panel
    // (CodeMirror errors use this role).
    error_text: "#c60e36",
    plot_bg: "#eff1f5",
    plot_grid: "rgba(100,100,120,0.2)",
    // Code-panel tokens are a smaller darkening than the headings: crust
    // is lighter than surface0, so the same hue clears 4.5 sooner.
    // Stock peach #fe640b is 2.25:1 on crust and 2.98:1 on white paper;
    // green #40a02b is 2.53:1; overlay0 #9ca0b0 is 1.97:1.
    syn_keyword: "#802cee",
    syn_function: "#0a55ea",
    syn_number: "#ad4001",
    syn_string: "#2d711e",
    syn_comment: "#5e6376",
    syn_operator: "#116e74",
    // surface2 #9ca0b0 is 2.30:1 on base.
    footer_text: "#686d82",
    // yellow #df8e1d at 25% over base. Text contrast ~5.74.
    cm_mark_bg: "#ebd8bf",
    cm_note_bg: "#e6e9ef",
    cm_note_border: "#094dd3",
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
    fn mocha_css_tokens_match_palette_fields() {
        let tokens = MOCHA.css_tokens();
        assert_eq!(tokens.len(), 26);
        assert_eq!(tokens[0], ("--rl-bg", "#1e1e2e"));
        assert_eq!(tokens[2], ("--rl-text", "#cdd6f4"));
        assert_eq!(tokens[6], ("--rl-accent-primary", "#cba6f7"));
        assert_eq!(tokens[7], ("--rl-accent-secondary", "#89b4fa"));
        assert!(MOCHA.css_custom_properties().contains("--rl-bg: #1e1e2e;"));
        assert_eq!(
            MOCHA.css_var("accent-secondary"),
            "var(--rl-accent-secondary, #89b4fa)"
        );
        assert_eq!(
            MOCHA.css_var("plot-grid"),
            "var(--rl-plot-grid, rgba(150,150,180,0.3))"
        );
    }

    #[test]
    fn latte_css_tokens_use_light_palette() {
        assert!(LATTE.css_custom_properties().contains("--rl-bg: #eff1f5;"));
        assert_eq!(LATTE.css_var("bg"), "var(--rl-bg, #eff1f5)");
        assert_eq!(LATTE.css_var("text"), "var(--rl-text, #4c4f69)");
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
            assert!(u >= 4.5, "{name} accent_secondary contrast {u:.2} < 4.5");
            assert!(v >= 4.5, "{name} accent_primary contrast {v:.2} < 4.5");
        }
    }

    #[test]
    fn comment_mark_and_note_meet_wcag_aa() {
        for (name, colors) in [
            ("mocha", &MOCHA),
            ("macchiato", &MACCHIATO),
            ("frappe", &FRAPPE),
            ("latte", &LATTE),
        ] {
            let mark = contrast_ratio(colors.text, colors.cm_mark_bg).unwrap();
            let note = contrast_ratio(colors.text, colors.cm_note_bg).unwrap();
            let border = contrast_ratio(colors.cm_note_border, colors.cm_note_bg).unwrap();
            assert!(mark >= 4.5, "{name} text/mark {mark:.2} < 4.5");
            assert!(note >= 4.5, "{name} text/note {note:.2} < 4.5");
            assert!(border >= 4.5, "{name} border/note {border:.2} < 4.5");
        }
    }

    /// Font role → the backgrounds that role is actually painted on.
    /// A swatch that clears the page can still fail on the code panel,
    /// the output panel, or the sidebar border.
    fn font_surfaces(
        c: &ThemeColors,
    ) -> Vec<(
        &'static str,
        &'static str,
        Vec<(&'static str, &'static str)>,
    )> {
        let bg = ("page", c.bg);
        let secondary = ("sidebar", c.bg_secondary);
        let border = ("border", c.border);
        let code = ("code panel", c.code_bg);
        let output = ("output panel", c.output_bg);
        let inline = ("inline code / table head", c.inline_code_bg);
        let err = ("error panel", c.error_bg);
        vec![
            ("text", c.text, vec![bg, code, secondary, inline, border]),
            (
                "text_dim",
                c.text_dim,
                vec![bg, secondary, output, code, border],
            ),
            ("footer_text", c.footer_text, vec![bg]),
            (
                "accent_primary",
                c.accent_primary,
                vec![bg, secondary, border, inline],
            ),
            (
                "accent_secondary",
                c.accent_secondary,
                vec![bg, secondary, border, code],
            ),
            ("accent_tertiary", c.accent_tertiary, vec![bg, secondary]),
            ("error_text", c.error_text, vec![err, secondary, code]),
            ("syn_keyword", c.syn_keyword, vec![code]),
            ("syn_function", c.syn_function, vec![code]),
            ("syn_number", c.syn_number, vec![code]),
            ("syn_string", c.syn_string, vec![code]),
            ("syn_comment", c.syn_comment, vec![code]),
            ("syn_operator", c.syn_operator, vec![code]),
        ]
    }

    fn assert_aa(theme: &str, role: &str, fg: &str, surface: &str, bg: &str) {
        let v = contrast_ratio(fg, bg).unwrap_or(0.0);
        assert!(
            v >= 4.5,
            "{theme} {role} {fg} on {surface} {bg} is {v:.2}:1"
        );
    }

    #[test]
    fn every_builtin_font_meets_wcag_aa_on_its_surfaces() {
        for (name, colors) in [
            ("mocha", &MOCHA),
            ("macchiato", &MACCHIATO),
            ("frappe", &FRAPPE),
            ("latte", &LATTE),
        ] {
            // Active toolbar button paints the page color on the accent.
            assert_aa(name, "toolbar", colors.bg, "accent", colors.accent_primary);
            for (role, fg, surfaces) in font_surfaces(colors) {
                for (surface, bg) in surfaces {
                    assert_aa(name, role, fg, surface, bg);
                }
            }
        }
        // PDF / LaTeX force Latte onto white paper (no pagecolor).
        let paper = "#ffffff";
        for (role, fg) in [
            ("text", LATTE.text),
            ("text_dim", LATTE.text_dim),
            ("accent_primary", LATTE.accent_primary),
            ("accent_secondary", LATTE.accent_secondary),
            ("accent_tertiary", LATTE.accent_tertiary),
            ("error_text", LATTE.error_text),
            ("syn_keyword", LATTE.syn_keyword),
            ("syn_function", LATTE.syn_function),
            ("syn_number", LATTE.syn_number),
            ("syn_string", LATTE.syn_string),
            ("syn_comment", LATTE.syn_comment),
            ("syn_operator", LATTE.syn_operator),
        ] {
            assert_aa("latte", role, fg, "white paper", paper);
        }
    }

    #[test]
    fn named_unreadable_pairs_are_not_theme_fonts() {
        // Dark blue on near-black, and light yellow / peach on white.
        // These ratios must stay failing so the guard itself cannot rot,
        // and none of them may be a font role.
        let failures = [
            ("#0000ee", "#1e1e2e"),
            ("#0000ff", "#11111b"),
            ("#0000cc", "#11111b"),
            ("#999977", "#dce0e8"),
            ("#ffffaa", "#ffffff"),
            ("#fe640b", "#ffffff"),
            ("#fe640b", "#dce0e8"),
        ];
        for (fg, bg) in failures {
            let v = contrast_ratio(fg, bg).unwrap();
            assert!(
                v < 4.5,
                "{fg} on {bg} contrast {v:.2} no longer documents a failure"
            );
        }
        for colors in [&MOCHA, &MACCHIATO, &FRAPPE, &LATTE] {
            for (_role, fg, _) in font_surfaces(colors) {
                let lower = fg.to_ascii_lowercase();
                for banned in [
                    "#0000ee", "#0000ff", "#0000cc", "#999977", "#ffffaa", "#fe640b",
                ] {
                    assert_ne!(lower, banned, "{fg} is a named unreadable font");
                }
            }
        }
    }

    #[test]
    fn readable_stock_swatches_stay_put() {
        assert_eq!(MOCHA.text, "#cdd6f4");
        assert_eq!(MOCHA.syn_keyword, "#cba6f7");
        assert_eq!(MOCHA.syn_number, "#fab387");
        assert_eq!(MOCHA.error_text, "#f38ba8");
        assert_eq!(MOCHA.bg, "#1e1e2e");
        assert_eq!(MOCHA.code_bg, "#11111b");
        // Frappé keywords only sit on the code panel, which the stock
        // mauve already clears, so they do not follow the accent nudge.
        assert_eq!(FRAPPE.syn_keyword, "#ca9ee6");
        assert_ne!(FRAPPE.accent_primary, FRAPPE.syn_keyword);
        assert_eq!(LATTE.text, "#4c4f69");
        assert_eq!(LATTE.bg, "#eff1f5");
        assert_eq!(LATTE.code_bg, "#dce0e8");
        assert_eq!(LATTE.error_bg, "#fce4e4");
    }
}
