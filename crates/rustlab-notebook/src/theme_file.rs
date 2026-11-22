//! Theme files: TOML that names a parent theme and overrides some tokens.
//!
//! ```toml
//! name = "course"          # optional; defaults to the file stem
//! inherits = "latte"       # builtin or another theme file; default mocha
//!
//! [palette]                # private swatches, referenced by name below
//! brand = "#0b5394"
//!
//! [colors]                 # roles; #RGB, #RRGGBB, or a palette name
//! accent_primary = "brand"
//!
//! [typography]
//! body = ["Charter", "Georgia", "serif"]
//! measure = "72ch"
//!
//! [shape]
//! radius = "4px"
//!
//! [space]
//! 3 = "1.25rem"
//! ```
//!
//! Values end up inside a `<style>` block, so they are a CSS injection
//! surface (`docs/security.md`, plan §7.1). Each value must match a closed
//! grammar and is re-emitted from its parsed parts; anything else is a
//! load error naming the file and key. There is no `url()`, `@import`,
//! `var()`, or free text, so a theme cannot fetch a resource or close the
//! style element. Fonts are system font stacks only: family names, emitted
//! quoted, ending in a generic family.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rustlab_plot::contrast::ContrastCheck;
use rustlab_plot::{theme_colors, ResolvedTheme, ThemeColors, ThemeMode};

/// Names a theme file may not take: today's builtins and aliases, and the
/// builtins the plan adds next, so a user file cannot shadow either.
pub const RESERVED_NAMES: &[&str] = &[
    "mocha",
    "macchiato",
    "frappe",
    "latte",
    "dark",
    "light",
    "catppuccin",
    "clean",
    "paper",
    "docs",
    "contrast",
];

/// Builtins `theme list` prints, canonical names first.
pub const BUILTIN_LISTING: &[(&str, &str)] = &[
    ("mocha", "Catppuccin Mocha (default; alias dark)"),
    ("macchiato", "Catppuccin Macchiato"),
    ("frappe", "Catppuccin Frappé"),
    ("latte", "Catppuccin Latte (alias light)"),
];

const MAX_CHAIN: usize = 16;
const MAX_VALUE_LEN: usize = 64;

/// Where named themes are looked up, after the builtins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeSearch {
    /// `<project root>/themes`, or `rustlab.toml` `[notebook] theme_dir`.
    pub project: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME/rustlab/themes` (default `~/.config/rustlab/themes`).
    pub user: Option<PathBuf>,
}

impl ThemeSearch {
    /// The user theme directory from `XDG_CONFIG_HOME` / `HOME`.
    pub fn user_dir_from_env() -> Option<PathBuf> {
        let xdg = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty());
        let base = match xdg {
            Some(x) => PathBuf::from(x),
            None => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
        };
        Some(base.join("rustlab").join("themes"))
    }

    fn dirs(&self) -> Vec<(ThemeOrigin, &Path)> {
        let mut v = Vec::new();
        if let Some(p) = &self.project {
            v.push((ThemeOrigin::Project, p.as_path()));
        }
        if let Some(u) = &self.user {
            v.push((ThemeOrigin::User, u.as_path()));
        }
        v
    }
}

/// Where a resolved theme came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeOrigin {
    Builtin,
    Project,
    User,
    /// `--theme-file PATH` or `theme check PATH`.
    File,
}

impl ThemeOrigin {
    pub fn label(self) -> &'static str {
        match self {
            ThemeOrigin::Builtin => "builtin",
            ThemeOrigin::Project => "project",
            ThemeOrigin::User => "user",
            ThemeOrigin::File => "file",
        }
    }
}

/// A theme after `inherits` has been applied.
#[derive(Debug, Clone)]
pub struct LoadedTheme {
    pub theme: ResolvedTheme,
    pub origin: ThemeOrigin,
    pub path: Option<PathBuf>,
    /// This theme, then each parent, ending at a builtin.
    pub chain: Vec<String>,
}

impl LoadedTheme {
    /// Screen pairs that miss their threshold. PDF always prints Latte,
    /// so a theme file cannot change the print checks.
    pub fn contrast_failures(&self) -> Vec<ContrastCheck> {
        rustlab_plot::contrast::screen_checks(&self.theme.colors)
            .into_iter()
            .filter(|c| !c.passes())
            .collect()
    }
}

/// One row of `theme list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeListing {
    pub name: String,
    pub origin: ThemeOrigin,
    pub path: Option<PathBuf>,
    /// Builtin description, or the load error for a broken file.
    pub note: String,
}

/// `true` for one of the builtin names or aliases (any case).
pub fn is_builtin(name: &str) -> bool {
    theme_colors(name).is_some()
}

