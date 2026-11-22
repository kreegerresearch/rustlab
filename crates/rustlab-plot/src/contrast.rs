//! WCAG 2.x contrast checks for a notebook palette.
//!
//! One list of (foreground, surface) pairs, each tied to the places the
//! notebook HTML, `notebook watch` chrome, or the PDF paints that pair.
//! The unit tests run it over the four builtins, and `rustlab-notebook
//! theme check` runs it over a theme file, so both apply the same
//! thresholds to the same surfaces.
//!
//! Text needs 4.5:1 (WCAG 1.4.3). Large text, user-interface boundaries,
//! state indicators, and focus rings need 3:1 (1.4.3 large, 1.4.11).
//! Surfaces the stylesheet computes (table head and stripe, a button at
//! partial opacity) are blended here the same way the CSS blends them.

use crate::theme::{blend_srgb, contrast_ratio, ThemeColors};

/// Minimum ratio a pair must reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Body-size text: 4.5:1.
    Text,
    /// Large text, component boundaries, state, focus: 3:1.
    Ui,
}

impl Need {
    pub fn min(self) -> f64 {
        match self {
            Need::Text => 4.5,
            Need::Ui => 3.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Need::Text => "text 4.5",
            Need::Ui => "UI 3.0",
        }
    }
}

/// Where a pair is painted. PDF checks are Latte ink on white paper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Html,
    Watch,
    Pdf,
}

impl Surface {
    pub fn label(self) -> &'static str {
        match self {
            Surface::Html => "HTML",
            Surface::Watch => "watch",
            Surface::Pdf => "PDF",
        }
    }
}

/// One foreground on one surface.
#[derive(Debug, Clone, PartialEq)]
pub struct ContrastCheck {
    /// Stable short id, used in tests and the `theme check` table.
    pub id: &'static str,
    /// Elements that paint this pair.
    pub what: &'static str,
    /// Role (or computed role) of the foreground.
    pub fg_role: &'static str,
    /// Role (or computed role) of the surface.
    pub bg_role: &'static str,
    pub fg: String,
    pub bg: String,
    pub need: Need,
    pub surface: Surface,
}

impl ContrastCheck {
    /// `None` when either side is not a `#RRGGBB` swatch.
    pub fn ratio(&self) -> Option<f64> {
        contrast_ratio(&self.fg, &self.bg)
    }

    /// A pair that cannot be computed fails.
    pub fn passes(&self) -> bool {
        self.ratio().is_some_and(|r| r >= self.need.min())
    }
}

/// Table header and stripe are `color-mix(text N%, bg)`; the PDF uses the
/// same mix on white. Kept equal to the stylesheet's percentages.
pub const TABLE_HEAD_MIX: f64 = 0.11;
pub const TABLE_STRIPE_MIX: f64 = 0.06;
/// `#rl-toolbar button.rl-quiet` opacity (the ⋯ menu button).
pub const QUIET_BUTTON_OPACITY: f64 = 0.85;
/// `section.rl-block.rl-cell-stale` opacity while an edited cell waits.
pub const STALE_CELL_OPACITY: f64 = 0.85;
/// Form-field boundary: `color-mix(text_dim 75%, bg)`. Number inputs,
/// the watch ref field, and the inline cell editor draw it.
pub const FIELD_BORDER_MIX: f64 = 0.75;
/// White `article` paper for PDF.
pub const PDF_PAPER: &str = "#ffffff";

fn mix(fg: &str, bg: &str, amount: f64) -> String {
    match blend_srgb(fg, bg, amount) {
        Some(h) => format!("#{h}"),
        None => String::new(),
    }
}

