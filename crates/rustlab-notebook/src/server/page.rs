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
    /// `watch --git-diff`. When false Show changes is disabled and the
    /// page never asks for `/git/refs`.
    pub git_diff: bool,
}

/// Inject the toolbar + source-pane chrome into a fully-rendered page.
pub fn inject_chrome(html: &str, theme: &ThemeColors, opts: PageOpts) -> String {
    inject_chrome_nonced(html, theme, opts, None)
}

/// [`inject_chrome`] with the server's CSP nonce on the chrome's own
/// `<script>` tags (CodeMirror bundles + the pane controller).
pub fn inject_chrome_nonced(
    html: &str,
    theme: &ThemeColors,
    opts: PageOpts,
    nonce: Option<&str>,
) -> String {
    let head_extra = head_extra(theme, opts);
    let body_extra = body_extra(opts, nonce);

    let with_head = match html.find("</head>") {
        Some(idx) => {
            let (head, rest) = html.split_at(idx);
            format!("{head}{head_extra}{rest}")
        }
        None => format!("{head_extra}{html}"),
    };
    let page = match with_head.rfind("</body>") {
        Some(idx) => {
            let (pre, rest) = with_head.split_at(idx);
            format!("{pre}{body_extra}{rest}")
        }
        None => format!("{with_head}{body_extra}"),
    };
    let page = hoist_comments_toggle(&page);
    crate::server::drawer::install_nav_drawer(&page, nonce)
}

/// Move the header Comments checkbox into the ⋯ panel.
///
/// Static HTML keeps the checkbox in `header.topbar`. Watch is the only
/// page with the menu, so the hoist runs here. The input keeps
/// `id="rl-comments"`: the margin CSS is `body:has(#rl-comments:not(:checked))`
/// and the saved choice is still `rl-comments-visible`. The status span
/// stays outside `<main>` so a saved-while-hidden note still has a place
/// to land.
fn hoist_comments_toggle(html: &str) -> String {
    const START: &str = "<label class=\"rl-comments-toggle\">";
    const PANEL: &str = "<div id=\"rl-more-panel\" class=\"rl-more-panel\" hidden role=\"region\" aria-label=\"More\">";
    let Some(i) = html.find(START) else {
        return html.to_string();
    };
    let Some(rel_end) = html[i..].find("</label>") else {
        return html.to_string();
    };
    let end = i + rel_end + "</label>".len();
    let Some(panel) = html.find(PANEL) else {
        return html.to_string();
    };
    let checked = html[i..end].contains(" checked");
    let switch = comments_menu_switch(checked);
    let mut out = String::with_capacity(html.len() + switch.len());
    out.push_str(&html[..i]);
    out.push_str(&html[end..]);
    // The panel tag is after the label in document order, so dropping
    // the label shifts its index.
    let panel = if panel >= end {
        panel - (end - i)
    } else {
        panel
    };
    let at = panel + PANEL.len();
    if at > out.len() || !out[panel..].starts_with(PANEL) {
        return html.to_string();
    }
    out.insert_str(at, &switch);
    if !checked && out.contains("<span id=\"rl-diff-dot\" class=\"rl-diff-dot\" hidden>") {
        out = out.replace(
            "<span id=\"rl-diff-dot\" class=\"rl-diff-dot\" hidden>",
            "<span id=\"rl-diff-dot\" class=\"rl-diff-dot\">",
        );
        out = out.replace(
            "aria-label=\"More\" title=\"More\"",
            "aria-label=\"More, comments off\" title=\"More, comments off\"",
        );
    }
    out
}

fn comments_menu_switch(checked: bool) -> String {
    let attr = if checked { " checked" } else { "" };
    format!(
        "<label class=\"rl-comments-toggle rl-menu-switch\"><input type=\"checkbox\" id=\"rl-comments\" role=\"switch\"{attr}><span class=\"rl-switch\" aria-hidden=\"true\"></span>Comments</label>"
    )
}