/// Resolve `name`: builtin, then `search.project`, then `search.user`.
pub fn resolve(name: &str, search: &ThemeSearch) -> Result<LoadedTheme, String> {
    let mut stack = Vec::new();
    resolve_inner(name, search, &mut stack).map(|r| r.loaded)
}

/// Load a theme from an explicit path (`--theme-file`, `theme check`).
pub fn load_path(path: &Path, search: &ThemeSearch) -> Result<LoadedTheme, String> {
    let mut stack = Vec::new();
    load_file(path, None, ThemeOrigin::File, search, &mut stack).map(|r| r.loaded)
}

/// Builtins, then project files, then user files. A name found in the
/// project directory hides the same name in the user directory.
pub fn list(search: &ThemeSearch) -> Vec<ThemeListing> {
    let mut out: Vec<ThemeListing> = BUILTIN_LISTING
        .iter()
        .map(|(n, d)| ThemeListing {
            name: n.to_string(),
            origin: ThemeOrigin::Builtin,
            path: None,
            note: d.to_string(),
        })
        .collect();
    for (origin, dir) in search.dirs() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
            .collect();
        files.sort();
        for path in files {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !rustlab_config::is_theme_name(stem) || RESERVED_NAMES.contains(&stem) {
                continue;
            }
            if out.iter().any(|l| l.name == stem) {
                continue;
            }
            let note = match resolve(stem, search) {
                Ok(t) if t.path.as_deref() == Some(path.as_path()) => {
                    format!(
                        "inherits {}",
                        t.chain.get(1).map(String::as_str).unwrap_or("mocha")
                    )
                }
                Ok(_) => continue,
                Err(e) => format!("error: {e}"),
            };
            out.push(ThemeListing {
                name: stem.to_string(),
                origin,
                path: Some(path),
                note,
            });
        }
    }
    out
}

struct Resolved {
    loaded: LoadedTheme,
    palette: BTreeMap<String, String>,
}

fn resolve_inner(
    name: &str,
    search: &ThemeSearch,
    stack: &mut Vec<String>,
) -> Result<Resolved, String> {
    if let Some(colors) = theme_colors(name) {
        let theme = ResolvedTheme::from_colors(colors);
        let canonical = theme.name.clone();
        return Ok(Resolved {
            loaded: LoadedTheme {
                theme,
                origin: ThemeOrigin::Builtin,
                path: None,
                chain: vec![canonical],
            },
            palette: BTreeMap::new(),
        });
    }
    if !rustlab_config::is_theme_name(name) {
        return Err(format!(
            "`{name}` is not a theme name (lowercase letters, digits, - and _; a name, never a path)"
        ));
    }
    if stack.iter().any(|n| n == name) {
        let mut chain = stack.clone();
        chain.push(name.to_string());
        return Err(format!("theme inheritance cycle: {}", chain.join(" -> ")));
    }
    for (origin, dir) in search.dirs() {
        let path = dir.join(format!("{name}.toml"));
        if path.is_file() {
            check_inside(&path, dir)?;
            return load_file(&path, Some(name), origin, search, stack);
        }
    }
    let mut looked: Vec<String> = search
        .dirs()
        .iter()
        .map(|(_, d)| d.join(format!("{name}.toml")).display().to_string())
        .collect();
    if looked.is_empty() {
        looked.push("no theme directories".to_string());
    }
    Err(format!(
        "unknown theme `{name}` (builtins: {}; looked for {})",
        rustlab_plot::builtin_theme_names().join(", "),
        looked.join(", ")
    ))
}

/// A discovered file must resolve inside its directory (no symlink out).
fn check_inside(path: &Path, dir: &Path) -> Result<(), String> {
    let (Ok(p), Ok(d)) = (std::fs::canonicalize(path), std::fs::canonicalize(dir)) else {
        return Ok(());
    };
    if p.starts_with(&d) {
        Ok(())
    } else {
        Err(format!(
            "{}: resolves outside {} (theme files must live in the theme directory)",
            path.display(),
            dir.display()
        ))
    }
}

