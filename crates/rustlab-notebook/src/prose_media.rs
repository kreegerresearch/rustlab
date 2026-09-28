//! Prose figures and wide tables in notebook HTML / PDF.
//!
//! Markdown images and safe raw `<img>` tags are copied into the plot
//! directory after a path-jail check, so static HTML, `notebook watch`,
//! and PDF all reference a file the renderer placed. A missing file, a
//! jail escape, or a scheme PDF cannot embed becomes a visible
//! placeholder and a stderr warning — never a broken `\includegraphics`.
//!
//! Wide tables are wrapped in a horizontally scrolling `<div>` (HTML) or
//! a `tabularx` that fits `\linewidth` (LaTeX; see `render_latex`).

use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, Tag, TagEnd};

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "svg", "webp"];
const PDF_EXTS: &[&str] = &["png", "jpg", "jpeg", "svg"];

/// Where copied prose images land, and the directory relative paths
/// resolve against (the notebook directory at render time).
pub struct ProseAssets<'a> {
    plot_dir: &'a Path,
    href_prefix: String,
    /// Notebook directory. Relative image paths join this, not a later cwd.
    base: PathBuf,
    n: usize,
    stashed: Vec<String>,
}

impl<'a> ProseAssets<'a> {
    pub fn new(plot_dir: &'a Path, href_prefix: &str) -> Self {
        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::with_base(plot_dir, href_prefix, base)
    }

    pub fn with_base(plot_dir: &'a Path, href_prefix: &str, base: PathBuf) -> Self {
        Self {
            plot_dir,
            href_prefix: href_prefix.trim_end_matches('/').to_string(),
            base,
            n: 0,
            stashed: Vec::new(),
        }
    }

    /// HTML for a markdown or raw-HTML image. Local files are copied.
    pub fn html_for(&mut self, src: &str, alt: &str) -> String {
        match self.locate(src) {
            Located::Copied { html_src, .. } | Located::Remote(html_src) => {
                html_img(&html_src, alt)
            }
            Located::Missing(msg) => missing_html(&msg),
        }
    }

    /// LaTeX for a markdown image. No `\includegraphics` unless the file
    /// was copied and pdfTeX can embed that extension.
    pub fn latex_for(&mut self, src: &str, alt: &str) -> String {
        match self.locate(src) {
            Located::Copied { html_src, ext } if PDF_EXTS.contains(&ext.as_str()) => {
                let stem = strip_ext(&html_src);
                let mut out = format!(
                    "\\begin{{center}}\\includegraphics[width=\\linewidth,height=\\textheight,keepaspectratio]{{{stem}}}\\end{{center}}\n"
                );
                if !alt.is_empty() {
                    out.push_str(&format!(
                        "{{\\small\\textit{{{}}}}}\\par\n",
                        latex_escape(alt)
                    ));
                }
                out
            }
            Located::Copied { ext, .. } => {
                missing_latex(&format!("image format .{ext} is not embedded in PDF"))
            }
            Located::Remote(_) => missing_latex("remote image is not embedded in PDF"),
            Located::Missing(msg) => missing_latex(&msg),
        }
    }

