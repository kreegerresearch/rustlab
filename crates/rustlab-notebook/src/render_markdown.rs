//! GitHub-friendly Markdown emitter.
//!
//! Walks executed notebook blocks and produces a single `.md` document with
//! each captured plot written as an SVG file on disk and referenced inline
//! with `![plot N](relative/path.svg)`. GitHub's web UI renders the result
//! directly: prose as markdown, code as fenced blocks, plots as inline SVG.
//!
//! The emitter never touches Plotly or HTML — for the interactive form, use
//! the HTML output format instead.

use crate::execute::Rendered;
use crate::parse::CalloutKind;
use crate::render::transform_wikilinks;
use rustlab_plot::theme::ThemeColors;
use std::path::Path;

/// Cross-notebook link emission style for the markdown renderer.
///
/// `Standard` is the default: source `[[Foo]]` becomes `[Foo](Foo.md)` so
/// GitHub renders the link correctly (it treats `[[...]]` as literal
/// text).
///
/// `Wiki` is the Obsidian-vault variant: source wikilinks pass through
/// untouched **and** any standard `[Text](Foo.md)` link is converted to
/// `[[Foo|Text]]` so Obsidian's graph view, backlinks panel, and
/// tab-completion treat the rendered notebooks as first-class vault
/// notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkStyle {
    Standard,
    Wiki,
}