/// Every screen pair (HTML and watch) for one palette.
pub fn screen_checks(c: &ThemeColors) -> Vec<ContrastCheck> {
    use Need::{Text, Ui};
    use Surface::{Html, Watch};
    let s = |x: &std::borrow::Cow<'static, str>| x.as_ref().to_string();
    let head = mix(c.text.as_ref(), c.bg.as_ref(), TABLE_HEAD_MIX);
    let stripe = mix(c.text.as_ref(), c.bg.as_ref(), TABLE_STRIPE_MIX);
    let quiet = mix(
        c.text.as_ref(),
        c.bg_secondary.as_ref(),
        QUIET_BUTTON_OPACITY,
    );
    let stale = mix(c.text.as_ref(), c.bg.as_ref(), STALE_CELL_OPACITY);
    let field = mix(c.text_dim.as_ref(), c.bg.as_ref(), FIELD_BORDER_MIX);
    let row = |id: &'static str,
               what: &'static str,
               fg_role: &'static str,
               fg: String,
               bg_role: &'static str,
               bg: String,
               need: Need,
               surface: Surface| ContrastCheck {
        id,
        what,
        fg_role,
        bg_role,
        fg,
        bg,
        need,
        surface,
    };
    let mut v = vec![
        // ── Body text ────────────────────────────────────────────────
        row("text/page", "body text, table cells, widget labels, number inputs", "text", s(&c.text), "bg", s(&c.bg), Text, Html),
        row("text/secondary", "callout and exercise body, topbar title, toolbar and cell buttons, menus, drawer, undo toast", "text", s(&c.text), "bg_secondary", s(&c.bg_secondary), Text, Html),
        row("text/border", "sidebar link hover, nav toggle, file-browser current row", "text", s(&c.text), "border", s(&c.border), Text, Html),
        row("text/code", "source cells, CodeMirror, watch source pane", "text", s(&c.text), "code_bg", s(&c.code_bg), Text, Html),
        row("text/output", "table display cells (table.rl-table)", "text", s(&c.text), "output_bg", s(&c.output_bg), Text, Html),
        row("text/inline-code", "inline code", "text", s(&c.text), "inline_code_bg", s(&c.inline_code_bg), Text, Html),
        row("text/table-stripe", "even table rows", "text", s(&c.text), "table stripe (text 6% over bg)", stripe.clone(), Text, Html),
        row("text/mark", "==highlight== text", "text", s(&c.text), "cm_mark_bg", s(&c.cm_mark_bg), Text, Html),
        row("text/note", "margin notes and comment cards", "text", s(&c.text), "cm_note_bg", s(&c.cm_note_bg), Text, Html),
        row("text-dim/page", "blockquote, language chip, missing figure, comment labels, menu hints", "text_dim", s(&c.text_dim), "bg", s(&c.bg), Text, Html),
        row("text-dim/secondary", "sidebar links, topbar crumbs, page-nav labels, CodeMirror gutter, diff hunk headers, radio rings", "text_dim", s(&c.text_dim), "bg_secondary", s(&c.bg_secondary), Text, Html),
        row("text-dim/output", "printed output", "text_dim", s(&c.text_dim), "output_bg", s(&c.output_bg), Text, Html),
        row("text-dim/code", "watch source pane header and status", "text_dim", s(&c.text_dim), "code_bg", s(&c.code_bg), Text, Watch),
        row("text-dim/border", "dim text on a hovered row", "text_dim", s(&c.text_dim), "border", s(&c.border), Text, Html),
        row("footer/page", "page footer", "footer_text", s(&c.footer_text), "bg", s(&c.bg), Text, Html),
        // ── Headings and links ───────────────────────────────────────
        row("accent-primary/page", "h1, visited links, comment refs, cell error note, widget drives link", "accent_primary", s(&c.accent_primary), "bg", s(&c.bg), Text, Html),
        row("accent-primary/secondary", "TOC title, exercise title, IMPORTANT callout title, widget value, file-browser folder", "accent_primary", s(&c.accent_primary), "bg_secondary", s(&c.bg_secondary), Text, Html),
        row("accent-primary/border", "accent text on a hovered row", "accent_primary", s(&c.accent_primary), "border", s(&c.border), Text, Html),
        row("accent-primary/inline-code", "accent inside inline code", "accent_primary", s(&c.accent_primary), "inline_code_bg", s(&c.inline_code_bg), Text, Html),
        row("accent-primary/table-head", "table header text", "accent_primary", s(&c.accent_primary), "table head (text 11% over bg)", head.clone(), Text, Html),
        row("accent-primary/code", "CodeMirror headers", "accent_primary", s(&c.accent_primary), "code_bg", s(&c.code_bg), Text, Watch),
        row("accent-secondary/page", "h2, links", "accent_secondary", s(&c.accent_secondary), "bg", s(&c.bg), Text, Html),
        row("accent-secondary/secondary", "links in callouts, topbar links, page-nav links, NOTE callout title, solution summary", "accent_secondary", s(&c.accent_secondary), "bg_secondary", s(&c.bg_secondary), Text, Html),
        row("accent-secondary/border", "page-nav link hover", "accent_secondary", s(&c.accent_secondary), "border", s(&c.border), Text, Html),
        row("accent-secondary/code", "CodeMirror links", "accent_secondary", s(&c.accent_secondary), "code_bg", s(&c.code_bg), Text, Watch),
        row("accent-secondary/table-stripe", "link in an even table row", "accent_secondary", s(&c.accent_secondary), "table stripe (text 6% over bg)", stripe.clone(), Text, Html),
        row("accent-secondary/table-head", "link in a table header", "accent_secondary", s(&c.accent_secondary), "table head (text 11% over bg)", head.clone(), Text, Html),
        row("accent-tertiary/page", "h3, link hover", "accent_tertiary", s(&c.accent_tertiary), "bg", s(&c.bg), Text, Html),
        row("accent-tertiary/secondary", "TIP callout title, link hover in a callout", "accent_tertiary", s(&c.accent_tertiary), "bg_secondary", s(&c.bg_secondary), Text, Html),
        // ── Errors and callouts ──────────────────────────────────────
        row("error/error-bg", "failed-cell panel, error chip in the topbar", "error_text", s(&c.error_text), "error_bg", s(&c.error_bg), Text, Html),
        row("error/secondary", "WARNING and CAUTION callout titles", "error_text", s(&c.error_text), "bg_secondary", s(&c.bg_secondary), Text, Html),
        row("error/code", "CodeMirror errors", "error_text", s(&c.error_text), "code_bg", s(&c.code_bg), Text, Watch),
        row("error/page", "failed-cell left rule", "error_text", s(&c.error_text), "bg", s(&c.bg), Ui, Html),
        // ── Syntax ───────────────────────────────────────────────────
        row("syn-keyword/code", "keywords", "syn_keyword", s(&c.syn_keyword), "code_bg", s(&c.code_bg), Text, Html),
        row("syn-function/code", "function names", "syn_function", s(&c.syn_function), "code_bg", s(&c.code_bg), Text, Html),
        row("syn-number/code", "numbers", "syn_number", s(&c.syn_number), "code_bg", s(&c.code_bg), Text, Html),
        row("syn-string/code", "strings", "syn_string", s(&c.syn_string), "code_bg", s(&c.code_bg), Text, Html),
        row("syn-comment/code", "comments", "syn_comment", s(&c.syn_comment), "code_bg", s(&c.code_bg), Text, Html),
        row("syn-operator/code", "operators", "syn_operator", s(&c.syn_operator), "code_bg", s(&c.code_bg), Text, Html),
        // ── Comments ─────────────────────────────────────────────────
        row("note-number/note", "margin note numbers", "cm_note_border", s(&c.cm_note_border), "cm_note_bg", s(&c.cm_note_bg), Text, Html),
        row("mark-underline/page", "underline that marks a ==highlight==", "cm_note_border", s(&c.cm_note_border), "bg", s(&c.bg), Ui, Html),
        // ── Watch chrome ─────────────────────────────────────────────
        row("page-on-accent", "active toolbar button, open drawer button, selected menu item, primary comment button", "bg", s(&c.bg), "accent_primary", s(&c.accent_primary), Text, Watch),
        row("quiet-button", "⋯ menu button (text at 85% opacity)", "text at 85% over bg_secondary", quiet, "bg_secondary", s(&c.bg_secondary), Text, Watch),
        row("stale-cell", "cell output waiting for a re-run (85% opacity)", "text at 85% over bg", stale, "bg", s(&c.bg), Text, Watch),
        row("banner", "disconnected banner", "banner_text", s(&c.banner_text), "banner_bg", s(&c.banner_bg), Text, Watch),
        row("field-border/page", "number input, ref field, and inline cell editor boundary against the page or field", "field border (text_dim 75% over bg)", field.clone(), "bg", s(&c.bg), Ui, Html),
        row("field-border/secondary", "ref field boundary against the menu", "field border (text_dim 75% over bg)", field.clone(), "bg_secondary", s(&c.bg_secondary), Ui, Watch),
        row("field-border/code", "inline cell editor boundary against its code panel", "field border (text_dim 75% over bg)", field, "code_bg", s(&c.code_bg), Ui, Watch),
        row("focus/secondary", "focus ring and switch fill in menus, slider", "accent_primary", s(&c.accent_primary), "bg_secondary", s(&c.bg_secondary), Ui, Watch),
        row("focus/page", "running-cell outline, plot flash, code cell rule", "accent_primary", s(&c.accent_primary), "bg", s(&c.bg), Ui, Html),
        row("switch-knob/page", "switch knob (off) on its track", "text", s(&c.text), "bg", s(&c.bg), Ui, Watch),
        // ── Git diff ─────────────────────────────────────────────────
        row("diff-add-bar/page", "added bar", "dx_add_bar", s(&c.dx_add_bar), "bg", s(&c.bg), Ui, Watch),
        row("diff-mod-bar/page", "modified bar", "dx_mod_bar", s(&c.dx_mod_bar), "bg", s(&c.bg), Ui, Watch),
        row("diff-del-bar/page", "removed bar", "dx_del_bar", s(&c.dx_del_bar), "bg", s(&c.bg), Ui, Watch),
        row("diff-add-underline/line", "added code word underline", "dx_add_bar", s(&c.dx_add_bar), "dx_add_line", s(&c.dx_add_line), Ui, Watch),
        row("diff-del-underline/line", "removed code word underline", "dx_del_bar", s(&c.dx_del_bar), "dx_del_line", s(&c.dx_del_line), Ui, Watch),
        row("text/diff-add-word", "added prose word", "text", s(&c.text), "dx_add_word", s(&c.dx_add_word), Text, Watch),
        row("text/diff-del-word", "removed prose word", "text", s(&c.text), "dx_del_word", s(&c.dx_del_word), Text, Watch),
    ];
    for (line_role, line) in [
        ("dx_add_line", &c.dx_add_line),
        ("dx_del_line", &c.dx_del_line),
    ] {
        for (fg_role, fg) in [
            ("text", &c.text),
            ("syn_keyword", &c.syn_keyword),
            ("syn_function", &c.syn_function),
            ("syn_number", &c.syn_number),
            ("syn_string", &c.syn_string),
            ("syn_comment", &c.syn_comment),
            ("syn_operator", &c.syn_operator),
        ] {
            v.push(row(
                if line_role == "dx_add_line" {
                    "code/diff-add-line"
                } else {
                    "code/diff-del-line"
                },
                "code on a changed line",
                fg_role,
                s(fg),
                line_role,
                s(line),
                Text,
                Watch,
            ));
        }
    }
    v
}

