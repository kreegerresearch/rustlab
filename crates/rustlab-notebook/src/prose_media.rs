//! Prose figures for notebook HTML, `notebook watch`, and PDF / LaTeX.
//!
//! Markdown images and a safe raw `<img src alt>` are copied into the plot
//! directory after a path-jail check. HTML and watch reference that copy
//! (`plots/…` or `/plots/<slug>/…`); PDF includes it with `\includegraphics`
//! scaled to `\linewidth`. A missing file or a jail escape becomes a visible
//! placeholder and a stderr warning. Remote URLs are not fetched. Wide PDF
//! tables are a `tabularx` in `render_latex`, not this module.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, Parser, Tag};

/// Shown when `POST /save`, a cell save, or `POST /annotate` (including
/// undo) would add an SVG prose image that the file does not already
/// reference.
pub const SVG_PROSE_REJECTION: &str =
    "refusing to add an SVG prose image; edit the notebook file on disk to add one";

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

    /// HTML for a markdown or raw image. Local files are the copied plot
    /// path. `data:image/*` is left inline. Anything else we cannot place
    /// is a placeholder, not an `<img>` with a broken `src`.
    pub fn html_for(&mut self, src: &str, alt: &str) -> String {
        match self.locate(src) {
            Located::Copied { html_src, .. } => html_img(&html_src, alt),
            // Remote URLs are not fetched. `data:image/*` is allowed by the
            // watch CSP; `http(s)` is left as the author wrote it and the
            // watch `img-src 'self'` policy blocks it in the browser.
            Located::Remote(url) => html_img(&url, alt),
            Located::Missing(msg) => missing_html(&msg),
        }
    }

    /// Replace safe `<img src alt>` tags with placeholders before the HTML
    /// sanitiser, which escapes every tag that carries an attribute. The
    /// placeholder has no `<`.
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
            Located::Remote(url) => {
                missing_latex(&format!("remote image is not embedded in PDF ({url})"))
            }
            Located::Missing(msg) => missing_latex(&msg),
        }
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
            Err(e) => {
                // The stderr line names the path for the author. The page
                // placeholder does not: an absolute path outside the jail
                // would otherwise be written into the HTML.
                eprintln!("warning: prose image: {e}");
                return Located::Missing("image is outside the notebook directory".to_string());
            }
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
    format!(
        "<img src=\"{}\" alt=\"{}\">",
        html_escape(src),
        html_escape(alt)
    )
}