/// Render executed notebook blocks into a Markdown document string.
///
/// Plot images are written to `plot_dir` as SVG files and referenced from
/// the rendered `.md` using `plot_href_prefix`, which is the relative path
/// from the markdown file's directory to the plot directory. Splitting the
/// two lets callers nest plots under a single `plots/<stem>/` umbrella
/// (or any other layout) without coupling the on-disk path to the
/// markdown reference.
///
/// The `title` argument is not emitted as a heading: Markdown documents
/// conventionally express their title as the first `#` line, which the
/// source notebook already provides. The `title` is kept in the signature
/// for API parity with the other emitters and is available for future
/// needs (e.g. writing into a frontmatter block).
///
/// When `iframe_href` is `Some`, an HTML `<iframe>` pointing at that path
/// is appended at the end of the document — used by the Obsidian-vault
/// variant so an Obsidian Reading view shows the interactive Plotly
/// version inline. GitHub strips iframes, leaving the static SVG plots.
///
/// `link_style` selects how cross-notebook links emit (see [`LinkStyle`]).
pub fn render_markdown(
    _title: &str,
    blocks: &[Rendered],
    plot_dir: &Path,
    plot_href_prefix: &str,
    theme: &ThemeColors,
    iframe_href: Option<&str>,
    link_style: LinkStyle,
    emit_header: bool,
) -> String {
    let mut body = String::new();
    let mut plot_idx = 0usize;

    // Ensure plot directory exists.
    let _ = std::fs::create_dir_all(plot_dir);
    let href_prefix = plot_href_prefix.trim_end_matches('/').to_string();

    if emit_header {
        body.push_str(crate::GENERATED_HEADER);
        body.push_str("\n\n");
    }

    for block in blocks {
        match block {
            Rendered::Markdown(md) => {
                let transformed = match link_style {
                    LinkStyle::Standard => {
                        // Source `[[Foo]]` / `![[img]]` → standard markdown
                        // so the committed `.md` renders correctly on
                        // GitHub (which treats `[[…]]` as literal text).
                        transform_wikilinks(md)
                    }
                    LinkStyle::Wiki => {
                        // Vault mode: leave source wikilinks alone, and
                        // additionally lift any standard `[Text](Foo.md)`
                        // link into `[[Foo|Text]]` so Obsidian's graph and
                        // backlinks panel see them.
                        rewrite_md_links_to_wikilinks(md)
                    }
                };
                body.push_str(transformed.trim_end());
                if !body.ends_with("\n\n") {
                    body.push_str("\n\n");
                }
            }
            Rendered::Code {
                source,
                text_output,
                error,
                figures,
                animations,
                hidden,
                details,
                grid_cols: _,
            } => {
                if !hidden {
                    body.push_str("```rustlab\n");
                    body.push_str(source.trim_end());
                    body.push_str("\n```\n\n");
                }

                // Collect everything the *runtime* contributed (text,
                // errors, plots, animations, plus the <details> wrapping)
                // into a single sentinel-delimited region. The pre-parse
                // strip removes this region on re-render so we don't
                // accumulate copies of the output each pass.
                let mut output_chunk = String::new();

                if let Some(heading) = details {
                    output_chunk
                        .push_str(&format!("<details>\n<summary>{}</summary>\n\n", heading));
                }

                let trimmed_out = text_output.trim();
                if !trimmed_out.is_empty() {
                    output_chunk.push_str("```text\n");
                    output_chunk.push_str(trimmed_out);
                    output_chunk.push_str("\n```\n\n");
                }

                if let Some(err) = error {
                    output_chunk.push_str("```text\n");
                    output_chunk.push_str("error: ");
                    output_chunk.push_str(err.trim_end());
                    output_chunk.push_str("\n```\n\n");
                }

                for fig in figures {
                    plot_idx += 1;
                    // Write to a stable temp name first, hash the bytes,
                    // then rename to plot-<idx>-<hash>.svg. The hash goes
                    // INTO the filename (not a `?v=` query string) so
                    // local-file renderers like Obsidian resolve the path
                    // correctly — they treat URLs literally as filesystem
                    // paths and don't strip query strings the way a
                    // browser would when fetching from HTTP. Same-content
                    // renders produce the same hash and the same filename,
                    // so the `.md` stays byte-stable for the watcher's
                    // no-op skip.
                    let tmp_file = plot_dir.join(format!("plot-{plot_idx}.svg"));
                    if let Err(e) = rustlab_plot::render_figure_state_to_file_themed(
                        fig,
                        &tmp_file.to_string_lossy(),
                        theme,
                    ) {
                        eprintln!("warning: could not render plot-{plot_idx}: {e}");
                        continue;
                    }
                    let stem = hashed_plot_stem(&format!("plot-{plot_idx}"), &tmp_file);
                    let final_file = plot_dir.join(format!("{stem}.svg"));
                    if final_file != tmp_file {
                        if let Err(e) = std::fs::rename(&tmp_file, &final_file) {
                            eprintln!(
                                "warning: could not rename plot-{plot_idx} to hashed name: {e}"
                            );
                            // Fall through with the un-hashed name so we
                            // at least produce a working URL.
                        }
                    }
                    output_chunk.push_str(&format!(
                        "![plot {plot_idx}]({prefix}/{stem}.svg)\n\n",
                        prefix = href_prefix,
                        stem = if final_file.exists() {
                            stem.clone()
                        } else {
                            format!("plot-{plot_idx}")
                        },
                    ));
                }

                // Animations.
                // .html-format animations are too bulky to commit (multi-MB
                // Plotly bundles, and GitHub strips iframes anyway) so we
                // emit a placeholder note pointing at the HTML notebook.
                // .gif-format animations are written to plot_dir as
                // sidecars and embedded inline — GitHub markdown renders
                // animated GIFs in `![..](..)` references natively.
                for anim in animations {
                    plot_idx += 1;
                    match anim.format {
                        rustlab_plot::NotebookAnimationFormat::Html => {
                            output_chunk.push_str(&format!(
                                "> ▶ **Animation: {} frames at {:.0} fps** — open the HTML version of this notebook to view.\n\n",
                                anim.frames.len(),
                                anim.fps,
                            ));
                        }
                        rustlab_plot::NotebookAnimationFormat::Gif => {
                            // Same hash-in-filename treatment as plot SVGs:
                            // Obsidian doesn't understand `?v=` query
                            // strings on local files, so the cache-bust has
                            // to live in the filename itself.
                            let tmp_gif = plot_dir.join(format!("anim-{plot_idx}.gif"));
                            if let Err(e) = rustlab_plot::write_animation_gif(
                                &tmp_gif.to_string_lossy(),
                                &anim.frames,
                                anim.fps,
                            ) {
                                eprintln!(
                                    "warning: could not write anim-{plot_idx}.gif: {e}"
                                );
                                continue;
                            }
                            let stem = hashed_plot_stem(&format!("anim-{plot_idx}"), &tmp_gif);
                            let final_gif = plot_dir.join(format!("{stem}.gif"));
                            if final_gif != tmp_gif {
                                if let Err(e) = std::fs::rename(&tmp_gif, &final_gif) {
                                    eprintln!(
                                        "warning: could not rename anim-{plot_idx} to hashed name: {e}"
                                    );
                                }
                            }
                            output_chunk.push_str(&format!(
                                "![animation {plot_idx}]({prefix}/{stem}.gif)\n\n",
                                prefix = href_prefix,
                                stem = if final_gif.exists() {
                                    stem.clone()
                                } else {
                                    format!("anim-{plot_idx}")
                                },
                            ));
                        }
                    }
                }

                if details.is_some() {
                    output_chunk.push_str("</details>\n\n");
                }

                if !output_chunk.is_empty() {
                    body.push_str(crate::OUTPUT_BLOCK_START);
                    body.push('\n');
                    body.push_str(&output_chunk);
                    body.push_str(crate::OUTPUT_BLOCK_END);
                    body.push_str("\n\n");
                }
            }
            Rendered::Mermaid {
                source,
                hidden,
                details,
                caption,
            } => {
                if *hidden {
                    continue;
                }
                if let Some(heading) = details {
                    body.push_str(&format!("<details>\n<summary>{}</summary>\n\n", heading));
                }
                body.push_str("```mermaid\n");
                body.push_str(source.trim_end());
                body.push_str("\n```\n\n");
                if let Some(cap) = caption {
                    body.push_str(&format!("*{}*\n\n", cap.trim()));
                }
                if details.is_some() {
                    body.push_str("</details>\n\n");
                }
            }
            Rendered::Callout {
                kind,
                title,
                content,
            } => {
                // Emit the GitHub / Obsidian-native blockquote syntax so the
                // committed `book/*.md` renders as a styled callout on both
                // surfaces without any per-target translation.
                let tag = match kind {
                    CalloutKind::Note => "NOTE",
                    CalloutKind::Tip => "TIP",
                    CalloutKind::Important => "IMPORTANT",
                    CalloutKind::Warning => "WARNING",
                    CalloutKind::Caution => "CAUTION",
                };
                if let Some(t) = title {
                    body.push_str(&format!("> [!{tag}] {t}\n"));
                } else {
                    body.push_str(&format!("> [!{tag}]\n"));
                }
                let indented = content.trim().replace('\n', "\n> ");
                body.push_str("> ");
                body.push_str(&indented);
                body.push_str("\n\n");
            }
            Rendered::ExerciseStart { number } => {
                body.push_str(&format!("**Exercise {number}.** "));
            }
            Rendered::SolutionStart => {
                body.push_str("<details>\n<summary>Solution</summary>\n\n");
            }
        }
    }

    if let Some(href) = iframe_href {
        // Wrap in sentinels so the pre-parse strip removes it on the
        // next render — otherwise an in-place watch accumulates one
        // copy of the iframe per pass.
        body.push_str(crate::OUTPUT_BLOCK_START);
        body.push('\n');
        body.push_str(&format!(
            "<iframe src=\"{href}\" width=\"100%\" height=\"600\" style=\"border: 0;\"></iframe>\n\n"
        ));
        body.push_str(crate::OUTPUT_BLOCK_END);
        body.push_str("\n\n");
    }

    body
}

