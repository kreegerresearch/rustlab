//! Collapsible collection file browser shared by directory `watch` and
//! static directory HTML.
//!
//! One tree, two href spellings. Callers pass entries already sorted by
//! [`crate::compare_notebook_order`] and hrefs already resolved for the
//! page being rendered (static pages climb out of their directory;
//! directory `watch` uses `/n/<relative-path>`). This module only groups that sequence into
//! folders and emits the markup, so the two outputs cannot disagree about
//! membership or order.
//!
//! Collapse is native `<details>` / `<summary>`. The watch CSP forbids
//! inline event handlers, and static HTML has no script of its own for
//! this widget, so there is nothing to click-wire.

use rustlab_plot::theme::ThemeColors;

/// One listable notebook, as the browser should show it on one page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub title: String,
    /// Collection-relative source path, `/`-joined (`ch2/filters.md`).
    pub rel_md: String,
    /// Href for *this* page (relative in static HTML, absolute on `watch`).
    pub href: String,
}

/// Directory-mode sidebar. `current_rel` is the open notebook's
/// `rel_md`, or `None` on the index page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionBrowser {
    /// Root disclosure label (the collection title).
    pub label: String,
    pub current_rel: Option<String>,
    pub entries: Vec<FileEntry>,
}

enum Node {
    File(FileEntry),
    Dir {
        /// Collection-relative directory (`ch2`, `ch2/lab`).
        path: String,
        name: String,
        children: Vec<Node>,
    },
}

/// Group `entries` by directory without re-sorting them.
///
/// Entries must already be in listing order. A folder is created where
/// its first descendant appears and later siblings of that folder stay
/// after it, so an explicit `order:` that surfaces a nested notebook
/// early pulls that folder up — the same way on every page.
fn build_tree(entries: &[FileEntry]) -> Vec<Node> {
    let mut roots = Vec::new();
    for entry in entries {
        let parts: Vec<&str> = entry.rel_md.split('/').filter(|p| !p.is_empty()).collect();
        insert(&mut roots, &parts, "", entry.clone());
    }
    roots
}

fn insert(nodes: &mut Vec<Node>, parts: &[&str], parent: &str, entry: FileEntry) {
    if parts.len() <= 1 {
        nodes.push(Node::File(entry));
        return;
    }
    let name = parts[0];
    let path = if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    };
    if let Some(Node::Dir { children, .. }) = nodes.iter_mut().find(|n| match n {
        Node::Dir { path: p, .. } => p == &path,
        Node::File(_) => false,
    }) {
        insert(children, &parts[1..], &path, entry);
        return;
    }
    let mut children = Vec::new();
    insert(&mut children, &parts[1..], &path, entry);
    nodes.push(Node::Dir {
        path,
        name: name.to_string(),
        children,
    });
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `<nav class="file-browser">` for `browser`. Empty when there is
/// nothing to show — callers should then omit the collection sidebar
/// entirely (single-file pages pass no browser).
pub fn render_nav(browser: &CollectionBrowser) -> String {
    if browser.entries.is_empty() {
        return String::new();
    }
    let tree = build_tree(&browser.entries);
    let mut inner = String::new();
    render_nodes(&tree, browser.current_rel.as_deref(), &mut inner);
    format!(
        "<nav class=\"file-browser\" aria-label=\"{label}\">\n\
<details class=\"fb-root\" open data-path=\".\">\
<summary class=\"fb-summary\"><span class=\"fb-label\">{label}</span></summary>\n\
{inner}</details></nav>\n",
        label = escape(&browser.label),
        inner = inner,
    )
}

fn render_nodes(nodes: &[Node], current: Option<&str>, out: &mut String) {
    out.push_str("<ul class=\"fb-list\">\n");
    for node in nodes {
        match node {
            Node::File(entry) => {
                let (class, aria) = if current == Some(entry.rel_md.as_str()) {
                    ("fb-link fb-current", " aria-current=\"page\"")
                } else {
                    ("fb-link", "")
                };
                let name = entry
                    .rel_md
                    .rsplit('/')
                    .next()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(entry.rel_md.as_str());
                out.push_str(&format!(
                    "<li class=\"fb-file\"><a class=\"{class}\"{aria} href=\"{href}\">\
<span class=\"fb-name\">{name}</span></a></li>\n",
                    href = escape(&entry.href),
                    name = escape(name),
                ));
            }
            Node::Dir {
                path,
                name,
                children,
            } => {
                out.push_str(&format!(
                    "<li class=\"fb-dir\"><details class=\"fb-folder\" open data-path=\"{path}\">\
<summary class=\"fb-summary\"><span class=\"fb-label\">{name}</span></summary>\n",
                    path = escape(path),
                    name = escape(name),
                ));
                render_nodes(children, current, out);
                out.push_str("</details></li>\n");
            }
        }
    }
    out.push_str("</ul>\n");
}