fn load_file(
    path: &Path,
    lookup_name: Option<&str>,
    origin: ThemeOrigin,
    search: &ThemeSearch,
    stack: &mut Vec<String>,
) -> Result<Resolved, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let at = |key: &str, msg: String| format!("{}: {key}: {msg}", path.display());
    let file = parse_file(&text).map_err(|(key, msg)| at(&key, msg))?;

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let name = match (&file.name, lookup_name) {
        (Some(n), Some(look)) if n != look => {
            return Err(at(
                "name",
                format!("\"{n}\" does not match the file name `{look}.toml`"),
            ))
        }
        (Some(n), _) => n.clone(),
        (None, Some(look)) => look.to_string(),
        (None, None) if rustlab_config::is_theme_name(stem) => stem.to_string(),
        (None, None) => {
            return Err(at(
                "name",
                format!("the file name `{stem}` is not a theme name; set `name = \"…\"`"),
            ))
        }
    };
    if !rustlab_config::is_theme_name(&name) {
        return Err(at("name", format!("\"{name}\" is not a theme name")));
    }
    if RESERVED_NAMES.contains(&name.as_str()) {
        return Err(at(
            "name",
            format!("\"{name}\" is a builtin name; choose another name"),
        ));
    }
    if stack.len() >= MAX_CHAIN {
        return Err(at(
            "inherits",
            format!("more than {MAX_CHAIN} levels of inherits"),
        ));
    }
    if stack.iter().any(|n| *n == name) {
        let mut chain = stack.clone();
        chain.push(name.clone());
        return Err(format!("theme inheritance cycle: {}", chain.join(" -> ")));
    }
    stack.push(name.clone());
    let parent_name = file.inherits.clone().unwrap_or_else(|| "mocha".to_string());
    let parent = resolve_inner(&parent_name, search, stack)
        .map_err(|e| format!("{}: inherits: {e}", path.display()))?;
    stack.pop();

    let mut palette = parent.palette;
    palette.extend(file.palette.clone());
    let mut theme = parent.loaded.theme;
    theme.name = name.clone();
    for (role, raw) in &file.colors {
        let value =
            color_value(raw, &palette, role).map_err(|m| at(&format!("colors.{role}"), m))?;
        *role_mut(&mut theme.colors, role).expect("role checked at parse") = value.into();
    }
    file.apply_design(&mut theme);
    theme.mode = if theme.colors.is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    let mut chain = vec![name];
    chain.extend(parent.loaded.chain);
    Ok(Resolved {
        loaded: LoadedTheme {
            theme,
            origin,
            path: Some(path.to_path_buf()),
            chain,
        },
        palette,
    })
}

/// Parsed and grammar-checked file contents, before inheritance.
#[derive(Debug, Default)]
struct ThemeFile {
    name: Option<String>,
    inherits: Option<String>,
    palette: BTreeMap<String, String>,
    /// Role → raw value (hex or palette name), checked again on apply.
    colors: BTreeMap<String, String>,
    typography: BTreeMap<String, String>,
    shape: BTreeMap<String, String>,
    space: BTreeMap<String, String>,
}

type KeyError = (String, String);

/// Tables a later phase of the theme plan adds.
const LATER: &[(&str, &str)] = &[
    ("dark", "light/dark pairs are not supported yet"),
    ("light", "light/dark pairs are not supported yet"),
    (
        "print",
        "print variants are not supported yet; PDF prints Latte",
    ),
    ("plot", "plot theming is not supported yet"),
    ("table", "table styles are not supported yet"),
];

fn parse_file(text: &str) -> Result<ThemeFile, KeyError> {
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| ("toml".to_string(), e.message().to_string()))?;
    let mut out = ThemeFile::default();
    for (key, val) in &table {
        match key.as_str() {
            "name" => out.name = Some(string(val, key)?),
            "inherits" => {
                let s = string(val, key)?;
                let s = s.trim().to_ascii_lowercase();
                if !is_builtin(&s) && !rustlab_config::is_theme_name(&s) {
                    return Err((key.clone(), format!("\"{s}\" is not a theme name")));
                }
                out.inherits = Some(s);
            }
            "palette" => {
                for (k, v) in sub_table(val, key)? {
                    let full = format!("palette.{k}");
                    if !is_ident(k) {
                        return Err((
                            full,
                            "palette names are lowercase letters, digits, - and _".into(),
                        ));
                    }
                    let raw = string(v, &full)?;
                    hex_color(&raw, true).map_err(|m| (full.clone(), m))?;
                    out.palette.insert(k.clone(), raw);
                }
            }
            "colors" => {
                for (k, v) in sub_table(val, key)? {
                    let full = format!("colors.{k}");
                    if role_mut(&mut rustlab_plot::Theme::Mocha.colors().clone(), k).is_none() {
                        return Err((
                            full,
                            format!("unknown color role (known: {})", ROLES.join(", ")),
                        ));
                    }
                    out.colors.insert(k.clone(), string(v, &full)?);
                }
            }
            "typography" => {
                for (k, v) in sub_table(val, key)? {
                    let full = format!("typography.{k}");
                    let value = match k.as_str() {
                        "body" | "heading" | "mono" => font_stack(v, &full)?,
                        "size" => {
                            let s = string(v, &full)?;
                            if s == "medium" {
                                s
                            } else {
                                length(&s).map_err(|m| (full.clone(), m))?
                            }
                        }
                        "line_height" => {
                            let s = string(v, &full)?;
                            number(&s).or_else(|_| length(&s)).map_err(|_| {
                                (
                                    full.clone(),
                                    format!("expected a number or a length; got \"{s}\""),
                                )
                            })?
                        }
                        "measure" => length(&string(v, &full)?).map_err(|m| (full.clone(), m))?,
                        _ => return Err((
                            full,
                            "unknown key (known: body, heading, mono, size, line_height, measure)"
                                .into(),
                        )),
                    };
                    out.typography.insert(k.clone(), value);
                }
            }
            "shape" => {
                for (k, v) in sub_table(val, key)? {
                    let full = format!("shape.{k}");
                    if !matches!(
                        k.as_str(),
                        "radius_sm" | "radius" | "radius_lg" | "border_width"
                    ) {
                        return Err((
                            full,
                            "unknown key (known: radius_sm, radius, radius_lg, border_width)"
                                .into(),
                        ));
                    }
                    let value = length(&string(v, &full)?).map_err(|m| (full.clone(), m))?;
                    out.shape.insert(k.clone(), value);
                }
            }
            "space" => {
                for (k, v) in sub_table(val, key)? {
                    let full = format!("space.{k}");
                    if !matches!(k.as_str(), "1" | "2" | "3" | "4" | "5" | "6") {
                        return Err((full, "unknown step (known: 1 to 6)".into()));
                    }
                    let value = length(&string(v, &full)?).map_err(|m| (full.clone(), m))?;
                    out.space.insert(k.clone(), value);
                }
            }
            other => {
                if let Some((_, why)) = LATER.iter().find(|(k, _)| *k == other) {
                    return Err((other.to_string(), why.to_string()));
                }
                return Err((
                    other.to_string(),
                    "unknown key (known: name, inherits, palette, colors, typography, shape, space)".into(),
                ));
            }
        }
    }
    Ok(out)
}

