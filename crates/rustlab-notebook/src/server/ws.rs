//! WebSocket endpoint — pushes re-rendered HTML to the browser on
//! save. Phase 2 implementation: full-document refresh only,
//! discriminated message envelope `{"kind":"full","html":"…"}` shipped
//! over text frames. Phase 3 will add `{"kind":"partial","blocks":[…]}`
//! as a sibling variant without changing the schema.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rustlab_script::WidgetValue;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc::UnboundedSender;

use super::http::{Notebook, ServerState};
use super::render_loop::RenderRequest;

/// Axum upgrade handler for `/n/{slug}/ws`. Resolves the notebook by
/// slug, then hands the socket to [`handle_socket`] bound to that
/// notebook's broadcast channel. Unknown slug → 404 (no upgrade).
pub async fn ws_upgrade(
    State(state): State<Arc<ServerState>>,
    AxPath(slug): AxPath<String>,
    ws: WebSocketUpgrade,
) -> Response {
    match state.notebook(&slug) {
        Some(nb) => {
            let nb = nb.clone();
            // Clone the render-request sender so the widget_update handler
            // can ask the coordinator to re-render this notebook.
            let render_tx = state.render_tx.get().cloned();
            let editable = state.editable;
            ws.on_upgrade(move |socket| handle_socket(socket, nb, slug, render_tx, editable))
        }
        None => (StatusCode::NOT_FOUND, "notebook not found").into_response(),
    }
}