/// Chrome for [`render_nav`]. Colors come from the page's `--rl-*`
/// tokens (with this palette as the fallback) so the browser follows
/// the active theme without its own palette.
pub fn styles(theme: &ThemeColors) -> String {
    let bg = theme.css_var("bg-secondary");
    let text = theme.css_var("text");
    let text_dim = theme.css_var("text-dim");
    let border = theme.css_var("border");
    let accent = theme.css_var("accent-primary");
    format!(
        r#"
  /* ── Collection file browser (directory mode) ──
     Width tracks <details open>: closing the root disclosure shrinks
     the column via :has(), with no script and no inline handler. */
  body.has-files {{ --fb-w: 16.5rem; }}
  body.has-files:has(nav.file-browser > details.fb-root:not([open])) {{
    --fb-w: 2.75rem;
  }}
  nav.file-browser {{
    position: fixed;
    top: var(--topbar-h, 0px);
    left: 0;
    width: var(--fb-w, 16.5rem);
    height: calc(100vh - var(--topbar-h, 0px));
    background: {bg};
    color: {text};
    border-right: 1px solid {border};
    overflow: auto;
    z-index: 120;
  }}
  nav.file-browser .fb-summary {{
    cursor: pointer;
    list-style: none;
    user-select: none;
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }}
  nav.file-browser .fb-summary::-webkit-details-marker {{ display: none; }}
  nav.file-browser .fb-summary::marker {{ content: ""; }}
  nav.file-browser .fb-summary::before {{
    content: "▾";
    flex: 0 0 auto;
    width: 1.1em;
    color: {accent};
  }}
  nav.file-browser details:not([open]) > .fb-summary::before {{
    content: "▸";
  }}
  nav.file-browser > details.fb-root > .fb-summary {{
    font-weight: 700;
    color: {accent};
    padding: 0.9rem 0.85rem 0.7rem;
    border-bottom: 1px solid {border};
  }}
  nav.file-browser > details.fb-root:not([open]) > .fb-summary {{
    border-bottom: 0;
    padding: 0.75rem 0.55rem;
  }}
  nav.file-browser > details.fb-root:not([open]) > .fb-summary .fb-label {{
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }}
  nav.file-browser .fb-folder > .fb-summary {{
    padding: 0.35rem 0.75rem;
    color: {text_dim};
    font-size: 0.78rem;
    font-weight: 650;
    letter-spacing: 0.01em;
  }}
  nav.file-browser .fb-list {{
    list-style: none;
    margin: 0;
    padding: 0.15rem 0 0.3rem;
  }}
  nav.file-browser li {{
    margin: 0;
  }}
  nav.file-browser .fb-folder > .fb-list {{
    padding-left: 0.55rem;
  }}
  nav.file-browser a.fb-link {{
    display: block;
    padding: 0.28rem 0.85rem 0.28rem 1.35rem;
    text-decoration: none;
    color: {text};
    background: transparent;
    border: 0;
    border-radius: 0;
  }}
  nav.file-browser a.fb-link:hover {{
    background: {border};
    color: {text};
  }}
  nav.file-browser .fb-name {{
    font-size: 0.82rem;
    line-height: 1.35;
    color: {text};
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    overflow-wrap: anywhere;
  }}
  nav.file-browser a.fb-current {{
    background: {border};
    box-shadow: inset 3px 0 0 {accent};
  }}
  @media (max-width: 768px) {{
    body.has-files,
    body.has-files:has(nav.file-browser > details.fb-root:not([open])) {{
      --fb-w: 0px;
    }}
    nav.file-browser {{
      width: min(18rem, 88vw);
      z-index: 90;
      transform: translateX(calc(-100% + 2.6rem));
    }}
    nav.file-browser:has(> details.fb-root[open]) {{
      transform: none;
      z-index: 180;
      box-shadow: 4px 0 24px rgba(0, 0, 0, 0.35);
    }}
    nav.sidebar.open {{
      z-index: 170;
    }}
  }}
"#,
        bg = bg,
        text = text,
        text_dim = text_dim,
        border = border,
        accent = accent,
    )
}

