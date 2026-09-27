//! PDF compilation without TeX shell-escape.
//!
//! Plot SVGs are converted to PDF with a fixed-argv Inkscape invocation
//! (no shell), then included via `\includegraphics`. pdflatex/tectonic
//! never receive `-shell-escape` / `-Z shell-escape`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Build the argv for the PDF engine. Public for unit tests that assert
/// shell-escape is never present.
pub fn pdf_engine_args(engine: &str) -> Option<Vec<&'static str>> {
    match engine {
        "pdflatex" => Some(vec!["-interaction=nonstopmode", "-halt-on-error"]),
        "tectonic" => Some(vec![]),
        _ => None,
    }
}

/// True if any arg would enable TeX shell escape (defence-in-depth for tests).
pub fn args_enable_shell_escape(args: &[&str]) -> bool {
    args.iter().any(|a| {
        *a == "-shell-escape"
            || *a == "--shell-escape"
            || a.contains("shell-escape")
            || (*a == "shell-escape") // tectonic `-Z shell-escape`
    })
}

/// Convert every `*.svg` under `dir` (recursive) to a sibling `.pdf` via
/// Inkscape. Uses `Command` with a fixed argv — never `shell: true`.
///
/// Returns the number of SVGs converted. Errors if Inkscape is missing
/// when at least one SVG is present, or if a conversion fails.
pub fn convert_svgs_to_pdf(dir: &Path) -> Result<usize, String> {
    let svgs = collect_svgs(dir);
    if svgs.is_empty() {
        return Ok(0);
    }
    if !which_exists("inkscape") {
        return Err(
            "inkscape not found in PATH (required to convert plot SVGs for PDF without TeX shell-escape)\n\
             Install Inkscape: https://inkscape.org/"
                .to_string(),
        );
    }
    for svg in &svgs {
        let pdf = svg.with_extension("pdf");
        convert_one_svg(svg, &pdf)?;
    }
    Ok(svgs.len())
}

fn collect_svgs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(collect_svgs(&p));
        } else if p.extension().and_then(|e| e.to_str()) == Some("svg") {
            out.push(p);
        }
    }
    out
}

/// Fixed-argv Inkscape conversion. Tries the Inkscape 1.x CLI first, then
/// the legacy `-A` form. On failure the error carries the tail of
/// Inkscape's stderr so the user can see *why* (bad SVG, missing plugin,
/// sandboxing) instead of a bare "failed".
fn convert_one_svg(svg: &Path, pdf: &Path) -> Result<(), String> {
    // Prefer Inkscape 1.x: inkscape in.svg --export-type=pdf --export-filename=out.pdf
    let modern = Command::new("inkscape")
        .arg(svg)
        .arg("--export-type=pdf")
        .arg(format!("--export-filename={}", pdf.display()))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    let modern_err = match modern {
        Ok(out) if out.status.success() && pdf.exists() => return Ok(()),
        Ok(out) => stderr_tail(&out.stderr),
        Err(e) => format!("failed to run inkscape: {e}"),
    };

    // Legacy: inkscape -A out.pdf in.svg
    let legacy = Command::new("inkscape")
        .arg("-A")
        .arg(pdf)
        .arg(svg)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("failed to run inkscape: {e}"))?;

    if legacy.status.success() && pdf.exists() {
        Ok(())
    } else {
        let legacy_err = stderr_tail(&legacy.stderr);
        Err(format!(
            "inkscape failed to convert {} → {}\n  inkscape 1.x CLI: {}\n  legacy -A CLI: {}",
            svg.display(),
            pdf.display(),
            if modern_err.is_empty() { "(no output)" } else { &modern_err },
            if legacy_err.is_empty() { "(no output)" } else { &legacy_err },
        ))
    }
}

/// Last few non-empty stderr lines, single-spaced, for error messages.
fn stderr_tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(4);
    lines[start..].join(" | ")
}

