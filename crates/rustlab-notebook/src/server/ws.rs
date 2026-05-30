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
use tokio::sync::broadcast::error::RecvError;

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
            ws.on_upgrade(move |socket| handle_socket(socket, nb))
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
/// Inbound messages are logged and dropped — Phase 2 has no
/// client→server kinds. The inbound match arm is the future
/// widget-update extension site per
/// `dev/plans/notebook_interactive_server.md` locked-in #14; see
/// `dev/plans/notebook_interactive_widgets.md` for the planned
/// `{"kind":"widget_update",…}` payload that would land here.
async fn handle_socket(mut socket: WebSocket, nb: Arc<Notebook>) {
    let mut rx = nb.broadcast.subscribe();

    loop {
        tokio::select! {
            // Inbound from client.
            inbound = socket.recv() => {
                match inbound {
                    None => return, // socket closed
                    Some(Ok(Message::Close(_))) => return,
                    Some(Ok(Message::Text(payload))) => {
                        // ── Widget integration extension site ──────
                        // Future: parse {"kind":"widget_update",…}
                        // and forward to the render coordinator.
                        // See dev/plans/notebook_interactive_widgets.md.
                        eprintln!(
                            "[watch] ws: unexpected text message (Phase 2 has no client→server kinds): {}",
                            truncate_for_log(&payload),
                        );
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

  function rerunBodyScripts() {
    document.body.querySelectorAll('script').forEach(old => {
      const s = document.createElement('script');
      for (const attr of old.attributes) s.setAttribute(attr.name, attr.value);
      s.textContent = old.textContent;
      old.parentNode.replaceChild(s, old);
    });
  }
  function rerunKaTeX() {
    if (window.renderMathInElement) {
      window.renderMathInElement(document.body, {
        delimiters: [
          {left: '$$', right: '$$', display: true},
          {left: '$',  right: '$',  display: false}
        ]
      });
    }
  }

  function applyFull(html) {
    const parsed = new DOMParser().parseFromString(html, 'text/html');
    document.body.innerHTML = parsed.body.innerHTML;
    rerunBodyScripts();
    rerunKaTeX();
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
      fresh.querySelectorAll('script').forEach(old => {
        const s = document.createElement('script');
        for (const attr of old.attributes) s.setAttribute(attr.name, attr.value);
        s.textContent = old.textContent;
        old.parentNode.replaceChild(s, old);
      });
      if (window.renderMathInElement) {
        window.renderMathInElement(fresh, {
          delimiters: [
            {left: '$$', right: '$$', display: true},
            {left: '$',  right: '$',  display: false}
          ]
        });
      }
    }
  }

  function connect() {
    ws = new WebSocket(url);
    ws.onopen = () => {
      reconnectDelay = 500;
      reconnectTries = 0;
      hideBanner();
      if (!firstConnect) {
        // Reconnect path: a save we missed during the gap could mean
        // the document is stale. Hard-reload to get the latest.
        location.reload();
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
