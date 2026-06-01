//! Page chrome injected into server-rendered notebooks: a small
//! toolbar + a slide-in source pane (split view), and — under
//! `--editable` — an in-browser CodeMirror editor that writes back to
//! the `.md`.
//!
//! The chrome lives *outside* `<main>` (the rendered notebook), so the
//! WS-client's `<main>`-scoped DOM swaps (see `ws.rs`) leave it — and
//! the open editor buffer — untouched across live re-renders. The
//! editor's save path is: edit → `POST /save/<slug>` → the server
//! writes the `.md` → the fs watcher re-renders → the WS push updates
//! only the rendered side. The editor never reloads itself from a
//! re-render, so in-progress edits are never clobbered.
//!
//! Injection points mirror `ws::inject_ws_client`: extra `<head>`
//! content before `</head>` and the markup + script before `</body>`
//! (falling back to append when the tags are absent).

use rustlab_plot::ThemeColors;

/// Knobs for [`inject_chrome`].
#[derive(Debug, Clone, Copy)]
pub struct PageOpts {
    /// When true the source pane is a CodeMirror editor that writes
    /// back via `POST /save/<slug>`; the CodeMirror bundle is linked
    /// from `/assets/codemirror/…`. When false it is a read-only view
    /// of the on-disk `.md` and no editor assets are referenced.
    pub editable: bool,
}

/// Inject the toolbar + source-pane chrome into a fully-rendered page.
pub fn inject_chrome(html: &str, theme: &ThemeColors, opts: PageOpts) -> String {
    let head_extra = head_extra(theme, opts);
    let body_extra = body_extra(opts);

    let with_head = match html.find("</head>") {
        Some(idx) => {
            let (head, rest) = html.split_at(idx);
            format!("{head}{head_extra}{rest}")
        }
        None => format!("{head_extra}{html}"),
    };
    match with_head.rfind("</body>") {
        Some(idx) => {
            let (pre, rest) = with_head.split_at(idx);
            format!("{pre}{body_extra}{rest}")
        }
        None => format!("{with_head}{body_extra}"),
    }
}

/// `<head>` additions: the chrome `<style>`, plus (when editable) the
/// CodeMirror stylesheet link and a small set of dark-theme overrides
/// so the editor matches the page.
fn head_extra(c: &ThemeColors, opts: PageOpts) -> String {
    let mut out = chrome_style(c);
    if opts.editable {
        out.push_str(
            "<link rel=\"stylesheet\" href=\"/assets/codemirror/codemirror.min.css\">\n",
        );
        out.push_str(&editor_style(c));
    }
    out
}

fn chrome_style(c: &ThemeColors) -> String {
    format!(
        r##"<style>
  #rl-toolbar {{
    position: fixed; top: 10px; right: 14px; z-index: 100000;
    display: flex; gap: 6px; align-items: center;
  }}
  /* Directory-mode pages carry a sticky breadcrumb topbar; reserve room
     on its right so the title doesn't slide under the fixed toolbar. */
  body.topbar-layout .topbar {{ padding-right: 9rem; }}
  #rl-toolbar button {{
    font: 12px/1 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    background: {bg_secondary}; color: {text};
    border: 1px solid {border}; border-radius: 6px;
    padding: 6px 11px; cursor: pointer;
  }}
  #rl-toolbar button:hover {{ border-color: {accent}; }}
  #rl-toolbar button.active {{ background: {accent}; color: {bg}; border-color: {accent}; }}
  #rl-toolbar button[hidden] {{ display: none; }}
  #rl-source-pane {{
    position: fixed; top: 0; right: 0; height: 100vh;
    width: 42vw; min-width: 360px; max-width: 760px;
    background: {code_bg}; border-left: 1px solid {border};
    z-index: 99998; display: flex; flex-direction: column;
    transform: translateX(101%); transition: transform .18s ease;
    box-shadow: -8px 0 24px rgba(0,0,0,.25);
  }}
  body.rl-source-open #rl-source-pane {{ transform: translateX(0); }}
  body.rl-source-open main {{ margin-right: 42vw; max-width: none; }}
  #rl-source-head {{
    padding: 9px 14px; border-bottom: 1px solid {border};
    font: 12px/1.3 -apple-system, system-ui, sans-serif; color: {text_dim};
    display: flex; justify-content: space-between; align-items: center; gap: 10px;
  }}
  #rl-source-status {{ color: {text_dim}; font-size: 11px; }}
  #rl-source-body {{ flex: 1; overflow: auto; min-height: 0; }}
  #rl-source-pre {{
    margin: 0; padding: 12px 14px; white-space: pre-wrap; word-break: break-word;
    font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; color: {text};
  }}
  #rl-editor-host {{ height: 100%; }}
  @media (max-width: 768px) {{
    body.rl-source-open main {{ margin-right: 0; }}
    #rl-source-pane {{ width: 100vw; max-width: none; }}
  }}
