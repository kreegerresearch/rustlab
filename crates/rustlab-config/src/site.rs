//! `rustlab.toml` `[site]`: brand, nav, footer text, and stylesheets for
//! HTML notebook pages.
//!
//! Every value is plain text or a URL. Labels and the footer are escaped
//! by the renderer; hrefs and stylesheet URLs must pass [`check_site_href`]
//! / [`check_stylesheet_url`], which accept only `http:` / `https:` URLs,
//! relative paths, and (for hrefs) `#fragment`s.

use std::path::Path;

use toml::Value as TomlValue;

use crate::ConfigError;

/// Longest label, footer, or URL accepted, in bytes.
pub const MAX_SITE_TEXT: usize = 2048;
/// Most `nav` entries or `stylesheets` accepted.
pub const MAX_SITE_ITEMS: usize = 32;

/// One `{ label, href }` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteLink {
    pub label: String,
    pub href: String,
}

/// `[site]`, validated. Empty when the section is absent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SiteConfig {
    pub brand: Option<SiteLink>,
    pub nav: Vec<SiteLink>,
    /// `None` keeps the built-in footer. `Some("")` removes it.
    pub footer: Option<String>,
    pub stylesheets: Vec<String>,
}

impl SiteConfig {
    pub fn is_empty(&self) -> bool {
        self.brand.is_none()
            && self.nav.is_empty()
            && self.footer.is_none()
            && self.stylesheets.is_empty()
    }
}

/// Href for a brand or nav link: `http(s)://host…`, a relative or
/// root-relative path, `?query`, or `#fragment`.
pub fn check_site_href(href: &str) -> Result<(), String> {
    check_url(href, true)
}

/// Stylesheet URL: `http(s)://host…` or a relative / root-relative path.
pub fn check_stylesheet_url(url: &str) -> Result<(), String> {
    check_url(url, false)
}

/// `scheme://host[:port]` of an absolute stylesheet URL, lowercased, for a
/// CSP `style-src` source. `None` for a relative URL (covered by `'self'`)
/// and for anything [`check_stylesheet_url`] refuses.
pub fn stylesheet_origin(url: &str) -> Option<String> {
    check_stylesheet_url(url).ok()?;
    let (scheme, rest) = split_scheme(url)?;
    let authority = authority_of(rest)?;
    Some(format!(
        "{}://{}",
        scheme.to_ascii_lowercase(),
        authority.to_ascii_lowercase()
    ))
}

/// Visible text: a label or the footer. Escaped when rendered; control
/// characters and bidirectional overrides are refused.
pub fn check_site_text(text: &str, allow_empty: bool) -> Result<(), String> {
    if text.len() > MAX_SITE_TEXT {
        return Err(format!("is longer than {MAX_SITE_TEXT} bytes"));
    }
    if !allow_empty && text.trim().is_empty() {
        return Err("is empty".into());
    }
    if let Some(c) = text.chars().find(|c| c.is_control() || is_format_char(*c)) {
        return Err(format!("contains the control character U+{:04X}", c as u32));
    }
    Ok(())
}

/// Zero-width and bidirectional formatting characters, which can make a
/// label or URL read differently from what it is.
fn is_format_char(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}')
}

/// `(scheme, rest after ':')` when `url` starts with a URL scheme, as a
/// browser would read it: letters, digits, `+ - .` up to a `:` that comes
/// before any `/ ? #`.
fn split_scheme(url: &str) -> Option<(&str, &str)> {
    let colon = url.find(':')?;
    if url[..colon].contains(['/', '?', '#']) {
        return None;
    }
    let scheme = &url[..colon];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic()
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    Some((scheme, &url[colon + 1..]))
}