/// `<head>` additions: the chrome `<style>`, plus (when editable) the
/// CodeMirror stylesheet link and a small set of dark-theme overrides
/// so the editor matches the page.
fn head_extra(c: &ThemeColors, opts: PageOpts) -> String {
    let mut out = chrome_style(c);
    if opts.editable {
        out.push_str("<link rel=\"stylesheet\" href=\"/assets/codemirror/codemirror.min.css\">\n");
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
  /* Every served page carries the topbar; reserve room on its right so the
     "Next" link and title don't slide under the fixed toolbar. This used to
     be scoped to `body.topbar-layout`, which no longer exists — leaving the
     toolbar sitting on top of the next-notebook link, unclickable. */
  .topbar {{ padding-right: 9rem; }}
  #rl-toolbar {{ flex-wrap: wrap; justify-content: flex-end; max-width: min(70vw, 40rem); }}
  #rl-toolbar button {{
    font: 12px/1 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    background: {bg_secondary}; color: {text};
    border: 1px solid {border}; border-radius: 6px;
    padding: 6px 11px; cursor: pointer;
  }}
  #rl-toolbar button:hover {{ border-color: {accent}; }}
  #rl-toolbar button.active {{ background: {accent}; color: {bg}; border-color: {accent}; }}
  #rl-toolbar button[hidden] {{ display: none; }}
  #rl-toolbar button.rl-quiet {{ opacity: 0.72; padding: 4px 8px; }}
  .rl-more {{ position: relative; display: inline-flex; align-items: center; }}
  #rl-more-btn {{ min-width: 2rem; letter-spacing: 0.08em; }}
  .rl-diff-dot {{
    position: absolute; top: -1px; right: -1px; width: 7px; height: 7px;
    border-radius: 50%; background: var(--rl-dx-mod-bar, {accent}); pointer-events: none;
  }}
  .rl-more-panel {{
    position: absolute; top: calc(100% + 6px); right: 0; z-index: 100002;
    display: flex; flex-direction: column; gap: 8px; align-items: stretch;
    min-width: 16rem; padding: 8px;
    background: {bg_secondary}; color: {text};
    border: 1px solid {border}; border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0,0,0,.28);
  }}
  #rl-toolbar #rl-diff-toggle {{
    display: flex; align-items: center; gap: 8px; width: 100%; text-align: left;
    background: transparent; color: {text};
  }}
  #rl-toolbar #rl-diff-toggle.active {{ background: transparent; color: {text}; border-color: {border}; }}
  .rl-switch {{
    width: 28px; height: 16px; border-radius: 99px; flex: none; position: relative;
    border: 1px solid {border}; background: {bg};
  }}
  .rl-switch::after {{
    content: ""; position: absolute; top: 1px; left: 1px;
    width: 12px; height: 12px; border-radius: 50%; background: {text};
  }}
  #rl-diff-toggle[aria-checked="true"] .rl-switch {{ background: {accent}; border-color: {accent}; }}
  #rl-diff-toggle[aria-checked="true"] .rl-switch::after {{ left: 13px; background: {bg}; }}
  #rl-toolbar #rl-diff-toggle:disabled {{ opacity: 0.45; cursor: not-allowed; }}
  /* In-panel switches stay full contrast. A disabled Show changes stays dim,
     and the ⋯ button keeps .rl-quiet. */
  #rl-toolbar #rl-more-panel .rl-quiet:not(:disabled),
  #rl-toolbar .rl-menu-switch {{ opacity: 1; }}
  #rl-toolbar .rl-menu-switch {{
    position: relative;
    display: flex; align-items: center; gap: 8px; width: 100%;
    box-sizing: border-box; text-align: left;
    font: 12px/1 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    background: transparent; color: {text};
    border: 1px solid transparent; border-radius: 6px;
    padding: 6px 8px; cursor: pointer;
  }}
  #rl-toolbar .rl-menu-switch input {{
    position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px;
    overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0;
  }}
  #rl-comments:checked + .rl-switch {{ background: {accent}; border-color: {accent}; }}
  #rl-comments:checked + .rl-switch::after {{ left: 13px; background: {bg}; }}
  #rl-comments:focus-visible + .rl-switch {{
    outline: 2px solid {accent}; outline-offset: 2px;
  }}
  #rl-cm-confirm:empty {{ display: none !important; }}
  #rl-cm-confirm:not(:empty) {{
    position: fixed; top: 46px; right: 14px; z-index: 100001; margin: 0;
    padding: 4px 8px; max-width: 22rem;
    background: {bg_secondary}; color: {text};
    border: 1px solid {border}; border-radius: 6px;
    font: 12px/1.35 system-ui, sans-serif;
  }}
  p.rl-git-note {{ margin: 0; font: 11px/1.35 system-ui, sans-serif; color: {text_dim}; }}
  body.rl-diff-on .rl-more-panel {{ width: 22rem; max-height: calc(100vh - 64px); overflow: auto; }}
  #rl-diff {{ display: flex; flex-direction: column; gap: 4px; align-items: stretch; }}
  #rl-diff[hidden], #rl-diff-banner[hidden], .rl-diff-menu[hidden], #rl-dx-other[hidden],
  .rl-dx-opt[hidden], .rl-more-panel[hidden], #rl-diff-dot[hidden] {{ display: none !important; }}
  .rl-dx-hd {{
    margin: 8px 0 2px; font: 600 11px/1.2 system-ui, sans-serif; color: {text_dim};
    text-transform: uppercase; letter-spacing: 0.06em;
  }}
  #rl-toolbar .rl-dx-opt {{
    display: grid; grid-template-columns: 14px 1fr auto; gap: 2px 8px; align-items: start;
    width: 100%; text-align: left; padding: 6px 8px; border-radius: 6px;
    background: transparent; color: {text}; border: 1px solid transparent;
    font: 13px/1.35 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  }}
  #rl-toolbar .rl-dx-opt:hover {{ border-color: {border}; }}
  #rl-toolbar .rl-dx-opt[aria-checked="true"] {{ border-color: {accent}; }}
  #rl-toolbar .rl-dx-opt:disabled {{ opacity: 0.55; cursor: not-allowed; }}
  .rl-dx-dot {{
    width: 10px; height: 10px; margin-top: 3px; border-radius: 50%;
    border: 1.5px solid {text_dim}; box-sizing: border-box;
  }}
  .rl-dx-opt[aria-checked="true"] .rl-dx-dot {{ border-color: {accent}; background: {accent}; box-shadow: inset 0 0 0 2px {bg_secondary}; }}
  .rl-dx-t {{ font-weight: 600; }}
  .rl-dx-c {{ color: {text_dim}; font-size: 12px; white-space: nowrap; }}
  .rl-dx-d {{ grid-column: 2 / 4; color: {text_dim}; font-size: 12px; }}
  #rl-dx-other {{ display: flex; flex-direction: column; gap: 4px; margin: 2px 0 4px 22px; }}
  #rl-diff input {{
    font: 12px/1.2 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    background: {bg}; color: {text};
    border: 1px solid {border}; border-radius: 6px;
    padding: 4px 6px; width: 100%; box-sizing: border-box;
  }}
  #rl-diff input:focus {{ border-color: {accent}; outline: none; }}
  .rl-diff-lab {{ font: 12px/1 system-ui, sans-serif; color: {text_dim}; display: flex; gap: 6px; align-items: center; }}
  .rl-dx-lab {{ width: 3.8rem; flex: none; }}
  .rl-diff-combo {{ position: relative; display: flex; flex: 1; min-width: 0; }}
  .rl-dx-arrow, .rl-dx-hint, .rl-dx-keys {{ margin: 0; font: 11px/1.35 system-ui, sans-serif; color: {text_dim}; }}
  .rl-dx-arrow {{ text-align: center; }}
  .rl-diff-menu {{
    margin: 0; padding: 2px; list-style: none;
    max-height: 14rem; overflow: auto;
    background: {bg}; color: {text};
    border: 1px solid {border}; border-radius: 6px;
    font: 12px/1.35 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  }}
  .rl-diff-menu li {{ padding: 3px 6px; border-radius: 4px; cursor: pointer; display: flex; justify-content: space-between; gap: 10px; }}
  .rl-diff-menu li span {{ overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
  .rl-diff-menu li .rl-m {{ color: {text_dim}; flex: none; }}
  .rl-diff-menu li[aria-selected="true"] {{ background: {accent}; color: {bg}; }}
  .rl-diff-menu li[aria-selected="true"] .rl-m {{ color: {bg}; }}
  .rl-diff-menu li.rl-diff-hint {{ color: {text_dim}; cursor: default; }}
  .rl-diff-menu li.rl-dx-group {{
    cursor: default; padding-top: 6px; color: {text_dim};
    font: 600 10px/1.2 system-ui, sans-serif; text-transform: uppercase; letter-spacing: 0.05em;
  }}
  #rl-dx-list {{ list-style: none; margin: 0; padding: 0; font: 12px/1.35 system-ui, sans-serif; }}
  #rl-dx-list li {{
    display: grid; grid-template-columns: 4px 5.2rem 1fr; gap: 8px; align-items: center;
    padding: 3px 6px; border-radius: 5px; cursor: pointer;
  }}
  #rl-dx-list li:hover, #rl-dx-list li.rl-cur {{ background: {bg}; }}
  #rl-dx-list li:focus-visible {{ outline: 2px solid {accent}; outline-offset: -2px; }}
  #rl-dx-list li i {{ display: block; width: 4px; height: 14px; border-radius: 2px; }}
  #rl-dx-list li .rl-k {{ color: {text_dim}; }}
  #rl-dx-list li.rl-empty {{ display: block; color: {text_dim}; cursor: default; }}
  .rl-dx-keys kbd {{
    font: 11px ui-monospace, monospace; border: 1px solid {border}; border-radius: 3px; padding: 0 4px;
  }}
  #rl-diff-banner {{
    position: fixed; top: 46px; right: 14px; z-index: 99999; margin: 0;
    padding: 6px 10px; max-width: 30rem; text-align: left; cursor: pointer;
    background: {bg_secondary}; color: {text};
    border: 1px solid {border}; border-radius: 8px;
    font: 12px/1.35 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  }}
  #rl-diff-banner:hover {{ border-color: {accent}; }}
  #rl-diff-banner .rl-k {{ color: {text_dim}; }}
  #rl-diff-banner .rl-n {{ color: var(--rl-dx-mod-bar, {accent}); font-weight: 600; }}
  .rl-diff-notice {{
    margin: 0 0 1rem; padding: 0.6rem 0.85rem;
    background: {bg_secondary}; color: {text};
    border: 1px solid {border}; border-left: 4px solid {accent};
    border-radius: 6px;
    font: 14px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  }}
  /* Changes are a bar in the gutter left of the text. No wash, so body
     text keeps its own colors. A code cell's accent rule is on its left
     edge, so the bar sits a gutter further out. */
  main .rl-dx {{ position: relative; }}
  main .rl-dx::before {{
    content: ""; position: absolute; left: -16px; top: 2px; bottom: 2px;
    width: 4px; border-radius: 2px; pointer-events: none;
  }}
  main section.rl-dx::before {{ left: -22px; }}
  main .rl-dx-add::before {{ background: var(--rl-dx-add-bar); }}
  main .rl-dx-mod::before {{ background: var(--rl-dx-mod-bar); }}
  main .rl-dx-del::before {{ background: var(--rl-dx-del-bar); }}
  main details.rl-dx-removed {{ margin: 0.6rem 0; }}
  main details.rl-dx-removed > summary {{
    cursor: pointer; list-style: none; color: {text_dim};
    font: 13px/1.4 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  }}
  main details.rl-dx-removed > summary::-webkit-details-marker {{ display: none; }}
  main details.rl-dx-removed > summary::before {{ content: "▸ "; color: var(--rl-dx-del-bar); }}
  main details.rl-dx-removed[open] > summary::before {{ content: "▾ "; }}
  main details.rl-dx-removed > summary b {{ color: {text}; font-weight: 600; }}
  main .rl-dx-gone {{
    text-decoration: line-through; text-decoration-color: var(--rl-dx-del-bar);
  }}
  main .rl-dx-gone pre {{ text-decoration: inherit; }}
  .rl-dx-note {{ margin: 4px 0 0; font: 12px/1.3 -apple-system, system-ui, sans-serif; color: {text_dim}; }}
  main pre.rl-dx-code code {{ display: block; }}
  main pre.rl-dx-code .rl-ln {{ display: block; white-space: pre-wrap; padding-right: 0.6em; }}
  main pre.rl-dx-code .rl-ln-g {{
    display: inline-block; width: 1.8em; text-align: center; color: {text_dim}; user-select: none;
  }}
  main pre.rl-dx-code .rl-ln-add {{ background: var(--rl-dx-add-line); box-shadow: inset 2px 0 0 var(--rl-dx-add-bar); }}
  main pre.rl-dx-code .rl-ln-del {{ background: var(--rl-dx-del-line); box-shadow: inset 2px 0 0 var(--rl-dx-del-bar); }}
  main pre.rl-dx-code .rl-ln-add .rl-ln-g {{ color: var(--rl-dx-add-bar); }}
  main pre.rl-dx-code .rl-ln-del .rl-ln-g {{ color: var(--rl-dx-del-bar); }}
  main pre.rl-dx-code .rl-ln-del {{ text-decoration: line-through; text-decoration-color: var(--rl-dx-del-bar); }}
  main details.rl-dx-src > summary {{ cursor: pointer; color: {text_dim}; font: 12px/1.3 system-ui, sans-serif; }}
  /* Word marks. Prose words get a tint that keeps body text at 4.5:1, and
     take the body text color so a link inside stays readable. Headings
     and code keep their own colors and get a 2px line instead. */
  main ins.rl-w, main del.rl-w {{
    color: {text}; border-radius: 3px; padding: 0 1px;
    -webkit-box-decoration-break: clone; box-decoration-break: clone;
  }}
  main ins.rl-w {{ background: var(--rl-dx-add-word); text-decoration: none; }}
  main del.rl-w {{
    background: var(--rl-dx-del-word);
    text-decoration: line-through; text-decoration-color: var(--rl-dx-del-bar);
  }}
  main ins.rl-w a, main del.rl-w a {{ color: inherit; }}
  main ins.rl-w-all {{ padding: 1px 2px; }}
  main :is(h1, h2, h3, h4, h5, h6) :is(ins, del).rl-w,
  main pre.rl-dx-code :is(ins, del).rl-w,
  main .rl-dx-srcview :is(ins, del).rl-w {{
    background: none; color: inherit; padding: 0; border-radius: 0;
    text-decoration-thickness: 2px; text-underline-offset: 3px;
  }}
  main :is(h1, h2, h3, h4, h5, h6) ins.rl-w,
  main pre.rl-dx-code ins.rl-w,
  main .rl-dx-srcview ins.rl-w {{
    text-decoration-line: underline; text-decoration-color: var(--rl-dx-add-bar);
  }}
  main :is(h1, h2, h3, h4, h5, h6) del.rl-w {{
    text-decoration-line: line-through; text-decoration-color: var(--rl-dx-del-bar);
  }}
  main pre.rl-dx-code del.rl-w, main .rl-dx-srcview del.rl-w {{
    text-decoration-line: underline line-through; text-decoration-color: var(--rl-dx-del-bar);
  }}
  /* Source view: the unified diff of the .md. */
  main .rl-dx-srcview {{
    margin: 3rem 0 1rem; overflow: hidden;
    background: var(--rl-code-bg, {code_bg}); color: {text};
    border: 1px solid {border}; border-radius: 8px;
    font: 13px/1.55 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  }}
  main .rl-dx-srcview .rl-dx::before {{ display: none; }}
  main .rl-dx-fh {{
    padding: 8px 12px; border-bottom: 1px solid {border}; background: {bg_secondary}; color: {text_dim};
    font: 13px/1.35 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  }}
  main .rl-dx-hunk {{ padding: 3px 12px; background: {bg_secondary}; color: {text_dim}; font-size: 12px; }}
  main .rl-dx-l {{ display: grid; grid-template-columns: 3.4em 3.4em 1.8em 1fr; }}
  main .rl-dx-n {{ color: {text_dim}; text-align: right; padding-right: 6px; user-select: none; }}
  main .rl-dx-s {{ text-align: center; color: {text_dim}; user-select: none; }}
  main .rl-dx-x {{ white-space: pre-wrap; word-break: break-word; padding-right: 12px; }}
  main .rl-dx-l-add {{ background: var(--rl-dx-add-line); box-shadow: inset 2px 0 0 var(--rl-dx-add-bar); }}
  main .rl-dx-l-del {{ background: var(--rl-dx-del-line); box-shadow: inset 2px 0 0 var(--rl-dx-del-bar); }}
  main .rl-dx-l-add .rl-dx-s {{ color: var(--rl-dx-add-bar); }}
  main .rl-dx-l-del .rl-dx-s {{ color: var(--rl-dx-del-bar); }}
  main .rl-dx-none {{ padding: 8px 12px; }}
  .rl-dx-seg {{ display: inline-flex; align-self: flex-start; border: 1px solid {border}; border-radius: 6px; overflow: hidden; }}
  #rl-toolbar .rl-dx-seg button {{
    border: 0; border-radius: 0; background: transparent; color: {text}; padding: 4px 12px;
  }}
  #rl-toolbar .rl-dx-seg button[aria-checked="true"] {{ background: {accent}; color: {bg}; font-weight: 600; }}
  .fb-git {{
    display: inline-block; width: 7px; height: 7px; border-radius: 50%;
    margin-left: 6px; background: var(--rl-dx-mod-bar, currentColor); vertical-align: middle;
  }}
  body {{ --rl-pane-w: min(760px, max(360px, 42vw)); }}
  /* A comment column needs the pane narrower, or the fixed pane paints
     over the margin. The column starts at 14rem and can be dragged;
     the pane gives up width. */
  body.has-cm-margin {{ --rl-pane-w: min(560px, max(280px, 32vw)); }}
  #rl-source-pane {{
    position: fixed; top: 0; right: 0; height: 100vh;
    width: var(--rl-pane-w); min-width: 0; max-width: var(--rl-pane-w);
    background: {code_bg}; border-left: 1px solid {border};
    z-index: 99998; display: flex; flex-direction: column;
    transform: translateX(101%); transition: transform .18s ease;
    box-shadow: -8px 0 24px rgba(0,0,0,.25);
  }}
  body.rl-source-open #rl-source-pane {{ transform: translateX(0); }}
  /* With the pane open, main's viewport-centring margin (50% of the FULL
     width) would ignore the pane and squeeze the content against it —
     pin the margin to the pane width instead. */
  body.rl-source-open main {{ margin-left: 220px; margin-right: var(--rl-pane-w); max-width: none; }}
  body.no-toc.rl-source-open main {{ margin-left: 0; }}
  body.has-files.rl-source-open main {{ margin-left: calc(220px + var(--fb-w, 16.5rem)); }}
  body.has-files.no-toc.rl-source-open main {{ margin-left: var(--fb-w, 16.5rem); }}
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
    #rl-source-pane {{ width: 100vw; max-width: none; min-width: 0; }}
  }}
