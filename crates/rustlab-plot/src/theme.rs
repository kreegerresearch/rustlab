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

use std::borrow::Cow;
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
///
/// Fields are [`Cow`] so a theme loaded from a file can own its strings.
/// The four builtins stay [`Cow::Borrowed`] and the statics stay `const`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeColors {
    // Page
    pub bg: Cow<'static, str>,
    pub bg_secondary: Cow<'static, str>,
    pub text: Cow<'static, str>,
    pub text_dim: Cow<'static, str>,
    pub border: Cow<'static, str>,
    pub border_subtle: Cow<'static, str>,
    // Headings & accents
    pub accent_primary: Cow<'static, str>,
    pub accent_secondary: Cow<'static, str>,
    pub accent_tertiary: Cow<'static, str>,
    // Code blocks
    pub code_bg: Cow<'static, str>,
    pub output_bg: Cow<'static, str>,
    pub inline_code_bg: Cow<'static, str>,
    // Error
    pub error_bg: Cow<'static, str>,
    pub error_text: Cow<'static, str>,
    // Plot
    pub plot_bg: Cow<'static, str>,
    pub plot_grid: Cow<'static, str>,
    // Syntax highlighting
    pub syn_keyword: Cow<'static, str>,
    pub syn_function: Cow<'static, str>,
    pub syn_number: Cow<'static, str>,
    pub syn_string: Cow<'static, str>,
    pub syn_comment: Cow<'static, str>,
    pub syn_operator: Cow<'static, str>,
    // Footer
    pub footer_text: Cow<'static, str>,
    // Notebook highlights and margin notes. `cm_mark_bg` is Catppuccin
    // yellow blended over `bg` so body text stays at least 4.5:1.
    pub cm_mark_bg: Cow<'static, str>,
    pub cm_note_bg: Cow<'static, str>,
    pub cm_note_border: Cow<'static, str>,
    // Git diff on `notebook watch`. Changes are a 4 px bar beside the
    // text (no wash): green added, blue modified, red removed, each at
    // least 3:1 on the page. `*_line` tints a changed line in a code
    // listing: the code panel's luminance is kept (darker on dark themes,
    // lighter on Latte) so every syntax color and the body text keep
    // their 4.5:1 on it.
    pub dx_add_bar: Cow<'static, str>,
    pub dx_mod_bar: Cow<'static, str>,
    pub dx_del_bar: Cow<'static, str>,
    pub dx_add_line: Cow<'static, str>,
    pub dx_del_line: Cow<'static, str>,
    // Word marks inside prose: the bar hue at about a fifth over the
    // page, so body text keeps 4.5:1. Links inside a mark take the body
    // text color (accents miss 4.5:1 on some of these); code words use
    // an underline in the bar color instead, for the same reason as the
    // line tints.
    pub dx_add_word: Cow<'static, str>,
    pub dx_del_word: Cow<'static, str>,
    // `notebook watch` chrome only. These are not in the 33 page tokens,
    // so a static render's `:root` is unchanged. Shadows mix `shadow` at
    // each rule's own alpha; the drawer scrim mixes `scrim` at 28%.
    pub shadow: Cow<'static, str>,
    pub scrim: Cow<'static, str>,
    pub banner_bg: Cow<'static, str>,
    pub banner_text: Cow<'static, str>,
}

