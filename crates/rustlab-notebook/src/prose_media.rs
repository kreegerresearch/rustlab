//! Prose figures for notebook PDF / LaTeX.
//!
//! Markdown images and a safe raw `<img src alt>` are copied into the plot
//! directory after a path-jail check, then included with `\includegraphics`
//! scaled to `\linewidth`. A missing file, a jail escape, gif/webp, or a
//! remote URL becomes a visible placeholder and a stderr warning — never a
//! broken `\includegraphics`. Wide tables are a `tabularx` in
//! `render_latex`, not this module.

use std::path::{Path, PathBuf};

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
}
