//! Overlay drawer for the `notebook watch` file tree and section outline.
//!
//! Static `render` HTML keeps the fixed columns. Watch pages run
//! [`install_nav_drawer`], which moves those two `<nav>`s into one
//! panel that does not reserve layout width. The panel slides over the
//! page. It closes on Escape, a click outside, or choosing a file or
//! heading. `\` toggles it. Folders stay native `<details>`: they start
//! closed except the ones that contain the open notebook, and clicking
//! a folder summary does not close the drawer.

use crate::render::nonce_attr;

/// Decision for one key or click. `where` is `toggle`, `scrim`, `file`,
/// `section`, `folder`, `inside`, or `outside`. `typing` is an editor,
/// input, or textarea. `menu` is the comment menu or composer.
pub(crate) const DRAWER_SCRIPT: &str = r#"function rlDrawerAction(ev) {
  if (!ev) return 'ignore';
  if (ev.type === 'keydown') {
    if (ev.typing || ev.meta || ev.ctrl || ev.alt) return 'ignore';
    if (ev.key === '\\') return ev.open ? 'close' : 'open';
    if (ev.key === 'Escape') {
      if (!ev.open || ev.menu) return 'ignore';
      return 'close';
    }
    return 'ignore';
  }
  if (ev.type === 'click') {
    if (ev.where === 'toggle') return ev.open ? 'close' : 'open';
    if (!ev.open) return 'ignore';
    if (ev.where === 'file' || ev.where === 'section' || ev.where === 'scrim' || ev.where === 'outside') return 'close';
    return 'ignore';
  }
  return 'ignore';
}
function rlDrawerInitialOpen(search) {
  return /(?:^|[?&])contents=open(?:&|$)/.test(String(search || ''));
}
function rlDrawerCoverPx(drawerRight, contentLeft, viewportWidth) {
  if (!(viewportWidth > 768)) return 0;
  var n = drawerRight - contentLeft;
  if (!(n > 8)) return 0;
  return Math.ceil(n + 12);
}
function rlDrawerWhere(node) {
  if (!node || !node.closest) return 'outside';
  if (node.closest('#rl-drawer-toggle')) return 'toggle';
  if (node.closest('#rl-drawer-scrim')) return 'scrim';
  if (node.closest('#rl-drawer a.fb-link')) return 'file';
  if (node.closest('#rl-drawer nav.sidebar a')) return 'section';
  if (node.closest('#rl-drawer summary')) return 'folder';
  if (node.closest('#rl-drawer')) return 'inside';
  return 'outside';
}
"#;

const DRAWER_DOM: &str = r#"(function () {
  if (typeof document === 'undefined' || !document.getElementById) return;
  var drawer = document.getElementById('rl-drawer');
  var toggle = document.getElementById('rl-drawer-toggle');
  if (!drawer || !toggle) return;
  function menuOpen() {
    var menu = document.getElementById('rl-cm-menu');
    var pop = document.getElementById('rl-cm-pop');
    return (menu && !menu.hidden) || (pop && !pop.hidden);
  }
  function typingTarget(node) {
    return !!(node && node.closest && node.closest('input, textarea, select, [contenteditable="true"], .CodeMirror'));
  }
  function openNow() {
    return document.body.classList.contains('rl-drawer-open');
  }
  function place() {
    var open = openNow();
    toggle.setAttribute('aria-expanded', open ? 'true' : 'false');
    drawer.setAttribute('aria-hidden', open ? 'false' : 'true');
    if (!open) {
      document.body.style.removeProperty('--rl-drawer-cover');
      return;
    }
    var main = document.querySelector('main');
    var left = 0;
    if (main) {
      var box = main.getBoundingClientRect();
      var pad = parseFloat(window.getComputedStyle(main).paddingLeft) || 0;
      left = box.left + pad;
    }
    var cover = rlDrawerCoverPx(drawer.getBoundingClientRect().right, left, window.innerWidth);
    document.body.style.setProperty('--rl-drawer-cover', cover + 'px');
  }
  function setOpen(open, focus) {
    document.body.classList.toggle('rl-drawer-open', open);
    place();
    if (focus === 'drawer' && drawer) drawer.focus();
    else if (focus === 'toggle' && toggle) toggle.focus();
  }
  document.addEventListener('click', function (e) {
    var where = rlDrawerWhere(e.target);
    var action = rlDrawerAction({ type: 'click', where: where, open: openNow() });
    if (action === 'open') setOpen(true, 'drawer');
    else if (action === 'close') setOpen(false, where === 'toggle' ? 'toggle' : null);
  });
  document.addEventListener('keydown', function (e) {
    var action = rlDrawerAction({
      type: 'keydown',
      key: e.key,
      typing: typingTarget(e.target),
      meta: e.metaKey,
      ctrl: e.ctrlKey,
      alt: e.altKey,
      open: openNow(),
      menu: menuOpen()
    });
    if (action === 'ignore') return;
    e.preventDefault();
    if (action === 'open') setOpen(true, 'drawer');
    else if (action === 'close') setOpen(false, 'toggle');
  }, true);
  window.addEventListener('resize', place);
  if (rlDrawerInitialOpen(location.search)) setOpen(true, null);
  else place();
  var prev = window.__rlAfterUpdate;
  window.__rlAfterUpdate = function () {
    if (prev) prev();
    drawer = document.getElementById('rl-drawer');
    toggle = document.getElementById('rl-drawer-toggle');
    if (drawer && toggle) place();
  };
})();
"#;