/// `host[:port]` of `//host[:port]/…`, when it is a plain host name, IPv4,
/// or bracketed IPv6 address with an optional numeric port.
fn authority_of(rest: &str) -> Option<&str> {
    let rest = rest.strip_prefix("//")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let close = v6.find(']')?;
        let inner = &v6[..close];
        if inner.is_empty()
            || !inner
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
        {
            return None;
        }
        (&authority[..close + 2], &v6[close + 1..])
    } else {
        match authority.rfind(':') {
            Some(i) => (&authority[..i], &authority[i..]),
            None => (authority, ""),
        }
    };
    let host_ok = host.starts_with('[')
        || (!host.is_empty()
            && !host.starts_with(['.', '-'])
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-')));
    let port_ok = match port.strip_prefix(':') {
        None => port.is_empty(),
        Some(p) => !p.is_empty() && p.len() <= 5 && p.chars().all(|c| c.is_ascii_digit()),
    };
    (host_ok && port_ok).then_some(authority)
}

fn check_url(url: &str, allow_fragment: bool) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("is empty".into());
    }
    if url.len() > MAX_SITE_TEXT {
        return Err(format!("is longer than {MAX_SITE_TEXT} bytes"));
    }
    if let Some(c) = url.chars().find(|c| {
        c.is_control()
            || c.is_whitespace()
            || is_format_char(*c)
            || matches!(c, '<' | '>' | '"' | '\'' | '`' | '\\')
    }) {
        return Err(format!(
            "\"{}\" contains {}; use percent-encoding",
            url.escape_debug(),
            if c.is_whitespace() {
                "whitespace".to_string()
            } else {
                format!("'{}'", c.escape_debug())
            }
        ));
    }
    if url.starts_with("//") {
        return Err(format!(
            "\"{url}\" is protocol-relative; write https://… or a path"
        ));
    }
    if url.starts_with('#') && !allow_fragment {
        return Err(format!("\"{url}\" is only a fragment"));
    }
    match split_scheme(url) {
        None => Ok(()),
        Some((scheme, rest)) => {
            if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
                return Err(format!(
                    "\"{url}\" uses the {scheme}: scheme; only http:, https:, relative paths{} are allowed",
                    if allow_fragment { ", and #fragments" } else { "" }
                ));
            }
            authority_of(rest).map(|_| ()).ok_or_else(|| {
                format!("\"{url}\" needs a host: http(s)://host[:port]/path (no user@)")
            })
        }
    }
}

pub(crate) fn parse_site_table(
    val: &TomlValue,
    path: &Path,
    unknown: &mut Vec<String>,
) -> Result<SiteConfig, ConfigError> {
    let mut out = SiteConfig::default();
    for (k, v) in crate::expect_table(val, path, "site")? {
        match k.as_str() {
            "brand" => out.brand = Some(parse_link(v, path, "site.brand", unknown)?),
            "nav" => {
                let items = expect_array(v, path, "site.nav")?;
                for (i, item) in items.iter().enumerate() {
                    out.nav.push(parse_link(
                        item,
                        path,
                        &format!("site.nav[{}]", i + 1),
                        unknown,
                    )?);
                }
            }
            "footer" => {
                let s = crate::expect_string(v, path, "site.footer")?;
                check_site_text(s, true)
                    .map_err(|m| ConfigError::invalid(path, "site.footer", m))?;
                out.footer = Some(s.to_string());
            }
            "stylesheets" => {
                let items = expect_array(v, path, "site.stylesheets")?;
                for (i, item) in items.iter().enumerate() {
                    let key = format!("site.stylesheets[{}]", i + 1);
                    let s = crate::expect_string(item, path, &key)?;
                    check_stylesheet_url(s).map_err(|m| ConfigError::invalid(path, &key, m))?;
                    out.stylesheets.push(s.to_string());
                }
            }
            other => unknown.push(format!("site.{other}")),
        }
    }
    Ok(out)
}

fn expect_array<'a>(
    val: &'a TomlValue,
    path: &Path,
    key: &str,
) -> Result<&'a Vec<TomlValue>, ConfigError> {
    match val {
        TomlValue::Array(a) if a.len() <= MAX_SITE_ITEMS => Ok(a),
        TomlValue::Array(a) => Err(ConfigError::invalid(
            path,
            key,
            format!("has {} entries; at most {MAX_SITE_ITEMS}", a.len()),
        )),
        other => Err(ConfigError::invalid(
            path,
            key,
            format!("expected an array, got {}", crate::type_name(other)),
        )),
    }
}