/// Per-connection task: stream every re-render as it lands. We do
/// *not* send an initial-sync message — the client already has the
/// rendered body from `GET /notebook.html`, and sending it again
/// would force a wasteful DOM replacement + Plotly re-init on a
/// page that hasn't changed. On reconnect after a disconnect the
/// client triggers a hard `location.reload()` instead, which is the
/// honest "I may have missed something" recovery path.
///
/// Inbound `{"kind":"widget_update","name":…,"value":…}` messages update
/// this notebook's live widget values (server-side, ephemeral) and ping the
/// render coordinator, which re-renders with the new values as overrides and
/// pushes the result back over this same channel. Other inbound text is
/// logged and dropped. See `dev/plans/notebook_interactive_widgets.md`.
async fn handle_socket(
    mut socket: WebSocket,
    nb: Arc<Notebook>,
    slug: String,
    render_tx: Option<UnboundedSender<RenderRequest>>,
    editable: bool,
) {
    let mut rx = nb.broadcast.subscribe();

    loop {
        tokio::select! {
            // Inbound from client.
            inbound = socket.recv() => {
                match inbound {
                    None => return, // socket closed
                    Some(Ok(Message::Close(_))) => return,
                    Some(Ok(Message::Text(payload))) => {
                        if let Some((name, value)) = parse_widget_update(&payload) {
                            // Validate against the widget's declaration:
                            // unknown name or out-of-range / unknown-choice
                            // value is logged and ignored (never crashes the
                            // render loop). A valid value is clamped/accepted
                            // by `coerce` and stored as the live value.
                            let coerced = nb
                                .widget_decls
                                .lock()
                                .unwrap()
                                .iter()
                                .find(|d| d.name == name)
                                .map(|d| d.coerce(&value));
                            match coerced {
                                Some(Some(v)) => {
                                    nb.widget_values.lock().unwrap().insert(name, v);
                                    // Ask the coordinator to re-render.
                                    if let Some(tx) = &render_tx {
                                        let _ = tx.send(RenderRequest::rerender(slug.clone()));
                                    }
                                }
                                Some(None) => eprintln!(
                                    "[watch] ws: rejecting invalid value for widget '{name}': {value:?}"
                                ),
                                None => eprintln!(
                                    "[watch] ws: ignoring update for unknown widget '{name}'"
                                ),
                            }
                        } else if let Some(idx) = parse_run_block(&payload) {
                            // ▶ Run: broadcast the running status so every
                            // tab shows the spinner, then request a forced
                            // render through the coordinator (same debounce
                            // + preemption path as a save). The index is
                            // advisory — the executor clamps it, so a stale
                            // ordinal can widen the re-run scope but never
                            // corrupt state.
                            let env: Arc<str> = Arc::from(cell_status_running_envelope(idx));
                            let _ = nb.broadcast.send(env);
                            if let Some(tx) = &render_tx {
                                let _ = tx.send(RenderRequest {
                                    slug: slug.clone(),
                                    force_from: Some(idx),
                                });
                            }
                        } else if let Some(req) = parse_save_run_block(&payload) {
                            // Shift+Enter on an inline cell: splice the new
                            // block body into the .md (guarded by CAS +
                            // post-splice validation), then run from it.
                            // The per-request verdict goes back on THIS
                            // socket only; the running status broadcasts.
                            let idx = req.idx;
                            match handle_cell_save(&nb, editable, req).await {
                                Ok(()) => {
                                    let ok = cell_saved_envelope(idx, Ok(()));
                                    if socket.send(Message::Text(ok.into())).await.is_err() {
                                        return;
                                    }
                                    let env: Arc<str> =
                                        Arc::from(cell_status_running_envelope(idx));
                                    let _ = nb.broadcast.send(env);
                                    if let Some(tx) = &render_tx {
                                        let _ = tx.send(RenderRequest {
                                            slug: slug.clone(),
                                            force_from: Some(idx),
                                        });
                                    }
                                }
                                Err(e) => {
                                    eprintln!(
                                        "[watch] ws: cell save rejected ({slug}#{idx}): {e}"
                                    );
                                    let err = cell_saved_envelope(idx, Err(&e));
                                    if socket.send(Message::Text(err.into())).await.is_err() {
                                        return;
                                    }
                                }
                            }
                        } else {
                            eprintln!(
                                "[watch] ws: ignoring unrecognised text message: {}",
                                truncate_for_log(&payload),
                            );
                        }
                    }
                    Some(Ok(Message::Binary(_))) => {
                        eprintln!("[watch] ws: ignoring binary message");
                    }
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                    Some(Err(e)) => {
                        eprintln!("[watch] ws read error: {e}");
                        return;
                    }
                }
            }

            // Outbound from broadcast.
            outbound = rx.recv() => {
                match outbound {
                    Ok(msg) => {
                        if socket
                            .send(Message::Text((*msg).to_string().into()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(RecvError::Lagged(_)) => {
                        // Client fell so far behind we lost frames; re-sync
                        // from the latest state and keep going.
                        let resync = {
                            let guard = nb.html.read().await;
                            full_envelope(&guard)
                        };
                        if socket.send(Message::Text(resync.into())).await.is_err() {
                            return;
                        }
                    }
                    Err(RecvError::Closed) => return,
                }
            }
        }
    }
}

/// Wrap a chunk of HTML in the Phase-2 `{"kind":"full",…}` envelope.
/// Public so the render loop can pre-frame messages once per render
/// and broadcast the resulting `Arc<str>` to every receiver.
pub fn full_envelope(html: &str) -> String {
    serde_json::json!({ "kind": "full", "html": html }).to_string()
}

/// Parse an inbound `{"kind":"widget_update","name":"…","value":…}` frame
/// into `(name, value)`. The value is a JSON number (slider / number) or
/// string (option) → [`WidgetValue`]. Returns `None` for any other message
/// kind, a missing/blank name, a non-finite number, or a value that is
/// neither a number nor a string — so garbage never reaches the render
/// loop. Range / choice validation against the declaration happens in the
/// handler (see [`Notebook::widget_decls`]).
fn parse_widget_update(payload: &str) -> Option<(String, WidgetValue)> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    if v.get("kind")?.as_str()? != "widget_update" {
        return None;
    }
    let name = v.get("name")?.as_str()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let value = match v.get("value")? {
        serde_json::Value::Number(n) => {
            let f = n.as_f64()?;
            if !f.is_finite() {
                return None;
            }
            WidgetValue::Number(f)
        }
        serde_json::Value::String(s) => WidgetValue::Text(s.clone()),
        _ => return None,
    };
    Some((name, value))
}

/// Parse an inbound `{"kind":"run_block","idx":N}` frame into the
/// executable-block ordinal. Same defensive posture as
/// [`parse_widget_update`]: anything malformed (wrong kind, missing /
/// negative / fractional / non-numeric idx) returns `None` and is
/// logged-and-dropped by the caller.
fn parse_run_block(payload: &str) -> Option<usize> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    if v.get("kind")?.as_str()? != "run_block" {
        return None;
    }
    usize::try_from(v.get("idx")?.as_u64()?).ok()
}

/// An inbound `{"kind":"save_run_block",…}` request: replace executable
/// block `idx`'s source with `source`, provided the on-disk block still
/// reads `prev_source` (compare-and-swap against two-tab / external
/// edits), then force-run from it.
#[derive(Debug, PartialEq, Eq)]
struct SaveRunBlock {
    idx: usize,
    source: String,
    prev_source: String,
}

/// Parse an inbound save-and-run frame. Defensive like the other
/// parsers: wrong kind or any missing/mistyped field → `None`.
fn parse_save_run_block(payload: &str) -> Option<SaveRunBlock> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    if v.get("kind")?.as_str()? != "save_run_block" {
        return None;
    }
    Some(SaveRunBlock {
        idx: usize::try_from(v.get("idx")?.as_u64()?).ok()?,
        source: v.get("source")?.as_str()?.to_string(),
        prev_source: v.get("prev_source")?.as_str()?.to_string(),
    })
}