/// PDF pairs: this palette's ink on white paper and on its own panels.
/// LaTeX always prints Latte, so callers pass the Latte palette.
pub fn print_checks(c: &ThemeColors) -> Vec<ContrastCheck> {
    let paper = PDF_PAPER.to_string();
    let head = mix(c.text.as_ref(), PDF_PAPER, TABLE_HEAD_MIX);
    let stripe = mix(c.text.as_ref(), PDF_PAPER, TABLE_STRIPE_MIX);
    let mut v = Vec::new();
    for (role, fg) in [
        ("text", &c.text),
        ("text_dim", &c.text_dim),
        ("accent_primary", &c.accent_primary),
        ("accent_secondary", &c.accent_secondary),
        ("accent_tertiary", &c.accent_tertiary),
        ("error_text", &c.error_text),
        ("syn_keyword", &c.syn_keyword),
        ("syn_function", &c.syn_function),
        ("syn_number", &c.syn_number),
        ("syn_string", &c.syn_string),
        ("syn_comment", &c.syn_comment),
        ("syn_operator", &c.syn_operator),
    ] {
        v.push(ContrastCheck {
            id: "pdf/paper",
            what: "PDF ink on white paper",
            fg_role: role,
            bg_role: "white paper",
            fg: fg.as_ref().to_string(),
            bg: paper.clone(),
            need: Need::Text,
            surface: Surface::Pdf,
        });
    }
    for (role, fg) in [("text", &c.text), ("accent_primary", &c.accent_primary)] {
        v.push(ContrastCheck {
            id: "pdf/table-head",
            what: "PDF table header",
            fg_role: role,
            bg_role: "table head (text 11% over white)",
            fg: fg.as_ref().to_string(),
            bg: head.clone(),
            need: Need::Text,
            surface: Surface::Pdf,
        });
    }
    v.push(ContrastCheck {
        id: "pdf/table-stripe",
        what: "PDF even table rows",
        fg_role: "text",
        bg_role: "table stripe (text 6% over white)",
        fg: c.text.as_ref().to_string(),
        bg: stripe,
        need: Need::Text,
        surface: Surface::Pdf,
    });
    v
}

