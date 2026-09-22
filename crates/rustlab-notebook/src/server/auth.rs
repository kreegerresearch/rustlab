//! Session token + Origin checks for the interactive `notebook watch` server.
//!
//! Mutating endpoints (`POST /save/{slug}`, WebSocket upgrade, and mutate WS
//! messages) require the per-run secret printed in the startup URL
//! (`?token=…`). Requests whose `Origin` / `Host` are not loopback are
//! rejected. Bind remains `127.0.0.1` only.

use axum::http::{header, HeaderMap, StatusCode};
use rand::RngCore;

/// Generate a 32-byte hex session token.
pub fn generate_session_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Generate a CSP nonce (16 bytes, base64url-ish hex).
pub fn generate_csp_nonce() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Extract `token` from a query string (`?token=…` or `&token=…`).
pub fn token_from_query(query: Option<&str>) -> Option<String> {
    let q = query?;
    for pair in q.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next()?;
        if key == "token" {
            return parts.next().map(|v| v.to_string());
        }
    }
    None
}

/// Also accept `X-Rustlab-Token` / `Authorization: Bearer …`.
pub fn token_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get("x-rustlab-token").and_then(|v| v.to_str().ok()) {
        return Some(v.to_string());
    }
    if let Some(v) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        let v = v.trim();
        if let Some(rest) = v.strip_prefix("Bearer ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Constant-time-ish compare (length leak only).
pub fn token_matches(expected: &str, provided: Option<&str>) -> bool {
    let Some(p) = provided else {
        return false;
    };
    if p.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in p.bytes().zip(expected.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// Allowed Origins for this bind address (`http://127.0.0.1:PORT` and
/// `http://localhost:PORT`). Missing Origin is allowed for same-origin
/// navigations that omit it (e.g. some WS clients) **only when** the Host
/// header is also loopback — mutate paths still require the token.
pub fn origin_allowed(headers: &HeaderMap, port: u16) -> bool {
    let allowed = [
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
        format!("http://[::1]:{port}"),
    ];
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        return allowed.iter().any(|a| a == origin);
    }
    // No Origin: require Host to be loopback.
    host_is_loopback(headers, port)
}

fn host_is_loopback(headers: &HeaderMap, port: u16) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    let ok_hosts = [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ];
    ok_hosts.iter().any(|h| h == &host)
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

/// Reject with 401/403 when token or Origin fails.
pub fn authorize_mutate(
    headers: &HeaderMap,
    query: Option<&str>,
    expected_token: &str,
    port: u16,
) -> Result<(), StatusCode> {
    if !origin_allowed(headers, port) {
        return Err(StatusCode::FORBIDDEN);
    }
    let provided = token_from_query(query)
        .or_else(|| token_from_headers(headers));
    if !token_matches(expected_token, provided.as_deref()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

/// Soft check for read paths: Origin must be loopback when present; token
/// not required for GET HTML (the secret is in the URL the user opens).
pub fn authorize_read_origin(headers: &HeaderMap, port: u16) -> Result<(), StatusCode> {
    if headers.get(header::ORIGIN).is_some() && !origin_allowed(headers, port) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(())
}

/// Inject CSP nonces on every `<script` tag and embed the session token
/// so the page chrome / WS client can authenticate mutates.
pub fn prepare_served_html(html: &str, nonce: &str, token: &str) -> String {
    let mut out = inject_script_nonces(html, nonce);
    let boot = format!(
        "<meta name=\"rl-token\" content=\"{token}\">\n\
         <script nonce=\"{nonce}\">window.__RL_TOKEN={token_js};</script>\n",
        token = html_attr_escape(token),
        nonce = nonce,
        token_js = serde_json::to_string(token).unwrap_or_else(|_| "\"\"".into()),
    );
    if let Some(idx) = out.find("</head>") {
        out.insert_str(idx, &boot);
    } else {
        out = format!("{boot}{out}");
    }
    out
}

fn html_attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
                } else if tag.ends_with("/>") {
                    // unlikely for script
                    out.push_str(&format!(
                        "{} nonce=\"{}\"/>",
                        &tag[..tag.len() - 2].trim_end(),
                        nonce
                    ));
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

    #[test]
    fn token_roundtrip() {
        let t = generate_session_token();
        assert_eq!(t.len(), 64);
        assert!(token_matches(&t, Some(&t)));
        assert!(!token_matches(&t, Some("nope")));
        assert!(!token_matches(&t, None));
    }

    #[test]
    fn query_parse() {
        assert_eq!(
            token_from_query(Some("token=abc&x=1")).as_deref(),
            Some("abc")
        );
        assert_eq!(token_from_query(Some("x=1")), None);
    }

    #[test]
    fn origin_loopback_ok() {
        let mut h = HeaderMap::new();
        h.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://127.0.0.1:8042"),
        );
        assert!(origin_allowed(&h, 8042));
        h.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://evil.example:8042"),
        );
        assert!(!origin_allowed(&h, 8042));
    }

    #[test]
    fn prepare_served_html_adds_nonce_and_token() {
        let html = "<html><head></head><body><script>1</script></body></html>";
        let out = prepare_served_html(html, "abc", "tok");
        assert!(out.contains("nonce=\"abc\""));
        assert!(out.contains("name=\"rl-token\""));
        assert!(out.contains("__RL_TOKEN"));
    }
}