impl ThemeFile {
    fn apply_design(&self, t: &mut ResolvedTheme) {
        for (k, v) in &self.typography {
            let slot = match k.as_str() {
                "body" => &mut t.typography.body,
                "heading" => &mut t.typography.heading,
                "mono" => &mut t.typography.mono,
                "size" => &mut t.typography.size,
                "line_height" => &mut t.typography.line_height,
                _ => &mut t.typography.measure,
            };
            *slot = v.clone();
        }
        for (k, v) in &self.shape {
            let slot = match k.as_str() {
                "radius_sm" => &mut t.shape.radius_sm,
                "radius" => &mut t.shape.radius,
                "radius_lg" => &mut t.shape.radius_lg,
                _ => &mut t.shape.border_width,
            };
            *slot = v.clone();
        }
        for (k, v) in &self.space {
            let slot = match k.as_str() {
                "1" => &mut t.space.s1,
                "2" => &mut t.space.s2,
                "3" => &mut t.space.s3,
                "4" => &mut t.space.s4,
                "5" => &mut t.space.s5,
                _ => &mut t.space.s6,
            };
            *slot = v.clone();
        }
    }
}

fn string(v: &toml::Value, key: &str) -> Result<String, KeyError> {
    match v {
        toml::Value::String(s) if s.len() <= MAX_VALUE_LEN * 4 => Ok(s.clone()),
        toml::Value::String(_) => Err((key.to_string(), "value is too long".into())),
        other => Err((
            key.to_string(),
            format!("expected a string, got {}", other.type_str()),
        )),
    }
}

fn sub_table<'a>(v: &'a toml::Value, key: &str) -> Result<&'a toml::Table, KeyError> {
    v.as_table().ok_or_else(|| {
        (
            key.to_string(),
            format!("expected a table, got {}", v.type_str()),
        )
    })
}

fn is_ident(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_VALUE_LEN
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-' || *c == b'_')
}

/// Role names a `[colors]` key may use, in [`ThemeColors`] field order.
pub const ROLES: &[&str] = &[
    "bg",
    "bg_secondary",
    "text",
    "text_dim",
    "border",
    "border_subtle",
    "accent_primary",
    "accent_secondary",
    "accent_tertiary",
    "code_bg",
    "output_bg",
    "inline_code_bg",
    "error_bg",
    "error_text",
    "plot_bg",
    "plot_grid",
    "syn_keyword",
    "syn_function",
    "syn_number",
    "syn_string",
    "syn_comment",
    "syn_operator",
    "footer_text",
    "cm_mark_bg",
    "cm_note_bg",
    "cm_note_border",
    "dx_add_bar",
    "dx_mod_bar",
    "dx_del_bar",
    "dx_add_line",
    "dx_del_line",
    "dx_add_word",
    "dx_del_word",
    "shadow",
    "scrim",
    "banner_bg",
    "banner_text",
];

