//! Phase 1 theme-token regression.
//!
//! Goldens are the rendered pages *before* design tokens. [`normalize`]
//! drops the additive `:root` declarations and unwraps
//! `var(--rl-new, fallback)` (balanced parentheses, nested `var()`) so a
//! token refactor that keeps every fallback equal to today's literal
//! still matches. `font-size: medium` is the UA initial; a body rule that
//! only names it is removed so the golden stays the previous stylesheet.
//!
//! Regenerate with `UPDATE_GOLDEN=1` from this crate. The files are
//! hand-written baselines (no insta). Extensions are `.golden` because
//! the repo gitignores `*.html` and `*.svg`.

use std::path::{Path, PathBuf};

use rustlab_plot::{FigureState, LineStyle, PlotKind, Series, SeriesColor, Theme};

use crate::execute::Rendered;
use crate::parse::CalloutKind;
use crate::render::{render_html, LinkMode};
use crate::render_latex::render_latex;
use crate::render_markdown::{render_markdown, LinkStyle};
use crate::server::drawer::install_nav_drawer;
use crate::server::page::{inject_chrome, PageOpts};

const BODY_STACK: &str =
    r#"-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif"#;
const MONO_STACK: &str = r#""SF Mono", "Fira Code", "JetBrains Mono", monospace"#;