    /// Replace safe `<img src alt>` tags (those two attributes only) with
    /// placeholders. The caller restores them after the HTML sanitiser,
    /// which would otherwise escape the rewritten tag.
    pub fn stash_safe_imgs(&mut self, html: &str) -> String {
        let bytes = html.as_bytes();
        let mut out = String::with_capacity(html.len());
        let mut i = 0;
        while i < bytes.len() {
            if html[i..].starts_with("<!--") {
                match html[i + 4..].find("-->") {
                    Some(end) => {
                        let stop = i + 4 + end + 3;
                        out.push_str(&html[i..stop]);
                        i = stop;
                    }
                    None => {
                        out.push_str(&html[i..]);
                        break;
                    }
                }
                continue;
            }
            if is_img_open(html, i) {
                if let Some(end) = end_of_tag(html, i) {
                    let tag = &html[i..end];
                    if let Some((src, alt)) = parse_safe_img(tag) {
                        let fig = self.html_for(&src, &alt);
                        let idx = self.stashed.len();
                        self.stashed.push(fig);
                        out.push_str(&img_placeholder(idx));
                        i = end;
                        continue;
                    }
                }
            }
            let ch = html[i..].chars().next().unwrap_or('\0');
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    pub fn restore_stashed(&self, html: &str) -> String {
        let mut out = html.to_string();
        for (i, frag) in self.stashed.iter().enumerate() {
            out = out.replace(&img_placeholder(i), frag);
        }
        out
    }

    /// LaTeX for a raw-HTML fragment: safe `<img>` tags become figures,
    /// everything else is escaped text (never passed through to TeX).
    pub fn raw_html_to_latex(&mut self, html: &str) -> String {
        let bytes = html.as_bytes();
        let mut out = String::new();
        let mut i = 0;
        let mut text_start = 0;
        while i < bytes.len() {
            if is_img_open(html, i) {
                if let Some(end) = end_of_tag(html, i) {
                    if let Some((src, alt)) = parse_safe_img(&html[i..end]) {
                        if text_start < i {
                            let chunk = &html[text_start..i];
                            if !chunk.trim().is_empty() {
                                out.push_str(&latex_escape(chunk));
                            }
                        }
                        out.push_str(&self.latex_for(&src, &alt));
                        i = end;
                        text_start = i;
                        continue;
                    }
                }
            }
            let ch = html[i..].chars().next().unwrap_or('\0');
            i += ch.len_utf8();
        }
        if text_start < html.len() {
            let chunk = &html[text_start..];
            if !chunk.trim().is_empty() {
                out.push_str(&latex_escape(chunk));
            }
        }
        out
    }

    fn locate(&mut self, src: &str) -> Located {
        let src = src.trim();
        if src.is_empty() {
            return self.miss("image has an empty source");
        }
        if crate::render::is_dangerous_url(src) {
            if crate::render::is_data_image_url(src) {
                return Located::Remote(src.to_string());
            }
            return self.miss("image URL scheme is not allowed");
        }
        if src.starts_with("//") || crate::render::has_url_scheme(src) {
            return Located::Remote(src.to_string());
        }
        let decoded = crate::render::percent_decode(src);
        if decoded.contains('\0') {
            return self.miss("image path contains NUL");
        }
        let ext = match image_ext(&decoded) {
            Some(ext) => ext,
            None => return self.miss(&format!("image type is not supported: {src}")),
        };
        let candidate = {
            let path = Path::new(&decoded);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                self.base.join(path)
            }
        };
        let root = crate::execute::jail_root_override().unwrap_or_else(|| self.base.clone());
        let resolved = match rustlab_script::path_jail::check_under_root(&candidate, &root) {
            Ok(p) => p,
            Err(e) => return self.miss(&e),
        };
        if !resolved.is_file() {
            return self.miss(&format!("image not found: {src}"));
        }
        self.n += 1;
        let name = format!("prose-{}.{ext}", self.n);
        if let Err(e) = std::fs::create_dir_all(self.plot_dir) {
            return self.miss(&format!("cannot create plot directory: {e}"));
        }
        let dest = self.plot_dir.join(&name);
        if let Err(e) = std::fs::copy(&resolved, &dest) {
            return self.miss(&format!("cannot copy image: {e}"));
        }
        let html_src = if self.href_prefix.is_empty() {
            name
        } else {
            format!("{}/{name}", self.href_prefix)
        };
        Located::Copied { html_src, ext }
    }

    fn miss(&self, msg: &str) -> Located {
        eprintln!("warning: prose image: {msg}");
        Located::Missing(msg.to_string())
    }
}

enum Located {
    Copied { html_src: String, ext: String },
    Remote(String),
    Missing(String),
}

fn image_ext(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (_, ext) = name.rsplit_once('.')?;
    if ext.is_empty() || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let ext = ext.to_ascii_lowercase();
    if IMAGE_EXTS.contains(&ext.as_str()) {
        Some(ext)
    } else {
        None
    }
}

fn strip_ext(href: &str) -> &str {
    match href.rfind('.') {
        Some(i) if !href[i + 1..].contains('/') => &href[..i],
        _ => href,
    }
}

fn html_img(src: &str, alt: &str) -> String {
    let src = html_escape(src);
    let alt_e = html_escape(alt);
    if alt.is_empty() {
        format!("<img src=\"{src}\" alt=\"\">")
    } else {
        format!(
            "<figure class=\"prose-figure\"><img src=\"{src}\" alt=\"{alt_e}\"><figcaption>{alt_e}</figcaption></figure>"
        )
    }
}

fn missing_html(msg: &str) -> String {
    format!(
        "<span class=\"missing-figure\">missing figure: {}</span>",
        html_escape(msg)
    )
}

fn missing_latex(msg: &str) -> String {
    format!("\\textit{{[missing figure: {}]}}\n\n", latex_escape(msg))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn latex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("\\&"),
            '%' => out.push_str("\\%"),
            '#' => out.push_str("\\#"),
            '_' => out.push_str("\\_"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            '\\' => out.push_str("\\textbackslash{}"),
            '$' => out.push_str("\\$"),
            _ => out.push(ch),
        }
    }
    out
}

