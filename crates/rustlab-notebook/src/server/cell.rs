//! Per-block cell UI — the Jupyter-style layer of the interactive
//! server. Injects, into every server-rendered notebook page:
//!
//! - a ▶ Run button on each code section (`data-code-idx`), which sends
//!   `{"kind":"run_block","idx":N}` over the existing WS connection and
//!   force-re-executes that block and everything after it ("run from
//!   here" — upstream blocks replay from the prefix cache);
//! - a running indicator driven by broadcast
//!   `{"kind":"cell_status",…}` messages, so every connected tab sees
//!   which block is executing and when the render finished.
//!
//! Run never modifies source, so it ships in read-only mode too. Inline
//! *editing* of a cell (`cell_edit`) additionally requires `--editable`
//! and a notebook without markdown embeds — a cell edit is spliced back
//! into the host `.md` by executable ordinal, which is only sound when
//! every rendered block comes from that file.
//!
//! The script keeps all handlers *delegated* on `document` (the widget
//! precedent in `ws.rs`): sections inside `<main>` are replaced wholesale
//! by WS updates, so per-node listeners would be lost. Only the
//! decoration pass (adding the buttons back) re-runs after each update,
//! via the `__rlCellDecorate` hook called from the WS client's
//! `afterUpdate`.

use rustlab_plot::ThemeColors;

/// Inject the cell style + script into a fully-rendered page. Mirrors
/// `page::inject_chrome`'s injection points: `<style>` before `</head>`,
/// `<script>` before `</body>`, appending when the tags are absent.
pub fn inject_cell_client(html: &str, theme: &ThemeColors, cell_edit: bool) -> String {
    let style = cell_style(theme);
    let script = cell_script(cell_edit);

    let with_head = match html.find("</head>") {
        Some(idx) => {
            let (head, rest) = html.split_at(idx);
            format!("{head}{style}{rest}")
        }
        None => format!("{style}{html}"),
    };
    match with_head.rfind("</body>") {
        Some(idx) => {
            let (pre, rest) = with_head.split_at(idx);
            format!("{pre}{script}{rest}")
        }
        None => format!("{with_head}{script}"),
    }
}

fn cell_style(c: &ThemeColors) -> String {
    format!(
        r##"<style>
  main section.rl-block[data-code-idx] {{ position: relative; }}
  .rl-cell-bar {{
    position: absolute; top: 6px; right: 8px; z-index: 10;
    display: flex; gap: 6px; align-items: center;
    opacity: 0; transition: opacity .15s ease;
  }}
  main section.rl-block[data-code-idx]:hover .rl-cell-bar,
  main section.rl-block.rl-running .rl-cell-bar,
  main section.rl-block.rl-cell-editing .rl-cell-bar {{ opacity: 1; }}
  .rl-cell-bar button {{
    font: 11px/1 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    background: {bg_secondary}; color: {text};
    border: 1px solid {border}; border-radius: 5px;
    padding: 4px 9px; cursor: pointer;
  }}
  .rl-cell-bar button:hover {{ border-color: {accent}; }}
  .rl-cell-bar button:disabled {{ opacity: .55; cursor: default; }}
  .rl-cell-err {{
    font: 11px/1.3 -apple-system, system-ui, sans-serif; color: {accent};
    max-width: 40ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }}
  main section.rl-block.rl-running {{
    outline: 1px solid {accent}; outline-offset: 3px; border-radius: 2px;
    animation: rl-cell-pulse 1.2s ease-in-out infinite;
  }}
  @keyframes rl-cell-pulse {{
    0%, 100% {{ outline-color: transparent; }}
    50% {{ outline-color: {accent}; }}
  }}
  main section.rl-block.rl-cell-stale {{ opacity: .65; }}
  main section.rl-block.rl-cell-editing {{
    outline: 1px solid {border}; outline-offset: 3px; border-radius: 2px;
  }}
  .rl-cell-editor {{ border: 1px solid {border}; border-radius: 4px; margin: 4px 0; }}
  .rl-cell-editor .CodeMirror {{
    height: auto; background: {code_bg}; color: {text};
    font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  }}
  .rl-cell-editor .CodeMirror-cursor {{ border-left: 1px solid {text}; }}
  .rl-cell-editor .CodeMirror-selected {{ background: {border}; }}
  #rl-cell-banner {{
    position: fixed; bottom: 0; left: 0; right: 0; z-index: 99999;
    background: {bg_secondary}; color: {text}; border-top: 1px solid {accent};
    text-align: center; padding: 6px 10px;
    font: 12px/1.4 -apple-system, system-ui, sans-serif;
  }}