/// `{"kind":"cell_saved","idx":N,"ok":…}` — the per-request verdict for a
/// save-and-run, sent only to the requesting socket. Failures carry a
/// human-readable `error`.
fn cell_saved_envelope(idx: usize, result: Result<(), &str>) -> String {
    match result {
        Ok(()) => serde_json::json!({ "kind": "cell_saved", "idx": idx, "ok": true }),
        Err(e) => {
            serde_json::json!({ "kind": "cell_saved", "idx": idx, "ok": false, "error": e })
        }
    }
    .to_string()
}

/// Locate executable block `exec_idx` (Code + Mermaid ordinal — the
/// cache-slot / `data-code-idx` numbering) in a parsed block list.
/// Returns the block's ```rustlab-fence ordinal (counting Code blocks
/// only — what `replace_code_block_source` addresses) and its source.
fn locate_code_block(
    blocks: &[crate::parse::Block],
    exec_idx: usize,
) -> Result<(usize, String), String> {
    use crate::parse::Block;
    let mut code_ordinal = 0usize;
    let mut seen_exec = 0usize;
    for b in blocks {
        match b {
            Block::Code { source, .. } => {
                if seen_exec == exec_idx {
                    return Ok((code_ordinal, source.clone()));
                }
                code_ordinal += 1;
                seen_exec += 1;
            }
            Block::Mermaid { .. } => {
                if seen_exec == exec_idx {
                    return Err("that block is a mermaid diagram — not editable".to_string());
                }
                seen_exec += 1;
            }
            _ => {}
        }
    }
    Err("block index out of range — reload the page".to_string())
}

/// The read-splice-write core of a cell save. Holds the notebook's
/// `save_lock` across the whole read-modify-write so concurrent cell
/// saves (another tab) and whole-doc `POST /save` writes serialise.
/// Every rejection path leaves the file untouched.
async fn handle_cell_save(
    nb: &Notebook,
    editable: bool,
    req: SaveRunBlock,
) -> Result<(), String> {
    if !editable {
        return Err("cell editing requires --editable".to_string());
    }

    let _guard = nb.save_lock.lock().await;

    let on_disk = tokio::fs::read_to_string(&nb.source_path)
        .await
        .map_err(|e| format!("could not read notebook: {e}"))?;

    // The render pipeline parses `strip_render_artifacts(source)`. A file
    // that still carries baked render artifacts would make the fence
    // ordinals of the raw file diverge from what the browser saw — refuse
    // rather than splice at the wrong offset. (Live notebooks don't have
    // artifacts; `--output`-rendered copies do.)
    let stripped = crate::strip_render_artifacts(&on_disk);
    if stripped != on_disk {
        return Err(
            "notebook contains render artifacts — edit via the Edit pane or run `notebook clean`"
                .to_string(),
        );
    }

    // Embeds arrived since the page was rendered (or the page predates
    // the check): the ordinal maps to the *expanded* document, not this
    // file. The server is authoritative even if the page shows ✎ Edit.
    if crate::embed::has_markdown_embeds(&on_disk) {
        return Err("notebook uses ![[embeds]] — cell editing is disabled".to_string());
    }

    // CAS: the block the browser edited must still be on disk unchanged.
    let blocks = crate::parse::parse_notebook(&on_disk);
    let (code_ordinal, current) = locate_code_block(&blocks, req.idx)?;
    if current != req.prev_source {
        return Err("block changed on disk — reload the page".to_string());
    }

    let spliced = crate::parse::replace_code_block_source(&on_disk, code_ordinal, &req.source)
        .ok_or_else(|| "could not locate the block in the file — reload the page".to_string())?;

    // Post-splice validation (fail-safe): re-parse and require the target
    // block to read back exactly as the new source, with every other
    // executable block untouched. Any divergence between the splice
    // scanner and the parser becomes a rejected save, never a corrupted
    // file — e.g. a new body containing a bare ``` line closes the fence
    // early and fails here.
    let new_blocks = crate::parse::parse_notebook(&spliced);
    let (_, roundtrip) = locate_code_block(&new_blocks, req.idx)
        .map_err(|_| "edited source restructures the notebook — save rejected".to_string())?;
    if roundtrip != req.source || new_blocks.len() != blocks.len() {
        return Err(
            "edited source would not round-trip (does it contain a ``` line?) — save rejected"
                .to_string(),
        );
    }

    tokio::fs::write(&nb.source_path, spliced.as_bytes())
        .await
        .map_err(|e| format!("could not write notebook: {e}"))
}

/// `{"kind":"cell_status","state":"running","idx":N}` — broadcast when a
/// run/save-and-run is accepted so every connected tab shows the block's
/// spinner.
pub fn cell_status_running_envelope(idx: usize) -> String {
    serde_json::json!({ "kind": "cell_status", "state": "running", "idx": idx }).to_string()
}