</style>
"##,
        bg = c.css_var("bg"),
        bg_secondary = c.css_var("bg-secondary"),
        text = c.css_var("text"),
        text_dim = c.css_var("text-dim"),
        border = c.css_var("border"),
        accent = c.css_var("accent-primary"),
        code_bg = c.css_var("code-bg"),
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
{tokens}</style>
"##,
        bg_secondary = c.css_var("bg-secondary"),
        text = c.css_var("text"),
        text_dim = c.css_var("text-dim"),
        border = c.css_var("border"),
        code_bg = c.css_var("code-bg"),
        tokens = codemirror_token_css("#rl-source-pane", c),
    )
}

/// Map CodeMirror's default token classes onto theme font roles.
///
/// The vendored stylesheet assumes a light page: headers and links are
/// dark blue (`#00f`, `#00c`), brackets are a yellow-gray (`#997`), and
/// the search match is a yellow wash (`#ffa`). Those pairs fail on a
/// dark code panel and on Latte's near-white code panel. Every override
/// uses a role that already clears WCAG AA against `code_bg`. Search is
/// an underline so the match is not a second background the tokens were
/// not tuned for.
pub(crate) fn codemirror_token_css(scope: &str, c: &ThemeColors) -> String {
    let color = |classes: &[&str], role: &str| -> String {
        let sels: Vec<String> = classes.iter().map(|cl| format!("{scope} {cl}")).collect();
        format!("  {} {{ color: {}; }}\n", sels.join(", "), c.css_var(role))
    };
    let mut out = String::from(
        "  /* CodeMirror defaults are a light page. Token colors are theme roles. */\n",
    );
    out.push_str(&color(&[".cm-header"], "accent-primary"));
    out.push_str(&color(&[".cm-link", ".cm-url"], "accent-secondary"));
    out.push_str(&color(
        &[".cm-keyword", ".cm-builtin", ".cm-tag", ".cm-attribute"],
        "syn-keyword",
    ));
    out.push_str(&color(
        &[".cm-string", ".cm-string-2", ".cm-quote"],
        "syn-string",
    ));
    out.push_str(&color(&[".cm-comment"], "syn-comment"));
    out.push_str(&color(&[".cm-number", ".cm-atom"], "syn-number"));
    out.push_str(&color(
        &[".cm-def", ".cm-variable-2", ".cm-variable-3", ".cm-type"],
        "syn-function",
    ));
    out.push_str(&color(&[".cm-operator", ".cm-bracket"], "syn-operator"));
    out.push_str(&color(&[".cm-meta", ".cm-qualifier", ".cm-hr"], "text-dim"));
    out.push_str(&color(
        &[".cm-error", ".cm-invalidchar", ".cm-negative"],
        "error-text",
    ));
    out.push_str(&color(
        &[
            ".cm-positive",
            ".cm-matchingbracket",
            ".CodeMirror-matchingbracket",
        ],
        "syn-string",
    ));
    out.push_str(&color(&[".CodeMirror-nonmatchingbracket"], "error-text"));
    out.push_str(&format!(
        "  {scope} .cm-searching {{ background: transparent !important; background-color: transparent !important; text-decoration: underline; text-decoration-color: {}; }}\n",
        c.css_var("accent-secondary"),
    ));
    out
}