fn img_placeholder(idx: usize) -> String {
    format!("\u{E000}P{idx}\u{E001}")
}

fn is_img_open(html: &str, i: usize) -> bool {
    let rest = &html[i..];
    let bytes = rest.as_bytes();
    if bytes.first() != Some(&b'<') {
        return false;
    }
    let name = rest.get(1..4).unwrap_or("");
    if !name.eq_ignore_ascii_case("img") {
        return false;
    }
    match bytes.get(4) {
        Some(b) if b.is_ascii_whitespace() || *b == b'/' || *b == b'>' => true,
        _ => false,
    }
}

fn end_of_tag(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'>' => return Some(i + 1),
                _ => {}
            },
        }
    }
    None
}

/// `Some((src, alt))` when the tag is `<img>` with only quoted `src` and
/// optional `alt`. Any other attribute (including `on*`) returns `None`
/// so the sanitiser can escape the tag.
fn parse_safe_img(tag: &str) -> Option<(String, String)> {
    let s = tag.trim();
    let s = s.strip_prefix('<')?.trim_end();
    let s = s.strip_suffix('>')?.trim();
    let s = s.strip_suffix('/').unwrap_or(s).trim();
    let bytes = s.as_bytes();
    if bytes.len() < 3 || !s[..3].eq_ignore_ascii_case("img") {
        return None;
    }
    if bytes.len() > 3 && !bytes[3].is_ascii_whitespace() {
        return None;
    }
    let mut i = 3;
    let mut src = None;
    let mut alt = None;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b':')
        {
            i += 1;
        }
        if start == i {
            return None;
        }
        let name = s[start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            return None;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let quote = bytes[i];
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        i += 1;
        let vstart = i;
        while i < bytes.len() && bytes[i] != quote {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let value = html_unescape(&s[vstart..i]);
        i += 1;
        if name.starts_with("on") {
            return None;
        }
        match name.as_str() {
            "src" if src.is_none() => src = Some(value),
            "alt" if alt.is_none() => alt = Some(value),
            _ => return None,
        }
    }
    Some((src?, alt.unwrap_or_default()))
}

fn html_unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Turn markdown `Tag::Image` events into placed HTML (or a placeholder).
pub fn rewrite_markdown_images<'b>(
    events: Vec<Event<'b>>,
    assets: &mut ProseAssets<'_>,
) -> Vec<Event<'b>> {
    let mut out = Vec::with_capacity(events.len());
    let mut iter = events.into_iter();
    while let Some(ev) = iter.next() {
        match ev {
            Event::Start(Tag::Image { dest_url, .. }) => {
                let src = dest_url.into_string();
                let mut alt = String::new();
                for inner in iter.by_ref() {
                    match inner {
                        Event::End(TagEnd::Image) => break,
                        Event::Text(t) | Event::Code(t) => alt.push_str(&t),
                        _ => {}
                    }
                }
                out.push(Event::Html(assets.html_for(&src, &alt).into()));
            }
            other => out.push(other),
        }
    }
    out
}

