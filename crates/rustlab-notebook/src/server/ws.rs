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
            ws.on_upgrade(move |socket| handle_socket(socket, nb, slug, render_tx))
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
    render_tx: Option<UnboundedSender<String>>,
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
                        match parse_widget_update(&payload) {
                            Some((name, value)) => {
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
                                            let _ = tx.send(slug.clone());
                                        }
                                    }
                                    Some(None) => eprintln!(
                                        "[watch] ws: rejecting invalid value for widget '{name}': {value:?}"
                                    ),
                                    None => eprintln!(
                                        "[watch] ws: ignoring update for unknown widget '{name}'"
                                    ),
                                }
                            }
                            None => eprintln!(
                                "[watch] ws: ignoring unrecognised text message: {}",
                                truncate_for_log(&payload),
                            ),
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
  // Page chrome registers a hook here to react to re-renders (e.g.
  // refresh an open read-only source pane). Best-effort.
  function afterUpdate() {
    if (window.__rlAfterUpdate) { try { window.__rlAfterUpdate(); } catch (e) {} }
  }

  function applyFull(html) {
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
        // UNLESS the page chrome vetoes it (e.g. the --editable editor
        // has unsaved changes a reload would discard). In that case keep
        // the page and let the user reload manually.
        if (window.__rlBlockReload && window.__rlBlockReload()) {
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
}