fn role_mut<'a>(
    c: &'a mut ThemeColors,
    role: &str,
) -> Option<&'a mut std::borrow::Cow<'static, str>> {
    Some(match role {
        "bg" => &mut c.bg,
        "bg_secondary" => &mut c.bg_secondary,
        "text" => &mut c.text,
        "text_dim" => &mut c.text_dim,
        "border" => &mut c.border,
        "border_subtle" => &mut c.border_subtle,
        "accent_primary" => &mut c.accent_primary,
        "accent_secondary" => &mut c.accent_secondary,
        "accent_tertiary" => &mut c.accent_tertiary,
        "code_bg" => &mut c.code_bg,
        "output_bg" => &mut c.output_bg,
        "inline_code_bg" => &mut c.inline_code_bg,
        "error_bg" => &mut c.error_bg,
        "error_text" => &mut c.error_text,
        "plot_bg" => &mut c.plot_bg,
        "plot_grid" => &mut c.plot_grid,
        "syn_keyword" => &mut c.syn_keyword,
        "syn_function" => &mut c.syn_function,
        "syn_number" => &mut c.syn_number,
        "syn_string" => &mut c.syn_string,
        "syn_comment" => &mut c.syn_comment,
        "syn_operator" => &mut c.syn_operator,
        "footer_text" => &mut c.footer_text,
        "cm_mark_bg" => &mut c.cm_mark_bg,
        "cm_note_bg" => &mut c.cm_note_bg,
        "cm_note_border" => &mut c.cm_note_border,
        "dx_add_bar" => &mut c.dx_add_bar,
        "dx_mod_bar" => &mut c.dx_mod_bar,
        "dx_del_bar" => &mut c.dx_del_bar,
        "dx_add_line" => &mut c.dx_add_line,
        "dx_del_line" => &mut c.dx_del_line,
        "dx_add_word" => &mut c.dx_add_word,
        "dx_del_word" => &mut c.dx_del_word,
        "shadow" => &mut c.shadow,
        "scrim" => &mut c.scrim,
        "banner_bg" => &mut c.banner_bg,
        "banner_text" => &mut c.banner_text,
        _ => return None,
    })
}

/// A `[colors]` value: hex or a palette name, re-emitted as lowercase
/// `#rrggbb`. Only `plot_grid` may carry alpha (`#RRGGBBAA` → `rgba(…)`).
fn color_value(
    raw: &str,
    palette: &BTreeMap<String, String>,
    role: &str,
) -> Result<String, String> {
    let alpha_ok = role == "plot_grid";
    if raw.starts_with('#') {
        return hex_color(raw, alpha_ok);
    }
    if is_ident(raw) {
        return match palette.get(raw) {
            Some(hex) => hex_color(hex, alpha_ok).map_err(|m| format!("palette `{raw}`: {m}")),
            None => Err(format!("unknown palette name `{raw}`")),
        };
    }
    Err(format!(
        "expected #RGB, #RRGGBB, or a [palette] name; got \"{raw}\""
    ))
}

fn hex_color(raw: &str, alpha_ok: bool) -> Result<String, String> {
    let bad = || format!("expected #RGB, #RRGGBB, or #RRGGBBAA; got \"{raw}\"");
    let hex = raw.strip_prefix('#').ok_or_else(bad)?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let hex = hex.to_ascii_lowercase();
    match hex.len() {
        3 => Ok(format!(
            "#{}",
            hex.chars().flat_map(|c| [c, c]).collect::<String>()
        )),
        6 => Ok(format!("#{hex}")),
        8 if alpha_ok => {
            let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex checked");
            let a = (f64::from(byte(6)) / 255.0 * 1000.0).round() / 1000.0;
            Ok(format!("rgba({},{},{},{a})", byte(0), byte(2), byte(4)))
        }
        8 => Err(format!(
            "\"{raw}\": alpha is only accepted for plot_grid (as a [palette] value it is fine)"
        )),
        _ => Err(bad()),
    }
}

const GENERIC_FAMILIES: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
];

/// System keywords that only work unquoted.
const UNQUOTED_SYSTEM: &[&str] = &["-apple-system", "BlinkMacSystemFont"];