/// Convert standard `[Text](Foo.md[#anchor])` links into Obsidian
/// `[[Foo[#anchor]|Text]]` wikilinks for vault-native rendering.
///
/// Skipped:
///   - external URLs (`http://`, `https://`, `mailto:`, etc.)
///   - image links (`![alt](src)`)
///   - links whose target does not end in `.md` (or `.md#anchor`)
///   - anchor-only links (`[Sec](#section)`)
///   - links inside inline code spans
///
/// Drops the redundant `|Text` alias when `Text` matches the target's
/// basename, so `[Foo](Foo.md)` becomes `[[Foo]]` not `[[Foo|Foo]]` —
/// the form Obsidian users author by hand.
/// Build a filename stem like `plot-1-eeca0526` whose suffix is a stable
/// hash of the file's bytes. Used to put the cache-bust *inside* the
/// filename instead of in a `?v=` query string — Obsidian and other
/// local-file renderers resolve URLs as literal filesystem paths and
/// don't strip query strings, so a hash that lives in the URL but not on
/// disk leaves them looking for a file that doesn't exist.
///
/// Different content → different hash → different filename → Obsidian
/// fetches the new file. Same content → same filename → caching works
/// and the `.md` stays byte-stable across re-renders.
///
/// On read failure, returns the un-hashed base — at worst we lose
/// cache-busting on that render; the next successful one restores it.
fn hashed_plot_stem(base: &str, path: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let Ok(bytes) = std::fs::read(path) else {
        return base.to_string();
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    let digest = (h.finish() & 0xFFFF_FFFF) as u32;
    format!("{base}-{digest:08x}")
}

fn rewrite_md_links_to_wikilinks(md: &str) -> String {
    let s = md.as_bytes();
    let n = s.len();
    let mut out = String::with_capacity(n);
    let mut i = 0;
    let mut copied_to = 0;

    while i < n {
        let b = s[i];

        // Inline code span: copy verbatim through the matched closing run.
        if b == b'`' {
            let run_start = i;
            while i < n && s[i] == b'`' {
                i += 1;
            }
            let open_len = i - run_start;
            let mut j = i;
            let mut closed = false;
            while j < n {
                if s[j] == b'\n' {
                    break;
                }
                if s[j] == b'`' {
                    let cs = j;
                    while j < n && s[j] == b'`' {
                        j += 1;
                    }
                    if j - cs == open_len {
                        i = j;
                        closed = true;
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            if !closed {
                // Unclosed run: continue from where we paused.
                continue;
            }
            continue;
        }

        // Image-link prefix `![` — skip the leading `!` and let the
        // following `[` open a normal image-link scan that we won't
        // rewrite (only `.md`-targeted text links become wikilinks).
        if b == b'!' && i + 1 < n && s[i + 1] == b'[' {
            i += 2;
            continue;
        }

        // Candidate text-link opener: `[` not preceded by `!`.
        if b == b'[' {
            if let Some((text_start, text_end, url_start, url_end, end)) = scan_md_link(s, i) {
                let url = &md[url_start..url_end];
                if let Some((target, anchor)) = parse_relative_md_link(url) {
                    let text = &md[text_start..text_end];
                    let basename = strip_md_extension(target);
                    let wl = render_wikilink_for(basename, anchor, text);
                    out.push_str(&md[copied_to..i]);
                    out.push_str(&wl);
                    i = end;
                    copied_to = end;
                    continue;
                }
            }
        }

        i += 1;
    }
    out.push_str(&md[copied_to..]);
    out
}

/// Scan `[text](url)` starting at `bytes[start] == b'['`. Returns the
/// inner text span, the inner URL span, and the byte index just past
/// the closing `)`. Refuses links whose `text` or `url` spans contain
/// raw newlines (Markdown links are inline by spec).
fn scan_md_link(
    bytes: &[u8],
    start: usize,
) -> Option<(usize, usize, usize, usize, usize)> {
    let n = bytes.len();
    debug_assert!(bytes[start] == b'[');
    let text_start = start + 1;
    let mut i = text_start;
    let mut depth = 1;
    while i < n {
        match bytes[i] {
            b'\n' => return None,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            b'\\' if i + 1 < n => i += 1,
            _ => {}
        }
        i += 1;
    }
    if depth != 0 {
        return None;
    }
    let text_end = i;
    if i + 1 >= n || bytes[i + 1] != b'(' {
        return None;
    }
    let url_start = i + 2;
    let mut j = url_start;
    let mut paren_depth = 1;
    while j < n {
        match bytes[j] {
            b'\n' => return None,
            b'(' => paren_depth += 1,
            b')' => {
                paren_depth -= 1;
                if paren_depth == 0 {
                    break;
                }
            }
            b'\\' if j + 1 < n => j += 1,
            _ => {}
        }
        j += 1;
    }
    if paren_depth != 0 {
        return None;
    }
    Some((text_start, text_end, url_start, j, j + 1))
}

/// Parse a markdown-link URL into `(target, anchor)` if it points at a
/// relative `.md` document. Rejects external URLs and anchor-only
/// references.
fn parse_relative_md_link(url: &str) -> Option<(&str, Option<&str>)> {
    let url = url.trim();
    if url.is_empty() || url.starts_with('#') {
        return None;
    }
    if url.contains("://") {
        return None;
    }
    let scheme_prefixes = ["mailto:", "tel:", "javascript:"];
    if scheme_prefixes.iter().any(|p| url.starts_with(p)) {
        return None;
    }
    let (target, anchor) = match url.find('#') {
        Some(idx) => (&url[..idx], Some(&url[idx + 1..])),
        None => (url, None),
    };
    if !target.to_ascii_lowercase().ends_with(".md") {
        return None;
    }
    Some((target, anchor))
}

fn strip_md_extension(target: &str) -> &str {
    target
        .strip_suffix(".md")
        .or_else(|| target.strip_suffix(".MD"))
        .unwrap_or(target)
}

fn render_wikilink_for(target: &str, anchor: Option<&str>, text: &str) -> String {
    let target_basename = std::path::Path::new(target)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| target.to_string());
    let mut body = target_basename.clone();
    if let Some(a) = anchor {
        body.push('#');
        body.push_str(a);
    }
    if text == target_basename
        || (anchor.is_none() && text == target)
        || (anchor.is_some() && text == format!("{} § {}", target_basename, anchor.unwrap()))
    {
        format!("[[{body}]]")
    } else {
        format!("[[{body}|{text}]]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execute::Rendered;
    use rustlab_plot::Theme;

    fn tmp_plot_dir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "rustlab_md_plots_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    fn theme() -> &'static ThemeColors {
        Theme::Dark.colors()
    }

    #[test]
    fn generated_banner_is_emitted() {
        let md = render_markdown("Hello", &[], &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(md.contains("Generated by rustlab-notebook"));
        // No synthetic H1 — that comes from the source notebook.
        assert!(!md.contains("# Hello\n"));
    }

    #[test]
    fn markdown_passthrough() {
        let blocks = vec![Rendered::Markdown(
            "## Section\n\nSome *prose* with `code`.".to_string(),
        )];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(md.contains("## Section"));
        assert!(md.contains("*prose*"));
        assert!(md.contains("`code`"));
    }

    #[test]
    fn code_block_emits_rustlab_fence() {
        let blocks = vec![Rendered::Code {
            source: "x = 42".to_string(),
            text_output: String::new(),
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
        }];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(md.contains("```rustlab\nx = 42\n```"));
    }

    #[test]
    fn hidden_source_is_suppressed_but_output_shown() {
        let blocks = vec![Rendered::Code {
            source: "secret = 42".to_string(),
            text_output: "answer: 42".to_string(),
            error: None,
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: true,
            details: None,
            grid_cols: None,
        }];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(!md.contains("secret = 42"));
        assert!(md.contains("```text\nanswer: 42\n```"));
    }

    #[test]
    fn error_is_rendered_as_code_block() {
        let blocks = vec![Rendered::Code {
            source: "oops".to_string(),
            text_output: String::new(),
            error: Some("undefined variable 'x'".to_string()),
            figures: Vec::new(),
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
        }];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(md.contains("error: undefined variable 'x'"));
    }

    #[test]
    fn callout_becomes_gfm_blockquote() {
        let blocks = vec![Rendered::Callout {
            kind: CalloutKind::Note,
            title: None,
            content: "Pay attention here.".to_string(),
        }];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(
            md.contains("> [!NOTE]\n> Pay attention here."),
            "expected GFM-native callout syntax: {md}"
        );
    }

    #[test]
    fn callout_emits_inline_title_when_set() {
        let blocks = vec![Rendered::Callout {
            kind: CalloutKind::Important,
            title: Some("Heads up".to_string()),
            content: "key fact".to_string(),
        }];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(
            md.contains("> [!IMPORTANT] Heads up\n> key fact"),
            "expected inline title in callout header: {md}"
        );
    }

    #[test]
    fn wikilinks_become_standard_md_links() {
        // Wikilinks in source render to `[text](Foo.md)` in markdown output
        // so the committed `.md` displays as ordinary links on GitHub.
        let blocks = vec![Rendered::Markdown(
            "see [[Foo]] and [[Bar|alias]]".to_string(),
        )];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(md.contains("[Foo](Foo.md)"), "missing wikilink: {md}");
        assert!(md.contains("[alias](Bar.md)"), "missing alias link: {md}");
        assert!(!md.contains("[[Foo]]"), "raw wikilink leaked: {md}");
    }

    #[test]
    fn embeds_become_standard_md_images() {
        let blocks = vec![Rendered::Markdown(
            "![[diagram.svg]] then ![[chart.png|chart]]".to_string(),
        )];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(md.contains("![](diagram.svg)"), "missing embed: {md}");
        assert!(md.contains("![chart](chart.png)"), "missing embed alt: {md}");
        assert!(!md.contains("![["), "raw embed leaked: {md}");
    }

    #[test]
    fn callout_kinds_round_trip_to_gfm_tags() {
        // Each new kind emits the matching GitHub tag.
        let cases = [
            (CalloutKind::Note, "[!NOTE]"),
            (CalloutKind::Tip, "[!TIP]"),
            (CalloutKind::Important, "[!IMPORTANT]"),
            (CalloutKind::Warning, "[!WARNING]"),
            (CalloutKind::Caution, "[!CAUTION]"),
        ];
        for (kind, tag) in cases {
            let blocks = vec![Rendered::Callout {
                kind,
                title: None,
                content: "body".to_string(),
            }];
            let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
            assert!(md.contains(tag), "missing {tag} for {kind:?}: {md}");
        }
    }

    #[test]
    fn solution_wraps_in_details() {
        let blocks = vec![Rendered::SolutionStart];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(md.contains("<details>"));
        assert!(md.contains("<summary>Solution</summary>"));
    }

    #[test]
    fn plot_reference_uses_relative_path() {
        // Build a minimal FigureState with a single line series so the SVG
        // writer actually produces output.
        use rustlab_plot::{FigureState, SeriesColor};
        let mut fig = FigureState::new();
        fig.current_mut().series.push(rustlab_plot::Series {
            label: "s".to_string(),
            x_data: vec![0.0, 1.0],
            y_data: vec![0.0, 1.0],
            color: SeriesColor::Blue,
            style: rustlab_plot::LineStyle::Solid,
            kind: rustlab_plot::PlotKind::Line,
        });
        let blocks = vec![Rendered::Code {
            source: "plot([0, 1])".to_string(),
            text_output: String::new(),
            error: None,
            figures: vec![fig],
            animations: Vec::new(),
            hidden: false,
            details: None,
            grid_cols: None,
        }];
        let plot_dir = tmp_plot_dir();
        // Use a multi-segment relative href to verify the prefix is taken
        // verbatim and not derived from the plot directory's basename.
        let md = render_markdown("T", &blocks, &plot_dir, "plots/quick_look", theme(), None, LinkStyle::Standard, true);
        // Filename carries the cache-bust as a hex suffix (`plot-1-<hex>.svg`)
        // so Obsidian's local-file path resolution finds the actual file on
        // disk. A `?v=` query string would work in a browser but not in
        // Obsidian's literal-path renderer.
        assert!(
            md.contains("![plot 1](plots/quick_look/plot-1-"),
            "plot URL with hashed-filename cache-bust missing: {md}",
        );
        assert!(md.contains(".svg)"), "plot URL must end with .svg: {md}");
        // Find the actual hashed file on disk via the directory contents.
        let entries: Vec<_> = std::fs::read_dir(&plot_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        assert!(
            entries.iter().any(|n| n.starts_with("plot-1-") && n.ends_with(".svg")),
            "hashed plot file should exist on disk; found: {entries:?}",
        );
        let _ = std::fs::remove_dir_all(&plot_dir);
    }

    #[test]
    fn obsidian_iframe_appended_when_href_set() {
        let md = render_markdown(
            "T",
            &[],
            &tmp_plot_dir(),
            "img",
            theme(),
            Some("analysis.html"),
            LinkStyle::Standard,
            true,
        );
        assert!(
            md.contains(r#"<iframe src="analysis.html""#),
            "iframe with sibling href should be emitted; got:\n{md}"
        );
        assert!(md.contains(r#"width="100%""#));
    }

    #[test]
    fn no_iframe_when_href_none() {
        let md = render_markdown("T", &[], &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(!md.contains("<iframe"));
    }

    #[test]
    fn mermaid_md_passthrough_fence() {
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\nA-->B".to_string(),
            hidden: false,
            details: None,
            caption: None,
        }];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(md.contains("```mermaid\nflowchart LR\nA-->B\n```"));
    }

    #[test]
    fn mermaid_md_caption_emitted_as_italic() {
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\nA-->B".to_string(),
            hidden: false,
            details: None,
            caption: Some("Signal flow".to_string()),
        }];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(md.contains("*Signal flow*"));
    }

    #[test]
    fn mermaid_md_hidden_omits() {
        let blocks = vec![Rendered::Mermaid {
            source: "flowchart LR\nA-->B".to_string(),
            hidden: true,
            details: None,
            caption: None,
        }];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(!md.contains("```mermaid"));
    }

    // ── Wikilink emission (Obsidian mode) ──

    #[test]
    fn obsidian_emits_wikilink_for_cross_notebook_link() {
        let blocks = vec![Rendered::Markdown(
            "see [Foo](other.md) for details".to_string(),
        )];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Wiki, true);
        assert!(md.contains("[[other|Foo]]"), "expected wikilink: {md}");
        assert!(!md.contains("](other.md)"), "raw md link leaked: {md}");
    }

    #[test]
    fn obsidian_anchored_link_uses_wikilink_anchor() {
        let blocks = vec![Rendered::Markdown(
            "see [Sec](other.md#section) for context".to_string(),
        )];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Wiki, true);
        assert!(md.contains("[[other#section|Sec]]"), "expected anchored wikilink: {md}");
    }

    #[test]
    fn obsidian_drops_alias_when_text_matches_basename() {
        // `[Foo](Foo.md)` → `[[Foo]]` (no `|Foo` alias) so the rendered
        // markdown matches the form Obsidian users author by hand.
        let blocks = vec![Rendered::Markdown("see [Foo](Foo.md) here".to_string())];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Wiki, true);
        assert!(md.contains("[[Foo]]"), "expected bare wikilink: {md}");
        assert!(!md.contains("[[Foo|Foo]]"));
    }

    #[test]
    fn obsidian_external_link_unchanged() {
        let blocks = vec![Rendered::Markdown(
            "see [GH](https://github.com) and [mail](mailto:a@b)".to_string(),
        )];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Wiki, true);
        assert!(md.contains("[GH](https://github.com)"), "external URL rewritten: {md}");
        assert!(md.contains("[mail](mailto:a@b)"), "mailto rewritten: {md}");
    }

    #[test]
    fn obsidian_anchor_only_link_unchanged() {
        let blocks = vec![Rendered::Markdown("jump to [Top](#top)".to_string())];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Wiki, true);
        assert!(md.contains("[Top](#top)"), "anchor-only link rewritten: {md}");
    }

    #[test]
    fn obsidian_image_link_unchanged() {
        // Image links use `![alt](src)` which wikilinks should not eat.
        let blocks = vec![Rendered::Markdown(
            "![diagram](pic.svg) plus [doc](doc.md)".to_string(),
        )];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Wiki, true);
        assert!(md.contains("![diagram](pic.svg)"), "image rewritten: {md}");
        assert!(md.contains("[[doc]]"), "doc not rewritten: {md}");
    }

    #[test]
    fn obsidian_inline_code_link_left_alone() {
        let blocks = vec![Rendered::Markdown(
            "see `[Foo](bar.md)` literal".to_string(),
        )];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Wiki, true);
        assert!(md.contains("`[Foo](bar.md)`"), "inline code rewritten: {md}");
    }

    #[test]
    fn obsidian_source_wikilinks_pass_through() {
        // Under Wiki style, source `[[Foo]]` is preserved (NOT converted
        // to `[Foo](Foo.md)` like Standard mode does).
        let blocks = vec![Rendered::Markdown(
            "see [[Foo]] and [[Bar|alias]]".to_string(),
        )];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Wiki, true);
        assert!(md.contains("[[Foo]]"), "source wikilink lost: {md}");
        assert!(md.contains("[[Bar|alias]]"), "aliased wikilink lost: {md}");
        assert!(!md.contains("[Foo](Foo.md)"), "Standard transform applied: {md}");
    }

    #[test]
    fn math_spans_pass_through_verbatim() {
        // Regression: an earlier version rewrote `\,` `\;` `\:` `\!`
        // `\|` to the double-backslash form and `^*` / `_*` to
        // `^{\ast}` / `_{\ast}` under the (wrong) belief that GitHub
        // strips backslashes inside math. github.com actually feeds
        // math spans to KaTeX verbatim, so the rewrites rendered
        // literally. Keep math byte-identical to source.
        let src = r"Inline $\mathbf{x}^*$ and $a\,b\;c\:d\!e\|f$. Display: $$\min\!\left(1,\; \frac{c}{\lVert g \rVert_2}\right)$$";
        let blocks = vec![Rendered::Markdown(src.to_string())];
        let md = render_markdown(
            "T",
            &blocks,
            &tmp_plot_dir(),
            "img",
            theme(),
            None,
            LinkStyle::Standard,
            true,
        );
        assert!(md.contains(src), "math mangled in render output:\n{md}");
        assert!(!md.contains(r"\\,"), "double-backslash leaked: {md}");
        assert!(!md.contains(r"^{\ast}"), "ast rewrite leaked: {md}");
    }

    #[test]
    fn standard_mode_keeps_md_links_as_md_links() {
        // Lock down the no-regression contract: in Standard mode,
        // `[Foo](other.md)` passes through as a plain markdown link.
        let blocks = vec![Rendered::Markdown(
            "see [Foo](other.md) here".to_string(),
        )];
        let md = render_markdown("T", &blocks, &tmp_plot_dir(), "img", theme(), None, LinkStyle::Standard, true);
        assert!(md.contains("[Foo](other.md)"), "MD link rewritten in Standard: {md}");
        assert!(!md.contains("[[other"), "wikilink leaked into Standard mode: {md}");
    }
}