const DRAWER_CSS: &str = r#"
body.rl-watch { --topbar-h: 2.6rem; --fb-w: 0px; }
body.rl-watch.has-files,
body.rl-watch.has-files:has(nav.file-browser > details.fb-root:not([open])) { --fb-w: 0px; }
body.rl-watch main,
body.rl-watch.has-files main,
body.rl-watch.no-toc main,
body.rl-watch.has-files.no-toc main {
  margin-left: max(0px, calc(50% - 480px));
}
body.rl-watch.rl-source-open main,
body.rl-watch.has-files.rl-source-open main,
body.rl-watch.no-toc.rl-source-open main,
body.rl-watch.has-files.no-toc.rl-source-open main {
  margin-left: 0;
  margin-right: var(--rl-pane-w, 0px);
  max-width: none;
}
.rl-drawer-toggle {
  flex: none;
  margin-right: 0.45rem;
  font: 12px/1 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  background: var(--rl-bg, #1e1e2e);
  color: var(--rl-text, #cdd6f4);
  border: 1px solid var(--rl-border, #313244);
  border-radius: 6px;
  padding: 5px 9px;
  cursor: pointer;
}
body.rl-drawer-open .rl-drawer-toggle {
  background: var(--rl-accent-primary, #cba6f7);
  color: var(--rl-bg, #1e1e2e);
  border-color: var(--rl-accent-primary, #cba6f7);
}
.rl-drawer-bar {
  position: fixed;
  top: 0; left: 0; right: 0;
  height: var(--topbar-h);
  z-index: 320;
  display: flex;
  align-items: center;
  padding: 0 0.6rem;
  background: var(--rl-bg-secondary, #181825);
  border-bottom: 1px solid var(--rl-border, #313244);
}
body.rl-watch:has(.rl-drawer-bar) main {
  padding-top: calc(var(--topbar-h) + 1.25rem);
}
#rl-drawer-scrim {
  position: fixed;
  top: var(--topbar-h);
  left: 0; right: 0; bottom: 0;
  z-index: 230;
  background: rgba(17, 17, 27, 0.28);
  opacity: 0;
  pointer-events: none;
}
body.rl-drawer-open #rl-drawer-scrim { opacity: 1; pointer-events: auto; }
#rl-drawer {
  position: fixed;
  top: var(--topbar-h);
  left: 0;
  bottom: 0;
  width: min(22rem, 88vw);
  z-index: 240;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--rl-bg-secondary, #181825);
  color: var(--rl-text, #cdd6f4);
  border-right: 1px solid var(--rl-border, #313244);
  box-shadow: 8px 0 28px rgba(0, 0, 0, 0.45);
  transform: translateX(-105%);
  visibility: hidden;
  pointer-events: none;
}
body.rl-drawer-open #rl-drawer {
  transform: none;
  visibility: visible;
  pointer-events: auto;
}
#rl-drawer:focus { outline: none; }
#rl-drawer nav.file-browser,
#rl-drawer nav.sidebar {
  position: static;
  top: auto;
  left: auto;
  width: auto;
  height: auto;
  max-height: none;
  transform: none;
  box-shadow: none;
  border-right: 0;
  flex: 0 1 auto;
  overflow: auto;
}
#rl-drawer nav.file-browser {
  max-height: 46vh;
  border-bottom: 1px solid var(--rl-border, #313244);
}
#rl-drawer nav.sidebar { flex: 1 1 auto; }
body.rl-drawer-open .plot-container,
body.rl-drawer-open main h1,
body.rl-drawer-open main h2,
body.rl-drawer-open main h3 {
  margin-left: var(--rl-drawer-cover, 0px);
}
body.rl-drawer-open .plot-container {
  width: calc(100% - var(--rl-drawer-cover, 0px));
  max-width: calc(100% - var(--rl-drawer-cover, 0px));
}
@media (max-width: 768px) {
  body.rl-watch main,
  body.rl-watch.has-files main,
  body.rl-watch.no-toc main,
  body.rl-watch.has-files.no-toc main,
  body.rl-watch.rl-source-open main,
  body.rl-watch.has-files.rl-source-open main {
    margin-left: 0;
    margin-right: 0;
  }
  #rl-drawer nav.file-browser { transform: none; width: auto; }
}
@media (prefers-reduced-motion: reduce) {
  #rl-drawer { transition: none; }
}
"#;

/// Move the watch file tree and section outline into one overlay drawer.
/// Pages with neither nav (and prose that only mentions them inside
/// `<main>`) are unchanged. Static HTML never calls this.
pub fn install_nav_drawer(html: &str, nonce: Option<&str>) -> String {
    if html.contains("id=\"rl-drawer\"") {
        return html.to_string();
    }
    let main_at = content_main_at(html);
    let head = &html[..main_at];
    let has_files = head.contains("<nav class=\"file-browser\"");
    let has_toc = head.contains("<nav class=\"sidebar\">");
    if !has_files && !has_toc {
        return html.to_string();
    }

    let mut page = html.to_string();
    let sidebar = take_before_main(&mut page, "<nav class=\"sidebar\">", "</nav>");
    let _toggle = take_before_main(&mut page, "<button class=\"nav-toggle\"", "</button>");
    let files = take_before_main(&mut page, "<nav class=\"file-browser\"", "</nav>");
    let drawer = format!(
        "<div id=\"rl-drawer-scrim\" class=\"rl-drawer-scrim\"></div>\n\
<div id=\"rl-drawer\" class=\"rl-drawer\" role=\"dialog\" aria-modal=\"false\" aria-label=\"Contents\" aria-hidden=\"true\" tabindex=\"-1\">\n\
{files}{sidebar}</div>\n",
        files = files.unwrap_or_default(),
        sidebar = sidebar.unwrap_or_default(),
    );
    let at = content_main_at(&page);
    if at < page.len() {
        page.insert_str(at, &drawer);
    } else {
        page.push_str(&drawer);
    }
    let button = "<button type=\"button\" id=\"rl-drawer-toggle\" class=\"rl-drawer-toggle\" aria-expanded=\"false\" aria-controls=\"rl-drawer\" title=\"Contents (\\)\">Contents</button>";
    if let Some(header) = page.find("<header class=\"topbar\">") {
        let at = header + "<header class=\"topbar\">".len();
        page.insert_str(at, button);
    } else if let Some(body) = page.find("<body") {
        let at = body + page[body..].find('>').unwrap_or(0) + 1;
        page.insert_str(
            at,
            &format!("<div class=\"rl-drawer-bar\">{button}</div>\n"),
        );
    }
    page = add_body_class(&page, "rl-watch");
    let style = format!("<style>\n{DRAWER_CSS}\n</style>\n");
    if let Some(at) = page.rfind("</head>") {
        page.insert_str(at, &style);
    }
    let script = format!(
        "<script{}>\n{DRAWER_SCRIPT}\n{DRAWER_DOM}\n</script>\n",
        nonce_attr(nonce)
    );
    if let Some(at) = page.rfind("</body>") {
        page.insert_str(at, &script);
    } else {
        page.push_str(&script);
    }
    page
}

/// Index of `pat` outside `<script>`, or `None`.
fn element_at(html: &str, pat: &str) -> Option<usize> {
    let mut i = 0;
    let mut in_script = false;
    while i < html.len() {
        let rest = &html[i..];
        if !in_script && rest.starts_with("<script") {
            in_script = true;
        } else if in_script && rest.starts_with("</script") {
            in_script = false;
        } else if !in_script && rest.starts_with(pat) {
            return Some(i);
        }
        i += rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    None
}

/// The notebook `<main>` element, not the word `<main` inside a script.
fn content_main_at(html: &str) -> usize {
    let mut search_from = 0;
    while let Some(rel) = element_at(&html[search_from..], "<main") {
        let i = search_from + rel;
        let rest = &html[i..];
        if rest.starts_with("<main>")
            || rest.starts_with("<main ")
            || rest.starts_with("<main\n")
            || rest.starts_with("<main\r")
        {
            return i;
        }
        search_from = i + 5;
    }
    html.len()
}

fn take_before_main(html: &mut String, start_pat: &str, end_pat: &str) -> Option<String> {
    let main_at = content_main_at(html);
    let start = html[..main_at].find(start_pat)?;
    let end_rel = html[start..main_at].find(end_pat)?;
    let end = start + end_rel + end_pat.len();
    let chunk = html[start..end].to_string();
    html.replace_range(start..end, "");
    Some(chunk)
}

fn add_body_class(html: &str, class: &str) -> String {
    // The first `<body` in the file can be a word inside a script. The
    // element is the one outside `<script>`.
    let Some(i) = element_at(html, "<body") else {
        return html.to_string();
    };
    let Some(rel) = html[i..].find('>') else {
        return html.to_string();
    };
    let tag_end = i + rel;
    let tag = &html[i..tag_end];
    if let Some(c) = tag.find("class=\"") {
        let at = i + c + "class=\"".len();
        let mut out = String::with_capacity(html.len() + class.len() + 1);
        out.push_str(&html[..at]);
        out.push_str(class);
        out.push(' ');
        out.push_str(&html[at..]);
        out
    } else {
        let mut out = String::with_capacity(html.len() + class.len() + 16);
        out.push_str(&html[..tag_end]);
        out.push_str(" class=\"");
        out.push_str(class);
        out.push('"');
        out.push_str(&html[tag_end..]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        r##"<!DOCTYPE html><html><head><title>t</title></head>
<body class="has-files">
<header class="topbar"><a class="prev" href="/n/a">Prev</a><span class="current">Spectral</span></header>
<nav class="file-browser" aria-label="Lab">
<details class="fb-root" open><summary><span class="fb-label">Lab</span></summary>
<ul class="fb-list">
<li class="fb-dir"><details class="fb-folder" data-path="comm"><summary>comm</summary>
<ul><li><a class="fb-link" href="/n/comm/notes">notes.md</a></li></ul></details></li>
<li><a class="fb-link fb-current" aria-current="page" href="/n/spectral_estimation">spectral_estimation.md</a></li>
</ul></details></nav>
<button class="nav-toggle" type="button" aria-label="Toggle navigation">&#9776;</button>
<nav class="sidebar"><div class="nav-title">Spectral</div>
<a href="#test-signal">Test Signal</a></nav>
<main><h1>Spectral</h1><p>body</p></main>
</body></html>"##
            .to_string()
    }

    #[test]
    fn drawer_wraps_tree_and_outline_and_drops_the_hamburger() {
        let out = install_nav_drawer(&sample(), Some("abc"));
        let main = out.find("<main").unwrap();
        let head = &out[..main];
        assert!(head.contains("id=\"rl-drawer\""));
        assert!(head.contains("id=\"rl-drawer-toggle\""));
        assert!(head.contains("class=\"rl-watch has-files\"") || head.contains("rl-watch"));
        assert!(
            !head.contains("nav-toggle"),
            "watch uses the Contents button"
        );
        let drawer = head.find("id=\"rl-drawer\"").unwrap();
        let files = head.find("class=\"file-browser\"").unwrap();
        let side = head.find("class=\"sidebar\"").unwrap();
        assert!(drawer < files && files < side && side < main);
        assert!(head.contains("data-path=\"comm\""));
        assert!(head.contains("aria-current=\"page\""));
        assert!(out.contains("nonce=\"abc\""));
        assert!(out.contains("rlDrawerAction"));
        assert!(!out.contains("onclick="));
    }

    #[test]
    fn a_script_mention_of_body_does_not_steal_the_class() {
        let html = "<html><head></head><body class=\"has-files\"><script>// on <body> tag</script>\
<nav class=\"sidebar\"><a href=\"#a\">A</a></nav><main><p>x</p></main></body></html>";
        let out = install_nav_drawer(html, None);
        let body = out.find("<body class=").unwrap();
        assert!(
            out[body..].starts_with("<body class=\"rl-watch has-files\">"),
            "{}",
            &out[body..body + 60]
        );
        assert!(out.contains("// on <body> tag"));
    }

    #[test]
    fn a_script_comment_does_not_hide_the_real_main() {
        let html = "<html><head></head><body><script>// inside <main></script>\
<nav class=\"sidebar\"><a href=\"#a\">A</a></nav><main><p>x</p></main></body></html>";
        let out = install_nav_drawer(html, None);
        let drawer = out.find("id=\"rl-drawer\"").expect("drawer");
        let side = out.find("class=\"sidebar\"").unwrap();
        let main = out.find("<main><p>").unwrap();
        assert!(drawer < side && side < main);
        assert!(!out.contains("nav-toggle"));
    }

    #[test]
    fn prose_nav_inside_main_is_not_a_drawer() {
        let html =
            "<html><head></head><body><main><nav class=\"sidebar\">nope</nav></main></body></html>";
        let out = install_nav_drawer(html, None);
        assert!(!out.contains("id=\"rl-drawer\""));
        assert_eq!(out, html);
    }

    #[test]
    fn index_without_a_topbar_gets_a_bar() {
        let html = "<html><head></head><body class=\"has-files\"><nav class=\"file-browser\" aria-label=\"L\"></nav><main><h1>Index</h1></main></body></html>";
        let out = install_nav_drawer(html, None);
        assert!(out.contains("class=\"rl-drawer-bar\""));
        assert!(out.contains("id=\"rl-drawer-toggle\""));
        let bar = out.find("rl-drawer-bar").unwrap();
        let drawer = out.find("id=\"rl-drawer\"").unwrap();
        assert!(bar < drawer);
    }

    #[test]
    fn drawer_script_passes_node_check_and_actions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drawer.js");
        let mut harness = DRAWER_SCRIPT.to_string();
        harness.push_str(
            r#"
const assert = require('assert');
const base = { typing: false, meta: false, ctrl: false, alt: false, menu: false, open: false };
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'keydown', key: '\\' })), 'open');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'keydown', key: '\\', open: true })), 'close');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'keydown', key: '\\', typing: true, open: true })), 'ignore');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'keydown', key: '\\', meta: true })), 'ignore');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'keydown', key: 'Escape', open: true })), 'close');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'keydown', key: 'Escape', open: true, menu: true })), 'ignore');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'keydown', key: 'Escape' })), 'ignore');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'keydown', key: '[' })), 'ignore');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'click', where: 'toggle' })), 'open');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'click', where: 'toggle', open: true })), 'close');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'click', where: 'outside', open: true })), 'close');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'click', where: 'scrim', open: true })), 'close');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'click', where: 'file', open: true })), 'close');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'click', where: 'section', open: true })), 'close');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'click', where: 'folder', open: true })), 'ignore');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'click', where: 'inside', open: true })), 'ignore');
assert.strictEqual(rlDrawerAction(Object.assign({}, base, { type: 'click', where: 'outside' })), 'ignore');
assert.strictEqual(rlDrawerInitialOpen('?contents=open'), true);
assert.strictEqual(rlDrawerInitialOpen('?foo=1&contents=open'), true);
assert.strictEqual(rlDrawerInitialOpen('?contents=open&x=1'), true);
assert.strictEqual(rlDrawerInitialOpen(''), false);
assert.strictEqual(rlDrawerInitialOpen('?contents=closed'), false);
assert.strictEqual(rlDrawerCoverPx(352, 280, 1440), 84);
assert.strictEqual(rlDrawerCoverPx(200, 280, 1440), 0);
assert.strictEqual(rlDrawerCoverPx(300, 10, 390), 0);
"#,
        );
        std::fs::write(&path, harness).unwrap();
        match std::process::Command::new("node").arg(&path).status() {
            Ok(st) => assert!(st.success(), "drawer harness failed"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("SKIP drawer script: node not found");
            }
            Err(e) => panic!("{e}"),
        }
    }
}