pub(crate) fn which_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Select pdflatex or tectonic and return `(program, args)`.
pub fn select_pdf_engine() -> Result<(&'static str, Vec<&'static str>), String> {
    if which_exists("pdflatex") {
        Ok(("pdflatex", pdf_engine_args("pdflatex").unwrap()))
    } else if which_exists("tectonic") {
        Ok(("tectonic", pdf_engine_args("tectonic").unwrap()))
    } else {
        Err(
            "neither pdflatex nor tectonic found in PATH\n\
             Install TeX Live: https://tug.org/texlive/\n\
             Or tectonic:      https://tectonic-typesetting.github.io/"
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdflatex_args_never_enable_shell_escape() {
        let args = pdf_engine_args("pdflatex").unwrap();
        assert!(!args_enable_shell_escape(&args), "{args:?}");
        assert!(!args.iter().any(|a| a.contains("shell")));
    }

    #[test]
    fn tectonic_args_never_enable_shell_escape() {
        let args = pdf_engine_args("tectonic").unwrap();
        assert!(!args_enable_shell_escape(&args), "{args:?}");
    }

    #[test]
    fn shell_escape_detector_catches_common_forms() {
        assert!(args_enable_shell_escape(&["-shell-escape"]));
        assert!(args_enable_shell_escape(&["-Z", "shell-escape"]));
        assert!(args_enable_shell_escape(&["--shell-escape"]));
        assert!(!args_enable_shell_escape(&[
            "-interaction=nonstopmode",
            "-halt-on-error"
        ]));
    }

    #[test]
    fn convert_svgs_noop_on_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(convert_svgs_to_pdf(dir.path()).unwrap(), 0);
    }

    #[test]
    fn select_engine_args_never_enable_shell_escape() {
        // Even when an engine is present, the argv we build must be clean.
        // (If neither engine is installed this still exercises pdf_engine_args.)
        for eng in ["pdflatex", "tectonic"] {
            if let Some(args) = pdf_engine_args(eng) {
                assert!(
                    !args_enable_shell_escape(&args),
                    "{eng} args enable shell-escape: {args:?}"
                );
            }
        }
        if let Ok((_, args)) = select_pdf_engine() {
            assert!(!args_enable_shell_escape(&args), "{args:?}");
        }
    }

    #[test]
    fn convert_svgs_errors_clearly_when_inkscape_missing() {
        if which_exists("inkscape") {
            return; // soft-skip: cannot assert the missing-binary path
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("plot.svg"), b"<svg xmlns='http://www.w3.org/2000/svg'/>")
            .unwrap();
        let err = convert_svgs_to_pdf(dir.path()).unwrap_err();
        assert!(
            err.to_lowercase().contains("inkscape"),
            "expected inkscape hint, got: {err}"
        );
        assert!(!err.contains("shell-escape") || err.contains("without TeX shell-escape"));
    }
}

// ── Unicode fallbacks ─────────────────────────────────────────────────
//
// pdflatex with `[utf8]{inputenc}` rejects any character the preamble
// does not declare ("Unicode character ⁿ (U+207F) not set up for use
// with LaTeX"). `render_latex` ships a static `\newunicodechar` table
// for the common cases; the helpers below cover everything else at
// compile time: parse the rejected code points out of the build log,
// declare each with a LaTeX fallback (or a visible placeholder), and
// let `compile_pdf` retry. A character in prose never fails a build.

/// Code points pdflatex rejected, parsed from its build log.
///
/// Matches both `! LaTeX Error: Unicode character ⁿ (U+207F)` (current
/// LaTeX) and the older `! Package inputenc Error: …` wording; only the
/// `(U+XXXX)` suffix is relied on. Deduplicated, first-seen order.
pub fn rejected_unicode(log: &str) -> Vec<char> {
    let mut out = Vec::new();
    for line in log.lines() {
        let Some(idx) = line.find("Unicode character ") else {
            continue;
        };
        let rest = &line[idx..];
        let Some(open) = rest.rfind("(U+") else {
            continue;
        };
        let hex: String = rest[open + 3..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if hex.is_empty() {
            continue;
        }
        if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
            if !out.contains(&ch) {
                out.push(ch);
            }
        }
    }
    out
}

/// LaTeX for a character the static preamble table does not declare.
/// `None` means "no sensible mapping" and the caller prints a placeholder.
pub fn unicode_fallback(ch: char) -> Option<&'static str> {
    Some(match ch {
        // Superscripts / subscripts (U+2070–U+209C) beyond the digits
        // the preamble already declares.
        'ⁱ' => "\\textsuperscript{i}",
        'ⁿ' => "\\textsuperscript{n}",
        '⁺' => "\\textsuperscript{+}",
        '⁻' => "\\textsuperscript{-}",
        '⁼' => "\\textsuperscript{=}",
        '⁽' => "\\textsuperscript{(}",
        '⁾' => "\\textsuperscript{)}",
        '₊' => "\\textsubscript{+}",
        '₋' => "\\textsubscript{-}",
        '₌' => "\\textsubscript{=}",
        '₍' => "\\textsubscript{(}",
        '₎' => "\\textsubscript{)}",
        'ₐ' => "\\textsubscript{a}",
        'ₑ' => "\\textsubscript{e}",
        'ₒ' => "\\textsubscript{o}",
        'ₓ' => "\\textsubscript{x}",
        'ₕ' => "\\textsubscript{h}",
        'ₖ' => "\\textsubscript{k}",
        'ₗ' => "\\textsubscript{l}",
        'ₘ' => "\\textsubscript{m}",
        'ₙ' => "\\textsubscript{n}",
        'ₚ' => "\\textsubscript{p}",
        'ₛ' => "\\textsubscript{s}",
        'ₜ' => "\\textsubscript{t}",
        // Modifier letters (U+02B0 block and U+1D2C–U+1D6A): `Aᵀ`, `xⁱʲ`.
        'ʰ' => "\\textsuperscript{h}",
        'ʲ' => "\\textsuperscript{j}",
        'ʳ' => "\\textsuperscript{r}",
        'ʷ' => "\\textsuperscript{w}",
        'ʸ' => "\\textsuperscript{y}",
        'ˡ' => "\\textsuperscript{l}",
        'ˢ' => "\\textsuperscript{s}",
        'ˣ' => "\\textsuperscript{x}",
        'ᴬ' => "\\textsuperscript{A}",
        'ᴮ' => "\\textsuperscript{B}",
        'ᴰ' => "\\textsuperscript{D}",
        'ᴱ' => "\\textsuperscript{E}",
        'ᴳ' => "\\textsuperscript{G}",
        'ᴴ' => "\\textsuperscript{H}",
        'ᴵ' => "\\textsuperscript{I}",
        'ᴶ' => "\\textsuperscript{J}",
        'ᴷ' => "\\textsuperscript{K}",
        'ᴸ' => "\\textsuperscript{L}",
        'ᴹ' => "\\textsuperscript{M}",
        'ᴺ' => "\\textsuperscript{N}",
        'ᴼ' => "\\textsuperscript{O}",
        'ᴾ' => "\\textsuperscript{P}",
        'ᴿ' => "\\textsuperscript{R}",
        'ᵀ' => "\\textsuperscript{T}",
        'ᵁ' => "\\textsuperscript{U}",
        'ᵂ' => "\\textsuperscript{W}",
        'ᵃ' => "\\textsuperscript{a}",
        'ᵇ' => "\\textsuperscript{b}",
        'ᵈ' => "\\textsuperscript{d}",
        'ᵉ' => "\\textsuperscript{e}",
        'ᵍ' => "\\textsuperscript{g}",
        'ᵏ' => "\\textsuperscript{k}",
        'ᵐ' => "\\textsuperscript{m}",
        'ᵒ' => "\\textsuperscript{o}",
        'ᵖ' => "\\textsuperscript{p}",
        'ᵗ' => "\\textsuperscript{t}",
        'ᵘ' => "\\textsuperscript{u}",
        'ᵛ' => "\\textsuperscript{v}",
        // Greek letters the static table leaves out. Capitals that look
        // like Latin letters have no LaTeX macro and map to the letter.
        'ζ' => "\\ensuremath{\\zeta}",
        'ι' => "\\ensuremath{\\iota}",
        'κ' => "\\ensuremath{\\kappa}",
        'ν' => "\\ensuremath{\\nu}",
        'ξ' => "\\ensuremath{\\xi}",
        'ο' => "o",
        'ρ' => "\\ensuremath{\\rho}",
        'ς' => "\\ensuremath{\\varsigma}",
        'τ' => "\\ensuremath{\\tau}",
        'υ' => "\\ensuremath{\\upsilon}",
        'χ' => "\\ensuremath{\\chi}",
        'ϵ' => "\\ensuremath{\\epsilon}",
        'ϑ' => "\\ensuremath{\\vartheta}",
        'ϕ' => "\\ensuremath{\\phi}",
        'ϱ' => "\\ensuremath{\\varrho}",
        'ϖ' => "\\ensuremath{\\varpi}",
        'Ξ' => "\\ensuremath{\\Xi}",
        'Υ' => "\\ensuremath{\\Upsilon}",
        'Α' => "A",
        'Β' => "B",
        'Ε' => "E",
        'Ζ' => "Z",
        'Η' => "H",
        'Ι' => "I",
        'Κ' => "K",
        'Μ' => "M",
        'Ν' => "N",
        'Ο' => "O",
        'Ρ' => "P",
        'Τ' => "T",
        'Χ' => "X",
        // Physics / math symbols (amsmath + amssymb are loaded).
        'ħ' | 'ℏ' => "\\ensuremath{\\hbar}",
        'ℓ' => "\\ensuremath{\\ell}",
        'ℝ' => "\\ensuremath{\\mathbb{R}}",
        'ℂ' => "\\ensuremath{\\mathbb{C}}",
        'ℕ' => "\\ensuremath{\\mathbb{N}}",
        'ℤ' => "\\ensuremath{\\mathbb{Z}}",
        'ℚ' => "\\ensuremath{\\mathbb{Q}}",
        '∈' => "\\ensuremath{\\in}",
        '∉' => "\\ensuremath{\\notin}",
        '∋' => "\\ensuremath{\\ni}",
        '⊂' => "\\ensuremath{\\subset}",
        '⊃' => "\\ensuremath{\\supset}",
        '⊆' => "\\ensuremath{\\subseteq}",
        '⊇' => "\\ensuremath{\\supseteq}",
        '∪' => "\\ensuremath{\\cup}",
        '∅' => "\\ensuremath{\\emptyset}",
        '∝' => "\\ensuremath{\\propto}",
        '∼' => "\\ensuremath{\\sim}",
        '≃' => "\\ensuremath{\\simeq}",
        '≅' => "\\ensuremath{\\cong}",
        '≪' => "\\ensuremath{\\ll}",
        '≫' => "\\ensuremath{\\gg}",
        '∣' => "\\ensuremath{\\mid}",
        '∥' => "\\ensuremath{\\parallel}",
        '⊥' => "\\ensuremath{\\perp}",
        '∘' => "\\ensuremath{\\circ}",
        '⋅' => "\\ensuremath{\\cdot}",
        '∗' => "\\ensuremath{\\ast}",
        '⊗' => "\\ensuremath{\\otimes}",
        '⊕' => "\\ensuremath{\\oplus}",
        '⟨' | '〈' => "\\ensuremath{\\langle}",
        '⟩' | '〉' => "\\ensuremath{\\rangle}",
        '‖' => "\\ensuremath{\\|}",
        '′' => "\\ensuremath{'}",
        '″' => "\\ensuremath{''}",
        '‴' => "\\ensuremath{'''}",
        '⋯' => "\\ensuremath{\\cdots}",
        '⋮' => "\\ensuremath{\\vdots}",
        '⋱' => "\\ensuremath{\\ddots}",
        '∀' => "\\ensuremath{\\forall}",
        '∃' => "\\ensuremath{\\exists}",
        '¬' => "\\ensuremath{\\neg}",
        '∧' => "\\ensuremath{\\wedge}",
        '∨' => "\\ensuremath{\\vee}",
        '⊤' => "\\ensuremath{\\top}",
        '∆' => "\\ensuremath{\\Delta}",
        '∮' => "\\ensuremath{\\oint}",
        '∬' => "\\ensuremath{\\iint}",
        '⁄' => "/",
        // Arrows beyond the static table.
        '↑' => "\\ensuremath{\\uparrow}",
        '↓' => "\\ensuremath{\\downarrow}",
        '⇐' => "\\ensuremath{\\Leftarrow}",
        '⇑' => "\\ensuremath{\\Uparrow}",
        '⇓' => "\\ensuremath{\\Downarrow}",
        '↦' => "\\ensuremath{\\mapsto}",
        '⟶' => "\\ensuremath{\\longrightarrow}",
        '⟵' => "\\ensuremath{\\longleftarrow}",
        '⟹' => "\\ensuremath{\\Longrightarrow}",
        '⟺' => "\\ensuremath{\\Longleftrightarrow}",
        '↩' => "\\ensuremath{\\hookleftarrow}",
        '↪' => "\\ensuremath{\\hookrightarrow}",
        '⇌' => "\\ensuremath{\\rightleftharpoons}",
        // Text symbols.
        '•' | '‣' => "\\textbullet{}",
        '✔' => "\\checkmark{}",
        '✘' => "\\ensuremath{\\times}",
        '★' | '☆' => "\\ensuremath{\\star}",
        '†' => "\\dag{}",
        '‡' => "\\ddag{}",
        '‰' => "\\textperthousand{}",
        '™' => "\\texttrademark{}",
        '‐' | '‑' | '‒' => "-",
        '\u{00A0}' | '\u{2009}' | '\u{200A}' | '\u{202F}' | '\u{3000}' => " ",
        // Invisible code points: zero-width joiners, BOM, emoji variation
        // selectors, combining marks. Drop them.
        '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}' | '\u{FE0E}'
        | '\u{FE0F}' => "",
        c if ('\u{0300}'..='\u{036F}').contains(&c) => "",
        _ => return None,
    })
}

