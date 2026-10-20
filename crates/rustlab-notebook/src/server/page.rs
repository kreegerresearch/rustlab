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
    /// `watch --git-diff`. When false Show diff is disabled and the
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
    hoist_comments_toggle(&page)
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
    border-radius: 50%; background: var(--rl-diff-add-bar, {accent}); pointer-events: none;
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
  /* In-panel switches stay full contrast. A disabled Show diff stays dim,
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
  #rl-diff {{ display: flex; flex-direction: column; gap: 6px; align-items: stretch; }}
  .rl-diff-nav {{ display: flex; gap: 4px; align-items: center; }}
  #rl-diff[hidden], #rl-diff-banner[hidden], .rl-diff-menu[hidden],
  .rl-more-panel[hidden], #rl-diff-dot[hidden] {{ display: none !important; }}
  #rl-diff input {{
    font: 12px/1.2 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    background: {bg}; color: {text};
    border: 1px solid {border}; border-radius: 6px;
    padding: 3px 6px; width: 100%; box-sizing: border-box;
  }}
  .rl-diff-lab {{ font: 12px/1 system-ui, sans-serif; color: {text_dim}; display: flex; gap: 4px; align-items: center; }}
  .rl-diff-combo {{ position: relative; display: flex; flex: 1; min-width: 0; }}
  .rl-diff-menu {{
    position: absolute; top: calc(100% + 2px); left: 0; z-index: 100001;
    margin: 0; padding: 2px; list-style: none;
    min-width: 100%; width: max-content; max-width: 16rem; max-height: 11rem; overflow: auto;
    background: {bg_secondary}; color: {text};
    border: 1px solid {border}; border-radius: 6px;
    font: 12px/1.35 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  }}
  .rl-diff-menu li {{ padding: 2px 6px; cursor: pointer; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }}
  .rl-diff-menu li[aria-selected="true"] {{ background: {accent}; color: {bg}; }}
  .rl-diff-menu li.rl-diff-hint {{ color: {text_dim}; cursor: default; }}
  #rl-diff-pos {{ font: 11px/1 ui-monospace, monospace; color: {text_dim}; min-width: 2.2rem; text-align: center; }}
  #rl-diff-banner {{
    position: fixed; top: 46px; right: 14px; z-index: 100000; margin: 0;
    padding: 4px 8px; max-width: 28rem;
    background: {bg_secondary}; color: {text_dim};
    border: 1px solid {border}; border-radius: 6px;
    font: 12px/1.35 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  }}
  .rl-diff-notice {{
    margin: 0 0 1rem; padding: 0.6rem 0.85rem;
    background: {bg_secondary}; color: {text};
    border: 1px solid {border}; border-left: 4px solid {accent};
    border-radius: 6px;
    font: 14px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  }}
  section.rl-diff-add {{
    background: var(--rl-diff-add-bg, transparent);
    box-shadow: inset 4px 0 0 var(--rl-diff-add-bar, transparent);
  }}
  section.rl-diff-del {{
    background: var(--rl-diff-del-bg, transparent);
    box-shadow: inset 4px 0 0 var(--rl-diff-del-bar, transparent);
  }}
  section.rl-diff-del .prose,
  section.rl-diff-del pre {{
    text-decoration: line-through;
    text-decoration-color: var(--rl-diff-del-bar, currentColor);
  }}
  section.rl-diff-chg {{
    background: var(--rl-diff-chg-bg, transparent);
    box-shadow: inset 4px 0 0 var(--rl-diff-chg-bar, transparent);
  }}
  .rl-diff-tag, .rl-diff-note {{
    font: 11px/1.3 -apple-system, system-ui, sans-serif; color: {text_dim}; margin: 4px 8px;
  }}
  .rl-diff-lines {{ margin: 4px 0 8px; }}
  .rl-ln {{
    display: flex; align-items: stretch;
    background: var(--rl-code-bg, {code_bg}); color: {text};
    font: 12px/1.45 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  }}
  .rl-ln-gutter {{
    width: 1.4em; flex: none; text-align: center; color: inherit; user-select: none;
  }}
  .rl-ln-add {{
    background: var(--rl-diff-add-line, var(--rl-diff-add-bg));
    box-shadow: inset 4px 0 0 var(--rl-diff-add-bar, transparent);
  }}
  .rl-ln-del {{
    background: var(--rl-diff-del-line, var(--rl-diff-del-bg));
    box-shadow: inset 4px 0 0 var(--rl-diff-del-bar, transparent);
  }}
  .rl-ln-del code {{ text-decoration: line-through; text-decoration-color: var(--rl-diff-del-bar, currentColor); }}
  .rl-ln code {{ white-space: pre-wrap; flex: 1; }}
  .fb-git {{
    display: inline-block; width: 7px; height: 7px; border-radius: 50%;
    margin-left: 6px; background: var(--rl-diff-add-bar, currentColor); vertical-align: middle;
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
    // Show diff live in that panel. The picker stays hidden until diff
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
    let diff_chrome = r#"<div id="rl-more" class="rl-more"><button id="rl-more-btn" type="button" class="rl-quiet" aria-expanded="false" aria-haspopup="true" aria-controls="rl-more-panel" aria-label="More" title="More">⋯</button><span id="rl-diff-dot" class="rl-diff-dot" hidden></span><div id="rl-more-panel" class="rl-more-panel" hidden role="region" aria-label="More"><button id="rl-diff-toggle" type="button" class="rl-quiet" role="switch" aria-checked="false"><span class="rl-switch" aria-hidden="true"></span>Show diff</button><div id="rl-diff" hidden><div class="rl-diff-nav"><button id="rl-diff-prev" type="button" title="Previous change ([)">‹</button><span id="rl-diff-pos"></span><button id="rl-diff-next" type="button" title="Next change (])">›</button></div><label class="rl-diff-lab">vs <span class="rl-diff-combo"><input id="rl-diff-ref" class="rl-diff-filter" type="text" autocomplete="off" spellcheck="false" aria-label="Compare against" aria-expanded="false" aria-controls="rl-diff-ref-list"><ul id="rl-diff-ref-list" class="rl-diff-menu" hidden role="listbox"></ul></span></label><label class="rl-diff-lab">from <span class="rl-diff-combo"><input id="rl-diff-base" class="rl-diff-filter" type="text" autocomplete="off" spellcheck="false" aria-label="Compare from" aria-expanded="false" aria-controls="rl-diff-base-list"><ul id="rl-diff-base-list" class="rl-diff-menu" hidden role="listbox"></ul></span></label></div></div></div><p id="rl-diff-banner" hidden></p>"#;
    let diff_chrome = diff_chrome
        .replace(
            "aria-checked=\"false\"",
            &format!("aria-checked=\"false\"{git_extra}"),
        )
        .replace(
            "Show diff</button>",
            &format!("Show diff</button><p class=\"rl-git-note\">{git_note}</p>"),
        );
    let toolbar = if opts.editable {
        format!(
            "<div id=\"rl-toolbar\">{diff_chrome}<button id=\"rl-source-save\" hidden title=\"Save (Ctrl/Cmd-S)\">Save</button><button id=\"rl-source-toggle\" title=\"Edit notebook source\">Edit</button></div>"
        )
    } else {
        format!(
            "<div id=\"rl-toolbar\">{diff_chrome}<button id=\"rl-source-toggle\" title=\"Show notebook source\">Source</button></div>"
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

/// Picker, keyboard prev/next, and branch list. A raw string so the
/// braces are not doubled by the chrome `format!`.
///
/// `rlDiffFilter` / `rlDiffRefOk` / `rlDiffChoose` stay at the top so a
/// node test can load this file. `rlDiffRefOk` matches `gitdiff::ref_ok`.
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
  if (diffOn) bits.push('diff on');
  if (commentsOff) bits.push('comments off');
  return {
    dot: bits.length > 0,
    label: bits.length ? ('More, ' + bits.join(', ')) : 'More'
  };
}
function rlDiffPrefApply(queryDiff, queryBase, bodyOn, saved) {
  if (queryDiff === 'off') return { action: 'store-off', diff: '', base: '' };
  if (queryDiff) return { action: 'store-on', diff: queryDiff, base: queryBase || '' };
  if (bodyOn) return { action: 'keep', diff: '', base: '' };
  if (saved && saved.on) {
    var diff = saved.diff || 'HEAD';
    var base = saved.base || '';
    if (!rlDiffRefOk(diff) || (base && !rlDiffRefOk(base))) return { action: 'none', diff: '', base: '' };
    return { action: 'navigate', diff: diff, base: base };
  }
  return { action: 'none', diff: '', base: '' };
}
(function () {
  if (typeof document === 'undefined') return;
  var toggle = document.getElementById('rl-diff-toggle');
  if (!toggle) return;
  var gitOn = toggle.getAttribute('data-rl-git') === 'on';
  var moreBtn = document.getElementById('rl-more-btn');
  var panel = document.getElementById('rl-more-panel');
  var dot = document.getElementById('rl-diff-dot');
  var vs = document.getElementById('rl-diff-ref');
  var from = document.getElementById('rl-diff-base');
  var vsList = document.getElementById('rl-diff-ref-list');
  var fromList = document.getElementById('rl-diff-base-list');
  var pos = document.getElementById('rl-diff-pos');
  var PREF = 'rl-diff-pref';
  function readPref() {
    try {
      var raw = sessionStorage.getItem(PREF);
      return raw ? JSON.parse(raw) : null;
    } catch (err) { return null; }
  }
  function store(onFlag, diff, base) {
    try {
      sessionStorage.setItem(PREF, JSON.stringify({
        on: !!onFlag, diff: diff || 'HEAD', base: base || ''
      }));
    } catch (err) {}
  }

  function params() {
    var cur = new URLSearchParams(location.search);
    if (!cur.get('diff') && document.body.dataset.rlDiff) {
      cur.set('diff', document.body.dataset.rlDiff);
    }
    if (!cur.get('base') && document.body.dataset.rlBase) {
      cur.set('base', document.body.dataset.rlBase);
    }
    return cur;
  }
  function go(diff, base) {
    var u = new URL(location.href);
    var cur = params();
    if (!diff || diff === 'off') {
      store(false, '', '');
      u.searchParams.set('diff', 'off');
      u.searchParams.delete('base');
    } else {
      store(true, diff, base || '');
      u.searchParams.set('diff', diff);
      if (base) u.searchParams.set('base', base);
      else u.searchParams.delete('base');
    }
    if (cur.get('cm')) u.searchParams.set('cm', cur.get('cm'));
    location.assign(u.toString());
  }
  function on() {
    return document.body.classList.contains('rl-diff-on');
  }
  (function restore() {
    if (!gitOn) return;
    var q = new URLSearchParams(location.search);
    var decision = rlDiffPrefApply(q.get('diff'), q.get('base') || '', on(), readPref());
    if (decision.action === 'store-off') store(false, '', '');
    else if (decision.action === 'store-on') store(true, decision.diff, decision.base);
    else if (decision.action === 'keep') {
      store(true, document.body.dataset.rlDiff || 'HEAD', document.body.dataset.rlBase || '');
    } else if (decision.action === 'navigate') go(decision.diff, decision.base);
  })();
  function menuOpen() { return !!(panel && !panel.hidden); }
  function setMenu(open) {
    if (!panel || !moreBtn) return;
    panel.hidden = !open;
    moreBtn.setAttribute('aria-expanded', open ? 'true' : 'false');
    if (open) {
      var first = panel.querySelector('button, input, a, select, textarea');
      if (first) first.focus();
      else toggle.focus();
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
      setMenu(false);
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
    if (on()) go('off', '');
    else go('HEAD', '');
  });

  function labelOf(items, value) {
    for (var i = 0; i < items.length; i++) {
      if (items[i].value === value) return items[i].label;
    }
    return value;
  }
  function mount(input, list, which) {
    if (!input || !list) return { setItems: function () {}, value: function () { return ''; } };
    var items = [];
    var filtering = false;
    var highlight = 0;
    var armed = false;
    function committed() { return input.getAttribute('data-value') || ''; }
    function query() { return filtering ? input.value : ''; }
    function rows() { return rlDiffFilter(items, query()); }
    function close(restore) {
      list.hidden = true;
      input.setAttribute('aria-expanded', 'false');
      if (restore) input.value = labelOf(items, committed());
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
        found.forEach(function (it, i) {
          var li = document.createElement('li');
          li.textContent = it.label;
          li.setAttribute('role', 'option');
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
      var diff = which === 'diff' ? value : (vs ? (vs.getAttribute('data-value') || 'HEAD') : 'HEAD');
      var base = which === 'base' ? value : (from ? (from.getAttribute('data-value') || '') : '');
      if (value === committed()) { close(true); return; }
      go(diff, base);
    }
    input.addEventListener('focus', function () {
      filtering = false;
      armed = false;
      var all = rlDiffFilter(items, '');
      highlight = 0;
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
        if (!n) { render(); return; }
        if (list.hidden) { render(); return; }
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
        input.value = labelOf(items, value);
        close(false);
        input.value = labelOf(items, value);
      }
    };
  }
  var vsCombo = mount(vs, vsList, 'diff');
  var fromCombo = mount(from, fromList, 'base');
  if (vs && document.body.dataset.rlDiff) {
    vs.setAttribute('data-value', document.body.dataset.rlDiff);
    vs.value = document.body.dataset.rlDiff;
  }
  if (from) {
    var initialBase = document.body.dataset.rlBase || '';
    from.setAttribute('data-value', initialBase);
    from.value = initialBase || 'working copy';
  }
  function fill(data) {
    var branches = (data && data.branches) || [];
    var shared = [];
    branches.forEach(function (b) {
      shared.push({ value: b.name, label: b.current ? b.name + ' (current)' : b.name });
    });
    shared.push({ value: 'HEAD', label: 'HEAD' });
    shared.push({ value: 'index', label: 'index' });
    ((data && data.tags) || []).forEach(function (t) {
      shared.push({ value: t.name, label: t.name });
    });
    ((data && data.commits) || []).forEach(function (c) {
      shared.push({ value: c.rev, label: c.label });
    });
    var want = document.body.dataset.rlDiff || 'HEAD';
    var base = document.body.dataset.rlBase || '';
    var vsItems = shared.slice();
    if (!vsItems.some(function (it) { return it.value === want; })) {
      vsItems.push({ value: want, label: want });
    }
    var fromItems = [{ value: '', label: 'working copy' }].concat(shared);
    if (base && !fromItems.some(function (it) { return it.value === base; })) {
      fromItems.push({ value: base, label: base });
    }
    vsCombo.setItems(vsItems, want);
    fromCombo.setItems(fromItems, base);
    if (data && data.error) {
      var banner = document.getElementById('rl-diff-banner');
      if (banner && !banner.textContent) banner.textContent = data.error;
    }
  }
  if (gitOn && on() && vs) {
    fetch('/git/refs').then(function (r) { return r.json(); }).then(fill).catch(function () {});
  }

  function changes() {
    return Array.prototype.slice.call(document.querySelectorAll('main section.rl-diff'));
  }
  function fullyVisible(el) {
    var r = el.getBoundingClientRect();
    return r.top >= 0 && r.bottom <= window.innerHeight;
  }
  function step(dir) {
    var list = changes();
    if (!list.length) { if (pos) pos.textContent = '0'; return; }
    var cur = -1;
    list.forEach(function (el, i) {
      if (el.getBoundingClientRect().top < 80) cur = i;
    });
    var i = cur + dir;
    if (dir > 0) { while (i < list.length && fullyVisible(list[i])) i++; }
    else { while (i >= 0 && fullyVisible(list[i])) i--; }
    if (i < 0) i = 0;
    if (i >= list.length) i = list.length - 1;
    list[i].scrollIntoView({ block: 'center' });
    if (pos) pos.textContent = (i + 1) + '/' + list.length;
  }
  function showPos() {
    if (!pos) return;
    var list = changes();
    if (!list.length) { pos.textContent = '0/0'; return; }
    var cur = 0;
    list.forEach(function (el, i) {
      if (el.getBoundingClientRect().top < 80) cur = i;
    });
    pos.textContent = (cur + 1) + '/' + list.length;
  }
  if (on()) showPos();
  var prev = document.getElementById('rl-diff-prev');
  var next = document.getElementById('rl-diff-next');
  if (prev) prev.addEventListener('click', function () { step(-1); });
  if (next) next.addEventListener('click', function () { step(1); });
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
    var cur = params();
    ['diff', 'base', 'cm'].forEach(function (k) {
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
      var cur = params();
      if (cur.get('diff')) u.searchParams.set('diff', cur.get('diff'));
      if (cur.get('base')) u.searchParams.set('base', cur.get('base'));
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
      var dot = document.createElement('span');
      dot.className = 'fb-git';
      dot.title = hit.label || hit.status || 'changed';
      a.appendChild(dot);
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
assert.strictEqual(rlDiffPrefApply('off', '', false, {on:true, diff:'HEAD'}).action, 'store-off');
assert.strictEqual(rlDiffPrefApply('main', 'topic', false, null).action, 'store-on');
assert.strictEqual(rlDiffPrefApply('main', 'topic', false, null).base, 'topic');
assert.strictEqual(rlDiffPrefApply(null, '', true, null).action, 'keep');
assert.strictEqual(rlDiffPrefApply(null, '', false, {on:true, diff:'HEAD', base:''}).action, 'navigate');
assert.strictEqual(rlDiffPrefApply(null, '', false, {on:true, diff:'HEAD', base:''}).diff, 'HEAD');
assert.strictEqual(rlDiffPrefApply(null, '', false, {on:true, diff:'main..HEAD'}).action, 'none');
assert.strictEqual(rlDiffPrefApply(null, '', false, null).action, 'none');
var mark = rlMoreMark(false, false);
assert.strictEqual(mark.dot, false);
assert.strictEqual(mark.label, 'More');
mark = rlMoreMark(true, false);
assert.strictEqual(mark.dot, true);
assert.strictEqual(mark.label, 'More, diff on');
mark = rlMoreMark(false, true);
assert.strictEqual(mark.dot, true);
assert.strictEqual(mark.label, 'More, comments off');
mark = rlMoreMark(true, true);
assert.strictEqual(mark.dot, true);
assert.strictEqual(mark.label, 'More, diff on, comments off');
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
        assert!(out.contains(">Show diff<") || out.contains("Show diff</button>"));
        assert!(out.contains("role=\"switch\""), "diff is a switch");
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
            menu.contains("Show diff</button>"),
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