fn font_stack(v: &toml::Value, key: &str) -> Result<String, KeyError> {
    let toml::Value::Array(items) = v else {
        return Err((
            key.to_string(),
            "expected a list of family names ending in a generic family, e.g. [\"Georgia\", \"serif\"]".into(),
        ));
    };
    if items.is_empty() || items.len() > 16 {
        return Err((key.to_string(), "expected 1 to 16 family names".into()));
    }
    let mut parts = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let name = string(item, key)?;
        let name = name.trim();
        let ok = !name.is_empty()
            && name.len() <= MAX_VALUE_LEN
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b' ' || b == b'_' || b == b'-');
        if !ok {
            return Err((
                key.to_string(),
                format!("font family \"{name}\" may only use letters, digits, spaces, - and _ (system fonts only; no url() or @font-face)"),
            ));
        }
        let last = i + 1 == items.len();
        if GENERIC_FAMILIES.contains(&name) {
            parts.push(name.to_string());
        } else if last {
            return Err((
                key.to_string(),
                format!(
                    "a font stack must end in a generic family ({}); got \"{name}\"",
                    GENERIC_FAMILIES.join(", ")
                ),
            ));
        } else if UNQUOTED_SYSTEM.contains(&name) {
            parts.push(name.to_string());
        } else {
            parts.push(format!("\"{name}\""));
        }
    }
    Ok(parts.join(", "))
}

fn number(s: &str) -> Result<String, String> {
    let bad = || format!("expected a number; got \"{s}\"");
    let mut parts = s.splitn(2, '.');
    let int = parts.next().unwrap_or("");
    let frac = parts.next();
    let digits = |p: &str| !p.is_empty() && p.len() <= 6 && p.bytes().all(|b| b.is_ascii_digit());
    let ok = digits(int) && frac.map_or(true, digits);
    if ok {
        Ok(s.to_string())
    } else {
        Err(bad())
    }
}