/// Screen checks, then print checks against `print` (Latte for builtins).
pub fn all_checks(c: &ThemeColors, print: &ThemeColors) -> Vec<ContrastCheck> {
    let mut v = screen_checks(c);
    v.extend(print_checks(print));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    fn builtins() -> [(&'static str, &'static ThemeColors); 4] {
        [
            ("mocha", Theme::Mocha.colors()),
            ("macchiato", Theme::Macchiato.colors()),
            ("frappe", Theme::Frappe.colors()),
            ("latte", Theme::Latte.colors()),
        ]
    }

    #[test]
    fn every_builtin_pair_meets_its_wcag_threshold() {
        let mut failures = Vec::new();
        for (name, c) in builtins() {
            for check in all_checks(c, Theme::Latte.colors()) {
                if !check.passes() {
                    failures.push(format!(
                        "{name} {} [{} on {}] {} on {} = {:.2} < {}",
                        check.id,
                        check.fg_role,
                        check.bg_role,
                        check.fg,
                        check.bg,
                        check.ratio().unwrap_or(0.0),
                        check.need.min()
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn dark_and_light_aliases_are_the_same_audit() {
        assert_eq!(
            screen_checks(Theme::Dark.colors()),
            screen_checks(Theme::Mocha.colors())
        );
        assert_eq!(
            screen_checks(Theme::Light.colors()),
            screen_checks(Theme::Latte.colors())
        );
    }

    #[test]
    fn checker_rejects_a_known_failing_pair() {
        let mut c = Theme::Mocha.colors().clone();
        c.text_dim = std::borrow::Cow::Borrowed("#45475a");
        let bad: Vec<_> = screen_checks(&c)
            .into_iter()
            .filter(|ch| !ch.passes())
            .map(|ch| ch.id)
            .collect();
        assert!(bad.contains(&"text-dim/page"), "{bad:?}");
        // A swatch that is not #RRGGBB cannot be checked, so it fails.
        c.text = std::borrow::Cow::Borrowed("rgba(1,2,3,0.5)");
        assert!(screen_checks(&c)
            .iter()
            .any(|ch| ch.id == "text/page" && !ch.passes()));
    }

    /// Markdown audit table: `cargo test -p rustlab-plot contrast::tests::audit_table -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn audit_table() {
        let names: Vec<_> = builtins().iter().map(|(n, _)| *n).collect();
        println!("| Pair | Where | Need | {} |", names.join(" | "));
        println!("|{}", "---|".repeat(3 + names.len()));
        let per: Vec<Vec<ContrastCheck>> =
            builtins().iter().map(|(_, c)| screen_checks(c)).collect();
        for (i, first) in per[0].iter().enumerate() {
            let cells: Vec<String> = per
                .iter()
                .map(|checks| {
                    let ch = &checks[i];
                    let r = ch.ratio().unwrap_or(0.0);
                    let mark = if ch.passes() { "" } else { " **FAIL**" };
                    format!("{r:.2}{mark}")
                })
                .collect();
            println!(
                "| `{}` on `{}` | {} ({}) | {} | {} |",
                first.fg_role,
                first.bg_role,
                first.what,
                first.surface.label(),
                first.need.label(),
                cells.join(" | ")
            );
        }
        for ch in print_checks(Theme::Latte.colors()) {
            let r = ch.ratio().unwrap_or(0.0);
            let mark = if ch.passes() { "" } else { " **FAIL**" };
            println!(
                "| `{}` on `{}` | {} (PDF, Latte) | {} | {r:.2}{mark} |",
                ch.fg_role,
                ch.bg_role,
                ch.what,
                ch.need.label()
            );
        }
    }
}
