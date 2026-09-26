//! Loopback Origin + Host checks for the interactive `notebook watch` server.
//!
//! There is no per-run session token. Embedding a secret in every page
//! (`window.__RL_TOKEN`) does not authenticate local callers — any process
//! that can GET a page can read it — and the WebSocket client captured
//! that global *before* the assignment script ran. Cross-notebook links
//! are rewritten to `/n/<slug>` with no `?token=`, so a click opened the
//! socket with an empty secret, the upgrade returned 401, and the page
//! stuck on "disconnected — reconnecting…".
//!
//! CSRF / DNS-rebinding defense is therefore:
//! - every request must present a loopback `Host` for the bound port;
//! - `POST /save/{slug}` and the WebSocket upgrade must present a
//!   loopback `Origin` (missing Origin is rejected).
//!
//! Bind remains `127.0.0.1` only. Other local processes can still reach
//! that port; that residual risk is documented in `docs/security.md`.

use axum::http::{header, HeaderMap, StatusCode};
use rand::RngCore;

/// Generate a CSP nonce (16 bytes, hex).
pub fn generate_csp_nonce() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn loopback_origins(port: u16) -> [String; 3] {
    [
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
        format!("http://[::1]:{port}"),
    ]
}

fn loopback_hosts(port: u16) -> [String; 3] {
    [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ]
}

/// `Origin` must be present and equal to one of the loopback origins for
/// `port`. A missing header is rejected — mutate paths cannot treat
/// "no Origin" as same-site.
pub fn origin_allowed(headers: &HeaderMap, port: u16) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    loopback_origins(port).iter().any(|a| a == origin)
}

/// `Host` must be a loopback name plus the bound port. Used as a
/// DNS-rebinding check on every request, including GETs.
pub fn host_is_loopback(headers: &HeaderMap, port: u16) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    loopback_hosts(port).iter().any(|h| h == &host)
}

/// Build the Content-Security-Policy header value for watch-served pages.
pub fn csp_header(nonce: &str) -> String {
    format!(
        "default-src 'self'; \
         script-src 'self' 'nonce-{nonce}' 'strict-dynamic'; \
         style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; \
         font-src 'self'; \
         connect-src 'self' ws://127.0.0.1:* ws://localhost:* ws://[::1]:*; \
         object-src 'none'; \
         base-uri 'none'; \
         form-action 'self'; \
         frame-ancestors 'none'"
    )
}

/// Reject mutate paths whose Origin is missing or not loopback.
pub fn authorize_mutate(headers: &HeaderMap, port: u16) -> Result<(), StatusCode> {
    if !origin_allowed(headers, port) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(())
}

/// Add `nonce="…"` to every `<script` opening tag that lacks one.
pub fn prepare_served_html(html: &str, nonce: &str) -> String {
    inject_script_nonces(html, nonce)
}

/// Add `nonce="…"` to every `<script` / `<script ` opening tag that lacks one.
fn inject_script_nonces(html: &str, nonce: &str) -> String {
    let mut out = String::with_capacity(html.len() + 64);
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"<script") {
            // Find end of opening tag.
            if let Some(rel) = html[i..].find('>') {
                let tag = &html[i..i + rel + 1];
                if tag.contains("nonce=") {
                    out.push_str(tag);
                } else if let Some(stripped) = tag.strip_suffix("/>") {
                    // unlikely for script
                    out.push_str(&format!("{} nonce=\"{}\"/>", stripped.trim_end(), nonce));
                } else {
                    out.push_str(&tag[..tag.len() - 1]);
                    out.push_str(&format!(" nonce=\"{}\">", nonce));
                }
                i += rel + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers_with(name: axum::http::HeaderName, value: &'static str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(name, HeaderValue::from_static(value));
        h
    }

    #[test]
    fn origin_loopback_requires_header() {
        let port = 8042u16;
        assert!(!origin_allowed(&HeaderMap::new(), port));
        for origin in [
            "http://127.0.0.1:8042",
            "http://localhost:8042",
            "http://[::1]:8042",
        ] {
            assert!(
                origin_allowed(&headers_with(header::ORIGIN, origin), port),
                "{origin}"
            );
        }
        assert!(!origin_allowed(
            &headers_with(header::ORIGIN, "http://evil.example:8042"),
            port
        ));
        // Right host, wrong port — the bound port is part of the allowlist.
        assert!(!origin_allowed(
            &headers_with(header::ORIGIN, "http://127.0.0.1:9"),
            port
        ));
    }

    #[test]
    fn host_must_be_loopback_with_port() {
        let port = 9000u16;
        assert!(!host_is_loopback(&HeaderMap::new(), port));
        for host in ["127.0.0.1:9000", "localhost:9000", "[::1]:9000"] {
            assert!(
                host_is_loopback(&headers_with(header::HOST, host), port),
                "{host}"
            );
        }
        // Case-insensitive host name.
        assert!(host_is_loopback(
            &headers_with(header::HOST, "LocalHost:9000"),
            port
        ));
        assert!(!host_is_loopback(
            &headers_with(header::HOST, "evil.example:9000"),
            port
        ));
        assert!(!host_is_loopback(
            &headers_with(header::HOST, "127.0.0.1:8042"),
            port
        ));
    }

    #[test]
    fn prepare_served_html_adds_nonce_not_token() {
        let html = "<html><head></head><body><script>1</script></body></html>";
        let out = prepare_served_html(html, "abc");
        assert!(out.contains("nonce=\"abc\""));
        assert!(!out.contains("__RL_TOKEN"));
        assert!(!out.contains("rl-token"));
    }
}