/// File names in browser rows, document order.
#[cfg(test)]
pub(crate) fn path_texts(html: &str) -> Vec<String> {
    const OPEN: &str = "<span class=\"fb-name\">";
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find(OPEN) {
        rest = &rest[i + OPEN.len()..];
        let Some(end) = rest.find("</span>") else {
            break;
        };
        out.push(rest[..end].to_string());
        rest = &rest[end + 7..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustlab_plot::Theme;

    fn entry(title: &str, rel: &str, href: &str) -> FileEntry {
        FileEntry {
            title: title.to_string(),
            rel_md: rel.to_string(),
            href: href.to_string(),
        }
    }

    fn sample() -> CollectionBrowser {
        // Already in listing order: explicit order pulled ch2 ahead of
        // the unordered root file, and ch2/late.md sorts after ch1 in
        // the flat list but belongs in the folder created earlier.
        CollectionBrowser {
            label: "Lab <notes>".to_string(),
            current_rel: Some("ch1/deep.md".to_string()),
            entries: vec![
                entry("Z First", "z-first.md", "z-first.html"),
                entry("Early", "ch2/early.md", "ch2/early.html"),
                entry("A & Unordered", "a-unordered.md", "a.html"),
                entry("Deep", "ch1/deep.md", "../ch1/deep.html"),
                entry("Late", "ch2/late.md", "ch2/late.html"),
                entry("Nested", "ch2/lab/nested.md", "ch2/lab/nested.html"),
            ],
        }
    }

    #[test]
    fn groups_folders_without_resorting_and_marks_current() {
        let html = render_nav(&sample());
        assert_eq!(
            path_texts(&html),
            vec![
                "z-first.md",
                "early.md",
                "late.md",
                "nested.md",
                "a-unordered.md",
                "deep.md",
            ]
        );
        let ch2 = html.find("data-path=\"ch2\"").unwrap();
        let early = html.find(">early.md<").unwrap();
        let nested_dir = html.find("data-path=\"ch2/lab\"").unwrap();
        let nested = html.find(">nested.md<").unwrap();
        let unordered = html.find(">a-unordered.md<").unwrap();
        let ch1 = html.find("data-path=\"ch1\"").unwrap();
        assert!(ch2 < early && early < nested_dir && nested_dir < nested);
        assert!(nested < unordered && unordered < ch1);
        assert!(html.contains("<details class=\"fb-root\" open"));
        assert!(html.contains("<details class=\"fb-folder\" open data-path=\"ch2\">"));
        assert!(html.contains("<summary class=\"fb-summary\">"));
        assert!(html.contains("aria-current=\"page\""));
        assert!(html.contains("class=\"fb-link fb-current\""));
        // The marked row is the open notebook, not merely the first link.
        let marked = html.find("class=\"fb-link fb-current\"").unwrap();
        let marked_path = html[marked..].find(">deep.md<").unwrap();
        assert!(marked_path < 400, "current marker is not on deep.md");
        assert!(!html.contains("onclick"));
        assert!(!html.contains("onchange"));
        assert!(html.contains("Lab &lt;notes&gt;"));
        assert!(!html.contains("A &amp; Unordered"), "row shows the title");
        assert!(!html.contains("Z First"));
        assert!(!html.contains("ch2/early.md"), "row shows the full path");
        assert!(html.contains("href=\"../ch1/deep.html\""));
    }

    #[test]
    fn empty_browser_emits_nothing() {
        let html = render_nav(&CollectionBrowser {
            label: "Empty".into(),
            current_rel: None,
            entries: vec![],
        });
        assert!(html.is_empty());
    }

    #[test]
    fn styles_follow_theme_tokens() {
        let css = styles(Theme::Dark.colors());
        assert!(css.contains("var(--rl-bg-secondary"));
        assert!(css.contains("var(--rl-accent-primary"));
        assert!(css.contains("nav.file-browser"));
        assert!(css.contains(":has(nav.file-browser > details.fb-root:not([open]))"));
        let light = styles(Theme::Light.colors());
        assert_ne!(css, light, "theme fallback colors should differ");
    }
}