/// A markdown image is a block figure, but pulldown still wraps the
/// replacement in `<p>…</p>`. Drop that paragraph when it contains only
/// the figure so the HTML stays valid.
pub fn unwrap_paragraph_figures(html: &str) -> String {
    const OPEN: &str = "<p><figure class=\"prose-figure\">";
    const CLOSE: &str = "</figure></p>";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        let after = start + OPEN.len();
        if let Some(rel) = rest[after..].find(CLOSE) {
            let end = after + rel;
            if !rest[after..end].contains("<p") {
                out.push_str("<figure class=\"prose-figure\">");
                out.push_str(&rest[after..end]);
                out.push_str("</figure>");
                rest = &rest[end + CLOSE.len()..];
                continue;
            }
        }
        out.push_str(OPEN);
        rest = &rest[after..];
    }
    out.push_str(rest);
    out
}

/// Wrap each `<table>` in `<div class="table-scroll">` so a wide grid
/// scrolls inside the page instead of widening it.
pub fn wrap_scroll_tables(html: &str) -> String {
    let mut out = String::with_capacity(html.len() + 64);
    let mut rest = html;
    while let Some(start) = find_table_open(rest) {
        out.push_str(&rest[..start]);
        match table_span(&rest[start..]) {
            Some(end) => {
                out.push_str("<div class=\"table-scroll\">");
                out.push_str(&rest[start..start + end]);
                out.push_str("</div>");
                rest = &rest[start + end..];
            }
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

fn starts_with_ignore_ascii(hay: &[u8], pat: &[u8]) -> bool {
    hay.len() >= pat.len()
        && hay[..pat.len()]
            .iter()
            .zip(pat)
            .all(|(a, b)| a.to_ascii_lowercase() == *b)
}

fn find_table_open(hay: &str) -> Option<usize> {
    let bytes = hay.as_bytes();
    let mut i = 0;
    while i + 6 <= bytes.len() {
        if bytes[i] == b'<' && starts_with_ignore_ascii(&bytes[i + 1..], b"table") {
            let after = i + 6;
            if after == bytes.len()
                || bytes[after].is_ascii_whitespace()
                || bytes[after] == b'>'
                || bytes[after] == b'/'
            {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn table_span(hay: &str) -> Option<usize> {
    let bytes = hay.as_bytes();
    let mut i = 0;
    let mut depth = 0i32;
    while i + 6 <= bytes.len() {
        if bytes[i] == b'<' {
            let close = bytes.get(i + 1) == Some(&b'/');
            let name_at = if close { i + 2 } else { i + 1 };
            if starts_with_ignore_ascii(bytes.get(name_at..).unwrap_or(&[]), b"table") {
                let after = name_at + 5;
                let boundary = after >= bytes.len()
                    || bytes[after].is_ascii_whitespace()
                    || bytes[after] == b'>'
                    || bytes[after] == b'/';
                if boundary {
                    if close {
                        depth -= 1;
                        if depth == 0 {
                            let gt = bytes[i..].iter().position(|b| *b == b'>')?;
                            return Some(i + gt + 1);
                        }
                    } else {
                        depth += 1;
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Drop `<a href="#fragment">…</a>` when `fragment` is not an `id` in
/// `html`. Cross-file links (`other.html#setup`, `/n/slug#setup`) stay.
/// The inner HTML is kept, so the link text remains.
pub fn drop_dangling_fragment_links(html: &str) -> String {
    let ids = collect_ids(html);
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while i < bytes.len() {
        if is_anchor_open(html, i) {
            if let Some(tag_end) = end_of_tag(html, i) {
                let open = &html[i..tag_end];
                if let Some(href) = attr_value(open, "href") {
                    if let Some(frag) = pure_fragment(&href) {
                        if !ids.contains(&frag) {
                            if let Some((rel_close, closer_len)) =
                                find_close_anchor(&html[tag_end..])
                            {
                                let inner_end = tag_end + rel_close;
                                out.push_str(&html[tag_end..inner_end]);
                                i = inner_end + closer_len;
                                continue;
                            }
                        }
                    }
                }
            }
        }
        let ch = html[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn is_anchor_open(html: &str, i: usize) -> bool {
    let bytes = html.as_bytes();
    if bytes.get(i) != Some(&b'<') {
        return false;
    }
    let name = html.get(i + 1..i + 2).unwrap_or("");
    if !name.eq_ignore_ascii_case("a") {
        return false;
    }
    match bytes.get(i + 2) {
        Some(b) if b.is_ascii_whitespace() || *b == b'>' || *b == b'/' => true,
        _ => false,
    }
}

/// `(offset of `</a…>`, byte length of that closer)`.
fn find_close_anchor(hay: &str) -> Option<(usize, usize)> {
    let lower = hay.to_ascii_lowercase();
    let bytes = hay.as_bytes();
    let mut from = 0;
    while let Some(rel) = lower[from..].find("</a") {
        let at = from + rel;
        let after = at + 3;
        let b = *bytes.get(after)?;
        if b == b'>' {
            return Some((at, 4));
        }
        if b.is_ascii_whitespace() {
            if let Some(gt) = hay[after..].find('>') {
                return Some((at, (after - at) + gt + 1));
            }
        }
        from = at + 3;
    }
    None
}

fn collect_ids(html: &str) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    let bytes = html.as_bytes();
    let mut i = 0;
    while i + 4 < bytes.len() {
        // `id="…"` only when it starts an attribute (whitespace before it).
        // Compare bytes: `i` walks every byte and may sit inside a multibyte
        // character such as `…`.
        if starts_with_ignore_ascii(&bytes[i..], b"id=")
            && (i == 0 || bytes[i - 1].is_ascii_whitespace())
        {
            let quote = bytes[i + 3];
            if quote == b'"' || quote == b'\'' {
                let vstart = i + 4;
                if let Some(rel) = html[vstart..].find(quote as char) {
                    ids.insert(html_unescape(&html[vstart..vstart + rel]));
                    i = vstart + rel + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    ids
}

fn attr_value(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let needle = format!("{name}=");
    let mut from = 0;
    let lower = tag.to_ascii_lowercase();
    while let Some(rel) = lower[from..].find(&needle) {
        let at = from + rel;
        if at > 0 && !bytes[at - 1].is_ascii_whitespace() {
            from = at + needle.len();
            continue;
        }
        let mut i = at + needle.len();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let quote = bytes[i];
        if quote == b'"' || quote == b'\'' {
            let vstart = i + 1;
            let end = tag[vstart..].find(quote as char)?;
            return Some(html_unescape(&tag[vstart..vstart + end]));
        }
        return None;
    }
    None
}

/// `Some(fragment)` when `href` is a pure in-page `#fragment` (no path).
fn pure_fragment(href: &str) -> Option<String> {
    let href = href.trim();
    let rest = href.strip_prefix('#')?;
    if rest.contains(':') && crate::render::has_url_scheme(href) {
        return None;
    }
    let decoded = crate::render::percent_decode(rest);
    Some(html_unescape(&decoded))
}

/// 1×1 PNG used by prose-image tests.
pub const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_safe_img_rejects_handlers_and_extra_attrs() {
        assert_eq!(
            parse_safe_img(r#"<img src="a.png" alt="hi">"#),
            Some(("a.png".into(), "hi".into()))
        );
        assert_eq!(
            parse_safe_img(r#"<img alt='x' src="b.png"/>"#),
            Some(("b.png".into(), "x".into()))
        );
        assert!(parse_safe_img(r#"<img src="a.png" onerror="alert(1)">"#).is_none());
        assert!(parse_safe_img(r#"<img src=a.png>"#).is_none());
        assert!(parse_safe_img(r#"<img src="a.png" width="10">"#).is_none());
    }

    #[test]
    fn unwraps_a_paragraph_that_only_holds_a_figure() {
        let html = "<p><figure class=\"prose-figure\"><img src=\"a.png\" alt=\"A\"><figcaption>A</figcaption></figure></p>";
        let out = unwrap_paragraph_figures(html);
        assert!(out.starts_with("<figure"), "{out}");
        assert!(!out.contains("<p>"), "{out}");
        assert!(out.contains("<figcaption>A</figcaption>"), "{out}");
    }

    #[test]
    fn wrap_scroll_tables_wraps_each_table() {
        let html = "<p>x</p><table><tr><td>a</td></tr></table><table><tr></tr></table>";
        let out = wrap_scroll_tables(html);
        assert_eq!(out.matches("<div class=\"table-scroll\">").count(), 2);
        assert!(out.contains("</table></div>"));
    }

    #[test]
    fn dangling_fragment_dropped_real_target_kept() {
        let html = r##"<h2 id="setup">Setup</h2><a href="#setup">ok</a> <a href="#nope">gone</a> <a href="other.html#setup">cross</a>"##;
        let out = drop_dangling_fragment_links(html);
        assert!(out.contains("href=\"#setup\""));
        assert!(out.contains("href=\"other.html#setup\""));
        assert!(!out.contains("#nope"));
        assert!(out.contains(">gone</") || out.contains("gone"));
        assert!(out.contains("gone"));
    }

    #[test]
    fn local_image_is_copied_and_escape_is_not() {
        let nb = tempfile::tempdir().unwrap();
        let nb_path = nb.path().canonicalize().unwrap();
        std::fs::write(nb_path.join("dot.png"), TINY_PNG).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.png"), TINY_PNG).unwrap();
        let plots = tempfile::tempdir().unwrap();
        let _jail = crate::execute::JailRootGuard::new(Some(nb_path.clone()));

        let mut assets = ProseAssets::with_base(plots.path(), "plots/nb", nb_path.clone());
        let html = assets.html_for("dot.png", "Scope");
        assert!(html.contains("plots/nb/prose-1.png"), "{html}");
        assert!(html.contains("<figcaption>Scope</figcaption>"), "{html}");
        assert!(plots.path().join("prose-1.png").is_file());

        let plots_out = tempfile::tempdir().unwrap();
        let mut assets = ProseAssets::with_base(plots_out.path(), "plots/nb", nb_path);
        let secret = outside.path().join("secret.png");
        let html = assets.html_for(&secret.display().to_string(), "nope");
        assert!(html.contains("missing-figure"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(
            std::fs::read_dir(plots_out.path())
                .unwrap()
                .next()
                .is_none(),
            "jail escape must not copy a file"
        );
    }

    #[test]
    fn latex_missing_image_is_not_includegraphics() {
        let nb = tempfile::tempdir().unwrap();
        let plots = tempfile::tempdir().unwrap();
        let mut assets = ProseAssets::with_base(plots.path(), "plots/nb", nb.path().to_path_buf());
        let tex = assets.latex_for("missing.png", "cap");
        assert!(tex.contains("missing figure"), "{tex}");
        assert!(!tex.contains("includegraphics"), "{tex}");
    }

    #[test]
    fn latex_png_scales_to_linewidth_without_extension() {
        let nb = tempfile::tempdir().unwrap();
        let nb_path = nb.path().canonicalize().unwrap();
        std::fs::write(nb_path.join("dot.png"), TINY_PNG).unwrap();
        let plots = tempfile::tempdir().unwrap();
        let _jail = crate::execute::JailRootGuard::new(Some(nb_path.clone()));
        let mut assets = ProseAssets::with_base(plots.path(), "plots/nb", nb_path);
        let tex = assets.latex_for("dot.png", "A & B");
        assert!(
            tex.contains("\\includegraphics[width=\\linewidth,height=\\textheight,keepaspectratio]{plots/nb/prose-1}"),
            "{tex}"
        );
        assert!(tex.contains("A \\& B"), "{tex}");
        assert!(plots.path().join("prose-1.png").is_file());
    }
}