fn parse_link(
    val: &TomlValue,
    path: &Path,
    key: &str,
    unknown: &mut Vec<String>,
) -> Result<SiteLink, ConfigError> {
    let table = match val {
        TomlValue::Table(t) => t,
        other => {
            return Err(ConfigError::invalid(
                path,
                key,
                format!(
                    "expected {{ label = \"…\", href = \"…\" }}, got {}",
                    crate::type_name(other)
                ),
            ))
        }
    };
    let mut label = None;
    let mut href = None;
    for (k, v) in table {
        match k.as_str() {
            "label" => {
                let key = format!("{key}.label");
                let s = crate::expect_string(v, path, &key)?;
                check_site_text(s, false).map_err(|m| ConfigError::invalid(path, &key, m))?;
                label = Some(s.to_string());
            }
            "href" => {
                let key = format!("{key}.href");
                let s = crate::expect_string(v, path, &key)?;
                check_site_href(s).map_err(|m| ConfigError::invalid(path, &key, m))?;
                href = Some(s.to_string());
            }
            other => unknown.push(format!("{key}.{other}")),
        }
    }
    let label =
        label.ok_or_else(|| ConfigError::invalid(path, &format!("{key}.label"), "is missing"))?;
    let href =
        href.ok_or_else(|| ConfigError::invalid(path, &format!("{key}.href"), "is missing"))?;
    Ok(SiteLink { label, href })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_project_toml, PROJECT_CONFIG};
    use std::path::PathBuf;

    fn project(text: &str) -> Result<crate::ProjectConfig, ConfigError> {
        parse_project_toml(text, &PathBuf::from("/proj").join(PROJECT_CONFIG))
    }

    #[test]
    fn site_hrefs_allow_http_relative_and_fragments() {
        for ok in [
            "https://kreegerlabs.com/",
            "HTTP://example.com/a?b=c#d",
            "https://example.com:8443/x",
            "/notebooks/",
            "notebooks/index.html",
            "../index.html",
            "./a.html",
            "#top",
            "?page=2",
            "a%20b.html",
            "page.html#sec-2",
        ] {
            assert!(check_site_href(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn site_hrefs_reject_javascript_and_other_schemes() {
        for bad in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            " javascript:alert(1)",
            "java\tscript:alert(1)",
            "data:text/html,<b>x</b>",
            "vbscript:msgbox",
            "blob:https://x/1",
            "file:///etc/passwd",
            "mailto:a@b.c",
            "ftp://example.com/",
            "//evil.example/x",
            "/\\evil.example",
            "https:/missing-slash",
            "https://",
            "https://user@host/",
            "a b.html",
            "x\"onmouseover=\"y",
            "<x>",
            "",
            "   ",
        ] {
            assert!(check_site_href(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(check_site_href(&"a".repeat(MAX_SITE_TEXT + 1)).is_err());
    }

    #[test]
    fn stylesheet_urls_and_origins() {
        assert!(check_stylesheet_url("https://kreegerlabs.com/css/site.css").is_ok());
        assert!(check_stylesheet_url("/css/site.css").is_ok());
        assert!(check_stylesheet_url("site.css").is_ok());
        for bad in [
            "javascript:alert(1)",
            "data:text/css,body{}",
            "#frag",
            "//cdn.example/x.css",
            "https://a.example/x.css;script-src *",
            "https://a.example/x.css x",
            "https://a.example,b.example/x.css",
        ] {
            assert!(check_stylesheet_url(bad).is_err(), "{bad}");
        }
        assert_eq!(
            stylesheet_origin("https://KreegerLabs.com/css/site.css?v=2").as_deref(),
            Some("https://kreegerlabs.com")
        );
        assert_eq!(
            stylesheet_origin("http://localhost:4000/a.css").as_deref(),
            Some("http://localhost:4000")
        );
        assert_eq!(
            stylesheet_origin("https://[::1]:8443/a.css").as_deref(),
            Some("https://[::1]:8443")
        );
        assert_eq!(stylesheet_origin("/css/site.css"), None);
        assert_eq!(stylesheet_origin("javascript:alert(1)"), None);
    }

    #[test]
    fn site_text_rejects_control_characters() {
        assert!(check_site_text("© 2026 Kreeger Labs <beta> & \"friends\"", false).is_ok());
        assert!(check_site_text("", true).is_ok());
        assert!(check_site_text("", false).is_err());
        assert!(check_site_text("  ", false).is_err());
        assert!(check_site_text("a\u{0}b", false).is_err());
        assert!(check_site_text("a\nb", false).is_err());
        assert!(check_site_text("a\u{202e}b", false).is_err());
    }

    #[test]
    fn project_site_section_parses() {
        let p = project(
            r##"
[site]
brand = { label = "Kreeger Labs", href = "https://kreegerlabs.com/" }
nav = [
  { label = "Notebooks", href = "/notebooks/" },
  { label = "About", href = "/about/" },
]
footer = "© 2026 Kreeger Labs"
stylesheets = ["https://kreegerlabs.com/css/site.css", "/css/extra.css"]
"##,
        )
        .unwrap();
        let site = &p.site;
        assert_eq!(
            site.brand,
            Some(SiteLink {
                label: "Kreeger Labs".into(),
                href: "https://kreegerlabs.com/".into()
            })
        );
        assert_eq!(site.nav.len(), 2);
        assert_eq!(site.nav[1].label, "About");
        assert_eq!(site.footer.as_deref(), Some("© 2026 Kreeger Labs"));
        assert_eq!(site.stylesheets.len(), 2);
        assert!(p.unknown_keys.is_empty(), "{:?}", p.unknown_keys);
    }

    #[test]
    fn project_site_empty_footer_removes_it() {
        let p = project("[site]\nfooter = \"\"\n").unwrap();
        assert_eq!(p.site.footer.as_deref(), Some(""));
        assert!(!p.site.is_empty());
        assert!(project("").unwrap().site.is_empty());
    }

    #[test]
    fn project_site_rejects_bad_values_with_the_key() {
        let cases = [
            (
                "[site]\nbrand = { label = \"x\", href = \"javascript:alert(1)\" }\n",
                "site.brand.href",
            ),
            (
                "[site]\nnav = [{ label = \"a\", href = \"/a\" }, { label = \"b\", href = \"data:x\" }]\n",
                "site.nav[2].href",
            ),
            ("[site]\nbrand = { href = \"/\" }\n", "site.brand.label"),
            ("[site]\nbrand = { label = \"x\" }\n", "site.brand.href"),
            ("[site]\nbrand = \"Kreeger\"\n", "site.brand"),
            ("[site]\nnav = { label = \"a\", href = \"/a\" }\n", "site.nav"),
            ("[site]\nfooter = 3\n", "site.footer"),
            ("[site]\nfooter = \"a\\u0007b\"\n", "site.footer"),
            (
                "[site]\nstylesheets = [\"javascript:alert(1)\"]\n",
                "site.stylesheets[1]",
            ),
            ("[site]\nstylesheets = \"/a.css\"\n", "site.stylesheets"),
            ("[site]\nnav = [{ label = \"<b>x</b>\", href = \"/a b\" }]\n", "site.nav[1].href"),
        ];
        for (text, key) in cases {
            let err = project(text).unwrap_err().to_string();
            assert!(err.contains(key), "{text}: expected {key} in {err}");
        }
    }

    #[test]
    fn project_site_unknown_keys_warn() {
        let p = project(
            "[site]\ntitle = \"x\"\nbrand = { label = \"a\", href = \"/\", icon = \"x.png\" }\nnav = [{ label = \"a\", href = \"/a\", rel = \"me\" }]\n",
        )
        .unwrap();
        assert_eq!(
            p.unknown_keys,
            vec![
                "site.brand.icon".to_string(),
                "site.nav[1].rel".to_string(),
                "site.title".to_string()
            ]
        );
    }

    #[test]
    fn project_site_caps_list_lengths() {
        let nav: Vec<String> = (0..=MAX_SITE_ITEMS)
            .map(|i| format!("{{ label = \"n{i}\", href = \"/{i}\" }}"))
            .collect();
        let err = project(&format!("[site]\nnav = [{}]\n", nav.join(", ")))
            .unwrap_err()
            .to_string();
        assert!(err.contains("site.nav"), "{err}");
    }
}
