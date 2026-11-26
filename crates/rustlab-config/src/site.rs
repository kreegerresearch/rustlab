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
    let _ = href;
    Ok(())
}

/// Stylesheet URL: `http(s)://host…` or a relative / root-relative path.
pub fn check_stylesheet_url(url: &str) -> Result<(), String> {
    let _ = url;
    Ok(())
}

/// `scheme://host[:port]` of an absolute stylesheet URL, lowercased, for a
/// CSP `style-src` source. `None` for a relative URL (covered by `'self'`).
pub fn stylesheet_origin(url: &str) -> Option<String> {
    let _ = url;
    None
}

/// Visible text: a label or the footer.
pub fn check_site_text(text: &str, allow_empty: bool) -> Result<(), String> {
    let _ = (text, allow_empty);
    Ok(())
}

pub(crate) fn parse_site_table(
    val: &TomlValue,
    path: &Path,
    unknown: &mut Vec<String>,
) -> Result<SiteConfig, ConfigError> {
    let _ = (val, path, unknown);
    Ok(SiteConfig::default())
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