/// `\newunicodechar` lines for every char in `chars`: the
/// [`unicode_fallback`] when one exists, else a visible `[U+XXXX]`
/// placeholder. Returns the declarations and the placeholder chars.
pub fn fallback_declarations(chars: &[char]) -> (String, Vec<char>) {
    let mut decls = String::new();
    let mut unmapped = Vec::new();
    for &ch in chars {
        let body = match unicode_fallback(ch) {
            Some(b) => b.to_string(),
            None => {
                unmapped.push(ch);
                format!("\\texttt{{[U+{:04X}]}}", ch as u32)
            }
        };
        decls.push_str(&format!("\\newunicodechar{{{ch}}}{{{body}}}\n"));
    }
    (decls, unmapped)
}

/// Insert `decls` into `tex` right after `\usepackage{newunicodechar}`,
/// or before `\begin{document}` (loading the package) when the preamble
/// line is absent.
pub fn insert_declarations(tex: &str, decls: &str) -> String {
    const PKG: &str = "\\usepackage{newunicodechar}\n";
    if let Some(i) = tex.find(PKG) {
        let at = i + PKG.len();
        format!("{}{}{}", &tex[..at], decls, &tex[at..])
    } else if let Some(i) = tex.find("\\begin{document}") {
        format!("{}{PKG}{}{}", &tex[..i], decls, &tex[i..])
    } else {
        format!("{PKG}{decls}{tex}")
    }
}