</style>
"##,
        code_bg = c.code_bg,
        bg_secondary = c.bg_secondary,
        text = c.text,
        border = c.border,
        accent = c.accent_primary,
    )
}

fn cell_script(cell_edit: bool) -> String {
    let cell_edit_js = if cell_edit { "true" } else { "false" };
    format!(
        r##"<script>
(() => {{
  // Cells exist only on notebook pages (`/n/<slug>`), like the WS client.
  const m = location.pathname.match(/^\/n\/([^\/]+)\/?$/);
  if (!m) return;
  const CELL_EDIT = {cell_edit_js};

  function codeSections() {{
    return document.querySelectorAll('main section.rl-block[data-code-idx]');
  }}

  // ── Decoration: (re-)attach the button bar to every code section ──
  // Sections are replaced wholesale by WS updates, which drops the bar;
  // this pass adds it back. Idempotent by the .rl-cell-bar guard. Also
  // reaps editor state whose section vanished in a swap (clean editors
  // only — dirty ones veto the swaps that could remove them).
  function decorate() {{
    if (ed && !document.contains(ed.sec)) ed = null;
    codeSections().forEach(sec => {{
      if (sec.querySelector(':scope > .rl-cell-bar')) return;
      const bar = document.createElement('div');
      bar.className = 'rl-cell-bar';
      const run = document.createElement('button');
      run.className = 'rl-cell-run';
      run.title = 'Run from this block (downstream blocks re-run too)';
      run.textContent = '▶ Run';
      bar.appendChild(run);
      if (CELL_EDIT && sec.querySelector('pre.source')) {{
        const edit = document.createElement('button');
        edit.className = 'rl-cell-edit';
        edit.title = 'Edit this block (Shift+Enter saves and runs, Esc cancels)';
        edit.textContent = '✎ Edit';
        bar.appendChild(edit);
      }}
      const err = document.createElement('span');
      err.className = 'rl-cell-err';
      bar.appendChild(err);
      sec.prepend(bar);
    }});
  }}

  function sectionFor(el) {{
    return el.closest('main section.rl-block[data-code-idx]');
  }}
  function idxOf(sec) {{
    const n = parseInt(sec.getAttribute('data-code-idx'), 10);
    return isFinite(n) && n >= 0 ? n : null;
  }}
  function setRunning(sec) {{
    if (sec) sec.classList.add('rl-running');
  }}
  function clearRunning() {{
    document.querySelectorAll('main section.rl-running')
      .forEach(s => s.classList.remove('rl-running'));
  }}
  function setErr(sec, msg) {{
    const el = sec && sec.querySelector('.rl-cell-err');
    if (el) el.textContent = msg;
  }}

  // ── Inline cell editor (CELL_EDIT only) ───────────────────────────
  // At most one editor at a time — the simplest model that stays
  // coherent with whole-document re-renders. `ed.seed` is both the
  // dirty baseline and the save's CAS `prev_source`.
  let ed = null; // {{ sec, cm, pre, host, seed, idx }}
  let deferredReload = false;
  let banner = null;

  function showBanner(text) {{
    if (!banner) {{
      banner = document.createElement('div');
      banner.id = 'rl-cell-banner';
      document.body.appendChild(banner);
    }}
    banner.textContent = text;
  }}
  function hideBanner() {{
    if (banner) {{ banner.remove(); banner = null; }}
  }}

  function isDirty() {{
    return !!ed && ed.cm.getValue() !== ed.seed;
  }}

  function openEditor(sec) {{
    if (!CELL_EDIT || typeof CodeMirror === 'undefined') return;
    // The whole-document editor and inline cells are mutually
    // exclusive: both write the same file.
    if (document.body.classList.contains('rl-source-open')) {{
      setErr(sec, 'close the source pane first');
      return;
    }}
    if (ed) {{
      if (isDirty()) {{ setErr(sec, 'another cell has unsaved changes'); return; }}
      closeEditor();
    }}
    const pre = sec.querySelector('pre.source');
    if (!pre) return;
    // The highlighted <pre> round-trips to the exact block source via
    // textContent (spans strip, entities unescape — pinned by a unit
    // test on highlight_rustlab).
    const code = pre.querySelector('code');
    const seed = (code || pre).textContent;
    const host = document.createElement('div');
    host.className = 'rl-cell-editor';
    pre.after(host);
    pre.style.display = 'none';
    const cm = CodeMirror(host, {{
      value: seed, lineNumbers: false, lineWrapping: true,
      viewportMargin: Infinity,
      extraKeys: {{ 'Shift-Enter': saveRun, 'Esc': closeEditor }},
    }});
    sec.classList.add('rl-cell-editing');
    ed = {{ sec, cm, pre, host, seed, idx: idxOf(sec) }};
    cm.focus();
  }}

  // Close the editor and restore the rendered source. If a structural
  // update was deferred (or this section was patched around us) the
  // page is stale — reload to resync.
  function closeEditor() {{
    if (!ed) return;
    const stale = deferredReload || ed.sec.classList.contains('rl-cell-stale');
    ed.pre.style.display = '';
    ed.host.remove();
    ed.sec.classList.remove('rl-cell-editing');
    ed = null;
    hideBanner();
    if (stale) {{ deferredReload = false; location.reload(); }}
  }}

  function saveRun() {{
    if (!ed || ed.idx === null) return;
    setErr(ed.sec, '');
    setRunning(ed.sec);
    if (window.__rlSend) {{
      window.__rlSend({{
        kind: 'save_run_block',
        idx: ed.idx,
        source: ed.cm.getValue(),
        prev_source: ed.seed,
      }});
    }}
  }}

  // ── Delegated clicks (survive DOM swaps) ──────────────────────────
  document.addEventListener('click', (ev) => {{
    const t = ev.target;
    if (!t || !t.closest) return;
    const runBtn = t.closest('.rl-cell-run');
    if (runBtn) {{
      const sec = sectionFor(runBtn);
      if (!sec) return;
      // Run on the section being edited = save-and-run.
      if (ed && sec === ed.sec) {{ saveRun(); return; }}
      const idx = idxOf(sec);
      if (idx === null) return;
      // Optimistic local indicator; the broadcast `running` status
      // confirms (and mirrors it to other tabs).
      setRunning(sec);
      if (window.__rlSend) window.__rlSend({{ kind: 'run_block', idx }});
      return;
    }}
    const editBtn = t.closest('.rl-cell-edit');
    if (editBtn) {{
      const sec = sectionFor(editBtn);
      if (sec) openEditor(sec);
    }}
  }});

  // ── Inbound cell messages (forwarded by the WS client) ────────────
  window.__rlCellMessage = (msg) => {{
    if (msg.kind === 'cell_status') {{
      if (msg.state === 'running' && typeof msg.idx === 'number') {{
        setRunning(document.querySelector(
          'main section.rl-block[data-code-idx="' + msg.idx + '"]'));
      }} else if (msg.state === 'done') {{
        // Renders are whole-document: one terminal status clears all.
        clearRunning();
      }}
      return;
    }}
    if (msg.kind === 'cell_saved') {{
      if (!ed || msg.idx !== ed.idx) return;
      if (msg.ok) {{
        // Mark clean and close immediately; the follow-up render patch
        // repaints the section. (A re-save of identical content yields
        // no patch at all — waiting for one would hang the editor.)
        ed.seed = ed.cm.getValue();
        closeEditor();
      }} else {{
        clearRunning();
        setErr(ed.sec, msg.error || 'save failed');
      }}
    }}
  }};

  // ── Hooks for the WS transport (see ws.rs) ────────────────────────
  // A dirty editor's own section must not be clobbered by a partial
  // patch; structural swaps are deferred wholesale until it closes.
  window.__rlCellVetoSection = (el) => !!ed && isDirty() && el === ed.sec;
  window.__rlCellVetoStructural = () => {{
    if (ed && isDirty()) {{
      deferredReload = true;
      showBanner('notebook changed on disk — save or close the cell editor to refresh');
      return true;
    }}
    return false;
  }};
  window.__rlCellDirty = () => !!ed && isDirty();

  // Re-decorate after every WS DOM update, and once at load.
  window.__rlCellDecorate = decorate;
  decorate();
}})();
</script>
"##,
        cell_edit_js = cell_edit_js,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustlab_plot::Theme;

    fn page(cell_edit: bool) -> String {
        let html = "<!doctype html><html><head><title>x</title></head>\
                    <body><main><section class=\"rl-block\" id=\"b-a\" data-code-idx=\"0\">\
                    <div class=\"code-block\">x</div></section></main></body></html>";
        inject_cell_client(html, Theme::Dark.colors(), cell_edit)
    }

    #[test]
    fn injects_style_in_head_and_script_before_body_close() {
        let out = page(false);
        let head_close = out.find("</head>").unwrap();
        assert!(
            out.find(".rl-cell-bar {").unwrap() < head_close,
            "cell style must land in <head>"
        );
        let body_close = out.rfind("</body>").unwrap();
        assert!(
            out.find("__rlCellDecorate").unwrap() < body_close,
            "cell script must land before </body>"
        );
        // Original content preserved.
        assert!(out.contains("data-code-idx=\"0\""));
    }

    #[test]
    fn cell_edit_flag_threads_into_script() {
        assert!(page(false).contains("CELL_EDIT = false"));
        assert!(page(true).contains("CELL_EDIT = true"));
    }

    #[test]
    fn run_button_and_status_handling_present() {
        let out = page(false);
        assert!(out.contains("rl-cell-run"), "run button class");
        assert!(out.contains("run_block"), "outbound run message kind");
        assert!(out.contains("cell_status"), "inbound status kind");
        assert!(out.contains("rl-running"), "running indicator class");
    }

    #[test]
    fn falls_back_when_no_head_or_body() {
        let out = inject_cell_client("<p>bare</p>", Theme::Dark.colors(), false);
        assert!(out.contains("rl-cell-bar"));
        assert!(out.contains("<p>bare</p>"));
    }

    /// The generated script must be syntactically valid JS. Uses
    /// `node --check` when node is available; skips (with a note)
    /// otherwise so the suite doesn't hard-depend on node.
    #[test]
    fn cell_script_passes_node_check() {
        for edit in [false, true] {
            let script = cell_script(edit);
            let js = script
                .trim_start()
                .strip_prefix("<script>")
                .unwrap()
                .strip_suffix("</script>\n")
                .unwrap_or_else(|| panic!("script wrapper shape changed"));
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("cell.js");
            std::fs::write(&path, js).unwrap();
            match std::process::Command::new("node").arg("--check").arg(&path).output() {
                Ok(out) => assert!(
                    out.status.success(),
                    "node --check failed (cell_edit={edit}):\n{}",
                    String::from_utf8_lossy(&out.stderr)
                ),
                Err(_) => {
                    eprintln!("node not available — skipping JS syntax check");
                    return;
                }
            }
        }
    }
}