fn length(s: &str) -> Result<String, String> {
    if s == "0" {
        return Ok(s.to_string());
    }
    for unit in ["rem", "px", "em", "ch", "%"] {
        if let Some(n) = s.strip_suffix(unit) {
            if number(n).is_ok() {
                return Ok(s.to_string());
            }
        }
    }
    Err(format!(
        "expected a length: a number with px, rem, em, ch, or %; got \"{s}\""
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustlab_plot::Theme;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rl-theme-file-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(format!("{name}.toml"));
        std::fs::write(&p, body).unwrap();
        p
    }

    fn search(project: &Path) -> ThemeSearch {
        ThemeSearch {
            project: Some(project.to_path_buf()),
            user: None,
        }
    }

    #[test]
    fn builtins_resolve_before_files() {
        let d = dir("builtin");
        let t = resolve("Light", &search(&d)).unwrap();
        assert_eq!(t.origin, ThemeOrigin::Builtin);
        assert_eq!(t.theme.colors, *Theme::Latte.colors());
        assert_eq!(t.chain, vec!["latte".to_string()]);
        // A file cannot take a builtin's name, so it can never shadow one.
        write(&d, "mocha", "[colors]\nbg = \"#000000\"\n");
        assert_eq!(
            resolve("mocha", &search(&d)).unwrap().origin,
            ThemeOrigin::Builtin
        );
        let err = load_path(&d.join("mocha.toml"), &search(&d)).unwrap_err();
        assert!(err.contains("builtin name"), "{err}");
    }

    #[test]
    fn palette_colors_and_design_groups_apply_over_the_parent() {
        let d = dir("apply");
        write(
            &d,
            "course",
            r##"
inherits = "latte"
[palette]
brand = "#0B5394"
ink = "#222"
[colors]
accent_primary = "brand"
text = "ink"
plot_grid = "#00000033"
[typography]
body = ["Charter", "Bitstream Charter", "Georgia", "serif"]
mono = ["JetBrains Mono", "ui-monospace", "monospace"]
measure = "72ch"
line_height = "1.6"
[shape]
radius = "4px"
[space]
3 = "1.25rem"
"##,
        );
        let t = resolve("course", &search(&d)).unwrap();
        let c = &t.theme.colors;
        assert_eq!(c.accent_primary, "#0b5394");
        assert_eq!(c.text, "#222222");
        assert_eq!(c.plot_grid, "rgba(0,0,0,0.2)");
        assert_eq!(c.bg, Theme::Latte.colors().bg);
        assert_eq!(t.theme.name, "course");
        assert_eq!(t.theme.mode, ThemeMode::Light);
        assert_eq!(
            t.theme.typography.body,
            r#""Charter", "Bitstream Charter", "Georgia", serif"#
        );
        assert_eq!(
            t.theme.typography.mono,
            r#""JetBrains Mono", ui-monospace, monospace"#
        );
        assert_eq!(t.theme.typography.measure, "72ch");
        assert_eq!(t.theme.typography.line_height, "1.6");
        assert_eq!(t.theme.shape.radius, "4px");
        assert_eq!(t.theme.space.s3, "1.25rem");
        assert_eq!(t.chain, vec!["course".to_string(), "latte".to_string()]);
        assert_eq!(t.origin, ThemeOrigin::Project);
        let css = t.theme.css_custom_properties();
        assert!(css.contains("--rl-accent-primary: #0b5394;"));
        assert!(css.contains("--rl-measure: 72ch;"));
    }

    #[test]
    fn inherits_chains_through_files_and_defaults_to_mocha() {
        let d = dir("chain");
        write(
            &d,
            "base",
            "[palette]\nbrand = \"#336699\"\n[colors]\nbg = \"#101018\"\n",
        );
        write(
            &d,
            "child",
            "inherits = \"base\"\n[colors]\naccent_primary = \"brand\"\n",
        );
        let t = resolve("child", &search(&d)).unwrap();
        assert_eq!(t.chain, vec!["child", "base", "mocha"]);
        assert_eq!(t.theme.colors.bg, "#101018");
        assert_eq!(t.theme.colors.accent_primary, "#336699");
        assert_eq!(t.theme.colors.text, Theme::Mocha.colors().text);
    }

    #[test]
    fn inheritance_cycles_name_the_chain() {
        let d = dir("cycle");
        write(&d, "a", "inherits = \"b\"\n");
        write(&d, "b", "inherits = \"c\"\n");
        write(&d, "c", "inherits = \"a\"\n");
        let err = resolve("a", &search(&d)).unwrap_err();
        assert!(err.contains("cycle: a -> b -> c -> a"), "{err}");
        write(&d, "selfish", "inherits = \"selfish\"\n");
        let err = resolve("selfish", &search(&d)).unwrap_err();
        assert!(err.contains("selfish -> selfish"), "{err}");
    }

    #[test]
    fn unknown_names_and_keys_are_errors_with_the_path_and_key() {
        let d = dir("errors");
        let err = resolve("nope", &search(&d)).unwrap_err();
        assert!(
            err.contains("unknown theme `nope`") && err.contains("nope.toml"),
            "{err}"
        );
        for (body, key, needle) in [
            ("colour = 1\n", "colour", "unknown key"),
            (
                "[colors]\nbackground = \"#000\"\n",
                "colors.background",
                "unknown color role",
            ),
            (
                "[colors]\nbg = \"brandx\"\n",
                "colors.bg",
                "unknown palette name",
            ),
            ("[colors]\nbg = \"#12345\"\n", "colors.bg", "expected #RGB"),
            (
                "[colors]\nbg = \"#11223344\"\n",
                "colors.bg",
                "only accepted for plot_grid",
            ),
            (
                "[typography]\nbody = [\"Georgia\"]\n",
                "typography.body",
                "generic family",
            ),
            (
                "[typography]\nbody = \"serif\"\n",
                "typography.body",
                "list of family names",
            ),
            (
                "[shape]\nradius = \"4\"\n",
                "shape.radius",
                "expected a length",
            ),
            ("[space]\n7 = \"1rem\"\n", "space.7", "unknown step"),
            (
                "[dark.colors]\nbg = \"#000\"\n",
                "dark",
                "not supported yet",
            ),
            ("[print]\n", "print", "not supported yet"),
            ("[colors]\nbg = 3\n", "colors.bg", "expected a string"),
            ("inherits = \"../x\"\n", "inherits", "not a theme name"),
        ] {
            let p = write(&d, "t", body);
            let err = load_path(&p, &search(&d)).unwrap_err();
            let want = format!("{}: {key}:", p.display());
            assert!(
                err.starts_with(&want) && err.contains(needle),
                "{body:?}\n{err}"
            );
        }
        // A discovered file is looked up by its stem, so `name` must agree.
        let p = write(&d, "t", "name = \"other\"\n");
        let err = resolve("t", &search(&d)).unwrap_err();
        assert!(
            err.starts_with(&format!("{}: name:", p.display())) && err.contains("does not match"),
            "{err}"
        );
    }

    /// Values land in a `<style>` block. None of these may get through.
    #[test]
    fn value_grammar_rejects_css_injection() {
        let d = dir("inject");
        let attempts = [
            ("[colors]\nbg = \"red;}\"\n", "colors.bg"),
            (
                "[colors]\nbg = \"#000;} body{display:none} .x{\"\n",
                "colors.bg",
            ),
            (
                "[colors]\nbg = \"</style><script>alert(1)</script>\"\n",
                "colors.bg",
            ),
            (
                "[colors]\nbg = \"url(https://evil.example/x.png)\"\n",
                "colors.bg",
            ),
            ("[colors]\nbg = \"var(--x)\"\n", "colors.bg"),
            ("[colors]\nbg = \"expression(alert(1))\"\n", "colors.bg"),
            ("[colors]\nbg = \"red\"\n", "colors.bg"),
            (
                "[palette]\nx = \"red;}\"\n[colors]\nbg = \"x\"\n",
                "palette.x",
            ),
            (
                "[typography]\nbody = [\"x\\\"; } body { color: red\", \"serif\"]\n",
                "typography.body",
            ),
            (
                "[typography]\nbody = [\"url(evil)\", \"serif\"]\n",
                "typography.body",
            ),
            (
                "[typography]\nbody = [\"@font-face\", \"serif\"]\n",
                "typography.body",
            ),
            (
                "[typography]\nbody = [\"@import\", \"serif\"]\n",
                "typography.body",
            ),
            ("[typography]\nmeasure = \"72ch;}\"\n", "typography.measure"),
            (
                "[typography]\nmeasure = \"calc(100% - 1px)\"\n",
                "typography.measure",
            ),
            (
                "[typography]\nsize = \"1rem!important\"\n",
                "typography.size",
            ),
            ("[shape]\nradius = \"4px</style>\"\n", "shape.radius"),
            ("[space]\n1 = \"expression(1)\"\n", "space.1"),
        ];
        for (body, key) in attempts {
            let p = write(&d, "evil", body);
            let err = load_path(&p, &search(&d)).unwrap_err();
            assert!(
                err.contains(&format!(": {key}:")),
                "{body:?} not refused at {key}: {err}"
            );
        }
    }

    #[test]
    fn theme_file_name_defaults_to_the_stem_and_must_be_a_name() {
        let d = dir("stem");
        let p = d.join("My Theme.toml");
        std::fs::write(&p, "").unwrap();
        let err = load_path(&p, &search(&d)).unwrap_err();
        assert!(err.contains("set `name"), "{err}");
        std::fs::write(&p, "name = \"mine\"\n").unwrap();
        assert_eq!(load_path(&p, &search(&d)).unwrap().theme.name, "mine");
        let q = write(&d, "plain", "");
        let t = load_path(&q, &search(&d)).unwrap();
        assert_eq!(t.theme.name, "plain");
        assert_eq!(t.origin, ThemeOrigin::File);
        // An empty file is mocha under another name.
        assert_eq!(t.theme.colors, *Theme::Mocha.colors());
    }

    #[test]
    fn project_dir_wins_over_user_dir_and_list_shows_both() {
        let project = dir("list-project");
        let user = dir("list-user");
        write(&project, "shared", "[colors]\nbg = \"#111111\"\n");
        write(&user, "shared", "[colors]\nbg = \"#222222\"\n");
        write(&user, "solo", "inherits = \"latte\"\n");
        write(&user, "broken", "[colors]\nbg = \"oops;\"\n");
        let s = ThemeSearch {
            project: Some(project.clone()),
            user: Some(user.clone()),
        };
        assert_eq!(resolve("shared", &s).unwrap().theme.colors.bg, "#111111");
        assert_eq!(resolve("solo", &s).unwrap().origin, ThemeOrigin::User);
        let rows = list(&s);
        let names: Vec<_> = rows.iter().map(|r| (r.name.as_str(), r.origin)).collect();
        assert_eq!(
            names,
            vec![
                ("mocha", ThemeOrigin::Builtin),
                ("macchiato", ThemeOrigin::Builtin),
                ("frappe", ThemeOrigin::Builtin),
                ("latte", ThemeOrigin::Builtin),
                ("shared", ThemeOrigin::Project),
                ("broken", ThemeOrigin::User),
                ("solo", ThemeOrigin::User),
            ]
        );
        assert!(rows
            .iter()
            .any(|r| r.name == "broken" && r.note.starts_with("error:")));
        assert!(rows
            .iter()
            .any(|r| r.name == "solo" && r.note == "inherits latte"));
    }

    #[test]
    fn contrast_failures_come_from_the_shared_checker() {
        let d = dir("contrast");
        write(&d, "fine", "inherits = \"latte\"\n");
        assert!(resolve("fine", &search(&d))
            .unwrap()
            .contrast_failures()
            .is_empty());
        write(
            &d,
            "faint",
            "[colors]\ntext_dim = \"#45475a\"\nsyn_comment = \"#313244\"\n",
        );
        let bad = resolve("faint", &search(&d)).unwrap().contrast_failures();
        let ids: Vec<_> = bad.iter().map(|c| c.id).collect();
        assert!(
            ids.contains(&"text-dim/page") && ids.contains(&"syn-comment/code"),
            "{ids:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn discovered_symlink_out_of_the_directory_is_refused() {
        let d = dir("link-in");
        let outside = dir("link-out");
        let target = write(&outside, "elsewhere", "");
        std::os::unix::fs::symlink(&target, d.join("sneaky.toml")).unwrap();
        let err = resolve("sneaky", &search(&d)).unwrap_err();
        assert!(err.contains("outside"), "{err}");
    }
}