/// Engine args for the diagnostic pass: pdflatex's `-halt-on-error`
/// stops at the first rejected character, so the pass that collects
/// them all runs without it (`nonstopmode` still never prompts).
pub fn args_without_halt<'a>(args: &[&'a str]) -> Vec<&'a str> {
    args.iter()
        .copied()
        .filter(|a| *a != "-halt-on-error")
        .collect()
}

#[cfg(test)]
mod unicode_tests {
    use super::*;

    #[test]
    fn rejected_unicode_parses_both_wordings_and_dedups() {
        let log = "\
! LaTeX Error: Unicode character ⁿ (U+207F)
               not set up for use with LaTeX.
! Package inputenc Error: Unicode character ᵀ (U+1D40)
(inputenc)                not set up for use with LaTeX.
! LaTeX Error: Unicode character 🔬 (U+1F52C)
! LaTeX Error: Unicode character ⁿ (U+207F)
Package newunicodechar Warning: Redefining Unicode character on input line 20.
";
        assert_eq!(rejected_unicode(log), vec!['ⁿ', 'ᵀ', '🔬']);
        assert!(rejected_unicode("no errors here").is_empty());
    }

    #[test]
    fn fallbacks_map_common_prose_chars() {
        assert_eq!(unicode_fallback('ⁿ'), Some("\\textsuperscript{n}"));
        assert_eq!(unicode_fallback('ᵀ'), Some("\\textsuperscript{T}"));
        assert_eq!(unicode_fallback('ₖ'), Some("\\textsubscript{k}"));
        assert_eq!(unicode_fallback('ħ'), Some("\\ensuremath{\\hbar}"));
        assert_eq!(unicode_fallback('ζ'), Some("\\ensuremath{\\zeta}"));
        assert_eq!(unicode_fallback('\u{FE0F}'), Some(""));
        assert_eq!(unicode_fallback('🔬'), None);
    }

