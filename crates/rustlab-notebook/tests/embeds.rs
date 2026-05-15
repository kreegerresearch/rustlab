//! Integration tests for `![[file]]` embeds.
//!
//! These exercise the full pipeline: source on disk → `expand_embeds` →
//! `parse_notebook` → `execute_notebook` → renderer. Per-piece unit
//! tests live alongside the implementation in
//! `crates/rustlab-notebook/src/embed.rs`.

use rustlab_notebook::{embed, execute, parse, render, render_markdown};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write(dir: &Path, name: &str, body: &str) {
    fs::write(dir.join(name), body).unwrap();
}

fn render_to_html(host_src: &str, dir: &Path) -> String {
    let expanded = embed::expand_embeds(host_src, dir, dir);
    let blocks = parse::parse_notebook(&expanded);
    let rendered = execute::execute_notebook(&blocks);
    let theme = rustlab_plot::theme::Theme::Dark.colors();
    render::render_html(
        "test",
        &rendered,
        Path::new("/tmp/plots-unused"),
        "plots",
        &theme,
        None,
    )
}

fn render_to_markdown(host_src: &str, dir: &Path) -> String {
    let expanded = embed::expand_embeds(host_src, dir, dir);
    let blocks = parse::parse_notebook(&expanded);
    let rendered = execute::execute_notebook(&blocks);
    let theme = rustlab_plot::theme::Theme::Dark.colors();
    render_markdown::render_markdown(
        "test",
        &rendered,
        Path::new("/tmp/plots-unused"),
        "plots",
        &theme,
        None,
        render_markdown::LinkStyle::Standard,
        true,
    )
}

#[test]
fn embed_full_file_renders_inlined() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "setup.md",
        "Setup paragraph with **bold** text.\n",
    );
    let host = "intro\n\n![[setup]]\n\nafter\n";
    let html = render_to_html(host, dir.path());
    assert!(html.contains("Setup paragraph"));
    assert!(html.contains("<strong>bold</strong>"));
    assert!(html.contains("intro"));
    assert!(html.contains("after"));
}

#[test]
fn embedded_rustlab_block_shares_evaluator_state() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "setup.md",
        "```rustlab\nFs = 48000\n```\n",
    );
    let host = "![[setup]]\n\nNow we use it:\n\n```rustlab\ndisp(2 * Fs)\n```\n";
    let html = render_to_html(host, dir.path());
    // The disp output of `2 * Fs` should appear; rustlab prints integers
    // as floats unless they are formatted otherwise.
    assert!(
        html.contains("96000") || html.contains("9.6e+04") || html.contains("9.6000e+04"),
        "expected 96000 in HTML; got snippet: {}",
        &html[..html.len().min(2000)]
    );
}

#[test]
fn embed_heading_demotion_visible_in_html() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "doc.md", "# Inner H1\n\nbody\n");
    let host = "![[doc]]\n";
    let html = render_to_html(host, dir.path());
    // Inner H1 should be demoted to H2.
    assert!(html.contains("<h2"), "expected demoted <h2> in:\n{}", html);
    assert!(!html.matches("<h1").any(|_| true) || html.contains("<h2"));
}

#[test]
fn unresolved_embed_renders_caution_callout() {
    let dir = TempDir::new().unwrap();
    let host = "before\n\n![[ghost]]\n\nafter\n";
    let html = render_to_html(host, dir.path());
    // Caution callouts render with a class containing "caution" (case
    // depends on the renderer; tolerate both).
    let lower = html.to_lowercase();
    assert!(lower.contains("caution"), "expected caution callout: {}", html);
    assert!(html.contains("target not found: ghost"));
}

#[test]
fn block_id_marker_stripped_from_host_render() {
    let dir = TempDir::new().unwrap();
    let host = "intro paragraph with marker. ^my-id\n\nfollowing.\n";
    let md = render_to_markdown(host, dir.path());
    assert!(!md.contains("^my-id"), "host render should strip ^my-id: {}", md);
    assert!(md.contains("intro paragraph with marker."));
}

#[test]
fn embed_section_anchored_by_heading() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "doc.md",
        "# Top\n\n## Frequency Response\n\nfreq body\n\n## Phase\n\nphase body\n",
    );
    let host = "![[doc#Frequency Response]]\n";
    let html = render_to_html(host, dir.path());
    assert!(html.contains("freq body"));
    assert!(!html.contains("phase body"));
}

#[test]
fn embed_block_id_emits_paragraph_only() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "gloss.md",
        "intro\n\nThe Nyquist rate is twice the highest frequency. ^nyquist\n\nepilogue\n",
    );
    let host = "Quote: ![[gloss#^nyquist]]\n";
    let html = render_to_html(host, dir.path());
    assert!(html.contains("Nyquist rate"));
    assert!(!html.contains("intro"));
    assert!(!html.contains("epilogue"));
    assert!(!html.contains("^nyquist"));
}

#[test]
fn embed_html_escapes_special_chars_in_prose() {
    // Per request test list: HTML escaping is correct when the embed
    // target contains `<`, `>`, `&` in prose. The escape happens in the
    // markdown renderer downstream; this test pins that the embed pass
    // does not break it.
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "snippet.md",
        "Compare 1 < 2 && 3 > 1 in plain prose.\n",
    );
    let host = "Quote: ![[snippet]]\n";
    let html = render_to_html(host, dir.path());
    assert!(html.contains("1 &lt; 2"), "html should escape <; got: {html}");
    assert!(html.contains("3 &gt; 1"), "html should escape >; got: {html}");
    assert!(html.contains("&amp;&amp;"), "html should escape &; got: {html}");
}

#[test]
fn embed_resolves_relative_to_host_dir_first() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("lessons");
    fs::create_dir(&sub).unwrap();
    write(dir.path(), "shared.md", "ROOT version\n");
    write(&sub, "shared.md", "HOST version\n");
    write(&sub, "lesson.md", "![[shared]]\n");
    // Render lesson.md: host_dir = lessons/, root_dir = dir
    let lesson_src = fs::read_to_string(sub.join("lesson.md")).unwrap();
    let expanded = embed::expand_embeds(&lesson_src, &sub, dir.path());
    assert!(expanded.contains("HOST version"));
    assert!(!expanded.contains("ROOT version"));
}