/// `<body>` additions: toolbar + pane markup, the CodeMirror scripts
/// (editable only), and the chrome controller script.
fn body_extra(opts: PageOpts, nonce: Option<&str>) -> String {
    let nonce_attr = crate::render::nonce_attr(nonce);
    let editable_js = if opts.editable { "true" } else { "false" };

    // The toolbar shows ⋯ and Source until ⋯ is opened. Comments and
    // Show changes live in that panel. The presets stay hidden until changes
    // is on. A dot on the icon means a non-default choice (diff on, or
    // comments hidden). Comments on does not light it.
    let git_flag = if opts.git_diff { "on" } else { "off" };
    let git_extra = if opts.git_diff {
        format!(" data-rl-git=\"{git_flag}\"")
    } else {
        format!(" disabled aria-disabled=\"true\" data-rl-git=\"{git_flag}\"")
    };
    let git_note = if opts.git_diff {
        "Git reads this repository's config. Compare only a repo you trust."
    } else {
        "Git diff is off. Pass --git-diff to compare a repository you trust."
    };
    let diff_chrome = format!(
        r#"<div id="rl-more" class="rl-more"><button id="rl-more-btn" type="button" class="rl-quiet" aria-expanded="false" aria-haspopup="true" aria-controls="rl-more-panel" aria-label="More" title="More">⋯</button><span id="rl-diff-dot" class="rl-diff-dot" hidden></span><div id="rl-more-panel" class="rl-more-panel" hidden role="region" aria-label="More"><button id="rl-diff-toggle" type="button" class="rl-quiet" role="switch" aria-checked="false"{git_extra}><span class="rl-switch" aria-hidden="true"></span>Show changes</button><p class="rl-git-note">{git_note}</p>{DIFF_PANEL}</div></div>"#
    );
    let toolbar = if opts.editable {
        format!(
            "<div id=\"rl-toolbar\">{diff_chrome}<button id=\"rl-source-save\" hidden title=\"Save (Ctrl/Cmd-S)\">Save</button><button id=\"rl-source-toggle\" title=\"Edit notebook source\">Edit</button></div>{CHIP_SLOT}"
        )
    } else {
        format!(
            "<div id=\"rl-toolbar\">{diff_chrome}<button id=\"rl-source-toggle\" title=\"Show notebook source\">Source</button></div>{CHIP_SLOT}"
        )
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
        format!(
            "<script src=\"/assets/codemirror/codemirror.min.js\"{nonce_attr}></script>\n\
             <script src=\"/assets/codemirror/mode/markdown/markdown.min.js\"{nonce_attr}></script>\n"
        )
    } else {
        String::new()
    };

    let mut out = format!(
        r##"{toolbar}
<aside id="rl-source-pane" aria-hidden="true">
  <div id="rl-source-head"><span id="rl-source-name">source</span><span id="rl-source-status"></span></div>
  {pane_body}
</aside>
{cm_scripts}<script{nonce_attr}>
(() => {{
  // Directory pages are /n/<relative-path>. The internal slug (for
  // /raw/ and /save/) is the rl-slug meta. A single-segment URL still
  // works when that meta is absent. The index has neither.
  function notebookSlug() {{
    const meta = document.querySelector('meta[name="rl-slug"]');
    if (meta && meta.content) return meta.content;
    const m = location.pathname.match(/^\/n\/([^\/]+)\/?$/);
    return m ? m[1] : null;
  }}
  const slug = notebookSlug();
  if (!slug) return;
  // Encode each segment. encodeURIComponent(slug) would turn `/` into
  // `%2F` and miss the wildcard route.
  function encodeSlug(s) {{
    return String(s).split('/').map(encodeURIComponent).join('/');
  }}
  const EDITABLE = {editable_js};
  const toggle = document.getElementById('rl-source-toggle');
  const pane = document.getElementById('rl-source-pane');
  const status = document.getElementById('rl-source-status');
  const saveBtn = document.getElementById('rl-source-save');
  let open = false;
  let cm = null;

  function setStatus(t) {{ if (status) status.textContent = t; }}

  async function fetchSource() {{
    const r = await fetch('/raw/' + encodeSlug(slug), {{ cache: 'no-store' }});
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
      const r = await fetch('/save/' + encodeSlug(slug), {{
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
    // Mutual exclusion with the inline cell editor (see cell.rs): both
    // write the same file, so refuse to open over unsaved cell edits.
    if (v && window.__rlCellDirty && window.__rlCellDirty()) {{
      const orig = toggle.textContent;
      toggle.textContent = 'unsaved cell edits!';
      setTimeout(() => {{ toggle.textContent = orig; }}, 1800);
      return;
    }}
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
  // Chain whatever the page already registered (comment placement). This
  // script is injected last, so a plain assignment would drop that hook
  // and a saved comment would stay in document order until a resize.
  var prevAfter = window.__rlAfterUpdate;
  window.__rlAfterUpdate = () => {{
    if (prevAfter) prevAfter();
    if (open && !EDITABLE) loadReadonly();
  }};
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
        nonce_attr = nonce_attr,
    );
    out.push_str(&format!(
        "<script{nonce_attr}>\n{DIFF_CLIENT_SCRIPT}\n</script>\n"
    ));
    out
}

/// Where the status chip goes; `gitdiff::decorate` fills it on a diff page.
const CHIP_SLOT: &str = "<button id=\"rl-diff-banner\" type=\"button\" hidden></button>";

/// The comparison controls under Show changes: presets with counts,
/// Other… (base and compare), and the list of changes on this page.
const DIFF_PANEL: &str = r#"<div id="rl-diff" hidden><p class="rl-dx-hd" id="rl-dx-cmp-hd">Compare</p><div id="rl-dx-presets" role="radiogroup" aria-labelledby="rl-dx-cmp-hd"><button type="button" class="rl-dx-opt" role="radio" aria-checked="false" data-preset="uncommitted"><span class="rl-dx-dot" aria-hidden="true"></span><span class="rl-dx-t">Uncommitted changes</span><span class="rl-dx-c"></span><span class="rl-dx-d">Your edits since the last commit</span></button><button type="button" class="rl-dx-opt" role="radio" aria-checked="false" data-preset="branch" hidden><span class="rl-dx-dot" aria-hidden="true"></span><span class="rl-dx-t">This branch vs main</span><span class="rl-dx-c"></span><span class="rl-dx-d">Everything on this branch since it left main, like a pull request</span></button><button type="button" class="rl-dx-opt" role="radio" aria-checked="false" data-preset="last"><span class="rl-dx-dot" aria-hidden="true"></span><span class="rl-dx-t">Last commit</span><span class="rl-dx-c"></span><span class="rl-dx-d">What the most recent commit changed</span></button><button type="button" class="rl-dx-opt" role="radio" aria-checked="false" data-preset="custom"><span class="rl-dx-dot" aria-hidden="true"></span><span class="rl-dx-t">Other…</span><span class="rl-dx-c"></span><span class="rl-dx-d">Pick any two branches, tags or commits</span></button></div><div id="rl-dx-other" hidden><label class="rl-diff-lab"><span class="rl-dx-lab">base</span><span class="rl-diff-combo"><input id="rl-diff-base" class="rl-diff-filter" type="text" autocomplete="off" spellcheck="false" aria-label="Base: compare from" aria-expanded="false" aria-controls="rl-diff-base-list"></span></label><ul id="rl-diff-base-list" class="rl-diff-menu" hidden role="listbox" aria-label="Base"></ul><p class="rl-dx-arrow">↓ changes from base to compare ↓</p><label class="rl-diff-lab"><span class="rl-dx-lab">compare</span><span class="rl-diff-combo"><input id="rl-diff-compare" class="rl-diff-filter" type="text" autocomplete="off" spellcheck="false" aria-label="Compare: compare to" aria-expanded="false" aria-controls="rl-diff-compare-list"></span></label><ul id="rl-diff-compare-list" class="rl-diff-menu" hidden role="listbox" aria-label="Compare"></ul><p class="rl-dx-hint">Typing filters the list. A sha or <code>HEAD~3</code> works too.</p></div><p class="rl-dx-hd" id="rl-dx-view-hd">View</p><div class="rl-dx-seg" role="radiogroup" aria-labelledby="rl-dx-view-hd"><button type="button" role="radio" aria-checked="true" data-view="rendered">Rendered</button><button type="button" role="radio" aria-checked="false" data-view="source">Source</button></div><p class="rl-dx-hd" id="rl-dx-list-hd">Changes</p><ul id="rl-dx-list" aria-labelledby="rl-dx-list-hd"></ul><p class="rl-dx-keys"><kbd>[</kbd> <kbd>]</kbd> previous / next change</p></div>"#;

/// Presets, change list, keyboard prev/next, and the ref lists. A raw string so the
/// braces are not doubled by the chrome `format!`.
///
/// The pure helpers (`rlDiffFilter`, `rlDiffRefOk`, `rlDiffChoose`,
/// `rlDiffNorm`, `rlDiffPrefApply`, `rlDiffUrl`, `rlRelTime`) stay at the
/// top so a node test can load this file. `rlDiffRefOk` matches
/// `gitdiff::ref_ok`.
pub(crate) const DIFF_CLIENT_SCRIPT: &str = r#"function rlDiffFilter(items, query) {
  var q = String(query || '').toLowerCase();
  var out = [];
  for (var i = 0; i < items.length; i++) {
    var it = items[i];
    if (!q || String(it.label).toLowerCase().indexOf(q) !== -1 || String(it.value).toLowerCase().indexOf(q) !== -1) {
      out.push(it);
    }
  }
  return out;
}
function rlDiffRefOk(s) {
  if (!s || s.length > 256 || s.charAt(0) === '-' || s.indexOf('..') !== -1) return false;
  return /^[A-Za-z0-9._/^~@{}+-]+$/.test(s);
}
function rlDiffChoose(matches, query, highlight, armed) {
  var typed = String(query || '').trim();
  var hi = highlight | 0;
  if ((armed || typed) && matches.length && hi >= 0 && hi < matches.length) {
    return matches[hi].value;
  }
  if (typed && matches.length === 0 && rlDiffRefOk(typed)) return typed;
  return null;
}
function rlMoreMark(diffOn, commentsOff) {
  var bits = [];
  if (diffOn) bits.push('changes on');
  if (commentsOff) bits.push('comments off');
  return {
    dot: bits.length > 0,
    label: bits.length ? ('More, ' + bits.join(', ')) : 'More'
  };
}
var RL_PRESETS = ['uncommitted', 'branch', 'last', 'custom'];
// A saved or body choice in one shape: {changes, base, compare}. The
// 0.3.12 shape {diff, base} meant before/after and still restores.
function rlDiffNorm(p) {
  if (!p) return null;
  var view = p.view === 'source' ? 'source' : '';
  if (p.changes) {
    if (RL_PRESETS.indexOf(p.changes) === -1) return null;
    if (p.changes !== 'custom') return { changes: p.changes, base: '', compare: '', view: view };
    var b = p.base || 'HEAD';
    var c = p.compare || '';
    if (!rlDiffRefOk(b) || (c && !rlDiffRefOk(c))) return null;
    return { changes: 'custom', base: b, compare: c, view: view };
  }
  if (p.diff) {
    if (!rlDiffRefOk(p.diff) || (p.base && !rlDiffRefOk(p.base))) return null;
    if (p.diff === 'HEAD' && !p.base) return { changes: 'uncommitted', base: '', compare: '', view: view };
    return { changes: 'custom', base: p.diff, compare: p.base || '', view: view };
  }
  return null;
}
// What to do on load. `query` is the page's own diff keys; `body` is the
// comparison the server rendered (null when the page is plain).
function rlDiffPrefApply(query, body, saved) {
  var q = query || {};
  if (q.changes === 'off' || (!q.changes && q.diff === 'off')) return { action: 'store-off' };
  if (q.changes || q.diff) return { action: 'store-on', pref: rlDiffNorm(body) };
  if (body) return { action: 'keep', pref: rlDiffNorm(body) };
  if (saved && saved.on) {
    var pref = rlDiffNorm(saved);
    if (!pref) return { action: 'none' };
    return { action: 'navigate', pref: pref };
  }
  return { action: 'none' };
}
// The page URL for a choice. `null` turns changes off. Old keys go, so
// a link only ever carries one scheme.
function rlDiffUrl(href, pref) {
  var u = new URL(href);
  u.searchParams.delete('diff');
  u.searchParams.delete('base');
  u.searchParams.delete('compare');
  u.searchParams.delete('view');
  if (!pref) {
    u.searchParams.set('changes', 'off');
    return u.toString();
  }
  u.searchParams.set('changes', pref.changes);
  if (pref.changes === 'custom') {
    u.searchParams.set('base', pref.base || 'HEAD');
    if (pref.compare) u.searchParams.set('compare', pref.compare);
  }
  if (pref.view === 'source') u.searchParams.set('view', 'source');
  return u.toString();
}
function rlRelTime(then, now) {
  var s = Math.max(0, (now | 0) - (then | 0));
  if (!then) return '';
  if (s < 60) return 'just now';
  if (s < 3600) return Math.floor(s / 60) + ' min ago';
  if (s < 86400) return Math.floor(s / 3600) + ' h ago';
  var d = Math.floor(s / 86400);
  if (d < 60) return d === 1 ? '1 day ago' : d + ' days ago';
  var m = Math.floor(d / 30);
  if (m < 24) return m + ' months ago';
  return Math.floor(d / 365) + ' years ago';
}
function rlDxCount(entry) {
  if (!entry) return '';
  if (typeof entry.count === 'number') {
    return entry.count === 0 ? 'no changes' : entry.count === 1 ? '1 change' : entry.count + ' changes';
  }
  return entry.note || '';
}
(function () {
  if (typeof document === 'undefined') return;
  var toggle = document.getElementById('rl-diff-toggle');
  if (!toggle) return;
  var gitOn = toggle.getAttribute('data-rl-git') === 'on';
  var moreBtn = document.getElementById('rl-more-btn');
  var panel = document.getElementById('rl-more-panel');
  var dot = document.getElementById('rl-diff-dot');
  var chip = document.getElementById('rl-diff-banner');
  var other = document.getElementById('rl-dx-other');
  var baseIn = document.getElementById('rl-diff-base');
  var cmpIn = document.getElementById('rl-diff-compare');
  var baseList = document.getElementById('rl-diff-base-list');
  var cmpList = document.getElementById('rl-diff-compare-list');
  var listEl = document.getElementById('rl-dx-list');
  var listHd = document.getElementById('rl-dx-list-hd');
  var PREF = 'rl-diff-pref';
  var OPEN = 'rl-dx-open';
  function slug() {
    var meta = document.querySelector('meta[name="rl-slug"]');
    if (meta && meta.content) return meta.content;
    var m = location.pathname.match(/^\/n\/(.+?)\/?$/);
    return m ? decodeURIComponent(m[1]) : '';
  }
  function readPref() {
    try {
      var raw = sessionStorage.getItem(PREF);
      return raw ? JSON.parse(raw) : null;
    } catch (err) { return null; }
  }
  function store(pref) {
    try {
      sessionStorage.setItem(PREF, JSON.stringify(pref ? {
        on: true, changes: pref.changes, base: pref.base || '', compare: pref.compare || '',
        view: pref.view || ''
      } : { on: false }));
    } catch (err) {}
  }
  function on() {
    return document.body.classList.contains('rl-diff-on');
  }
  function bodyPref() {
    if (!on()) return null;
    var d = document.body.dataset;
    return {
      changes: d.rlChanges || 'uncommitted', base: d.rlBase || '', compare: d.rlCompare || '',
      view: d.rlView === 'source' ? 'source' : ''
    };
  }
  function go(pref) {
    store(pref);
    location.assign(rlDiffUrl(location.href, pref));
  }
  (function restore() {
    if (!gitOn) return;
    var q = new URLSearchParams(location.search);
    var decision = rlDiffPrefApply(
      { changes: q.get('changes'), diff: q.get('diff') }, bodyPref(), readPref());
    if (decision.action === 'store-off') store(null);
    else if (decision.action === 'store-on' || decision.action === 'keep') {
      if (decision.pref) store(decision.pref);
    } else if (decision.action === 'navigate') go(decision.pref);
  })();

  function menuOpen() { return !!(panel && !panel.hidden); }
  function setMenu(open) {
    if (!panel || !moreBtn) return;
    panel.hidden = !open;
    moreBtn.setAttribute('aria-expanded', open ? 'true' : 'false');
    if (open) {
      loadCounts();
      var first = panel.querySelector('[aria-checked="true"].rl-dx-opt') || toggle;
      if (first && first.focus) first.focus();
    }
  }
  if (moreBtn && panel) {
    moreBtn.addEventListener('click', function (ev) {
      ev.stopPropagation();
      setMenu(!menuOpen());
    });
    document.addEventListener('keydown', function (e) {
      if (e.key !== 'Escape' || !menuOpen()) return;
      setMenu(false);
      moreBtn.focus();
    });
    document.addEventListener('click', function (ev) {
      if (!menuOpen()) return;
      if (panel.contains(ev.target) || moreBtn.contains(ev.target)) return;
      if (chip && chip.contains(ev.target)) return;
      setMenu(false);
    });
  }
  if (chip) {
    chip.addEventListener('click', function (ev) {
      ev.stopPropagation();
      setMenu(!menuOpen());
    });
  }
  function syncMoreMark() {
    var box = document.getElementById('rl-comments');
    var mark = rlMoreMark(on(), !!(box && !box.checked));
    if (dot) dot.hidden = !mark.dot;
    if (moreBtn) {
      moreBtn.setAttribute('aria-label', mark.label);
      moreBtn.title = mark.label;
    }
  }
  syncMoreMark();
  toggle.addEventListener('click', function () {
    if (!gitOn) return;
    if (on()) { go(null); return; }
    var saved = rlDiffNorm(readPref());
    go(saved || { changes: 'uncommitted', base: '', compare: '', view: '' });
  });

  // ── Presets ────────────────────────────────────────────────────────
  var current = bodyPref();
  var opts = Array.prototype.slice.call(document.querySelectorAll('.rl-dx-opt'));
  function optFor(key) {
    for (var i = 0; i < opts.length; i++) if (opts[i].getAttribute('data-preset') === key) return opts[i];
    return null;
  }
  opts.forEach(function (b) {
    var key = b.getAttribute('data-preset');
    b.setAttribute('aria-checked', current && current.changes === key ? 'true' : 'false');
    b.addEventListener('click', function () {
      if (key === 'custom') {
        opts.forEach(function (o) { o.setAttribute('aria-checked', o === b ? 'true' : 'false'); });
        showOther();
        if (baseIn) baseIn.focus();
        return;
      }
      if (current && current.changes === key) return;
      go({ changes: key, base: '', compare: '', view: current ? current.view : '' });
    });
  });
  var counted = false;
  function loadCounts() {
    if (counted || !gitOn || !on()) return;
    counted = true;
    var box = document.getElementById('rl-comments');
    var url = '/git/changes?slug=' + encodeURIComponent(slug()) + (box && !box.checked ? '&cm=0' : '');
    fetch(url).then(function (r) { return r.json(); }).then(function (data) {
      if (!data || data.error) return;
      var u = optFor('uncommitted');
      if (u) u.querySelector('.rl-dx-c').textContent = rlDxCount(data.uncommitted);
      var br = optFor('branch');
      if (br && data.branch && !data.branch.hidden && data.default) {
        br.hidden = false;
        br.querySelector('.rl-dx-t').textContent = 'This branch vs ' + data.default;
        br.querySelector('.rl-dx-d').textContent = 'Everything on ' + (data.current || 'this branch') +
          ' since it left ' + data.default + ', like a pull request';
        br.querySelector('.rl-dx-c').textContent = data.branch.disabled ? '' : rlDxCount(data.branch);
        if (data.branch.disabled) {
          br.disabled = true;
          br.querySelector('.rl-dx-d').textContent = data.branch.disabled;
        }
      }
      var last = optFor('last');
      if (last && data.last) {
        if (data.last.disabled) {
          last.disabled = true;
          last.querySelector('.rl-dx-c').textContent = '—';
          last.querySelector('.rl-dx-d').textContent = data.last.disabled;
        } else {
          last.querySelector('.rl-dx-c').textContent = rlDxCount(data.last);
        }
      }
    }).catch(function () {});
  }
  if (current && current.changes === 'branch') { var b0 = optFor('branch'); if (b0) b0.hidden = false; }
  Array.prototype.forEach.call(document.querySelectorAll('.rl-dx-seg [data-view]'), function (b) {
    var v = b.getAttribute('data-view') === 'source' ? 'source' : '';
    b.setAttribute('aria-checked', (current ? current.view : '') === v ? 'true' : 'false');
    b.addEventListener('click', function () {
      if (!current || current.view === v) return;
      go({ changes: current.changes, base: current.base, compare: current.compare, view: v });
    });
  });

  // ── Other…: base and compare ───────────────────────────────────────
  function mount(input, list, which) {
    if (!input || !list) return { setItems: function () {}, value: function () { return ''; } };
    var items = [];
    var filtering = false;
    var highlight = 0;
    var armed = false;
    function committed() { return input.getAttribute('data-value') || ''; }
    function labelOf(value) {
      for (var i = 0; i < items.length; i++) if (items[i].value === value) return items[i].label;
      return value;
    }
    function query() { return filtering ? input.value : ''; }
    function rows() { return rlDiffFilter(items, query()); }
    function close(restore) {
      list.hidden = true;
      input.setAttribute('aria-expanded', 'false');
      if (restore) input.value = labelOf(committed());
      filtering = false;
      armed = false;
    }
    function render() {
      var found = rows();
      list.textContent = '';
      if (!found.length) {
        var hint = document.createElement('li');
        hint.className = 'rl-diff-hint';
        var typed = input.value.trim();
        hint.textContent = (typed && rlDiffRefOk(typed)) ? ('use ' + typed) : 'no match';
        list.appendChild(hint);
      } else {
        if (highlight >= found.length) highlight = found.length - 1;
        if (highlight < 0) highlight = 0;
        var group = null;
        found.forEach(function (it, i) {
          if (it.group && it.group !== group) {
            group = it.group;
            var g = document.createElement('li');
            g.className = 'rl-dx-group';
            g.setAttribute('role', 'presentation');
            g.textContent = group;
            list.appendChild(g);
          }
          var li = document.createElement('li');
          li.setAttribute('role', 'option');
          var name = document.createElement('span');
          name.textContent = it.label;
          li.appendChild(name);
          if (it.meta) {
            var meta = document.createElement('span');
            meta.className = 'rl-m';
            meta.textContent = it.meta;
            li.appendChild(meta);
          }
          if (i === highlight) li.setAttribute('aria-selected', 'true');
          li.addEventListener('mousedown', function (ev) {
            ev.preventDefault();
            pick(it.value);
          });
          list.appendChild(li);
        });
        var sel = list.querySelector('[aria-selected="true"]');
        if (sel && sel.scrollIntoView) sel.scrollIntoView({ block: 'nearest' });
      }
      list.hidden = false;
      input.setAttribute('aria-expanded', 'true');
    }
    function pick(value) {
      var base = which === 'base' ? value : (baseCombo.value() || 'HEAD');
      var cmp = which === 'compare' ? value : cmpCombo.value();
      if (current && current.changes === 'custom' && value === committed()) { close(true); return; }
      go({ changes: 'custom', base: base || 'HEAD', compare: cmp || '', view: current ? current.view : '' });
    }
    input.addEventListener('focus', function () {
      filtering = false;
      armed = false;
      highlight = 0;
      var all = rlDiffFilter(items, '');
      for (var i = 0; i < all.length; i++) {
        if (all[i].value === committed()) { highlight = i; break; }
      }
      input.select();
      render();
    });
    input.addEventListener('input', function () {
      filtering = true;
      armed = false;
      highlight = 0;
      render();
    });
    input.addEventListener('keydown', function (e) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        var n = rows().length;
        if (!n || list.hidden) { render(); return; }
        highlight += e.key === 'ArrowDown' ? 1 : -1;
        if (highlight < 0) highlight = 0;
        if (highlight >= n) highlight = n - 1;
        armed = true;
        render();
      } else if (e.key === 'Enter') {
        e.preventDefault();
        var chosen = rlDiffChoose(rows(), query(), highlight, armed);
        if (chosen === null) {
          var typed = input.value.trim();
          if (filtering && typed && rlDiffRefOk(typed) && !rows().length) chosen = typed;
        }
        if (chosen === null) return;
        pick(chosen);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        if (!list.hidden) {
          e.stopPropagation();
          close(true);
        }
      }
    });
    input.addEventListener('blur', function () {
      setTimeout(function () { close(true); }, 0);
    });
    return {
      value: committed,
      setItems: function (next, value) {
        items = next;
        input.setAttribute('data-value', value);
        // The list can arrive after the field took focus; show it then.
        if (document.activeElement === input && !filtering) {
          input.value = labelOf(value);
          render();
          input.select();
          return;
        }
        close(false);
        input.value = labelOf(value);
      }
    };
  }
  var baseCombo = mount(baseIn, baseList, 'base');
  var cmpCombo = mount(cmpIn, cmpList, 'compare');
  var refsLoaded = false;
  function fill(data) {
    var now = Math.floor(Date.now() / 1000);
    var shared = [];
    ((data && data.branches) || []).forEach(function (b) {
      shared.push({ value: b.name, label: b.name, group: 'Branches', meta: b.current ? 'current' : rlRelTime(b.date, now) });
    });
    shared.push({ value: 'HEAD', label: 'HEAD', group: 'Branches', meta: 'last commit' });
    ((data && data.tags) || []).forEach(function (t) {
      shared.push({ value: t.name, label: t.name, group: 'Tags', meta: rlRelTime(t.date, now) });
    });
    ((data && data.commits) || []).forEach(function (c) {
      shared.push({ value: c.rev, label: c.label, group: 'Recent commits', meta: rlRelTime(c.date, now) });
    });
    var base = (current && current.changes === 'custom' && current.base) || 'HEAD';
    var cmp = (current && current.changes === 'custom' && current.compare) || '';
    var baseItems = shared.slice();
    if (!baseItems.some(function (it) { return it.value === base; })) baseItems.push({ value: base, label: base, group: '' });
    var cmpItems = [{ value: '', label: 'Working copy (your files)', group: '' }].concat(shared);
    if (cmp && !cmpItems.some(function (it) { return it.value === cmp; })) cmpItems.push({ value: cmp, label: cmp, group: '' });
    baseCombo.setItems(baseItems, base);
    cmpCombo.setItems(cmpItems, cmp);
  }
  function showOther() {
    if (!other) return;
    other.hidden = false;
    if (refsLoaded || !gitOn) return;
    refsLoaded = true;
    var base = (current && current.changes === 'custom' && current.base) || 'HEAD';
    var cmp = (current && current.changes === 'custom' && current.compare) || '';
    if (baseIn) { baseIn.setAttribute('data-value', base); baseIn.value = base; }
    if (cmpIn) { cmpIn.setAttribute('data-value', cmp); cmpIn.value = cmp || 'Working copy (your files)'; }
    fetch('/git/refs').then(function (r) { return r.json(); }).then(fill).catch(function () {});
  }
  if (current && current.changes === 'custom') showOther();

  // ── The changes on this page ───────────────────────────────────────
  var BAR = { added: 'var(--rl-dx-add-bar)', modified: 'var(--rl-dx-mod-bar)', removed: 'var(--rl-dx-del-bar)' };
  function changes() {
    return Array.prototype.slice.call(document.querySelectorAll('main [data-rl-dx]'));
  }
  function mark(i) {
    if (!listEl) return;
    Array.prototype.forEach.call(listEl.children, function (li, k) {
      li.classList.toggle('rl-cur', k === i);
    });
  }
  function jump(el, i) {
    el.scrollIntoView({ block: 'center' });
    mark(i);
  }
  function buildList() {
    if (!listEl) return;
    var list = changes();
    listEl.textContent = '';
    if (listHd) listHd.textContent = list.length === 1 ? '1 change' : list.length ? list.length + ' changes' : 'Changes';
    if (!list.length) {
      var none = document.createElement('li');
      none.className = 'rl-empty';
      none.textContent = on() ? 'Nothing changed on this page.' : '';
      listEl.appendChild(none);
      return;
    }
    list.forEach(function (el, i) {
      var kind = el.getAttribute('data-rl-dx');
      var li = document.createElement('li');
      li.tabIndex = 0;
      var bar = document.createElement('i');
      bar.style.background = BAR[kind] || 'currentColor';
      var k = document.createElement('span');
      k.className = 'rl-k';
      k.textContent = kind;
      var w = document.createElement('span');
      w.textContent = el.getAttribute('data-rl-where') || '';
      li.appendChild(bar);
      li.appendChild(k);
      li.appendChild(w);
      li.addEventListener('click', function () { jump(el, i); });
      li.addEventListener('keydown', function (e) {
        if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); jump(el, i); }
      });
      listEl.appendChild(li);
    });
  }
  if (on()) buildList();
  function fullyVisible(el) {
    var r = el.getBoundingClientRect();
    return r.top >= 0 && r.bottom <= window.innerHeight;
  }
  function step(dir) {
    var list = changes();
    if (!list.length) return;
    var cur = -1;
    list.forEach(function (el, i) {
      if (el.getBoundingClientRect().top < 80) cur = i;
    });
    var i = cur + dir;
    if (dir > 0) { while (i < list.length && fullyVisible(list[i])) i++; }
    else { while (i >= 0 && fullyVisible(list[i])) i--; }
    if (i < 0) i = 0;
    if (i >= list.length) i = list.length - 1;
    jump(list[i], i);
  }
  document.addEventListener('keydown', function (e) {
    if (!on()) return;
    if (e.key !== '[' && e.key !== ']') return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    var t = e.target;
    if (!t || !t.closest) return;
    if (t.closest('input, textarea, select, .CodeMirror')) return;
    e.preventDefault();
    step(e.key === ']' ? 1 : -1);
  });

  // Removed blocks stay open or closed across the reload a live edit does.
  function openSet() {
    try { return JSON.parse(sessionStorage.getItem(OPEN) || '[]'); } catch (err) { return []; }
  }
  function keepOpen() {
    var ids = openSet();
    Array.prototype.forEach.call(document.querySelectorAll('main details.rl-dx-removed'), function (d) {
      if (ids.indexOf(d.id) !== -1) d.open = true;
      d.addEventListener('toggle', function () {
        var now = openSet().filter(function (x) { return x !== d.id; });
        if (d.open) now.push(d.id);
        try { sessionStorage.setItem(OPEN, JSON.stringify(now.slice(-64))); } catch (err) {}
      });
    });
  }
  if (on()) keepOpen();

  var KEYS = ['changes', 'diff', 'base', 'compare', 'view', 'cm'];
  document.addEventListener('click', function (ev) {
    if (!on()) return;
    var a = ev.target && ev.target.closest && ev.target.closest('a');
    if (!a || a.target === '_blank') return;
    var href = a.getAttribute('href');
    if (!href || href.charAt(0) === '#') return;
    var url;
    try { url = new URL(a.href, location.href); } catch (err) { return; }
    if (url.origin !== location.origin) return;
    if (!(url.pathname === '/' || url.pathname.indexOf('/n/') === 0)) return;
    var cur = new URLSearchParams(location.search);
    KEYS.forEach(function (k) {
      if (cur.get(k) && !url.searchParams.get(k)) url.searchParams.set(k, cur.get(k));
    });
    if (url.toString() !== a.href) {
      ev.preventDefault();
      location.assign(url.toString());
    }
  });

  var box = document.getElementById('rl-comments');
  if (box) {
    box.addEventListener('change', function () {
      syncMoreMark();
      if (!on()) return;
      var u = new URL(location.href);
      u.searchParams.set('cm', box.checked ? '1' : '0');
      location.assign(u.toString());
    });
  }

  function paint() {
    var el = document.getElementById('rl-git-files');
    if (!el) return;
    var data;
    try { data = JSON.parse(el.textContent || '{"files":[]}'); } catch (err) { return; }
    var files = data.files || [];
    Array.prototype.forEach.call(document.querySelectorAll('a.fb-link'), function (a) {
      var href = a.getAttribute('href') || '';
      var pub = href.replace(/^\/n\//, '');
      var hit = null;
      files.forEach(function (f) {
        var p = String(f.path || '').replace(/\\/g, '/');
        if (p === pub || p === pub + '.md' || p.endsWith('/' + pub) || p.endsWith('/' + pub + '.md')) hit = f;
      });
      var old = a.querySelector('.fb-git');
      if (old) old.remove();
      if (!hit) return;
      var d = document.createElement('span');
      d.className = 'fb-git';
      d.title = hit.label || hit.status || 'changed';
      a.appendChild(d);
    });
  }
  paint();
  var prevAfter = window.__rlAfterUpdate;
  window.__rlAfterUpdate = function () {
    if (prevAfter) prevAfter();
    paint();
  };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use rustlab_plot::Theme;

    #[test]
    fn diff_client_script_passes_node_check() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diff.js");
        std::fs::write(&path, DIFF_CLIENT_SCRIPT).unwrap();
        match std::process::Command::new("node")
            .arg("--check")
            .arg(&path)
            .status()
        {
            Ok(st) => assert!(st.success(), "node --check failed"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("SKIP diff script syntax: node not found");
            }
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn diff_filter_narrows_and_keeps_a_valid_free_ref() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("filter.js");
        let mut harness = DIFF_CLIENT_SCRIPT.to_string();
        harness.push_str(
            r#"
const assert = require('assert');
const items = [
  {value:'topic', label:'topic (current)'},
  {value:'main', label:'main'},
  {value:'feature-x', label:'feature-x'},
  {value:'HEAD', label:'HEAD'},
];
let m = rlDiffFilter(items, 'FEA');
assert.strictEqual(m.length, 1);
assert.strictEqual(m[0].value, 'feature-x');
m = rlDiffFilter(items, 'current');
assert.strictEqual(m.length, 1);
assert.strictEqual(m[0].value, 'topic');
m = rlDiffFilter(items, '');
assert.strictEqual(m.length, items.length);
assert.strictEqual(m[0].value, 'topic');
m = rlDiffFilter(items, 'a');
assert.deepStrictEqual(m.map(function (it) { return it.value; }), ['main', 'feature-x', 'HEAD']);
assert.strictEqual(rlDiffChoose(m, 'a', 1, false), 'feature-x');
assert.strictEqual(rlDiffChoose(rlDiffFilter(items, ''), '', 1, false), null);
assert.strictEqual(rlDiffChoose(rlDiffFilter(items, ''), '', 1, true), 'main');
assert.strictEqual(rlDiffRefOk('topic+1'), true);
assert.strictEqual(rlDiffChoose([], 'HEAD~2', 0, false), 'HEAD~2');
assert.strictEqual(rlDiffChoose([], 'refs/heads/off', 0, false), 'refs/heads/off');
assert.strictEqual(rlDiffChoose([], 'main..HEAD', 0, false), null);
assert.strictEqual(rlDiffChoose([], 'not a ref', 0, false), null);
assert.strictEqual(rlDiffChoose([], '-rf', 0, false), null);
var P = function (changes, diff) { return { changes: changes, diff: diff }; };
var unc = { changes: 'uncommitted', base: '', compare: '', view: '' };
assert.strictEqual(rlDiffPrefApply(P('off', null), null, {on:true, changes:'last'}).action, 'store-off');
assert.strictEqual(rlDiffPrefApply(P(null, 'off'), null, {on:true}).action, 'store-off');
var d = rlDiffPrefApply(P('custom', null), {changes:'custom', base:'main', compare:'topic'}, null);
assert.strictEqual(d.action, 'store-on');
assert.deepStrictEqual(d.pref, {changes:'custom', base:'main', compare:'topic', view:''});
assert.strictEqual(rlDiffPrefApply(P(null, null), unc, null).action, 'keep');
d = rlDiffPrefApply(P('last', null), {changes:'last', view:'source'}, null);
assert.strictEqual(d.pref.view, 'source');
d = rlDiffPrefApply(P(null, null), null, {on:true, changes:'branch'});
assert.strictEqual(d.action, 'navigate');
assert.strictEqual(d.pref.changes, 'branch');
// The 0.3.12 saved shape still restores: diff was before, base after.
d = rlDiffPrefApply(P(null, null), null, {on:true, diff:'HEAD', base:''});
assert.deepStrictEqual(d.pref, unc);
d = rlDiffPrefApply(P(null, null), null, {on:true, diff:'main', base:'topic'});
assert.deepStrictEqual(d.pref, {changes:'custom', base:'main', compare:'topic', view:''});
assert.strictEqual(rlDiffPrefApply(P(null, null), null, {on:true, diff:'main..HEAD'}).action, 'none');
assert.strictEqual(rlDiffPrefApply(P(null, null), null, {on:true, changes:'custom', base:'-x'}).action, 'none');
assert.strictEqual(rlDiffPrefApply(P(null, null), null, {on:true, changes:'nope'}).action, 'none');
assert.strictEqual(rlDiffPrefApply(P(null, null), null, null).action, 'none');
var u = new URL(rlDiffUrl('http://h/n/nb?diff=main&base=topic&cm=0', {changes:'branch'}));
assert.strictEqual(u.searchParams.get('changes'), 'branch');
assert.strictEqual(u.searchParams.get('diff'), null);
assert.strictEqual(u.searchParams.get('base'), null);
assert.strictEqual(u.searchParams.get('cm'), '0');
u = new URL(rlDiffUrl('http://h/n/nb', {changes:'custom', base:'v0.3', compare:''}));
assert.strictEqual(u.searchParams.get('base'), 'v0.3');
assert.strictEqual(u.searchParams.get('compare'), null);
u = new URL(rlDiffUrl('http://h/n/nb?view=source', {changes:'last', view:'source'}));
assert.strictEqual(u.searchParams.get('view'), 'source');
u = new URL(rlDiffUrl('http://h/n/nb?view=source', {changes:'last', view:''}));
assert.strictEqual(u.searchParams.get('view'), null);
u = new URL(rlDiffUrl('http://h/n/nb?changes=last', null));
assert.strictEqual(u.searchParams.get('changes'), 'off');
assert.strictEqual(rlRelTime(1000, 1030), 'just now');
assert.strictEqual(rlRelTime(1000, 1000 + 7200), '2 h ago');
assert.strictEqual(rlRelTime(1000, 1000 + 3 * 86400), '3 days ago');
assert.strictEqual(rlRelTime(0, 5000), '');
assert.strictEqual(rlDxCount({count: 0}), 'no changes');
assert.strictEqual(rlDxCount({count: 1}), '1 change');
assert.strictEqual(rlDxCount({count: 4}), '4 changes');
assert.strictEqual(rlDxCount({count: null, note: 'new file'}), 'new file');
var mark = rlMoreMark(false, false);
assert.strictEqual(mark.dot, false);
assert.strictEqual(mark.label, 'More');
mark = rlMoreMark(true, false);
assert.strictEqual(mark.dot, true);
assert.strictEqual(mark.label, 'More, changes on');
mark = rlMoreMark(false, true);
assert.strictEqual(mark.dot, true);
assert.strictEqual(mark.label, 'More, comments off');
mark = rlMoreMark(true, true);
assert.strictEqual(mark.dot, true);
assert.strictEqual(mark.label, 'More, changes on, comments off');
"#,
        );
        std::fs::write(&path, harness).unwrap();
        match std::process::Command::new("node").arg(&path).status() {
            Ok(st) => assert!(st.success(), "filter harness failed"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("SKIP diff filter: node not found");
            }
            Err(e) => panic!("{e}"),
        }
    }

    fn render(editable: bool) -> String {
        let html = "<!doctype html><html><head><title>x</title></head><body><main><p>hi</p></main></body></html>";
        inject_chrome(
            html,
            Theme::Dark.colors(),
            PageOpts {
                editable,
                git_diff: false,
            },
        )
    }

    #[test]
    fn readonly_injects_toolbar_and_pre_pane() {
        let out = render(false);
        assert!(out.contains("id=\"rl-toolbar\""));
        assert!(out.contains("id=\"rl-source-pane\""));
        assert!(
            out.contains("id=\"rl-source-pre\""),
            "read-only pane uses a <pre>"
        );
        assert!(
            out.contains(">Source<"),
            "button reads Source in read-only mode"
        );
        assert!(out.contains("id=\"rl-more-btn\""), "overflow button");
        assert!(
            out.contains("aria-expanded=\"false\""),
            "menu starts closed"
        );
        assert!(out.contains("id=\"rl-more-panel\" class=\"rl-more-panel\" hidden"));
        assert!(out.contains("Show changes</button>"));
        assert!(out.contains("role=\"switch\""), "show changes is a switch");
        assert!(
            out.contains("<button id=\"rl-diff-banner\" type=\"button\" hidden></button>"),
            "the status chip slot sits outside the toolbar"
        );
        for preset in ["uncommitted", "branch", "last", "custom"] {
            assert!(
                out.contains(&format!("data-preset=\"{preset}\"")),
                "{preset} preset"
            );
        }
        assert!(out.contains("id=\"rl-diff-compare\""), "compare field");
        assert!(out.contains("id=\"rl-dx-list\""), "change list");
        assert!(!out.contains("value: 'index'"), "index is not a list entry");
        assert!(out.contains("aria-checked=\"false\""));
        assert!(
            out.contains("data-rl-git=\"off\""),
            "git diff is off by default"
        );
        assert!(
            out.contains("disabled"),
            "show diff is unavailable until enabled"
        );
        assert!(
            out.contains("Git diff is off"),
            "the menu explains how to turn git diff on"
        );
        assert!(!out.contains("onclick="), "no inline handlers");
        assert!(out.contains("id=\"rl-diff-toggle\""), "show-diff switch");
        assert!(
            out.contains("id=\"rl-diff\" hidden"),
            "compare picker stays collapsed until diff is on"
        );
        assert!(
            !out.contains(">Diff</button>"),
            "the toolbar does not show a Diff label"
        );
        assert!(out.contains("/raw/' + encodeSlug(slug)"));
        // No editor assets referenced.
        assert!(
            !out.contains("/assets/codemirror/"),
            "no CodeMirror in read-only mode"
        );
        assert!(out.contains("<main><p>hi</p></main>"));
        assert!(
            out.contains("var prevAfter = window.__rlAfterUpdate;"),
            "chrome must chain the comment-placement hook"
        );
    }

    fn page_with_comments(checked: bool) -> String {
        let toggle = crate::comments::toggle_html(checked);
        format!(
            "<!doctype html><html><head><title>x</title></head><body><header class=\"topbar\">{toggle}</header><main><p>hi</p></main></body></html>"
        )
    }

    #[test]
    fn watch_moves_comments_into_the_menu() {
        let out = inject_chrome(
            &page_with_comments(true),
            Theme::Dark.colors(),
            PageOpts {
                editable: false,
                git_diff: true,
            },
        );
        let main_at = out.find("<main>").unwrap();
        let header = &out[..main_at];
        assert!(
            !header.contains("id=\"rl-comments\""),
            "the header no longer holds the checkbox"
        );
        assert!(
            header.contains("id=\"rl-cm-confirm\""),
            "the saved-note status stays outside the menu"
        );
        let panel = out.find("id=\"rl-more-panel\"").unwrap();
        let source = out.find("id=\"rl-source-toggle\"").unwrap();
        assert!(panel < source, "the menu comes before the Source button");
        let menu = &out[panel..source];
        assert!(menu.contains("id=\"rl-comments\""), "{menu}");
        assert!(menu.contains("role=\"switch\""), "{menu}");
        assert!(menu.contains(">Comments</label>"), "{menu}");
        assert!(menu.contains(" checked"), "default is on");
        assert!(
            menu.contains("Show changes</button>"),
            "both switches share the menu"
        );
        assert!(
            out.contains("<span id=\"rl-diff-dot\" class=\"rl-diff-dot\" hidden>"),
            "comments on does not light the dot"
        );
        assert!(!out.contains("onclick="));
        assert!(out.contains("#rl-comments:checked"));
        assert!(out.contains("rl-comments-visible") || out.contains("syncMoreMark"));
        // Toolbar controls outside the panel are ⋯ and Source.
        let toolbar = out.find("id=\"rl-toolbar\"").unwrap();
        let after = &out[toolbar..];
        let panel_in_toolbar = after.find("id=\"rl-more-panel\"").unwrap();
        let before_panel = &after[..panel_in_toolbar];
        assert!(before_panel.contains("⋯") || before_panel.contains("id=\"rl-more-btn\""));
        assert!(!before_panel.contains("Comments"));
    }

    #[test]
    fn comments_off_lights_the_menu_dot() {
        let out = inject_chrome(
            &page_with_comments(false),
            Theme::Latte.colors(),
            PageOpts {
                editable: false,
                git_diff: false,
            },
        );
        assert!(
            out.contains("<span id=\"rl-diff-dot\" class=\"rl-diff-dot\">"),
            "hiding comments is the non-default mark"
        );
        assert!(
            !out.contains("<span id=\"rl-diff-dot\" class=\"rl-diff-dot\" hidden>"),
            "the dot is not left hidden"
        );
        assert!(out.contains("aria-label=\"More, comments off\""));
        let panel = out.find("id=\"rl-more-panel\"").unwrap();
        let slice = &out[panel..panel + 800];
        assert!(
            slice.contains("id=\"rl-comments\" role=\"switch\""),
            "{slice}"
        );
        assert!(
            !slice.contains("id=\"rl-comments\" role=\"switch\" checked"),
            "off stays unchecked"
        );
        assert!(
            out.contains("var(--rl-"),
            "latte still paints the switch with theme tokens"
        );
    }

    #[test]
    fn git_diff_on_names_the_trust_note() {
        let html = "<!doctype html><html><head><title>x</title></head><body><main><p>hi</p></main></body></html>";
        let out = inject_chrome(
            html,
            Theme::Dark.colors(),
            PageOpts {
                editable: false,
                git_diff: true,
            },
        );
        assert!(out.contains("data-rl-git=\"on\""));
        assert!(out.contains("Compare only a repo you trust"));
        assert!(!out.contains("aria-disabled=\"true\""));
        assert!(
            out.contains("gitOn"),
            "the page skips git when the flag is off"
        );
    }

    #[test]
    fn editable_injects_codemirror_and_save() {
        let out = render(true);
        assert!(out.contains("id=\"rl-editor-host\""), "editor host present");
        assert!(out.contains("id=\"rl-source-save\""), "Save button present");
        assert!(out.contains(">Edit<"), "button reads Edit in editable mode");
        assert!(out.contains("/assets/codemirror/codemirror.min.js"));
        assert!(out.contains("/assets/codemirror/codemirror.min.css"));
        assert!(
            out.contains(".cm-link"),
            "markdown links must not keep CodeMirror's dark blue"
        );
        assert!(out.contains(".cm-keyword"), "keyword token override");
        assert!(out.contains(".cm-bracket"), "bracket token override");
        assert!(
            out.contains(".cm-searching"),
            "search match must not keep the yellow wash"
        );
        assert!(out.contains("background-color: transparent"));
        assert!(
            !out.contains("#00f")
                && !out.contains("#00c")
                && !out.contains("#ffa")
                && !out.contains("#997"),
            "default CodeMirror font colors must not be re-emitted"
        );
        assert!(out.contains("/assets/codemirror/mode/markdown/markdown.min.js"));
        assert!(
            out.contains("/save/' + encodeSlug(slug)"),
            "save POST target present"
        );
        assert!(out.contains("CodeMirror("), "editor is constructed");
        // Reconnect-reload guard + clean-on-save protect unsaved edits.
        assert!(
            out.contains("__rlBlockReload"),
            "reconnect-reload veto present"
        );
        assert!(out.contains("markClean"), "buffer marked clean after save");
        // The read-only <pre> element is absent in editable mode (the JS
        // still defines a loadReadonly helper, so check the element markup).
        assert!(
            !out.contains("id=\"rl-source-pre\""),
            "no read-only <pre> element in editable mode"
        );
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
        assert!(
            out.find("#rl-source-pane {").unwrap() < head_close,
            "style in <head>"
        );
        assert!(
            out.find("/assets/codemirror/codemirror.min.css").unwrap() < head_close,
            "editor stylesheet link in <head>"
        );
        let body_close = out.rfind("</body>").unwrap();
        assert!(
            out.find("id=\"rl-toolbar\"").unwrap() < body_close,
            "chrome before </body>"
        );
    }

    #[test]
    fn falls_back_when_no_head_or_body() {
        let out = inject_chrome(
            "<p>bare</p>",
            Theme::Dark.colors(),
            PageOpts {
                editable: false,
                git_diff: false,
            },
        );
        assert!(out.contains("rl-toolbar"));
        assert!(out.contains("<p>bare</p>"));
    }
}