/// `{"kind":"cell_status","state":"done"}` — broadcast when a
/// latest-generation render completes (any outcome, including
/// no-change and render-error). Renders are whole-document, so it
/// carries no idx: the client clears every spinner. A preempted or
/// stale render stays silent — its successor emits the terminal `done`.
pub fn cell_status_done_envelope() -> String {
    serde_json::json!({ "kind": "cell_status", "state": "done" }).to_string()
}

/// Client-side JavaScript injected into `<head>` of every render
/// (initial GET *and* every WS update). Lives in head so a body
/// replacement on `kind:"full"` doesn't re-execute it (which would
/// double-up the WS connection).
///
/// Connects to `/ws`, replaces `document.body` on each `{"kind":
/// "full",…}`, re-executes inline `<script>` tags in the new body
/// (DOMParser-set innerHTML doesn't execute them by default — that's
/// what re-creates Plotly charts), and re-invokes
/// `window.renderMathInElement` so KaTeX picks up new math spans.
/// Reconnects with exponential backoff 500 ms → 5 s capped at 10
/// attempts, then surfaces a visible banner; on a successful
/// reconnect *after* a disconnect, hard-reloads the page so we
/// don't ship stale content if updates were missed.
pub const WS_CLIENT_SCRIPT: &str = r#"<script>
(() => {
  // Derive this page's notebook slug from its URL (`/n/<slug>`). The
  // index page (`/`) has no slug, so it simply never opens a socket.
  const slugMatch = location.pathname.match(/^\/n\/([^\/]+)\/?$/);
  if (!slugMatch) return;
  const slug = slugMatch[1];
  const url = `ws://${location.host}/n/${slug}/ws`;
  let ws;
  let reconnectDelay = 500;
  let reconnectTries = 0;
  const MAX_TRIES = 10;
  let firstConnect = true;
  let banner = null;

  function showBanner(text) {
    if (!banner) {
      banner = document.createElement('div');
      banner.id = '__rustlab_ws_banner';
      banner.style.cssText =
        'position:fixed;top:0;left:0;right:0;background:#a23;color:white;'
        + 'text-align:center;padding:6px 10px;'
        + 'font-family:system-ui,sans-serif;font-size:13px;z-index:99999;';
      document.body.appendChild(banner);
    }
    banner.textContent = text;
  }
  function hideBanner() {
    if (banner) { banner.remove(); banner = null; }
  }

  // The rendered notebook lives inside <main>; the page chrome
  // (toolbar, source/editor pane — see server::page) sits OUTSIDE it.
  // Scoping updates to <main> keeps that chrome (and any open editor
  // buffer) intact across re-renders.
  function mainEl() { return document.querySelector('main'); }

  function rerunScripts(root) {
    root.querySelectorAll('script').forEach(old => {
      const s = document.createElement('script');
      for (const attr of old.attributes) s.setAttribute(attr.name, attr.value);
      s.textContent = old.textContent;
      old.parentNode.replaceChild(s, old);
    });
  }
  function rerunKaTeX(root) {
    if (window.renderMathInElement) {
      window.renderMathInElement(root, {
        delimiters: [
          {left: '\\[', right: '\\]', display: true},
          {left: '\\(', right: '\\)', display: false}
        ]
      });
    }
  }
  // Page chrome and the cell script register hooks here to react to
  // re-renders (refresh an open source pane, re-decorate Run buttons).
  // Best-effort.
  function afterUpdate() {
    if (window.__rlAfterUpdate) { try { window.__rlAfterUpdate(); } catch (e) {} }
    if (window.__rlCellDecorate) { try { window.__rlCellDecorate(); } catch (e) {} }
  }
  // A dirty inline cell editor vetoes structural swaps (full/reconcile)
  // — they would destroy the buffer. Best-effort; undefined → no veto.
  function cellVetoStructural() {
    return !!(window.__rlCellVetoStructural && window.__rlCellVetoStructural());
  }

  function applyFull(html) {
    if (cellVetoStructural()) return;
    const parsed = new DOMParser().parseFromString(html, 'text/html');
    const newMain = parsed.querySelector('main');
    const tgt = mainEl();
    if (newMain && tgt) {
      tgt.innerHTML = newMain.innerHTML;
    } else {
      // Degenerate render with no <main> — replace the whole body.
      document.body.innerHTML = parsed.body.innerHTML;
    }
    if (parsed.title) document.title = parsed.title;
    rerunScripts(tgt || document.body);
    rerunKaTeX(tgt || document.body);
    afterUpdate();
  }

  function applyPartial(blocks) {
    // Address blocks by source-order position (the server computed
    // the diff pairwise by index, not by content-hash id, so a
    // content edit stays at its current DOM position even though
    // the new <section> carries a fresh id="b-...").
    const targets = document.querySelectorAll('section.rl-block');
    for (const b of blocks) {
      const el = targets[b.position];
      if (!el) {
        console.warn('rustlab-notebook ws: partial position out of range', b.position);
        continue;
      }
      // A section holding a dirty open cell editor is skipped (marked
      // stale) rather than clobbered; the cell script reconciles on close.
      if (window.__rlCellVetoSection && window.__rlCellVetoSection(el)) {
        el.classList.add('rl-cell-stale');
        continue;
      }
      // outerHTML triggers a parse but DOMParser is *not* needed
      // here: the new <section> is a single sibling, browsers
      // accept it inline. Inline <script>s in the new content
      // won't run (innerHTML/outerHTML doesn't execute them), so
      // we walk and re-clone below.
      el.outerHTML = b.html;
    }
    // Re-execute scripts and re-render KaTeX in the affected nodes.
    // Re-querying after outerHTML swap because the original `el`
    // references are stale.
    const refreshed = document.querySelectorAll('section.rl-block');
    for (const b of blocks) {
      const fresh = refreshed[b.position];
      if (!fresh) continue;
      rerunScripts(fresh);
      rerunKaTeX(fresh);
    }
    afterUpdate();
  }

  // Item 5: removal-aware structural reconcile. The server only sends
  // this for "flat" notebooks (rl-block sections are direct children of
  // <main>), so we reconcile that direct-child list to the desired
  // id sequence: reuse existing nodes (moving if needed), create fresh
  // ones from their html, and drop leftovers. Untouched nodes keep their
  // DOM identity → Plotly/KaTeX state and scroll position are preserved.
  function applyReconcile(blocks) {
    if (cellVetoStructural()) return;
    const main = mainEl();
    if (!main) return;
    const byId = new Map();
    main.querySelectorAll(':scope > section.rl-block').forEach(n => byId.set(n.id, n));
    const keep = new Set();
    const tmp = document.createElement('div');
    let anchor = null; // last positioned section; fresh nodes go after it

    for (const b of blocks) {
      keep.add(b.id);
      let node = byId.get(b.id);
      const fresh = !node;
      if (fresh) { tmp.innerHTML = b.html || ''; node = tmp.firstElementChild; }
      if (!node) continue; // defensive: malformed html

      if (anchor === null) {
        const firstNow = main.querySelector(':scope > section.rl-block');
        if (node !== firstNow) main.insertBefore(node, firstNow);
      } else if (anchor.nextElementSibling !== node) {
        anchor.after(node);
      }
      // Reused nodes keep their DOM identity — re-stamp the executable
      // ordinal, which shifts when code blocks are inserted/removed
      // above them (fresh nodes carry it in their html already).
      if (typeof b.codeIdx === 'number') {
        node.setAttribute('data-code-idx', String(b.codeIdx));
      } else if (!b.html) {
        node.removeAttribute('data-code-idx');
      }
      // Only fresh (created or changed) blocks carry html and need
      // their scripts/KaTeX (re-)run; reused nodes keep their state.
      if (b.html) { rerunScripts(node); rerunKaTeX(node); }
      anchor = node;
    }

    // Remove sections no longer present.
    byId.forEach((node, id) => { if (!keep.has(id)) node.remove(); });
    afterUpdate();
  }

  function connect() {
    ws = new WebSocket(url);
    ws.onopen = () => {
      reconnectDelay = 500;
      reconnectTries = 0;
      hideBanner();
      if (!firstConnect) {
        // Reconnect path: a save we missed during the gap could mean
        // the document is stale, so hard-reload to get the latest —
        // UNLESS the page chrome vetoes it (the --editable editor or an
        // inline cell editor has unsaved changes a reload would
        // discard). In that case keep the page and let the user reload
        // manually.
        const dirtyDoc = window.__rlBlockReload && window.__rlBlockReload();
        const dirtyCell = window.__rlCellDirty && window.__rlCellDirty();
        if (dirtyDoc || dirtyCell) {
          showBanner('rustlab-notebook: reconnected — unsaved edits kept; reload to refresh');
        } else {
          location.reload();
        }
      }
      firstConnect = false;
    };
    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(ev.data);
        if (msg.kind === 'full' && typeof msg.html === 'string') {
          applyFull(msg.html);
        } else if (msg.kind === 'partial' && Array.isArray(msg.blocks)) {
          applyPartial(msg.blocks);
        } else if (msg.kind === 'reconcile' && Array.isArray(msg.blocks)) {
          applyReconcile(msg.blocks);
        } else if (window.__rlCellMessage) {
          // Cell-level kinds (cell_status, cell_saved) are handled by
          // the cell script when it's injected. Best-effort.
          try { window.__rlCellMessage(msg); } catch (e) {}
        }
      } catch (e) {
        console.error('rustlab-notebook ws: bad message', e);
      }
    };
    ws.onclose = () => {
      reconnectTries += 1;
      if (reconnectTries > MAX_TRIES) {
        showBanner('rustlab-notebook: disconnected — server may have stopped');
        return;
      }
      showBanner('rustlab-notebook: disconnected — reconnecting…');
      setTimeout(connect, reconnectDelay);
      reconnectDelay = Math.min(reconnectDelay * 2, 5000);
    };
    ws.onerror = () => { /* onclose will fire next; let it handle retry. */ };
  }

  // ── Interactive widgets ────────────────────────────────────────
  // Delegate `input` events from widget controls to a debounced
  // widget_update message. A single listener on `document` survives the
  // DOM swaps that re-renders perform (the controls live inside <main>,
  // which gets replaced, but this script in <head> does not re-run). The
  // <output> readout updates immediately for responsiveness; the server
  // round-trip re-renders the dependent plots. See
  // dev/plans/notebook_interactive_widgets.md.
  const widgetTimers = {};
  function sendWidget(name, value) {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ kind: 'widget_update', name, value }));
    }
  }
  // Shared outbound path for the cell script (run_block /
  // save_run_block). Silently drops when the socket isn't open — the
  // reconnect banner already tells the user the server is unreachable.
  window.__rlSend = (obj) => {
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(obj));
  };
  function onWidgetEvent(ev) {
    const input = ev.target;
    if (!input || !input.classList || !input.classList.contains('rl-widget-input')) return;
    const form = input.closest('.rl-widget');
    if (!form) return;
    const name = form.getAttribute('data-widget-name');
    if (!name) return;
    const type = form.getAttribute('data-widget-type');
    let value;
    if (type === 'option') {
      value = input.value; // string choice
    } else {
      value = parseFloat(input.value); // slider / number
      if (!isFinite(value)) return;
    }
    // Immediate local readout (sliders carry an <output>).
    const out = form.querySelector('.rl-widget-value');
    if (out) out.textContent = input.value;
    clearTimeout(widgetTimers[name]);
    widgetTimers[name] = setTimeout(() => sendWidget(name, value), 50);
  }
  // `input` covers slider drags / number typing; `change` covers radio
  // selection (and number commit). The debounce collapses any overlap.
  document.addEventListener('input', onWidgetEvent);
  document.addEventListener('change', onWidgetEvent);

  connect();
})();
</script>
"#;

