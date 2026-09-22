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
/// the legacy `-A` form.
fn convert_one_svg(svg: &Path, pdf: &Path) -> Result<(), String> {
    // Prefer Inkscape 1.x: inkscape in.svg --export-type=pdf --export-filename=out.pdf
    let modern = Command::new("inkscape")
        .arg(svg)
        .arg("--export-type=pdf")
        .arg(format!("--export-filename={}", pdf.display()))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match modern {
        Ok(s) if s.success() && pdf.exists() => return Ok(()),
        _ => {}
    }

    // Legacy: inkscape -A out.pdf in.svg
    let legacy = Command::new("inkscape")
        .arg("-A")
        .arg(pdf)
        .arg(svg)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("failed to run inkscape: {e}"))?;

    if legacy.success() && pdf.exists() {
        Ok(())
    } else {
        Err(format!(
            "inkscape failed to convert {} → {}",
            svg.display(),
            pdf.display()
        ))
    }
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