impl ThemeColors {
    /// `true` when the page background is closer to black than white
    /// (sRGB relative luminance &lt; 0.5). Used for CSS `color-scheme` —
    /// not pointer identity against a builtin static. LaTeX and PDF do
    /// not consult this; they are always Latte on white paper, with no
    /// `pagecolor`.
    pub fn is_dark(&self) -> bool {
        !matches!(relative_luminance(self.bg.as_ref()), Some(l) if l >= 0.5)
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
    pub fn css_tokens(&self) -> [(&'static str, &str); 33] {
        [
            ("--rl-bg", self.bg.as_ref()),
            ("--rl-bg-secondary", self.bg_secondary.as_ref()),
            ("--rl-text", self.text.as_ref()),
            ("--rl-text-dim", self.text_dim.as_ref()),
            ("--rl-border", self.border.as_ref()),
            ("--rl-border-subtle", self.border_subtle.as_ref()),
            ("--rl-accent-primary", self.accent_primary.as_ref()),
            ("--rl-accent-secondary", self.accent_secondary.as_ref()),
            ("--rl-accent-tertiary", self.accent_tertiary.as_ref()),
            ("--rl-code-bg", self.code_bg.as_ref()),
            ("--rl-output-bg", self.output_bg.as_ref()),
            ("--rl-inline-code-bg", self.inline_code_bg.as_ref()),
            ("--rl-error-bg", self.error_bg.as_ref()),
            ("--rl-error-text", self.error_text.as_ref()),
            ("--rl-plot-bg", self.plot_bg.as_ref()),
            ("--rl-plot-grid", self.plot_grid.as_ref()),
            ("--rl-syn-keyword", self.syn_keyword.as_ref()),
            ("--rl-syn-function", self.syn_function.as_ref()),
            ("--rl-syn-number", self.syn_number.as_ref()),
            ("--rl-syn-string", self.syn_string.as_ref()),
            ("--rl-syn-comment", self.syn_comment.as_ref()),
            ("--rl-syn-operator", self.syn_operator.as_ref()),
            ("--rl-footer-text", self.footer_text.as_ref()),
            ("--rl-cm-mark-bg", self.cm_mark_bg.as_ref()),
            ("--rl-cm-note-bg", self.cm_note_bg.as_ref()),
            ("--rl-cm-note-border", self.cm_note_border.as_ref()),
            ("--rl-dx-add-bar", self.dx_add_bar.as_ref()),
            ("--rl-dx-mod-bar", self.dx_mod_bar.as_ref()),
            ("--rl-dx-del-bar", self.dx_del_bar.as_ref()),
            ("--rl-dx-add-line", self.dx_add_line.as_ref()),
            ("--rl-dx-del-line", self.dx_del_line.as_ref()),
            ("--rl-dx-add-word", self.dx_add_word.as_ref()),
            ("--rl-dx-del-word", self.dx_del_word.as_ref()),
        ]
    }

    /// Form-field boundary: `text_dim` mixed into the page at
    /// [`crate::contrast::FIELD_BORDER_MIX`], 3:1 against page, menu, and
    /// code panel. The `border` role is 1.3:1, too faint for a field.
    pub fn field_border_css(&self) -> String {
        format!(
            "color-mix(in srgb, {} {}%, {})",
            self.css_var("text-dim"),
            (crate::contrast::FIELD_BORDER_MIX * 100.0).round() as u32,
            self.css_var("bg")
        )
    }

    /// Tokens only `notebook watch` chrome reads. Kept out of
    /// [`Self::css_tokens`] so a static page's `:root` does not change.
    pub fn chrome_tokens(&self) -> [(&'static str, &str); 4] {
        [
            ("--rl-shadow-color", self.shadow.as_ref()),
            ("--rl-scrim", self.scrim.as_ref()),
            ("--rl-banner-bg", self.banner_bg.as_ref()),
            ("--rl-banner-text", self.banner_text.as_ref()),
        ]
    }

    /// `var(--rl-<chrome token>, <literal>)` for [`Self::chrome_tokens`].
    pub fn chrome_var(&self, name: &str) -> String {
        match self.chrome_tokens().into_iter().find(|(n, _)| *n == name) {
            Some((n, value)) => format!("var({n}, {value})"),
            None => {
                debug_assert!(false, "unknown chrome token `{name}`");
                format!("var({name})")
            }
        }
    }

    /// `shadow` at `pct`% alpha: `color-mix(…, transparent)` paints the
    /// same pixels as `rgba(r, g, b, pct/100)`.
    pub fn shadow_mix(&self, pct: u32) -> String {
        format!(
            "color-mix(in srgb, {} {pct}%, transparent)",
            self.chrome_var("--rl-shadow-color")
        )
    }

    /// `:root { … }` with [`Self::chrome_tokens`], for watch pages.
    pub fn chrome_root_block(&self) -> String {
        let lines: Vec<String> = self
            .chrome_tokens()
            .iter()
            .map(|(n, v)| format!("    {n}: {v};"))
            .collect();
        format!("  :root {{\n{}\n  }}\n", lines.join("\n"))
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

/// Mix `fg` over `bg` by `amount` (`0` is the background, `1` is the foreground).
///
/// Both arguments are `#RRGGBB` or `RRGGBB`. The result is lowercase `RRGGBB`
/// with no hash, rounded half away from zero per channel. `None` when either
/// color is not 6-digit sRGB. Notebook tables use this so a stripe is a tint
/// of the theme's text color rather than a fixed gray.
pub fn blend_srgb(fg: &str, bg: &str, amount: f64) -> Option<String> {
    let (fr, fg_c, fb) = parse_hex_rgb(fg)?;
    let (br, bg_c, bb) = parse_hex_rgb(bg)?;
    let channel = |f: u8, b: u8| -> u8 {
        (f64::from(b) + (f64::from(f) - f64::from(b)) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Some(format!(
        "{:02x}{:02x}{:02x}",
        channel(fr, br),
        channel(fg_c, bg_c),
        channel(fb, bb)
    ))
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
    bg: Cow::Borrowed("#1e1e2e"),
    bg_secondary: Cow::Borrowed("#181825"),
    text: Cow::Borrowed("#cdd6f4"),
    text_dim: Cow::Borrowed("#a6adc8"),
    border: Cow::Borrowed("#313244"),
    border_subtle: Cow::Borrowed("#45475a"),
    accent_primary: Cow::Borrowed("#cba6f7"),
    accent_secondary: Cow::Borrowed("#89b4fa"),
    accent_tertiary: Cow::Borrowed("#74c7ec"),
    code_bg: Cow::Borrowed("#11111b"),
    output_bg: Cow::Borrowed("#181825"),
    inline_code_bg: Cow::Borrowed("#313244"),
    error_bg: Cow::Borrowed("#1e0a0a"),
    error_text: Cow::Borrowed("#f38ba8"),
    plot_bg: Cow::Borrowed("#1e1e2e"),
    plot_grid: Cow::Borrowed("rgba(150,150,180,0.3)"),
    syn_keyword: Cow::Borrowed("#cba6f7"),
    syn_function: Cow::Borrowed("#89b4fa"),
    syn_number: Cow::Borrowed("#fab387"),
    syn_string: Cow::Borrowed("#a6e3a1"),
    // overlay0 #6c7086 is 3.84:1 on crust; surface2 #585b70 is 2.46:1 on base.
    syn_comment: Cow::Borrowed("#787c92"),
    syn_operator: Cow::Borrowed("#89dceb"),
    footer_text: Cow::Borrowed("#81859c"),
    // yellow #f9e2af at 25% over base. Text contrast ~5.55.
    cm_mark_bg: Cow::Borrowed("#554f4e"),
    cm_note_bg: Cow::Borrowed("#181825"),
    cm_note_border: Cow::Borrowed("#89b4fa"),
    // Green 32%, red 40%, amber 28% over base. Flattened line colors match.
    dx_add_bar: Cow::Borrowed("#a6e3a1"),
    dx_mod_bar: Cow::Borrowed("#89b4fa"),
    dx_del_bar: Cow::Borrowed("#f38ba8"),
    dx_add_line: Cow::Borrowed("#0f1211"),
    dx_del_line: Cow::Borrowed("#161015"),
    dx_add_word: Cow::Borrowed("#3c4947"),
    dx_del_word: Cow::Borrowed("#4d3649"),
    shadow: Cow::Borrowed("#000000"),
    scrim: Cow::Borrowed("#11111b"),
    banner_bg: Cow::Borrowed("#aa2233"),
    banner_text: Cow::Borrowed("#ffffff"),
};

/// Catppuccin Macchiato (dark).
static MACCHIATO: ThemeColors = ThemeColors {
    bg: Cow::Borrowed("#24273a"),
    bg_secondary: Cow::Borrowed("#1e2030"),
    text: Cow::Borrowed("#cad3f5"),
    text_dim: Cow::Borrowed("#a5adcb"),
    border: Cow::Borrowed("#363a4f"),
    border_subtle: Cow::Borrowed("#494d64"),
    accent_primary: Cow::Borrowed("#c6a0f6"),
    accent_secondary: Cow::Borrowed("#8aadf4"),
    accent_tertiary: Cow::Borrowed("#7dc4e4"),
    code_bg: Cow::Borrowed("#181926"),
    output_bg: Cow::Borrowed("#1e2030"),
    inline_code_bg: Cow::Borrowed("#363a4f"),
    error_bg: Cow::Borrowed("#1e0a0a"),
    error_text: Cow::Borrowed("#ed8796"),
    plot_bg: Cow::Borrowed("#24273a"),
    plot_grid: Cow::Borrowed("rgba(150,150,180,0.3)"),
    syn_keyword: Cow::Borrowed("#c6a0f6"),
    syn_function: Cow::Borrowed("#8aadf4"),
    syn_number: Cow::Borrowed("#f5a97f"),
    syn_string: Cow::Borrowed("#a6da95"),
    // overlay0 #6e738d is 3.73:1 on crust; surface2 #5b6078 is 2.38:1 on base.
    syn_comment: Cow::Borrowed("#7c8199"),
    syn_operator: Cow::Borrowed("#91d7e3"),
    footer_text: Cow::Borrowed("#898ea5"),
    // yellow #eed49f at 25% over base. Text contrast ~5.19.
    cm_mark_bg: Cow::Borrowed("#565253"),
    cm_note_bg: Cow::Borrowed("#1e2030"),
    cm_note_border: Cow::Borrowed("#8aadf4"),
    // Green 30%, red 38%, amber 26% over base. The red bar is the stock
    // red nudged toward white so it still clears 3:1 on that wash.
    dx_add_bar: Cow::Borrowed("#a6da95"),
    dx_mod_bar: Cow::Borrowed("#8aadf4"),
    dx_del_bar: Cow::Borrowed("#f19fab"),
    dx_add_line: Cow::Borrowed("#161a1a"),
    dx_del_line: Cow::Borrowed("#1e181e"),
    dx_add_word: Cow::Borrowed("#414e4e"),
    dx_del_word: Cow::Borrowed("#514153"),
    shadow: Cow::Borrowed("#000000"),
    scrim: Cow::Borrowed("#11111b"),
    banner_bg: Cow::Borrowed("#aa2233"),
    banner_text: Cow::Borrowed("#ffffff"),
};

/// Catppuccin Frappé (dark).
static FRAPPE: ThemeColors = ThemeColors {
    bg: Cow::Borrowed("#303446"),
    bg_secondary: Cow::Borrowed("#292c3c"),
    text: Cow::Borrowed("#c6d0f5"),
    // subtext0 #a5adce is 4.26:1 on surface0 (sidebar / table head).
    text_dim: Cow::Borrowed("#aab2d1"),
    border: Cow::Borrowed("#414559"),
    border_subtle: Cow::Borrowed("#51576d"),
    // mauve #ca9ee6 is 4.30:1 and blue #8caaee is 4.10:1 on surface0.
    // Keywords stay the stock mauve: they are only drawn on crust.
    accent_primary: Cow::Borrowed("#cda4e7"),
    accent_secondary: Cow::Borrowed("#98b3f0"),
    accent_tertiary: Cow::Borrowed("#85c1dc"),
    code_bg: Cow::Borrowed("#232634"),
    output_bg: Cow::Borrowed("#292c3c"),
    inline_code_bg: Cow::Borrowed("#414559"),
    error_bg: Cow::Borrowed("#1e0a0a"),
    error_text: Cow::Borrowed("#e78284"),
    plot_bg: Cow::Borrowed("#303446"),
    plot_grid: Cow::Borrowed("rgba(150,150,180,0.3)"),
    syn_keyword: Cow::Borrowed("#ca9ee6"),
    syn_function: Cow::Borrowed("#8caaee"),
    syn_number: Cow::Borrowed("#ef9f76"),
    syn_string: Cow::Borrowed("#a6d189"),
    // overlay0 #737994 is 3.50:1 on crust; surface2 #626880 is 2.23:1 on base.
    syn_comment: Cow::Borrowed("#868ca3"),
    syn_operator: Cow::Borrowed("#99d1db"),
    footer_text: Cow::Borrowed("#979caf"),
    // yellow #e5c890 at 20% over base. 25% sits on the 4.5 threshold.
    cm_mark_bg: Cow::Borrowed("#545255"),
    cm_note_bg: Cow::Borrowed("#292c3c"),
    cm_note_border: Cow::Borrowed("#98b3f0"),
    // Green 22%, red 32%, amber 22% over base. Frappé's base is lighter,
    // so a smaller mix still reads as a tint. Red bar nudged like Macchiato.
    dx_add_bar: Cow::Borrowed("#a6d189"),
    dx_mod_bar: Cow::Borrowed("#8caaee"),
    dx_del_bar: Cow::Borrowed("#ec9b9d"),
    dx_add_line: Cow::Borrowed("#222728"),
    dx_del_line: Cow::Borrowed("#2b242c"),
    dx_add_word: Cow::Borrowed("#4a5755"),
    dx_del_word: Cow::Borrowed("#594b59"),
    shadow: Cow::Borrowed("#000000"),
    scrim: Cow::Borrowed("#11111b"),
    banner_bg: Cow::Borrowed("#aa2233"),
    banner_text: Cow::Borrowed("#ffffff"),
};

/// Catppuccin Latte (light) — also the `light` alias.
static LATTE: ThemeColors = ThemeColors {
    bg: Cow::Borrowed("#eff1f5"),
    bg_secondary: Cow::Borrowed("#e6e9ef"),
    text: Cow::Borrowed("#4c4f69"),
    // subtext0 #6c6f85 is 3.20:1 on surface0 and on the white PDF page.
    text_dim: Cow::Borrowed("#56586a"),
    border: Cow::Borrowed("#ccd0da"),
    border_subtle: Cow::Borrowed("#bcc0cc"),
    // Headings and links also sit on surface0 and on white paper.
    // Stock mauve #8839ef is 3.51:1 there, blue #1e66f5 is 3.18:1,
    // sapphire #179299 is 3.08:1 on mantle.
    accent_primary: Cow::Borrowed("#7113ec"),
    accent_secondary: Cow::Borrowed("#094dd3"),
    accent_tertiary: Cow::Borrowed("#12747a"),
    code_bg: Cow::Borrowed("#dce0e8"),
    output_bg: Cow::Borrowed("#e6e9ef"),
    inline_code_bg: Cow::Borrowed("#ccd0da"),
    error_bg: Cow::Borrowed("#fce4e4"),
    // Stock red #d20f39 is 4.46:1 on mantle and 4.10:1 on the code panel
    // (CodeMirror errors use this role).
    error_text: Cow::Borrowed("#c60e36"),
    plot_bg: Cow::Borrowed("#eff1f5"),
    plot_grid: Cow::Borrowed("rgba(100,100,120,0.2)"),
    // Code-panel tokens are a smaller darkening than the headings: crust
    // is lighter than surface0, so the same hue clears 4.5 sooner.
    // Stock peach #fe640b is 2.25:1 on crust and 2.98:1 on white paper;
    // green #40a02b is 2.53:1; overlay0 #9ca0b0 is 1.97:1.
    syn_keyword: Cow::Borrowed("#802cee"),
    syn_function: Cow::Borrowed("#0a55ea"),
    syn_number: Cow::Borrowed("#ad4001"),
    syn_string: Cow::Borrowed("#2d711e"),
    syn_comment: Cow::Borrowed("#5e6376"),
    syn_operator: Cow::Borrowed("#116e74"),
    // surface2 #9ca0b0 is 2.30:1 on base.
    footer_text: Cow::Borrowed("#686d82"),
    // yellow #df8e1d at 25% over base. Text contrast ~5.74.
    cm_mark_bg: Cow::Borrowed("#ebd8bf"),
    cm_note_bg: Cow::Borrowed("#e6e9ef"),
    cm_note_border: Cow::Borrowed("#094dd3"),
    // Opaque pastels: green 38%, red 22%, amber 42% over base. Stock green
    // and yellow are under 3:1 on base, so those bars are darker.
    dx_add_bar: Cow::Borrowed("#307820"),
    dx_mod_bar: Cow::Borrowed("#094dd3"),
    dx_del_bar: Cow::Borrowed("#d20f39"),
    dx_add_line: Cow::Borrowed("#d7e4db"),
    dx_del_line: Cow::Borrowed("#ebdce3"),
    dx_add_word: Cow::Borrowed("#c9dfc9"),
    dx_del_word: Cow::Borrowed("#eac8d3"),
    shadow: Cow::Borrowed("#000000"),
    scrim: Cow::Borrowed("#11111b"),
    banner_bg: Cow::Borrowed("#aa2233"),
    banner_text: Cow::Borrowed("#ffffff"),
};

/// Light or dark chrome, from background luminance. Not a theme name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
}

/// System font stacks and the sizes the notebook stylesheet already uses.
///
/// Stacks are system fonts only: no font files and no `@font-face`. Each
/// stack ends in a generic family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typography {
    pub body: String,
    pub heading: String,
    pub mono: String,
    pub size: String,
    pub line_height: String,
    pub measure: String,
}

/// Corner radii and the 1px border the page already draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeTokens {
    pub radius_sm: String,
    pub radius: String,
    pub radius_lg: String,
    pub border_width: String,
}

/// Spacing steps that already appear as margins, padding, or gaps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceTokens {
    pub s1: String,
    pub s2: String,
    pub s3: String,
    pub s4: String,
    pub s5: String,
    pub s6: String,
}

/// `color-mix` percentages for table header, zebra, and rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableTokens {
    pub head: String,
    pub stripe: String,
    pub rule: String,
}

/// Callout colors as references to the existing accent tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalloutTokens {
    pub note: String,
    pub tip: String,
    pub important: String,
    pub warning: String,
    pub caution: String,
    pub bg: String,
}

/// Unvisited and visited link colors. Visited is the primary accent,
/// which is what the prose `:visited` rule already paints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkTokens {
    pub link: String,
    pub visited: String,
}