/// Names added in Phase 1. The existing 33 `--rl-*` color tokens are not
/// in this list, so a golden still pins those bytes.
const NEW_TOKEN_NAMES: &[&str] = &[
    "--rl-font-body",
    "--rl-font-heading",
    "--rl-font-mono",
    "--rl-font-size",
    "--rl-line-height",
    "--rl-measure",
    "--rl-radius-sm",
    "--rl-radius-lg",
    "--rl-radius",
    "--rl-border-width",
    "--rl-space-1",
    "--rl-space-2",
    "--rl-space-3",
    "--rl-space-4",
    "--rl-space-5",
    "--rl-space-6",
    "--rl-table-head",
    "--rl-table-stripe",
    "--rl-table-rule",
    "--rl-callout-note",
    "--rl-callout-tip",
    "--rl-callout-important",
    "--rl-callout-warning",
    "--rl-callout-caution",
    "--rl-callout-bg",
    "--rl-link-visited",
    "--rl-link",
    "--rl-focus-ring",
    "--rl-plot-paper",
    "--rl-plot-text",
    "--rl-plot-font",
    "--rl-plot-1",
    "--rl-plot-2",
    "--rl-plot-3",
    "--rl-plot-4",
    "--rl-plot-5",
    "--rl-plot-6",
    "--rl-plot-7",
    "--rl-plot-8",
    "--rl-shadow",
];

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn fixture() -> Vec<Rendered> {
    let mut fig = FigureState::new();
    fig.subplots[0].series.push(Series {
        label: String::new(),
        x_data: vec![0.0, 1.0],
        y_data: vec![0.0, 1.0],
        color: SeriesColor::Blue,
        style: LineStyle::Solid,
        kind: PlotKind::Line,
    });
    let prose = "\
# Sample

A [link](https://example.com/notes) and $E=mc^2$.

| a | b |
|---|---|
| 1 | 2 |
";
    vec![
        Rendered::Markdown(prose.to_string()),
        Rendered::Callout {
            kind: CalloutKind::Note,
            title: None,
            content: "Note body.".to_string(),
        },
        Rendered::Callout {
            kind: CalloutKind::Tip,
            title: None,
            content: "Tip body.".to_string(),
        },
        Rendered::Callout {
            kind: CalloutKind::Important,
            title: None,
            content: "Important body.".to_string(),
        },
        Rendered::Callout {
            kind: CalloutKind::Warning,
            title: None,
            content: "Warning body.".to_string(),
        },
        Rendered::Callout {
            kind: CalloutKind::Caution,
            title: None,
            content: "Caution body.".to_string(),
        },
        Rendered::Code {
            source: "x = 1:2\nplot(x)".to_string(),
            text_output: "ans = 1  2".to_string(),
            error: None,
            figures: vec![fig],
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
            source_open: None,
            widget_reads: Vec::new(),
        },
        Rendered::Code {
            source: "error(\"nope\")".to_string(),
            text_output: String::new(),
            error: Some("nope".to_string()),
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
            source_open: None,
            widget_reads: Vec::new(),
        },
    ]
}

/// Page a reader of `notebook watch` gets: rendered HTML, toolbar chrome,
/// and the contents drawer. Comment CSS is snapshotted on its own because
/// it is injected only when comments are on.
fn watch_page(theme: &rustlab_plot::ThemeColors, plot_dir: &Path) -> String {
    let html = render_html(
        "Sample",
        &fixture(),
        plot_dir,
        "plots",
        theme,
        None,
        &LinkMode::single_file(),
    );
    let html = inject_chrome(
        &html,
        theme,
        PageOpts {
            editable: false,
            git_diff: false,
        },
    );
    install_nav_drawer(&html, None)
}

fn normalize(src: &str) -> String {
    // Unwrap first so a rule that only names the UA initial
    // (`font-size: var(--rl-font-size, medium)`) can be dropped after
    // it collapses. Declaration lines are removed either way.
    let unwrapped = unwrap_new_vars(src);
    let kept: Vec<&str> = unwrapped
        .lines()
        .filter(|line| !is_new_decl_line(line))
        .filter(|line| line.trim() != "font-size: medium;")
        .collect();
    let mut text = kept.join("\n");
    if src.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn is_new_decl_line(line: &str) -> bool {
    let t = line.trim_start();
    NEW_TOKEN_NAMES
        .iter()
        .any(|name| t.starts_with(name) && t.as_bytes().get(name.len()) == Some(&b':'))
}

fn unwrap_new_vars(src: &str) -> String {
    let mut names: Vec<&str> = NEW_TOKEN_NAMES.to_vec();
    names.sort_by_key(|name| std::cmp::Reverse(name.len()));
    let mut text = src.to_string();
    loop {
        let next = unwrap_once(&text, &names);
        if next == text {
            return text;
        }
        text = next;
    }
}

fn unwrap_once(src: &str, names: &[&str]) -> String {
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < src.len() {
        if let Some(name) = names
            .iter()
            .copied()
            .find(|name| src[i..].starts_with(&format!("var({name}, ")))
        {
            let open = i + 3; // '(' of var(
            let close = match matching_paren(src, open) {
                Some(c) => c,
                None => {
                    out.push_str(&src[i..]);
                    break;
                }
            };
            let fallback_start = i + "var(".len() + name.len() + ", ".len();
            out.push_str(&src[fallback_start..close]);
            i = close + 1;
            continue;
        }
        let ch = src[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Index of the `)` that matches `src[open] == '('`. Quotes are skipped.
fn matching_paren(src: &str, open: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    for (idx, &b) in bytes.iter().enumerate().skip(open) {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
                continue;
            }
            if b == b'\\' {
                escaped = true;
                continue;
            }
            if b == q {
                quote = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn normalize_markdown(src: &str) -> String {
    // `DefaultHasher` names plot files. The SVG bytes are golden'd
    // separately; the markdown text should not depend on the hasher.
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(at) = rest.find("plot-") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + "plot-".len()..];
        if let Some((n, after)) = hash_suffix(tail) {
            out.push_str("plot-");
            out.push_str(n);
            out.push_str(".svg");
            rest = after;
        } else {
            out.push_str("plot-");
            rest = tail;
        }
    }
    out.push_str(rest);
    normalize(&out)
}

/// `1-deadbeef.svg` → `("1", after)`.
fn hash_suffix(rest: &str) -> Option<(&str, &str)> {
    let (n, tail) = rest.split_once('-')?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (hex, after) = tail.split_once(".svg")?;
    if hex.len() == 8 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some((n, after))
    } else {
        None
    }
}

fn assert_golden(name: &str, actual: &str) {
    let path = golden_dir().join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "missing {} ({err}). Regenerate with UPDATE_GOLDEN=1",
            path.display()
        )
    });
    if expected != actual {
        panic!(
            "golden {} drifted\n{}",
            path.display(),
            first_diff(&expected, actual)
        );
    }
}

fn first_diff(expected: &str, actual: &str) -> String {
    for (i, (a, b)) in expected.lines().zip(actual.lines()).enumerate() {
        if a != b {
            return format!("line {}: \n  golden: {a}\n  actual: {b}", i + 1);
        }
    }
    format!(
        "line count golden {} actual {}",
        expected.lines().count(),
        actual.lines().count()
    )
}

fn themes() -> [(&'static str, Theme); 4] {
    [
        ("mocha", Theme::Mocha),
        ("macchiato", Theme::Macchiato),
        ("frappe", Theme::Frappe),
        ("latte", Theme::Latte),
    ]
}

fn read_svgs(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return files,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("svg") {
            if let Ok(bytes) = std::fs::read(&path) {
                files.push((
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    bytes,
                ));
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

#[test]
fn normalizer_strips_new_declarations_and_unwraps_fallbacks() {
    let raw = format!(
        "    font-family: {BODY_STACK};\n    --rl-font-body: {BODY_STACK};\n    \
         font-family: var(--rl-font-body, var(--rl-font-heading, {BODY_STACK}));\n    \
         font-size: var(--rl-font-size, medium);\n    \
         background: var(--rl-table-head, color-mix(in srgb, var(--rl-text, #cdd6f4) 11%, var(--rl-bg, #1e1e2e)));\n    \
         border-color: var(--rl-callout-note, var(--rl-accent-secondary, #89b4fa));\n"
    );
    let got = normalize(&raw);
    let expect = format!(
        "    font-family: {BODY_STACK};\n    \
         font-family: {BODY_STACK};\n    \
         background: color-mix(in srgb, var(--rl-text, #cdd6f4) 11%, var(--rl-bg, #1e1e2e));\n    \
         border-color: var(--rl-accent-secondary, #89b4fa);\n"
    );
    assert_eq!(got, expect);
    assert_eq!(
        normalize_markdown("![p](plots/plot-1-deadbeef.svg)\n"),
        "![p](plots/plot-1.svg)\n"
    );
}

#[test]
fn rendered_pages_match_pre_token_goldens() {
    let root = std::env::temp_dir().join(format!("rl-theme-golden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut mds = Vec::new();

    for (name, theme) in themes() {
        let colors = theme.colors();
        let plot_dir = root.join(format!("html-{name}"));
        std::fs::create_dir_all(&plot_dir).unwrap();
        let page = watch_page(colors, &plot_dir);
        let again = watch_page(colors, &plot_dir);
        assert_eq!(page, again, "{name} html render is not deterministic");
        assert!(
            !page.contains(root.to_str().unwrap()),
            "{name} html embeds the plot directory"
        );
        assert_golden(&format!("{name}.html.golden"), &normalize(&page));

        let index = crate::generate_index_html(
            "Index",
            &[("Sample".to_string(), "sample.html".to_string())],
            colors,
            "<p><a href=\"sample.html\">Sample</a></p>",
            None,
        );
        assert_golden(&format!("{name}.index.html.golden"), &normalize(&index));

        let md_dir = root.join(format!("md-{name}"));
        std::fs::create_dir_all(&md_dir).unwrap();
        let md = render_markdown(
            "Sample",
            &fixture(),
            &md_dir,
            "plots",
            colors,
            None,
            LinkStyle::Standard,
            true,
            "",
        );
        // Plot filenames are hashed, then stripped. The markdown text does
        // not carry theme colors; the SVG goldens do.
        let md_norm = normalize_markdown(&md);
        if let Some(first) = mds.first() {
            assert_eq!(&md_norm, first, "{name} markdown text drifted");
        }
        mds.push(md_norm);
        let svgs = read_svgs(&md_dir);
        assert!(
            !svgs.is_empty(),
            "{name} markdown produced no SVG; plot output would be unpinned"
        );
        for (i, (_file, bytes)) in svgs.iter().enumerate() {
            assert_golden(
                &format!("{name}.plot-{}.svg.golden", i + 1),
                &String::from_utf8_lossy(bytes),
            );
        }
    }
    assert_golden("markdown.md.golden", mds.first().unwrap());

    let mut latex_pages = Vec::new();
    for (name, theme) in themes() {
        let plot_dir = root.join(format!("tex-{name}"));
        std::fs::create_dir_all(&plot_dir).unwrap();
        let tex = render_latex(
            "Sample",
            &fixture(),
            &plot_dir,
            "plots",
            theme.colors(),
            &LinkMode::single_file(),
        );
        latex_pages.push(tex);
        if name == "latte" {
            let svgs = read_svgs(&plot_dir);
            assert!(!svgs.is_empty(), "latex produced no SVG");
            assert_golden(
                "latex.plot-1.svg.golden",
                &String::from_utf8_lossy(&svgs[0].1),
            );
        }
    }
    assert!(
        latex_pages.iter().all(|t| t == &latex_pages[0]),
        "LaTeX/PDF must ignore the requested theme and stay Latte"
    );
    assert_golden("latex.tex.golden", &latex_pages[0]);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn design_tokens_are_emitted_and_read() {
    let dir = std::env::temp_dir().join(format!("rl-theme-tokens-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mocha = Theme::Mocha.colors();
    let page = watch_page(mocha, &dir);
    let index = crate::generate_index_html("Index", &[], mocha, "", None);

    let decls = [
        format!("--rl-font-body: {BODY_STACK};"),
        format!("--rl-font-heading: {BODY_STACK};"),
        format!("--rl-font-mono: {MONO_STACK};"),
        "--rl-font-size: medium;".to_string(),
        "--rl-line-height: 1.7;".to_string(),
        "--rl-measure: 960px;".to_string(),
        "--rl-radius-sm: 3px;".to_string(),
        "--rl-radius: 6px;".to_string(),
        "--rl-radius-lg: 8px;".to_string(),
        "--rl-border-width: 1px;".to_string(),
        "--rl-space-1: 0.5rem;".to_string(),
        "--rl-space-2: 0.75rem;".to_string(),
        "--rl-space-3: 1rem;".to_string(),
        "--rl-space-4: 1.5rem;".to_string(),
        "--rl-space-5: 2rem;".to_string(),
        "--rl-space-6: 2.5rem;".to_string(),
        "--rl-table-head: color-mix(in srgb, var(--rl-text) 11%, var(--rl-bg));".to_string(),
        "--rl-table-stripe: color-mix(in srgb, var(--rl-text) 6%, var(--rl-bg));".to_string(),
        "--rl-table-rule: color-mix(in srgb, var(--rl-text) 30%, var(--rl-bg));".to_string(),
        "--rl-callout-note: var(--rl-accent-secondary);".to_string(),
        "--rl-callout-tip: var(--rl-accent-tertiary);".to_string(),
        "--rl-callout-important: var(--rl-accent-primary);".to_string(),
        "--rl-callout-warning: var(--rl-error-text);".to_string(),
        "--rl-callout-caution: var(--rl-error-text);".to_string(),
        "--rl-callout-bg: var(--rl-bg-secondary);".to_string(),
        "--rl-link: var(--rl-accent-secondary);".to_string(),
        "--rl-link-visited: var(--rl-accent-primary);".to_string(),
        "--rl-focus-ring: auto;".to_string(),
        "--rl-plot-paper: var(--rl-plot-bg);".to_string(),
        "--rl-plot-text: var(--rl-text);".to_string(),
        "--rl-plot-font: sans-serif;".to_string(),
        "--rl-plot-1: #17becf;".to_string(),
        "--rl-plot-2: #bcbd22;".to_string(),
        "--rl-plot-3: #2ca02c;".to_string(),
        "--rl-plot-4: #9467bd;".to_string(),
        "--rl-plot-5: #d62728;".to_string(),
        "--rl-plot-6: #1f77b4;".to_string(),
        "--rl-plot-7: #ff7f0e;".to_string(),
        "--rl-plot-8: #8c564b;".to_string(),
        "--rl-shadow: 0 8px 24px rgba(0,0,0,.25);".to_string(),
    ];
    for decl in &decls {
        assert!(
            page.contains(decl),
            "notebook page missing design token `{decl}`"
        );
        assert!(
            index.contains(decl),
            "index page missing design token `{decl}`"
        );
    }

    // The 33 color names stay, mocha values included.
    assert!(page.contains("--rl-bg: #1e1e2e;"));
    assert!(page.contains("--rl-plot-bg:"));
    assert!(page.contains("--rl-plot-grid:"));
    assert!(page.contains("color-scheme: dark"));
    assert!(page.contains("calc(50% - 480px)"));

    let reads = [
        "font-family: var(--rl-font-body,",
        "var(--rl-font-heading,",
        "font-family: var(--rl-font-mono,",
        "font-size: var(--rl-font-size, medium)",
        "line-height: var(--rl-line-height, 1.7)",
        "max-width: var(--rl-measure, 960px)",
        "border-radius: var(--rl-radius, 6px)",
        "border-radius: var(--rl-radius-sm, 3px)",
        "border-radius: var(--rl-radius-lg, 8px)",
        "var(--rl-border-width, 1px)",
        "var(--rl-space-1, 0.5rem)",
        "var(--rl-space-2, 0.75rem)",
        "var(--rl-space-3, 1rem)",
        "var(--rl-space-4, 1.5rem)",
        "var(--rl-space-5, 2rem)",
        "var(--rl-space-6, 2.5rem)",
        "var(--rl-table-head,",
        "var(--rl-table-stripe,",
        "var(--rl-table-rule,",
        "var(--rl-callout-note,",
        "var(--rl-callout-tip,",
        "var(--rl-callout-important,",
        "var(--rl-callout-warning,",
        "var(--rl-callout-caution,",
        "var(--rl-callout-bg,",
        "var(--rl-link,",
        "var(--rl-link-visited,",
    ];
    for needle in reads {
        assert!(page.contains(needle), "stylesheet does not read `{needle}`");
    }

    // Index keeps its narrower measure. Visited links stay accent-primary.
    assert!(index.contains("max-width: 720px"));
    assert!(!index.contains("var(--rl-measure"));
    assert!(index.contains("font-family: var(--rl-font-body,"));
    assert!(index.contains("var(--rl-link-visited,"));

    // Focus ring is declared and not applied (applying it would paint a
    // ring the UA does not draw today).
    assert!(page.contains("--rl-focus-ring: auto;"));
    assert!(!page.contains("outline: var(--rl-focus-ring"));

    // Plot series tokens are emitted for later phases. Plotly/SVG keep
    // today's colors, so the page must not restyle traces from them.
    assert!(!page.contains("var(--rl-plot-1)"));
    assert!(!page.contains("@font-face"));

    let comments = crate::comments::comment_css();
    assert!(comments.contains("border-radius: var(--rl-radius, 6px)"));
    assert!(comments.contains("border-radius: var(--rl-radius-lg, 8px)"));
    assert!(comments.contains("var(--rl-shadow, 0 8px 24px rgba(0,0,0,.25))"));

    let latte = watch_page(Theme::Latte.colors(), &dir);
    assert!(latte.contains("--rl-bg: #eff1f5;"));
    assert!(latte.contains("color-scheme: light"));
    assert!(latte.contains("--rl-font-body:"));

    let _ = std::fs::remove_dir_all(&dir);
}

/// The contrast audit (`rustlab_plot::contrast`) blends at fixed opacities
/// and mixes. The stylesheet must paint those same values, or the audit
/// checks a page nobody sees.
#[test]
fn stylesheet_paints_the_audited_values() {
    use rustlab_plot::contrast::{FIELD_BORDER_MIX, QUIET_BUTTON_OPACITY, STALE_CELL_OPACITY};
    let dir = std::env::temp_dir().join(format!("rl-theme-audit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (_, theme) in themes() {
        let c = theme.colors();
        let page = watch_page(c, &dir);
        let field = format!(
            "color-mix(in srgb, {} {}%, {})",
            c.css_var("text-dim"),
            (FIELD_BORDER_MIX * 100.0).round() as u32,
            c.css_var("bg")
        );
        assert!(page.contains(&format!(
            "#rl-toolbar button.rl-quiet {{ opacity: {QUIET_BUTTON_OPACITY};"
        )));
        assert!(
            page.contains(&format!("solid {field};\n    border-radius: 4px;")),
            "widget number input"
        );
        assert!(
            page.contains(&format!(
                "solid {field}; border-radius: var(--rl-radius, 6px);\n    padding: 4px 6px;"
            )),
            "ref field"
        );
        let cells = crate::server::cell::inject_cell_client(&page, c, true);
        assert!(cells.contains(&format!(
            "rl-cell-stale {{ opacity: {STALE_CELL_OPACITY}; }}"
        )));
        assert!(cells.contains(&format!(
            ".rl-cell-editor {{ border: var(--rl-border-width, 1px) solid {field};"
        )));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `var(--a, fallback)` → `var(--a)`, recursively, so what is left is the
/// colour text the chrome paints without a token.
fn strip_var_fallbacks(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < src.len() {
        if src[i..].starts_with("var(--") {
            let open = i + 3;
            if let Some(close) = matching_paren(src, open) {
                let inner = &src[open + 1..close];
                let name = inner.split(',').next().unwrap_or(inner).trim();
                out.push_str("var(");
                out.push_str(name);
                out.push(')');
                i = close + 1;
                continue;
            }
        }
        let ch = src[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn bare_colours(src: &str) -> Vec<String> {
    let mut found = Vec::new();
    let b = src.as_bytes();
    for (i, w) in src.char_indices() {
        if w == '#' {
            let hex: String = src[i + 1..]
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            let next = src[i + 1 + hex.len()..].chars().next();
            let ends = !next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if matches!(hex.len(), 3 | 4 | 6 | 8) && ends {
                found.push(format!("#{hex}"));
            }
        }
        if src[i..].starts_with("rgb(") || src[i..].starts_with("rgba(") {
            found.push(src[i..].chars().take(24).collect());
        }
        for kw in ["white", "black"] {
            if src[i..].starts_with(kw)
                && (i == 0 || !b[i - 1].is_ascii_alphanumeric())
                && !src[i + kw.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '-')
            {
                found.push(kw.to_string());
            }
        }
    }
    found
}

/// Every colour the watch chrome paints (toolbar, menus, source pane,
/// editor, cell bar, drawer, scrim, disconnected banner, comment popup)
/// goes through a `--rl-*` token, so a theme file reaches all of it.
#[test]
fn watch_chrome_colours_are_all_tokens() {
    use std::borrow::Cow;
    let mut c = Theme::Mocha.colors().clone();
    c.shadow = Cow::Borrowed("#102030");
    c.scrim = Cow::Borrowed("#203040");
    c.banner_bg = Cow::Borrowed("#304050");
    c.banner_text = Cow::Borrowed("#f0f0f1");
    let skeleton = "<!DOCTYPE html><html><head></head><body>\n<header class=\"topbar\"><span class=\"current\">S</span></header>\n<nav class=\"sidebar\">\n  <div class=\"nav-title\">S</div>\n<a href=\"#s\">S</a></nav>\n<main><h1 id=\"s\">S</h1></main></body></html>";
    let html = inject_chrome(
        skeleton,
        &c,
        PageOpts {
            editable: true,
            git_diff: true,
        },
    );
    let html = crate::server::cell::inject_cell_client(&html, &c, true);
    let html = install_nav_drawer(&html, None);
    let html = crate::server::ws::inject_ws_client(&html);
    let popup = {
        let css = crate::comments::comment_css();
        let at = css.find("#rl-cm-pop {").unwrap();
        css[at..at + css[at..].find('}').unwrap()].to_string()
    };
    let chrome = format!("{html}\n{popup}");
    for literal in [
        "0 10px 28px rgba(0, 0, 0, 0.35)",
        "rgba(17, 17, 27, 0.28)",
        "8px 0 28px rgba(0, 0, 0, 0.45)",
        "0 8px 24px rgba(0,0,0,.28)",
        "-8px 0 24px rgba(0,0,0,.25)",
        "background:#a23",
        "color:white",
    ] {
        assert!(
            !chrome.contains(literal),
            "watch chrome still has `{literal}`"
        );
    }
    let uses: String = chrome
        .lines()
        .filter(|l| !l.trim_start().starts_with("--rl-"))
        .collect::<Vec<_>>()
        .join("\n");
    let bare = bare_colours(&strip_var_fallbacks(&uses));
    assert!(bare.is_empty(), "colours not on a token: {bare:?}");
    for decl in [
        "--rl-shadow-color: #102030;",
        "--rl-scrim: #203040;",
        "--rl-banner-bg: #304050;",
        "--rl-banner-text: #f0f0f1;",
    ] {
        assert!(chrome.contains(decl), "missing `{decl}`");
    }
    assert!(chrome.contains("background:var(--rl-banner-bg,"));
    assert!(chrome.contains("var(--rl-scrim, #11111b) 28%"));

    // A static render does not gain the watch-only tokens.
    let dir = std::env::temp_dir().join(format!("rl-chrome-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let page = render_html(
        "Sample",
        &fixture(),
        &dir,
        "plots",
        &c,
        None,
        &LinkMode::single_file(),
    );
    assert!(!page.contains("--rl-shadow-color:"));
    assert!(!page.contains("--rl-banner-bg:"));
    let _ = std::fs::remove_dir_all(&dir);
}
