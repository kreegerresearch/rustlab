//! Page chrome injected into server-rendered notebooks: a small
//! toolbar + a slide-in source pane (split view). Phase 5b ships the
//! read-only source pane; Phase 5c upgrades it to a CodeMirror editor
//! when the server runs with `--editable`.
//!
//! The chrome lives *outside* `<main>` (the rendered notebook), so the
//! WS-client's `<main>`-scoped DOM swaps (see `ws.rs`) leave it — and
//! any open editor buffer — untouched across live re-renders. The
//! chrome's JS registers `window.__rlAfterUpdate` so an open read-only
//! pane refreshes itself when a re-render lands.
//!
//! Injection points mirror `ws::inject_ws_client`: a `<style>` before
//! `</head>` and the markup + script before `</body>` (falling back to
//! append when the tags are absent).

use rustlab_plot::ThemeColors;

/// Knobs for [`inject_chrome`].
#[derive(Debug, Clone, Copy)]
pub struct PageOpts {
    /// When true the source pane is an editor that writes back via
    /// `POST /save/<slug>` (wired in Phase 5c). When false it is a
    /// read-only view of the on-disk `.md`.
    pub editable: bool,
}

/// Inject the toolbar + source-pane chrome into a fully-rendered page.
pub fn inject_chrome(html: &str, theme: &ThemeColors, opts: PageOpts) -> String {
    let style = chrome_style(theme);
    let body = chrome_body(opts);

    let with_style = match html.find("</head>") {
        Some(idx) => {
            let (head, rest) = html.split_at(idx);
            format!("{head}{style}{rest}")
        }
        None => format!("{style}{html}"),
    };
    match with_style.rfind("</body>") {
        Some(idx) => {
            let (pre, rest) = with_style.split_at(idx);
            format!("{pre}{body}{rest}")
        }
        None => format!("{with_style}{body}"),
    }
}

fn chrome_style(c: &ThemeColors) -> String {
    format!(
        r##"<style>
  #rl-toolbar {{
    position: fixed; top: 10px; right: 14px; z-index: 100000;
    display: flex; gap: 6px;
  }}
  #rl-toolbar button {{
    font: 12px/1 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    background: {bg_secondary}; color: {text};
    border: 1px solid {border}; border-radius: 6px;
    padding: 6px 11px; cursor: pointer;
  }}
  #rl-toolbar button:hover {{ border-color: {accent}; }}
  #rl-toolbar button.active {{ background: {accent}; color: {bg}; border-color: {accent}; }}
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
  #rl-source-body {{ flex: 1; overflow: auto; }}
  #rl-source-pre {{
    margin: 0; padding: 12px 14px; white-space: pre-wrap; word-break: break-word;
    font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; color: {text};
  }}
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

fn chrome_body(opts: PageOpts) -> String {
    // `EDITABLE` is read by the script; Phase 5c hooks the CodeMirror
    // editor + Save button onto it. In 5b the pane is read-only
    // regardless, so the button always reads "Source".
    let editable_js = if opts.editable { "true" } else { "false" };
    format!(
        r##"<div id="rl-toolbar"><button id="rl-source-toggle" title="Show notebook source">Source</button></div>
<aside id="rl-source-pane" aria-hidden="true">
  <div id="rl-source-head"><span id="rl-source-name">source</span><span id="rl-source-status"></span></div>
  <div id="rl-source-body"><pre id="rl-source-pre"></pre></div>
</aside>
<script>
(() => {{
  // Only notebook pages (`/n/<slug>`) have a source pane; the index has none.
  const m = location.pathname.match(/^\/n\/([^\/]+)\/?$/);
  if (!m) return;
  const slug = m[1];
  const EDITABLE = {editable_js};  // Phase 5c: gates the in-browser editor
  const toggle = document.getElementById('rl-source-toggle');
  const pane = document.getElementById('rl-source-pane');
  const pre = document.getElementById('rl-source-pre');
  let open = false;

  async function loadSource() {{
    try {{
      const r = await fetch('/raw/' + slug, {{ cache: 'no-store' }});
      pre.textContent = await r.text();
    }} catch (e) {{
      pre.textContent = '(failed to load source: ' + e + ')';
    }}
  }}
  function setOpen(v) {{
    open = v;
    document.body.classList.toggle('rl-source-open', open);
    toggle.classList.toggle('active', open);
    pane.setAttribute('aria-hidden', open ? 'false' : 'true');
    if (open) loadSource();
  }}
  toggle.addEventListener('click', () => setOpen(!open));

  // Keep an open read-only pane in sync with re-renders.
  window.__rlAfterUpdate = () => {{ if (open && !EDITABLE) loadSource(); }};
}})();
</script>
"##,
        editable_js = editable_js,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustlab_plot::Theme;

    fn page() -> String {
        let html = "<!doctype html><html><head><title>x</title></head><body><main><p>hi</p></main></body></html>";
        inject_chrome(html, Theme::Dark.colors(), PageOpts { editable: false })
    }

    #[test]
    fn injects_toolbar_and_pane() {
        let out = page();
        assert!(out.contains("id=\"rl-toolbar\""), "toolbar missing");
        assert!(out.contains("id=\"rl-source-pane\""), "source pane missing");
        assert!(out.contains("id=\"rl-source-toggle\""), "toggle button missing");
        assert!(out.contains("/raw/' + slug"), "source fetch missing");
        // Original content survives.
        assert!(out.contains("<main><p>hi</p></main>"));
    }

    #[test]
    fn style_lands_in_head_and_chrome_before_body_close() {
        let out = page();
        let head_close = out.find("</head>").unwrap();
        let style_pos = out.find("#rl-source-pane {").unwrap();
        assert!(style_pos < head_close, "style must be inside <head>");

        let body_close = out.rfind("</body>").unwrap();
        let toolbar_pos = out.find("id=\"rl-toolbar\"").unwrap();
        assert!(toolbar_pos < body_close, "chrome must be before </body>");
    }

    #[test]
    fn editable_flag_threads_into_script() {
        let html = "<head></head><body><main></main></body>";
        let ro = inject_chrome(html, Theme::Dark.colors(), PageOpts { editable: false });
        assert!(ro.contains("EDITABLE = false"));
        let rw = inject_chrome(html, Theme::Dark.colors(), PageOpts { editable: true });
        assert!(rw.contains("EDITABLE = true"));
    }

    #[test]
    fn falls_back_when_no_head_or_body() {
        let out = inject_chrome("<p>bare</p>", Theme::Dark.colors(), PageOpts { editable: false });
        assert!(out.contains("rl-toolbar"));
        assert!(out.contains("<p>bare</p>"));
    }
}