</style>
"##,
        bg = c.bg,
        bg_secondary = c.bg_secondary,
        text = c.text,
        text_dim = c.text_dim,
        border = c.border,
        accent = c.accent_primary,
        code_bg = c.code_bg,
    )
}

/// Dark-theme overrides so the (light-by-default) CodeMirror editor
/// blends with the page. Only emitted in editable mode.
fn editor_style(c: &ThemeColors) -> String {
    format!(
        r##"<style>
  #rl-source-pane .CodeMirror {{
    height: 100%; background: {code_bg}; color: {text};
    font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  }}
  #rl-source-pane .CodeMirror-gutters {{ background: {bg_secondary}; border-right: 1px solid {border}; }}
  #rl-source-pane .CodeMirror-linenumber {{ color: {text_dim}; }}
  #rl-source-pane .CodeMirror-cursor {{ border-left: 1px solid {text}; }}
  #rl-source-pane .CodeMirror-selected {{ background: {border}; }}
  #rl-source-pane .CodeMirror-activeline-background {{ background: {bg_secondary}; }}
</style>
"##,
        bg_secondary = c.bg_secondary,
        text = c.text,
        text_dim = c.text_dim,
        border = c.border,
        code_bg = c.code_bg,
    )
}

/// `<body>` additions: toolbar + pane markup, the CodeMirror scripts
/// (editable only), and the chrome controller script.
fn body_extra(opts: PageOpts) -> String {
    let editable_js = if opts.editable { "true" } else { "false" };

    // Toolbar: read-only mode shows just "Source"; editable mode shows
    // "Edit" plus a Save button (hidden until the editor is open).
    let toolbar = if opts.editable {
        r#"<div id="rl-toolbar"><button id="rl-source-save" hidden title="Save (Ctrl/Cmd-S)">Save</button><button id="rl-source-toggle" title="Edit notebook source">Edit</button></div>"#
    } else {
        r#"<div id="rl-toolbar"><button id="rl-source-toggle" title="Show notebook source">Source</button></div>"#
    };

    // Pane body: editor host (editable) or read-only <pre>.
    let pane_body = if opts.editable {
        r#"<div id="rl-source-body"><div id="rl-editor-host"></div></div>"#
    } else {
        r#"<div id="rl-source-body"><pre id="rl-source-pre"></pre></div>"#
    };

    // CodeMirror bundle, loaded synchronously before the controller so
    // `CodeMirror` is defined when the script runs.
    let cm_scripts = if opts.editable {
        "<script src=\"/assets/codemirror/codemirror.min.js\"></script>\n\
         <script src=\"/assets/codemirror/mode/markdown/markdown.min.js\"></script>\n"
    } else {
        ""
    };

    format!(
        r##"{toolbar}
<aside id="rl-source-pane" aria-hidden="true">
  <div id="rl-source-head"><span id="rl-source-name">source</span><span id="rl-source-status"></span></div>
  {pane_body}
</aside>
{cm_scripts}<script>
(() => {{
  // Only notebook pages (`/n/<slug>`) have a source pane; the index has none.
  const m = location.pathname.match(/^\/n\/([^\/]+)\/?$/);
  if (!m) return;
  const slug = m[1];
  const EDITABLE = {editable_js};
  const toggle = document.getElementById('rl-source-toggle');
  const pane = document.getElementById('rl-source-pane');
  const status = document.getElementById('rl-source-status');
  const saveBtn = document.getElementById('rl-source-save');
  let open = false;
  let cm = null;

  function setStatus(t) {{ if (status) status.textContent = t; }}

  async function fetchSource() {{
    const r = await fetch('/raw/' + slug, {{ cache: 'no-store' }});
    if (!r.ok) throw new Error('HTTP ' + r.status);
    return await r.text();
  }}

  // ── Read-only pane ────────────────────────────────────────────────
  async function loadReadonly() {{
    const pre = document.getElementById('rl-source-pre');
    try {{ pre.textContent = await fetchSource(); }}
    catch (e) {{ pre.textContent = '(failed to load source: ' + e + ')'; }}
  }}

  // ── Editor (CodeMirror, editable mode) ────────────────────────────
  async function ensureEditor() {{
    if (cm || typeof CodeMirror === 'undefined') return;
    let text = '';
    try {{ text = await fetchSource(); }}
    catch (e) {{ setStatus('load failed'); }}
    cm = CodeMirror(document.getElementById('rl-editor-host'), {{
      value: text, mode: 'markdown', lineNumbers: true, lineWrapping: true,
      extraKeys: {{ 'Cmd-S': save, 'Ctrl-S': save }},
    }});
    cm.setSize('100%', '100%');
  }}

  async function save() {{
    if (!cm) return;
    setStatus('saving…');
    try {{
      const r = await fetch('/save/' + slug, {{
        method: 'POST',
        headers: {{ 'Content-Type': 'text/markdown' }},
        body: cm.getValue(),
      }});
      if (r.ok) {{ cm.markClean(); setStatus('saved ✓'); }}
      else {{ setStatus('save failed (' + r.status + ')'); }}
    }} catch (e) {{ setStatus('save failed'); }}
    setTimeout(() => {{ if ((status || {{}}).textContent === 'saved ✓') setStatus(''); }}, 1800);
  }}

  async function setOpen(v) {{
    open = v;
    document.body.classList.toggle('rl-source-open', open);
    toggle.classList.toggle('active', open);
    pane.setAttribute('aria-hidden', open ? 'false' : 'true');
    if (saveBtn) saveBtn.hidden = !open;
    if (!open) return;
    if (EDITABLE) {{ await ensureEditor(); if (cm) cm.refresh(); }}
    else {{ await loadReadonly(); }}
  }}

  toggle.addEventListener('click', () => setOpen(!open));
  if (saveBtn) saveBtn.addEventListener('click', save);
  // Global Ctrl/Cmd-S also saves while the pane is open (covers focus
  // outside the editor; CodeMirror's extraKeys covers focus inside).
  if (EDITABLE) {{
    document.addEventListener('keydown', (e) => {{
      if (open && (e.metaKey || e.ctrlKey) && (e.key === 's' || e.key === 'S')) {{
        e.preventDefault(); save();
      }}
    }});
  }}

  // Read-only panes refresh on re-render; the editor keeps its buffer.
  window.__rlAfterUpdate = () => {{ if (open && !EDITABLE) loadReadonly(); }};
  // Veto the WS-client's reconnect hard-reload while the editor has
  // unsaved changes, so a transient disconnect can't discard them.
  window.__rlBlockReload = () => EDITABLE && cm != null && !cm.isClean();
}})();
</script>
"##,
        toolbar = toolbar,
        pane_body = pane_body,
        cm_scripts = cm_scripts,
        editable_js = editable_js,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustlab_plot::Theme;

    fn render(editable: bool) -> String {
        let html = "<!doctype html><html><head><title>x</title></head><body><main><p>hi</p></main></body></html>";
        inject_chrome(html, Theme::Dark.colors(), PageOpts { editable })
    }

    #[test]
    fn readonly_injects_toolbar_and_pre_pane() {
        let out = render(false);
        assert!(out.contains("id=\"rl-toolbar\""));
        assert!(out.contains("id=\"rl-source-pane\""));
        assert!(out.contains("id=\"rl-source-pre\""), "read-only pane uses a <pre>");
        assert!(out.contains(">Source<"), "button reads Source in read-only mode");
        assert!(out.contains("/raw/' + slug"));
        // No editor assets referenced.
        assert!(!out.contains("/assets/codemirror/"), "no CodeMirror in read-only mode");
        assert!(out.contains("<main><p>hi</p></main>"));
    }

    #[test]
    fn editable_injects_codemirror_and_save() {
        let out = render(true);
        assert!(out.contains("id=\"rl-editor-host\""), "editor host present");
        assert!(out.contains("id=\"rl-source-save\""), "Save button present");
        assert!(out.contains(">Edit<"), "button reads Edit in editable mode");
        assert!(out.contains("/assets/codemirror/codemirror.min.js"));
        assert!(out.contains("/assets/codemirror/codemirror.min.css"));
        assert!(out.contains("/assets/codemirror/mode/markdown/markdown.min.js"));
        assert!(out.contains("/save/' + slug"), "save POST target present");
        assert!(out.contains("CodeMirror("), "editor is constructed");
        // Reconnect-reload guard + clean-on-save protect unsaved edits.
        assert!(out.contains("__rlBlockReload"), "reconnect-reload veto present");
        assert!(out.contains("markClean"), "buffer marked clean after save");
        // The read-only <pre> element is absent in editable mode (the JS
        // still defines a loadReadonly helper, so check the element markup).
        assert!(!out.contains("id=\"rl-source-pre\""), "no read-only <pre> element in editable mode");
    }

    #[test]
    fn editable_flag_threads_into_script() {
        assert!(render(false).contains("EDITABLE = false"));
        assert!(render(true).contains("EDITABLE = true"));
    }

    #[test]
    fn style_lands_in_head_and_chrome_before_body_close() {
        let out = render(true);
        let head_close = out.find("</head>").unwrap();
        assert!(out.find("#rl-source-pane {").unwrap() < head_close, "style in <head>");
        assert!(out.find("/assets/codemirror/codemirror.min.css").unwrap() < head_close,
            "editor stylesheet link in <head>");
        let body_close = out.rfind("</body>").unwrap();
        assert!(out.find("id=\"rl-toolbar\"").unwrap() < body_close, "chrome before </body>");
    }

    #[test]
    fn falls_back_when_no_head_or_body() {
        let out = inject_chrome("<p>bare</p>", Theme::Dark.colors(), PageOpts { editable: false });
        assert!(out.contains("rl-toolbar"));
        assert!(out.contains("<p>bare</p>"));
    }
}