fn missing_html(msg: &str) -> String {
    format!(
        "<span class=\"missing-figure\">missing figure: {}</span>",
        html_escape(msg)
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn img_placeholder(idx: usize) -> String {
    format!("\u{E000}P{idx}\u{E001}")
}

fn missing_latex(msg: &str) -> String {
    format!("\\textit{{[missing figure: {}]}}\n\n", latex_escape(msg))
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

fn is_img_open(html: &str, i: usize) -> bool {
    // Callers walk bytes. A multibyte character (the em dash in the
    // generated-header comment) is not a slice boundary.
    if !html.is_char_boundary(i) {
        return false;
    }
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

/// Paths of SVG prose images [`crate::render`] would copy.
///
/// Uses the same wikilink rewrite and markdown options as the renderer,
/// so a fenced or inline code sample that mentions `![x](fig.svg)` is not
/// a prose image. Counts markdown images (including reference-style and
/// `![[fig.svg]]` embeds) and any raw `<img>` whose quoted `src` ends in
/// `.svg`. The key drops a `?query` or `#fragment` and percent-decodes, so
/// `fig.svg` and `fig%2esvg?v=1` are one reference.
pub fn svg_prose_image_dests(md: &str) -> BTreeSet<String> {
    let md = crate::render::transform_wikilinks(md);
    let mut out = BTreeSet::new();
    let parser = Parser::new_ext(&md, crate::render::notebook_md_options());
    for event in parser {
        match event {
            Event::Start(Tag::Image { dest_url, .. }) => {
                if let Some(key) = svg_image_key(&dest_url) {
                    out.insert(key);
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => collect_img_svg_srcs(&html, &mut out),
            _ => {}
        }
    }
    out
}

/// True when `after` references an SVG prose image that `before` does not.
pub fn introduces_svg_prose_image(before: &str, after: &str) -> bool {
    !svg_prose_image_dests(after).is_subset(&svg_prose_image_dests(before))
}

fn collect_img_svg_srcs(html: &str, out: &mut BTreeSet<String>) {
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if is_img_open(html, i) {
            if let Some(end) = end_of_tag(html, i) {
                if let Some(src) = img_quoted_src(&html[i..end]) {
                    if let Some(key) = svg_image_key(&src) {
                        out.insert(key);
                    }
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
}

/// Quoted `src` on an `<img>` tag. Extra attributes do not hide the src:
/// the renderer refuses to copy an unsafe tag, and a save still refuses
/// to persist a new SVG reference so a sanitiser gap cannot start serving it.
fn img_quoted_src(tag: &str) -> Option<String> {
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
            return src;
        }
        let name = s[start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            return src;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return src;
        }
        let quote = bytes[i];
        if quote != b'"' && quote != b'\'' {
            return src;
        }
        i += 1;
        let vstart = i;
        while i < bytes.len() && bytes[i] != quote {
            i += 1;
        }
        if i >= bytes.len() {
            return src;
        }
        let value = html_unescape(&s[vstart..i]);
        i += 1;
        if name == "src" && src.is_none() {
            src = Some(value);
        }
    }
    src
}

fn svg_image_key(dest: &str) -> Option<String> {
    let dest = dest.trim();
    if dest.is_empty() || crate::render::has_url_scheme(dest) || dest.starts_with("//") {
        return None;
    }
    let dest = dest
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(dest)
        .trim();
    let path = dest.split(['?', '#']).next().unwrap_or(dest).trim();
    if path.is_empty() {
        return None;
    }
    let decoded = crate::render::percent_decode(path);
    let name = decoded.rsplit(['/', '\\']).next().unwrap_or(&decoded);
    let ext = name.rsplit_once('.')?.1;
    if ext.eq_ignore_ascii_case("svg") {
        Some(decoded)
    } else {
        None
    }
}

fn html_unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
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
    fn latex_jail_escape_is_not_copied() {
        let nb = tempfile::tempdir().unwrap();
        let nb_path = nb.path().canonicalize().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.png"), TINY_PNG).unwrap();
        let plots = tempfile::tempdir().unwrap();
        let _jail = crate::execute::JailRootGuard::new(Some(nb_path.clone()));
        let mut assets = ProseAssets::with_base(plots.path(), "plots/nb", nb_path);
        let secret = outside.path().join("secret.png");
        let tex = assets.latex_for(&secret.display().to_string(), "nope");
        assert!(tex.contains("missing figure"), "{tex}");
        assert!(tex.contains("outside the notebook directory"), "{tex}");
        assert!(!tex.contains("secret.png"), "{tex}");
        assert!(!tex.contains("includegraphics"), "{tex}");
        assert!(
            std::fs::read_dir(plots.path()).unwrap().next().is_none(),
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

    #[test]
    fn svg_prose_refs_cover_the_forms_the_renderer_copies() {
        let inline = svg_prose_image_dests("![x](eval.svg)\n");
        assert!(inline.contains("eval.svg"), "{inline:?}");
        let embed = svg_prose_image_dests("![[figs/Eval.SVG|diagram]]\n");
        assert!(embed.contains("figs/Eval.SVG"), "{embed:?}");
        let raw = svg_prose_image_dests("<img src=\"eval.svg\" onerror=\"alert(1)\">\n");
        assert!(raw.contains("eval.svg"), "{raw:?}");
        let reference = svg_prose_image_dests("![x][fig]\n\n[fig]: ./eval.svg \"t\"\n");
        assert!(reference.contains("./eval.svg"), "{reference:?}");
        let encoded = svg_prose_image_dests("![x](eval%2esvg?cache=1)\n");
        assert!(encoded.contains("eval.svg"), "{encoded:?}");
        let png = svg_prose_image_dests("![x](dot.png)\n");
        assert!(png.is_empty(), "{png:?}");
        let fenced = svg_prose_image_dests("```\n![x](eval.svg)\n```\n");
        assert!(fenced.is_empty(), "{fenced:?}");
        let code = svg_prose_image_dests("see `![x](eval.svg)` please\n");
        assert!(code.is_empty(), "{code:?}");
    }

    #[test]
    fn svg_scan_of_the_generated_header_does_not_panic() {
        let md = format!("{}\n\n# Notes\n\nSee this.\n", crate::GENERATED_HEADER);
        assert!(svg_prose_image_dests(&md).is_empty());
        assert!(!introduces_svg_prose_image(&md, &md));
    }

    #[test]
    fn introduces_svg_prose_image_ignores_refs_already_on_disk() {
        let before = "![kept](fig.svg)\n";
        assert!(!introduces_svg_prose_image(
            before,
            "![kept](fig.svg)\n\nmore text\n"
        ));
        assert!(introduces_svg_prose_image(
            before,
            "![kept](fig.svg)\n![x](eval.svg)\n"
        ));
        assert!(introduces_svg_prose_image("", "![[eval.svg]]\n"));
        // `==` is not markdown, so a highlight of image syntax is still
        // a prose image. Wrapping a reference the file already has is not
        // a new dest.
        assert!(introduces_svg_prose_image(
            "# notes\n",
            "# notes\n\n==![x](eval.svg)==\n"
        ));
        assert!(!introduces_svg_prose_image(
            "![x](eval.svg)\n",
            "==![x](eval.svg)==\n"
        ));
        assert!(introduces_svg_prose_image(
            "# notes\n",
            "# notes\n\n%%#c1: ![x](eval.svg)%%\n"
        ));
        assert!(!introduces_svg_prose_image(
            "# notes\n",
            "# notes\n\n%%#c1: see `![x](eval.svg)`%%\n"
        ));
    }

    #[test]
    fn svg_prose_image_is_an_img_not_an_inline_document() {
        let nb = tempfile::tempdir().unwrap();
        let svg =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>fetch('/save/nb')</script></svg>";
        std::fs::write(nb.path().join("eval.svg"), svg).unwrap();
        let plots = tempfile::tempdir().unwrap();
        let mut assets = ProseAssets::with_base(plots.path(), "/plots/nb", nb.path().to_path_buf());
        let html =
            crate::render::markdown_to_html_linked("![x](eval.svg)\n", None, Some(&mut assets));
        assert!(html.contains("<img "), "{html}");
        assert!(html.contains("src=\"/plots/nb/prose-1.svg\""), "{html}");
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.to_ascii_lowercase().contains("<object"));
        assert!(!html.to_ascii_lowercase().contains("<embed"));
        assert!(!html.to_ascii_lowercase().contains("<iframe"));
        assert!(!html.contains("<svg"), "{html}");
        let copied = std::fs::read_to_string(plots.path().join("prose-1.svg")).unwrap();
        assert!(
            copied.contains("<script>"),
            "the file is copied, not inlined"
        );
    }
}