/// Insert the [`WS_CLIENT_SCRIPT`] just before the closing `</head>`
/// tag. Falls back to appending if no closing head tag is found so
/// the page still gets the live-reload script in degenerate
/// renders.
pub fn inject_ws_client(html: &str) -> String {
    if let Some(idx) = html.find("</head>") {
        let (head, rest) = html.split_at(idx);
        format!("{head}{WS_CLIENT_SCRIPT}{rest}")
    } else {
        format!("{html}\n{WS_CLIENT_SCRIPT}")
    }
}

fn truncate_for_log(s: &str) -> &str {
    if s.len() > 80 {
        &s[..80]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_envelope_is_valid_json_with_kind_full() {
        let env = full_envelope("<h1>hi</h1>");
        let parsed: serde_json::Value = serde_json::from_str(&env).unwrap();
        assert_eq!(parsed["kind"], "full");
        assert_eq!(parsed["html"], "<h1>hi</h1>");
    }

    #[test]
    fn parse_widget_update_accepts_number_and_string() {
        assert_eq!(
            parse_widget_update(r#"{"kind":"widget_update","name":"gain","value":2.5}"#),
            Some(("gain".to_string(), WidgetValue::Number(2.5))),
        );
        assert_eq!(
            parse_widget_update(r#"{"kind":"widget_update","name":"win","value":"hann"}"#),
            Some(("win".to_string(), WidgetValue::Text("hann".into()))),
        );
    }

    #[test]
    fn parse_widget_update_rejects_garbage() {
        // Wrong kind, missing fields, non-finite, blank name, bad value type.
        assert!(parse_widget_update(r#"{"kind":"full","html":"x"}"#).is_none());
        assert!(parse_widget_update(r#"{"kind":"widget_update","name":"a"}"#).is_none());
        assert!(parse_widget_update(r#"{"kind":"widget_update","value":1}"#).is_none());
        assert!(
            parse_widget_update(r#"{"kind":"widget_update","name":" ","value":1}"#).is_none()
        );
        assert!(
            parse_widget_update(r#"{"kind":"widget_update","name":"a","value":null}"#).is_none()
        );
        assert!(parse_widget_update("not json").is_none());
    }

    #[test]
    fn parse_run_block_accepts_plain_index() {
        assert_eq!(parse_run_block(r#"{"kind":"run_block","idx":0}"#), Some(0));
        assert_eq!(parse_run_block(r#"{"kind":"run_block","idx":42}"#), Some(42));
    }

    #[test]
    fn parse_run_block_rejects_garbage() {
        // Wrong kind, missing idx, negative, fractional, non-numeric, not json.
        assert!(parse_run_block(r#"{"kind":"widget_update","idx":1}"#).is_none());
        assert!(parse_run_block(r#"{"kind":"run_block"}"#).is_none());
        assert!(parse_run_block(r#"{"kind":"run_block","idx":-1}"#).is_none());
        assert!(parse_run_block(r#"{"kind":"run_block","idx":1.5}"#).is_none());
        assert!(parse_run_block(r#"{"kind":"run_block","idx":"2"}"#).is_none());
        assert!(parse_run_block("not json").is_none());
    }

    #[test]
    fn parse_save_run_block_accepts_full_frame() {
        let req = parse_save_run_block(
            r#"{"kind":"save_run_block","idx":2,"source":"b = 3;","prev_source":"b = 2;"}"#,
        )
        .unwrap();
        assert_eq!(
            req,
            SaveRunBlock {
                idx: 2,
                source: "b = 3;".to_string(),
                prev_source: "b = 2;".to_string(),
            }
        );
    }

    #[test]
    fn parse_save_run_block_rejects_garbage() {
        assert!(parse_save_run_block(r#"{"kind":"run_block","idx":1}"#).is_none());
        assert!(parse_save_run_block(r#"{"kind":"save_run_block","idx":1}"#).is_none());
        assert!(parse_save_run_block(
            r#"{"kind":"save_run_block","idx":1,"source":"x"}"#
        )
        .is_none());
        assert!(parse_save_run_block(
            r#"{"kind":"save_run_block","idx":-1,"source":"x","prev_source":"y"}"#
        )
        .is_none());
        assert!(parse_save_run_block(
            r#"{"kind":"save_run_block","idx":1,"source":5,"prev_source":"y"}"#
        )
        .is_none());
        assert!(parse_save_run_block("not json").is_none());
    }

    #[test]
    fn cell_saved_envelope_shapes() {
        let ok: serde_json::Value =
            serde_json::from_str(&cell_saved_envelope(4, Ok(()))).unwrap();
        assert_eq!(ok["kind"], "cell_saved");
        assert_eq!(ok["idx"], 4);
        assert_eq!(ok["ok"], true);
        assert!(ok.get("error").is_none());

        let err: serde_json::Value =
            serde_json::from_str(&cell_saved_envelope(4, Err("nope"))).unwrap();
        assert_eq!(err["ok"], false);
        assert_eq!(err["error"], "nope");
    }

    #[test]
    fn locate_code_block_maps_executable_ordinals() {
        // markdown, code, mermaid, code — exec ordinals: code=0, mermaid=1, code=2.
        let blocks = crate::parse::parse_notebook(
            "prose\n\n```rustlab\na = 1;\n```\n\n```mermaid\nA-->B\n```\n\n```rustlab\nb = 2;\n```\n",
        );
        assert_eq!(
            locate_code_block(&blocks, 0).unwrap(),
            (0, "a = 1;".to_string())
        );
        assert!(
            locate_code_block(&blocks, 1).unwrap_err().contains("mermaid"),
            "exec slot 1 is the diagram"
        );
        assert_eq!(
            locate_code_block(&blocks, 2).unwrap(),
            (1, "b = 2;".to_string()),
            "second code block is fence ordinal 1"
        );
        assert!(locate_code_block(&blocks, 3).unwrap_err().contains("out of range"));
    }

    /// Async save-path units: every rejection leaves the file untouched;
    /// the happy path splices exactly the target block.
    mod cell_save {
        use super::super::*;
        use tempfile::TempDir;

        const SRC: &str = "# T\n\n```rustlab\na = 1;\n```\n\n```rustlab\nb = 2;\n```\n";

        fn notebook_at(dir: &TempDir, source: &str) -> Notebook {
            let path = dir.path().join("nb.md");
            std::fs::write(&path, source).unwrap();
            Notebook::new("nb".into(), path, "nb".into(), "<main></main>".into())
        }

        fn req(idx: usize, source: &str, prev: &str) -> SaveRunBlock {
            SaveRunBlock {
                idx,
                source: source.into(),
                prev_source: prev.into(),
            }
        }

        #[tokio::test]
        async fn rejects_without_editable() {
            let dir = TempDir::new().unwrap();
            let nb = notebook_at(&dir, SRC);
            let err = handle_cell_save(&nb, false, req(0, "a = 9;", "a = 1;"))
                .await
                .unwrap_err();
            assert!(err.contains("--editable"));
            assert_eq!(std::fs::read_to_string(&nb.source_path).unwrap(), SRC);
        }

        #[tokio::test]
        async fn rejects_on_cas_mismatch() {
            let dir = TempDir::new().unwrap();
            let nb = notebook_at(&dir, SRC);
            // Browser thinks the block still reads "a = 0;" — stale.
            let err = handle_cell_save(&nb, true, req(0, "a = 9;", "a = 0;"))
                .await
                .unwrap_err();
            assert!(err.contains("changed on disk"), "{err}");
            assert_eq!(std::fs::read_to_string(&nb.source_path).unwrap(), SRC);
        }

        #[tokio::test]
        async fn rejects_embedded_notebooks() {
            let dir = TempDir::new().unwrap();
            let nb = notebook_at(&dir, "![[other]]\n\n```rustlab\na = 1;\n```\n");
            let err = handle_cell_save(&nb, true, req(0, "a = 9;", "a = 1;"))
                .await
                .unwrap_err();
            assert!(err.contains("embed"), "{err}");
        }

        #[tokio::test]
        async fn rejects_body_that_breaks_the_fence() {
            let dir = TempDir::new().unwrap();
            let nb = notebook_at(&dir, SRC);
            let err = handle_cell_save(&nb, true, req(0, "x = 1;\n```\nescape!", "a = 1;"))
                .await
                .unwrap_err();
            assert!(err.contains("rejected"), "{err}");
            assert_eq!(
                std::fs::read_to_string(&nb.source_path).unwrap(),
                SRC,
                "a fence-breaking body must never reach disk"
            );
        }

        #[tokio::test]
        async fn happy_path_splices_only_the_target() {
            let dir = TempDir::new().unwrap();
            let nb = notebook_at(&dir, SRC);
            handle_cell_save(&nb, true, req(1, "b = 99;", "b = 2;"))
                .await
                .unwrap();
            let on_disk = std::fs::read_to_string(&nb.source_path).unwrap();
            assert!(on_disk.contains("b = 99;"));
            assert!(!on_disk.contains("b = 2;"));
            assert!(on_disk.contains("a = 1;"), "sibling block untouched");
            assert!(on_disk.contains("# T"), "prose untouched");
        }
    }

    #[test]
    fn cell_status_envelopes_have_expected_shape() {
        let running: serde_json::Value =
            serde_json::from_str(&cell_status_running_envelope(3)).unwrap();
        assert_eq!(running["kind"], "cell_status");
        assert_eq!(running["state"], "running");
        assert_eq!(running["idx"], 3);

        let done: serde_json::Value =
            serde_json::from_str(&cell_status_done_envelope()).unwrap();
        assert_eq!(done["kind"], "cell_status");
        assert_eq!(done["state"], "done");
        assert!(done.get("idx").is_none(), "done is whole-document, no idx");
    }

    #[test]
    fn full_envelope_escapes_html_with_quotes_and_scripts() {
        let html = r#"<script>alert("xss")</script>"#;
        let env = full_envelope(html);
        let parsed: serde_json::Value = serde_json::from_str(&env).unwrap();
        assert_eq!(parsed["html"], html);
    }

    #[test]
    fn inject_ws_client_inserts_before_closing_head() {
        let html = "<!doctype html><html><head><title>x</title></head><body>hi</body></html>";
        let out = inject_ws_client(html);
        let head_close = out.find("</head>").unwrap();
        let script_pos = out.find("__rustlab_ws_banner").unwrap();
        assert!(
            script_pos < head_close,
            "WS-client script must land before </head>",
        );
        assert!(out.contains("hi"), "body content survived");
    }

    #[test]
    fn inject_ws_client_falls_back_when_no_head() {
        let html = "<p>no head</p>";
        let out = inject_ws_client(html);
        assert!(out.contains("__rustlab_ws_banner"));
        assert!(out.contains("<p>no head</p>"));
    }

    /// The transport script must be syntactically valid JS. Uses
    /// `node --check` when node is available; skips otherwise so the
    /// suite doesn't hard-depend on node.
    #[test]
    fn ws_client_script_passes_node_check() {
        let js = WS_CLIENT_SCRIPT
            .trim_start()
            .strip_prefix("<script>")
            .unwrap()
            .trim_end()
            .strip_suffix("</script>")
            .expect("script wrapper shape changed");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ws.js");
        std::fs::write(&path, js).unwrap();
        match std::process::Command::new("node").arg("--check").arg(&path).output() {
            Ok(out) => assert!(
                out.status.success(),
                "node --check failed:\n{}",
                String::from_utf8_lossy(&out.stderr)
            ),
            Err(_) => eprintln!("node not available — skipping JS syntax check"),
        }
    }
}