/// Plot tokens for a later phase. Phase 1 declares them and does not
/// feed them to Plotly, SVG, or Mermaid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlotTokens {
    pub paper: String,
    pub text: String,
    pub font: String,
    /// Eight series colors. The first six are [`SeriesColor::cycle`];
    /// the last two are the `orange` and `brown` names already in that enum.
    pub series: [String; 8],
}

/// A builtin palette plus the design tokens whose values are today's
/// literals. `print` stays `None`: LaTeX and PDF remain Latte on white.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTheme {
    pub name: String,
    pub mode: ThemeMode,
    pub colors: ThemeColors,
    pub typography: Typography,
    pub shape: ShapeTokens,
    pub space: SpaceTokens,
    pub table: TableTokens,
    pub callout: CalloutTokens,
    pub link: LinkTokens,
    /// Declared as `auto` and not applied. Applying it would paint a
    /// focus ring the page does not draw today.
    pub focus_ring: String,
    pub plot: PlotTokens,
    pub shadow: String,
    pub print: Option<Box<ResolvedTheme>>,
}

const FONT_BODY: &str =
    r#"-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif"#;
const FONT_MONO: &str = r#""SF Mono", "Fira Code", "JetBrains Mono", monospace"#;

impl ResolvedTheme {
    /// Tokens for a builtin or any other [`ThemeColors`]. The name is
    /// `mocha` / `macchiato` / `frappe` / `latte` when `colors` is one of
    /// those statics, and `custom` otherwise. Design-token values do not
    /// depend on the palette: they point at the color tokens.
    pub fn from_colors(colors: &ThemeColors) -> Self {
        let name = if std::ptr::eq(colors, &MOCHA) {
            "mocha"
        } else if std::ptr::eq(colors, &MACCHIATO) {
            "macchiato"
        } else if std::ptr::eq(colors, &FRAPPE) {
            "frappe"
        } else if std::ptr::eq(colors, &LATTE) {
            "latte"
        } else {
            "custom"
        };
        let mode = if colors.is_dark() {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        let series = [
            series_hex(crate::figure::SeriesColor::cycle(0)),
            series_hex(crate::figure::SeriesColor::cycle(1)),
            series_hex(crate::figure::SeriesColor::cycle(2)),
            series_hex(crate::figure::SeriesColor::cycle(3)),
            series_hex(crate::figure::SeriesColor::cycle(4)),
            series_hex(crate::figure::SeriesColor::cycle(5)),
            series_hex(crate::figure::SeriesColor::parse("orange").expect("orange")),
            series_hex(crate::figure::SeriesColor::parse("brown").expect("brown")),
        ];
        Self {
            name: name.to_string(),
            mode,
            colors: colors.clone(),
            typography: Typography {
                body: FONT_BODY.to_string(),
                heading: FONT_BODY.to_string(),
                mono: FONT_MONO.to_string(),
                size: "medium".to_string(),
                line_height: "1.7".to_string(),
                measure: "960px".to_string(),
            },
            shape: ShapeTokens {
                radius_sm: "3px".to_string(),
                radius: "6px".to_string(),
                radius_lg: "8px".to_string(),
                border_width: "1px".to_string(),
            },
            space: SpaceTokens {
                s1: "0.5rem".to_string(),
                s2: "0.75rem".to_string(),
                s3: "1rem".to_string(),
                s4: "1.5rem".to_string(),
                s5: "2rem".to_string(),
                s6: "2.5rem".to_string(),
            },
            table: TableTokens {
                head: "color-mix(in srgb, var(--rl-text) 11%, var(--rl-bg))".to_string(),
                stripe: "color-mix(in srgb, var(--rl-text) 6%, var(--rl-bg))".to_string(),
                rule: "color-mix(in srgb, var(--rl-text) 30%, var(--rl-bg))".to_string(),
            },
            callout: CalloutTokens {
                note: "var(--rl-accent-secondary)".to_string(),
                tip: "var(--rl-accent-tertiary)".to_string(),
                important: "var(--rl-accent-primary)".to_string(),
                warning: "var(--rl-error-text)".to_string(),
                caution: "var(--rl-error-text)".to_string(),
                bg: "var(--rl-bg-secondary)".to_string(),
            },
            link: LinkTokens {
                link: "var(--rl-accent-secondary)".to_string(),
                visited: "var(--rl-accent-primary)".to_string(),
            },
            focus_ring: "auto".to_string(),
            plot: PlotTokens {
                paper: "var(--rl-plot-bg)".to_string(),
                text: "var(--rl-text)".to_string(),
                font: "sans-serif".to_string(),
                series,
            },
            shadow: "0 8px 24px rgba(0,0,0,.25)".to_string(),
            print: None,
        }
    }

    /// The 33 color custom properties, then the design tokens. Stripping
    /// the design-token lines leaves [`ThemeColors::css_custom_properties`].
    pub fn css_custom_properties(&self) -> String {
        let mut out = self.colors.css_custom_properties();
        out.push('\n');
        out.push_str(&self.design_lines());
        out
    }

    fn design_lines(&self) -> String {
        let t = &self.typography;
        let s = &self.shape;
        let sp = &self.space;
        let rows: Vec<(&str, &str)> = vec![
            ("--rl-font-body", t.body.as_str()),
            ("--rl-font-heading", t.heading.as_str()),
            ("--rl-font-mono", t.mono.as_str()),
            ("--rl-font-size", t.size.as_str()),
            ("--rl-line-height", t.line_height.as_str()),
            ("--rl-measure", t.measure.as_str()),
            ("--rl-radius-sm", s.radius_sm.as_str()),
            ("--rl-radius", s.radius.as_str()),
            ("--rl-radius-lg", s.radius_lg.as_str()),
            ("--rl-border-width", s.border_width.as_str()),
            ("--rl-space-1", sp.s1.as_str()),
            ("--rl-space-2", sp.s2.as_str()),
            ("--rl-space-3", sp.s3.as_str()),
            ("--rl-space-4", sp.s4.as_str()),
            ("--rl-space-5", sp.s5.as_str()),
            ("--rl-space-6", sp.s6.as_str()),
            ("--rl-table-head", self.table.head.as_str()),
            ("--rl-table-stripe", self.table.stripe.as_str()),
            ("--rl-table-rule", self.table.rule.as_str()),
            ("--rl-callout-note", self.callout.note.as_str()),
            ("--rl-callout-tip", self.callout.tip.as_str()),
            ("--rl-callout-important", self.callout.important.as_str()),
            ("--rl-callout-warning", self.callout.warning.as_str()),
            ("--rl-callout-caution", self.callout.caution.as_str()),
            ("--rl-callout-bg", self.callout.bg.as_str()),
            ("--rl-link", self.link.link.as_str()),
            ("--rl-link-visited", self.link.visited.as_str()),
            ("--rl-focus-ring", self.focus_ring.as_str()),
            ("--rl-plot-paper", self.plot.paper.as_str()),
            ("--rl-plot-text", self.plot.text.as_str()),
            ("--rl-plot-font", self.plot.font.as_str()),
            ("--rl-plot-1", self.plot.series[0].as_str()),
            ("--rl-plot-2", self.plot.series[1].as_str()),
            ("--rl-plot-3", self.plot.series[2].as_str()),
            ("--rl-plot-4", self.plot.series[3].as_str()),
            ("--rl-plot-5", self.plot.series[4].as_str()),
            ("--rl-plot-6", self.plot.series[5].as_str()),
            ("--rl-plot-7", self.plot.series[6].as_str()),
            ("--rl-plot-8", self.plot.series[7].as_str()),
            ("--rl-shadow", self.shadow.as_str()),
        ];
        rows.iter()
            .map(|(name, value)| format!("    {name}: {value};"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn font_body_css(&self) -> String {
        format!(
            "var(--rl-font-body, var(--rl-font-heading, {}))",
            self.typography.body
        )
    }

    pub fn font_mono_css(&self) -> String {
        format!("var(--rl-font-mono, {})", self.typography.mono)
    }

    pub fn font_size_css(&self) -> String {
        format!("var(--rl-font-size, {})", self.typography.size)
    }

    pub fn line_height_css(&self) -> String {
        format!("var(--rl-line-height, {})", self.typography.line_height)
    }

    pub fn measure_css(&self) -> String {
        format!("var(--rl-measure, {})", self.typography.measure)
    }

    pub fn radius_css(&self) -> String {
        format!("var(--rl-radius, {})", self.shape.radius)
    }

    pub fn radius_sm_css(&self) -> String {
        format!("var(--rl-radius-sm, {})", self.shape.radius_sm)
    }

    pub fn radius_lg_css(&self) -> String {
        format!("var(--rl-radius-lg, {})", self.shape.radius_lg)
    }

    pub fn border_width_css(&self) -> String {
        format!("var(--rl-border-width, {})", self.shape.border_width)
    }

    pub fn space_css(&self, step: u8) -> String {
        let (name, value) = match step {
            1 => ("--rl-space-1", self.space.s1.as_str()),
            2 => ("--rl-space-2", self.space.s2.as_str()),
            3 => ("--rl-space-3", self.space.s3.as_str()),
            4 => ("--rl-space-4", self.space.s4.as_str()),
            5 => ("--rl-space-5", self.space.s5.as_str()),
            6 => ("--rl-space-6", self.space.s6.as_str()),
            _ => unreachable!("space step {step} is not in the scale"),
        };
        format!("var({name}, {value})")
    }

    pub fn table_head_css(&self) -> String {
        self.mix_fallback("--rl-table-head", "11%")
    }

    pub fn table_stripe_css(&self) -> String {
        self.mix_fallback("--rl-table-stripe", "6%")
    }

    pub fn table_rule_css(&self) -> String {
        self.mix_fallback("--rl-table-rule", "30%")
    }

    fn mix_fallback(&self, name: &str, pct: &str) -> String {
        format!(
            "var({name}, color-mix(in srgb, {} {pct}, {}))",
            self.colors.css_var("text"),
            self.colors.css_var("bg")
        )
    }

    pub fn callout_note_css(&self) -> String {
        format!(
            "var(--rl-callout-note, {})",
            self.colors.css_var("accent-secondary")
        )
    }

    pub fn callout_tip_css(&self) -> String {
        format!(
            "var(--rl-callout-tip, {})",
            self.colors.css_var("accent-tertiary")
        )
    }

    pub fn callout_important_css(&self) -> String {
        format!(
            "var(--rl-callout-important, {})",
            self.colors.css_var("accent-primary")
        )
    }

    pub fn callout_warning_css(&self) -> String {
        format!(
            "var(--rl-callout-warning, {})",
            self.colors.css_var("error-text")
        )
    }

    pub fn callout_caution_css(&self) -> String {
        format!(
            "var(--rl-callout-caution, {})",
            self.colors.css_var("error-text")
        )
    }

    pub fn callout_bg_css(&self) -> String {
        format!(
            "var(--rl-callout-bg, {})",
            self.colors.css_var("bg-secondary")
        )
    }

    pub fn link_css(&self) -> String {
        format!(
            "var(--rl-link, {})",
            self.colors.css_var("accent-secondary")
        )
    }

    pub fn link_visited_css(&self) -> String {
        format!(
            "var(--rl-link-visited, {})",
            self.colors.css_var("accent-primary")
        )
    }
}

fn series_hex(color: crate::figure::SeriesColor) -> String {
    let (r, g, b) = color.to_rgb();
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

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
            bg: Cow::Borrowed("#111111"),
            ..MOCHA.clone()
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
        assert_eq!(tokens.len(), 33);
        assert_eq!(tokens[31], ("--rl-dx-add-word", "#3c4947"));
        assert_eq!(tokens[32], ("--rl-dx-del-word", "#4d3649"));
        assert_eq!(tokens[26], ("--rl-dx-add-bar", "#a6e3a1"));
        assert_eq!(tokens[27], ("--rl-dx-mod-bar", "#89b4fa"));
        assert_eq!(tokens[28], ("--rl-dx-del-bar", "#f38ba8"));
        assert_eq!(tokens[29], ("--rl-dx-add-line", "#0f1211"));
        assert_eq!(tokens[30], ("--rl-dx-del-line", "#161015"));
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
    fn blend_srgb_tints_foreground_over_the_page() {
        // Latte text on the white PDF paper.
        assert_eq!(blend_srgb("#4c4f69", "#ffffff", 0.06).unwrap(), "f4f4f6");
        assert_eq!(blend_srgb("#4c4f69", "#ffffff", 0.11).unwrap(), "ebecef");
        assert_eq!(blend_srgb("#4c4f69", "#ffffff", 0.30).unwrap(), "c9cad2");
        // Mocha text on the Mocha page. A dark theme must not reuse the light gray.
        assert_eq!(blend_srgb("#cdd6f4", "#1e1e2e", 0.06).unwrap(), "29293a");
        assert_eq!(blend_srgb("#cdd6f4", "#1e1e2e", 0.11).unwrap(), "313244");
        assert_eq!(blend_srgb("#cdd6f4", "#1e1e2e", 0.30).unwrap(), "535569");
        assert_eq!(
            blend_srgb(MOCHA.text.as_ref(), MOCHA.bg.as_ref(), 0.06).unwrap(),
            "29293a"
        );
        assert_ne!(
            blend_srgb(MOCHA.text.as_ref(), MOCHA.bg.as_ref(), 0.06).unwrap(),
            blend_srgb(LATTE.text.as_ref(), "#ffffff", 0.06).unwrap()
        );
        assert!(blend_srgb("nope", "#ffffff", 0.06).is_none());
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
            let u = contrast_ratio(colors.accent_secondary.as_ref(), colors.bg.as_ref()).unwrap();
            let v = contrast_ratio(colors.accent_primary.as_ref(), colors.bg.as_ref()).unwrap();
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
            let mark = contrast_ratio(colors.text.as_ref(), colors.cm_mark_bg.as_ref()).unwrap();
            let note = contrast_ratio(colors.text.as_ref(), colors.cm_note_bg.as_ref()).unwrap();
            let border =
                contrast_ratio(colors.cm_note_border.as_ref(), colors.cm_note_bg.as_ref()).unwrap();
            assert!(mark >= 4.5, "{name} text/mark {mark:.2} < 4.5");
            assert!(note >= 4.5, "{name} text/note {note:.2} < 4.5");
            assert!(border >= 4.5, "{name} border/note {border:.2} < 4.5");
        }
    }

    /// Font role → the backgrounds that role is actually painted on.
    /// A swatch that clears the page can still fail on the code panel,
    /// the output panel, or the sidebar border.
    fn font_surfaces(c: &ThemeColors) -> Vec<(&'static str, &str, Vec<(&'static str, &str)>)> {
        let bg = ("page", c.bg.as_ref());
        let secondary = ("sidebar", c.bg_secondary.as_ref());
        let border = ("border", c.border.as_ref());
        let code = ("code panel", c.code_bg.as_ref());
        let output = ("output panel", c.output_bg.as_ref());
        let inline = ("inline code / table head", c.inline_code_bg.as_ref());
        let err = ("error panel", c.error_bg.as_ref());
        vec![
            (
                "text",
                c.text.as_ref(),
                vec![bg, code, secondary, inline, border],
            ),
            (
                "text_dim",
                c.text_dim.as_ref(),
                vec![bg, secondary, output, code, border],
            ),
            ("footer_text", c.footer_text.as_ref(), vec![bg]),
            (
                "accent_primary",
                c.accent_primary.as_ref(),
                vec![bg, secondary, border, inline],
            ),
            (
                "accent_secondary",
                c.accent_secondary.as_ref(),
                vec![bg, secondary, border, code],
            ),
            (
                "accent_tertiary",
                c.accent_tertiary.as_ref(),
                vec![bg, secondary],
            ),
            (
                "error_text",
                c.error_text.as_ref(),
                vec![err, secondary, code],
            ),
            ("syn_keyword", c.syn_keyword.as_ref(), vec![code]),
            ("syn_function", c.syn_function.as_ref(), vec![code]),
            ("syn_number", c.syn_number.as_ref(), vec![code]),
            ("syn_string", c.syn_string.as_ref(), vec![code]),
            ("syn_comment", c.syn_comment.as_ref(), vec![code]),
            ("syn_operator", c.syn_operator.as_ref(), vec![code]),
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
            assert_aa(
                name,
                "toolbar",
                colors.bg.as_ref(),
                "accent",
                colors.accent_primary.as_ref(),
            );
            for (role, fg, surfaces) in font_surfaces(colors) {
                for (surface, bg) in surfaces {
                    assert_aa(name, role, fg, surface, bg);
                }
            }
        }
        // PDF / LaTeX force Latte onto white paper (no pagecolor).
        let paper = "#ffffff";
        for (role, fg) in [
            ("text", LATTE.text.as_ref()),
            ("text_dim", LATTE.text_dim.as_ref()),
            ("accent_primary", LATTE.accent_primary.as_ref()),
            ("accent_secondary", LATTE.accent_secondary.as_ref()),
            ("accent_tertiary", LATTE.accent_tertiary.as_ref()),
            ("error_text", LATTE.error_text.as_ref()),
            ("syn_keyword", LATTE.syn_keyword.as_ref()),
            ("syn_function", LATTE.syn_function.as_ref()),
            ("syn_number", LATTE.syn_number.as_ref()),
            ("syn_string", LATTE.syn_string.as_ref()),
            ("syn_comment", LATTE.syn_comment.as_ref()),
            ("syn_operator", LATTE.syn_operator.as_ref()),
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
        assert_eq!(MOCHA.text.as_ref(), "#cdd6f4");
        assert_eq!(MOCHA.syn_keyword.as_ref(), "#cba6f7");
        assert_eq!(MOCHA.syn_number.as_ref(), "#fab387");
        assert_eq!(MOCHA.error_text.as_ref(), "#f38ba8");
        assert_eq!(MOCHA.bg.as_ref(), "#1e1e2e");
        assert_eq!(MOCHA.code_bg.as_ref(), "#11111b");
        // Frappé keywords only sit on the code panel, which the stock
        // mauve already clears, so they do not follow the accent nudge.
        assert_eq!(FRAPPE.syn_keyword.as_ref(), "#ca9ee6");
        assert_ne!(FRAPPE.accent_primary.as_ref(), FRAPPE.syn_keyword.as_ref());
        assert_eq!(LATTE.text.as_ref(), "#4c4f69");
        assert_eq!(LATTE.bg.as_ref(), "#eff1f5");
        assert_eq!(LATTE.code_bg.as_ref(), "#dce0e8");
        assert_eq!(LATTE.error_bg.as_ref(), "#fce4e4");
    }

    /// Diff bars are non-text marks beside the text: 3:1 on the page.
    /// A tinted line in a code listing must keep body text and every
    /// syntax color at 4.5:1, which is why the tint keeps the code
    /// panel's luminance instead of being a translucent wash.
    #[test]
    fn diff_bars_and_line_tints_meet_contrast() {
        for (name, c) in [
            ("mocha", &MOCHA),
            ("macchiato", &MACCHIATO),
            ("frappe", &FRAPPE),
            ("latte", &LATTE),
        ] {
            for (label, bar) in [
                ("add", c.dx_add_bar.as_ref()),
                ("mod", c.dx_mod_bar.as_ref()),
                ("del", c.dx_del_bar.as_ref()),
            ] {
                let ratio = contrast_ratio(bar, c.bg.as_ref()).unwrap();
                assert!(ratio >= 3.0, "{name} {label} bar on page {ratio:.2} < 3");
            }
            for (label, line) in [
                ("add", c.dx_add_line.as_ref()),
                ("del", c.dx_del_line.as_ref()),
            ] {
                assert_ne!(
                    line,
                    c.code_bg.as_ref(),
                    "{name} {label} line must be a visible tint"
                );
                for (role, fg) in [
                    ("text", c.text.as_ref()),
                    ("syn_keyword", c.syn_keyword.as_ref()),
                    ("syn_function", c.syn_function.as_ref()),
                    ("syn_number", c.syn_number.as_ref()),
                    ("syn_string", c.syn_string.as_ref()),
                    ("syn_comment", c.syn_comment.as_ref()),
                    ("syn_operator", c.syn_operator.as_ref()),
                ] {
                    let ratio = contrast_ratio(fg, line).unwrap();
                    assert!(
                        ratio >= 4.5,
                        "{name} {role} on {label} line {line} is {ratio:.2} < 4.5"
                    );
                }
            }
            for (label, word) in [
                ("add", c.dx_add_word.as_ref()),
                ("del", c.dx_del_word.as_ref()),
            ] {
                let ratio = contrast_ratio(c.text.as_ref(), word).unwrap();
                assert!(
                    ratio >= 4.5,
                    "{name} text on {label} word {word} is {ratio:.2} < 4.5"
                );
                assert_ne!(
                    word,
                    c.bg.as_ref(),
                    "{name} {label} word mark must be visible"
                );
            }
            // Code words are underlined in the bar color, on the line tint.
            for (label, bar, line) in [
                ("add", c.dx_add_bar.as_ref(), c.dx_add_line.as_ref()),
                ("del", c.dx_del_bar.as_ref(), c.dx_del_line.as_ref()),
            ] {
                let ratio = contrast_ratio(bar, line).unwrap();
                assert!(
                    ratio >= 3.0,
                    "{name} {label} underline on line {ratio:.2} < 3"
                );
            }
            // The three bars must be told apart.
            assert_ne!(c.dx_add_bar.as_ref(), c.dx_mod_bar.as_ref(), "{name}");
            assert_ne!(c.dx_mod_bar.as_ref(), c.dx_del_bar.as_ref(), "{name}");
        }
    }

    #[test]
    fn resolved_builtin_tokens_match_todays_literals() {
        let mocha = ResolvedTheme::from_colors(&MOCHA);
        assert_eq!(mocha.name, "mocha");
        assert_eq!(mocha.mode, ThemeMode::Dark);
        assert!(mocha.print.is_none());
        let css = mocha.css_custom_properties();
        let colors = MOCHA.css_custom_properties();
        assert!(css.starts_with(&colors), "color block must stay first");
        assert_eq!(css.lines().next().unwrap(), colors.lines().next().unwrap());
        for (name, value) in MOCHA.css_tokens() {
            assert!(css.contains(&format!("    {name}: {value};")));
        }
        assert!(css.contains(&format!("    --rl-font-body: {FONT_BODY};")));
        assert!(css.contains(&format!("    --rl-font-heading: {FONT_BODY};")));
        assert!(css.contains(&format!("    --rl-font-mono: {FONT_MONO};")));
        assert!(css.contains("    --rl-font-size: medium;"));
        assert!(css.contains("    --rl-line-height: 1.7;"));
        assert!(css.contains("    --rl-measure: 960px;"));
        assert!(css.contains("    --rl-radius-sm: 3px;"));
        assert!(css.contains("    --rl-radius: 6px;"));
        assert!(css.contains("    --rl-radius-lg: 8px;"));
        assert!(css.contains("    --rl-border-width: 1px;"));
        assert!(css.contains("    --rl-space-1: 0.5rem;"));
        assert!(css.contains("    --rl-space-6: 2.5rem;"));
        assert!(css.contains(
            "    --rl-table-head: color-mix(in srgb, var(--rl-text) 11%, var(--rl-bg));"
        ));
        assert!(css.contains("    --rl-callout-note: var(--rl-accent-secondary);"));
        assert!(css.contains("    --rl-link: var(--rl-accent-secondary);"));
        assert!(css.contains("    --rl-link-visited: var(--rl-accent-primary);"));
        assert!(css.contains("    --rl-focus-ring: auto;"));
        assert!(css.contains("    --rl-plot-paper: var(--rl-plot-bg);"));
        assert!(css.contains("    --rl-plot-font: sans-serif;"));
        assert!(css.contains("    --rl-shadow: 0 8px 24px rgba(0,0,0,.25);"));
        for i in 0..6 {
            let hex = series_hex(crate::figure::SeriesColor::cycle(i));
            assert!(css.contains(&format!("    --rl-plot-{}: {hex};", i + 1)));
        }
        assert!(css.contains(&format!(
            "    --rl-plot-7: {};",
            series_hex(crate::figure::SeriesColor::parse("orange").unwrap())
        )));
        assert!(css.contains(&format!(
            "    --rl-plot-8: {};",
            series_hex(crate::figure::SeriesColor::parse("brown").unwrap())
        )));
        assert!(!css.contains("url("));
        assert!(!css.contains("@font-face"));
        assert!(mocha.typography.body.ends_with("sans-serif"));
        assert!(mocha.typography.heading.ends_with("sans-serif"));
        assert!(mocha.typography.mono.ends_with("monospace"));
        assert!(mocha.plot.font.ends_with("sans-serif"));
        let latte = ResolvedTheme::from_colors(&LATTE);
        assert_eq!(latte.name, "latte");
        assert_eq!(latte.mode, ThemeMode::Light);
        assert!(latte.print.is_none());
        // Design-token text does not depend on the palette.
        assert_eq!(
            mocha
                .css_custom_properties()
                .lines()
                .skip(33)
                .collect::<Vec<_>>(),
            latte
                .css_custom_properties()
                .lines()
                .skip(33)
                .collect::<Vec<_>>()
        );
        let mut owned = MOCHA.clone();
        owned.bg = Cow::Borrowed("#111111");
        assert_eq!(ResolvedTheme::from_colors(&owned).name, "custom");
        assert_eq!(
            mocha.font_body_css(),
            format!("var(--rl-font-body, var(--rl-font-heading, {FONT_BODY}))")
        );
        assert_eq!(
            mocha.link_visited_css(),
            format!(
                "var(--rl-link-visited, {})",
                MOCHA.css_var("accent-primary")
            )
        );
    }
}
