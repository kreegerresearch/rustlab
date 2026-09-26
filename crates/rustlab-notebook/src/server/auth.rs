//! Loopback Origin + Host checks and the CSP for the interactive
//! `notebook watch` server.
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
//!
//! **CSP nonce.** One nonce is generated per server process and stamped
//! at *render time* on the script tags the renderer and page chrome emit
//! (`render::render_html_nonced`, `ws::inject_ws_client_nonced`,
//! `page::inject_chrome_nonced`, `cell::inject_cell_client_nonced`). Served
//! HTML is never post-processed to add nonces: doing that would bless any
//! `<script>` that reached the document through author content. The nonce
//! is per-process rather than per-response because pages are rendered once
//! and cached; it is not a secret from local processes (they can GET the
//! page), which is the accepted residual risk above.

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

/// Human-readable 403 body for a rejected `Host` (someone reached the
/// server through a hostname alias, a proxy, or a rebinding attempt).
pub fn host_rejection_body(headers: &HeaderMap, port: u16) -> String {
    let got = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<missing>");
    format!(
        "rustlab-notebook watch only answers requests addressed to \
         127.0.0.1:{port}, localhost:{port} or [::1]:{port} (got Host: {got}). \
         Open the URL printed at startup; see docs/security.md.",
    )
}

/// Human-readable 403 body for a rejected `Origin` on a mutate path.
pub fn origin_rejection_body(headers: &HeaderMap, port: u16) -> String {
    let got = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<missing>");
    format!(
        "this request must come from a page served by rustlab-notebook watch: \
         Origin must be http://127.0.0.1:{port}, http://localhost:{port} or \
         http://[::1]:{port} (got Origin: {got}). See docs/security.md.",
    )
}

/// Build the Content-Security-Policy header value for watch-served pages.
///
/// `connect-src` does not list `ws://[::1]:*`. Chromium treats that token
/// as an invalid source (the port wildcard does not parse after an IPv6
/// literal) and ignores it, which also logs an error on every page. The
/// listener binds `127.0.0.1`, so `'self'` and `ws://127.0.0.1:*` cover
/// the page's WebSocket.
pub fn csp_header(nonce: &str) -> String {
    format!(
        "default-src 'self'; \
         script-src 'self' 'nonce-{nonce}' 'strict-dynamic'; \
         style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; \
         font-src 'self'; \
         connect-src 'self' ws://127.0.0.1:* ws://localhost:*; \
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
    fn rejection_bodies_name_the_expected_values() {
        let h = headers_with(header::HOST, "evil.example:8042");
        let body = host_rejection_body(&h, 8042);
        assert!(body.contains("127.0.0.1:8042"), "{body}");
        assert!(body.contains("evil.example:8042"), "{body}");
        let body = origin_rejection_body(&HeaderMap::new(), 8042);
        assert!(body.contains("http://localhost:8042"), "{body}");
        assert!(body.contains("<missing>"), "{body}");
    }

    #[test]
    fn csp_uses_nonce_and_strict_dynamic_without_unsafe_inline_scripts() {
        let csp = csp_header("abc");
        assert!(csp.contains("script-src 'self' 'nonce-abc' 'strict-dynamic'"));
        assert!(!csp.contains("script-src 'self' 'unsafe-inline'"));
        assert!(csp.contains("frame-ancestors 'none'"));
    }

    #[test]
    fn csp_omits_invalid_ipv6_wildcard() {
        let csp = csp_header("abc");
        assert!(!csp.contains("[::1]"));
        assert!(csp.contains("connect-src 'self' ws://127.0.0.1:* ws://localhost:*"));
        assert!(csp.contains("script-src 'self' 'nonce-abc' 'strict-dynamic'"));
    }
}