    #[test]
    fn declarations_use_fallback_or_placeholder() {
        let (decls, unmapped) = fallback_declarations(&['ⁿ', '🔬']);
        assert!(decls.contains("\\newunicodechar{ⁿ}{\\textsuperscript{n}}\n"));
        assert!(decls.contains("\\newunicodechar{🔬}{\\texttt{[U+1F52C]}}\n"));
        assert_eq!(unmapped, vec!['🔬']);
    }

    #[test]
    fn declarations_land_after_package_or_before_document() {
        let tex = "\\documentclass{article}\n\\usepackage{newunicodechar}\n\\begin{document}x\\end{document}\n";
        let out = insert_declarations(tex, "DECL\n");
        assert_eq!(
            out,
            "\\documentclass{article}\n\\usepackage{newunicodechar}\nDECL\n\\begin{document}x\\end{document}\n"
        );
        let bare = "\\documentclass{article}\n\\begin{document}x\\end{document}\n";
        let out = insert_declarations(bare, "DECL\n");
        assert!(out.contains("\\usepackage{newunicodechar}\nDECL\n\\begin{document}"));
    }

    #[test]
    fn diagnostic_args_drop_halt_on_error_only() {
        assert_eq!(
            args_without_halt(&["-interaction=nonstopmode", "-halt-on-error"]),
            vec!["-interaction=nonstopmode"]
        );
        assert!(args_without_halt(&[]).is_empty());
    }
}
