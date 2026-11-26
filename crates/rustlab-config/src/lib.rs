//! User-global settings for rustlab.
//!
//! v1 is a declarative TOML file — not an executable startup script.
//! Load order (first existing file wins):
//!
//! 1. `$XDG_CONFIG_HOME/rustlab/config.toml` (XDG home defaults to `~/.config`)
//! 2. `~/.rustlabrc`
//! 3. Built-in defaults (a missing file is not an error)
//!
//! Unknown keys warn (returned on [`LoadedConfig::unknown_keys`]); invalid
//! values error with the file path and key, except `[notebook] code`, which
//! warns (returned on [`LoadedConfig::warnings`]) and falls back to open.
//!
//! A project may also carry `rustlab.toml` (see [`find_project_config`] and
//! [`load_project`]). It accepts only `[notebook] theme`,
//! `[notebook] theme_dir`, and `[notebook] jail_root`; other keys warn.
//! `startup.rlab` is out of scope.

use std::env;
use std::path::{Path, PathBuf};

use toml::Value as TomlValue;

/// Resolved user settings. Every field is `Option` so callers can tell
/// "omitted → keep the built-in default" from an explicit value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserSettings {
    pub display: DisplaySettings,
    pub plot: PlotSettings,
    pub viewer: ViewerSettings,
    pub notebook: NotebookSettings,
    pub repl: ReplSettings,
    pub cache: CacheSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DisplaySettings {
    /// Numeric auto-print mode (`format` command).
    pub format: Option<DisplayFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayFormat {
    Short,
    Long,
    Hex,
    Commas,
}

impl DisplayFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Short => "short",
            Self::Long => "long",
            Self::Hex => "hex",
            Self::Commas => "commas",
        }
    }

    /// `"default"` is accepted as an alias for `short` (matches the
    /// in-script `format default` command).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "short" | "default" => Some(Self::Short),
            "long" => Some(Self::Long),
            "hex" => Some(Self::Hex),
            "commas" => Some(Self::Commas),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlotSettings {
    pub theme: Option<ColorTheme>,
    pub default_axis: Option<DefaultAxis>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorTheme {
    /// Catppuccin Mocha (alias `dark`).
    Mocha,
    Macchiato,
    Frappe,
    /// Catppuccin Latte (alias `light`).
    Latte,
}

impl ColorTheme {
    /// Canonical scheme name (`"mocha"`, …), never the `dark`/`light` alias.
    /// Consumers map it with `rustlab_plot::parse_theme`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mocha => "mocha",
            Self::Macchiato => "macchiato",
            Self::Frappe => "frappe",
            Self::Latte => "latte",
        }
    }

    /// Accepted values for the `theme` keys.
    pub const NAMES: &'static str = "mocha, macchiato, frappe, latte (aliases: dark, light)";

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "mocha" | "dark" => Some(Self::Mocha),
            "macchiato" => Some(Self::Macchiato),
            "frappe" | "frappé" => Some(Self::Frappe),
            "latte" | "light" => Some(Self::Latte),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultAxis {
    Ij,
    Xy,
}

impl DefaultAxis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ij => "ij",
            Self::Xy => "xy",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ij" => Some(Self::Ij),
            "xy" => Some(Self::Xy),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewerSettings {
    /// When `true`, the REPL (and `rustlab run` without an explicit
    /// `--plot`) try to connect to rustlab-viewer at startup.
    pub auto_connect: Option<bool>,
    /// Named viewer session (`rustlab-viewer --name`).
    pub name: Option<String>,
}

/// Initial open/collapsed state of a notebook's source disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeFold {
    Open,
    Collapsed,
}

impl CodeFold {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Collapsed => "collapsed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "open" => Some(Self::Open),
            "collapsed" => Some(Self::Collapsed),
            _ => None,
        }
    }

    pub fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotebookSettings {
    /// `[notebook] theme`: a builtin (stored by its canonical name) or a
    /// theme-file name. Resolved by the notebook renderer.
    pub theme: Option<String>,
    /// `[notebook] code`. Missing key → built-in default open.
    pub code: Option<CodeFold>,
    /// `[notebook] comments` = `"on"` | `"off"`. `None` leaves the format default.
    pub comments: Option<bool>,
    /// `[notebook] git_diff`. Missing key → off.
    pub git_diff: Option<bool>,
}

/// `[cache]` — SQLite persistent function cache. Off unless `enabled` is true.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CacheSettings {
    /// `[cache] enabled`. Missing or `false` keeps `cache enable` from
    /// creating or opening a store. `--cache` / `--no-cache` override this.
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplSettings {
    /// rustyline history cap. Built-in default is 1000.
    pub history_limit: Option<usize>,
    /// `[repl] history`. Missing key means history is on.
    /// `false` skips loading and saving `~/.rustlab_history`.
    /// `RUSTLAB_NO_HISTORY=1` forces this off even when the key is true.
    pub history: Option<bool>,
}

/// Built-in REPL history cap used when `[repl] history_limit` is omitted.
pub const DEFAULT_HISTORY_LIMIT: usize = 1000;

impl UserSettings {
    /// Effective numeric format, falling back to `short`.
    pub fn display_format(&self) -> DisplayFormat {
        self.display.format.unwrap_or(DisplayFormat::Short)
    }

    /// Effective plot theme, falling back to `dark`.
    pub fn plot_theme(&self) -> ColorTheme {
        self.plot.theme.unwrap_or(ColorTheme::Mocha)
    }

    /// Effective default axis, falling back to `ij`.
    pub fn default_axis(&self) -> DefaultAxis {
        self.plot.default_axis.unwrap_or(DefaultAxis::Ij)
    }

    /// Initial source disclosure. `[notebook] code`, else open.
    /// An invalid rc value is dropped at parse time, so this stays open.
    pub fn notebook_code_open(&self) -> bool {
        self.notebook.code.map(CodeFold::is_open).unwrap_or(true)
    }

    /// `[notebook] comments`, when set. `None` means the format default.
    pub fn notebook_comments(&self) -> Option<bool> {
        self.notebook.comments
    }

    /// `[notebook] git_diff`. Missing key stays off.
    pub fn notebook_git_diff(&self) -> bool {
        self.notebook.git_diff.unwrap_or(false)
    }

    /// Notebook page/plot theme name: `[notebook] theme`, else
    /// `[plot] theme`, else `mocha`. Any theme-file name is allowed here.
    pub fn notebook_theme(&self) -> String {
        match (&self.notebook.theme, self.plot.theme) {
            (Some(name), _) => name.clone(),
            (None, Some(t)) => t.as_str().to_string(),
            (None, None) => ColorTheme::Mocha.as_str().to_string(),
        }
    }

    /// Effective REPL history cap.
    pub fn history_limit(&self) -> usize {
        self.repl.history_limit.unwrap_or(DEFAULT_HISTORY_LIMIT)
    }

    /// `[repl] history`, default on. Does not read `RUSTLAB_NO_HISTORY`;
    /// the REPL applies that override itself.
    pub fn repl_history_enabled(&self) -> bool {
        self.repl.history.unwrap_or(true)
    }

    /// Effective viewer auto-connect flag.
    pub fn viewer_auto_connect(&self) -> bool {
        self.viewer.auto_connect.unwrap_or(false)
    }

    /// `[cache] enabled`. Missing key stays off, so `cache enable` does
    /// not create `cache.db` unless the user opts in.
    pub fn cache_enabled(&self) -> bool {
        self.cache.enabled.unwrap_or(false)
    }
}

/// Where the settings came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    Xdg(PathBuf),
    Rustlabrc(PathBuf),
    Defaults,
}

impl ConfigSource {
    /// Path of the file that was read, if any.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Xdg(p) | Self::Rustlabrc(p) => Some(p),
            Self::Defaults => None,
        }
    }
}

/// Result of a successful load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConfig {
    pub settings: UserSettings,
    pub source: ConfigSource,
    /// Dotted keys that were present but not in the v1 schema.
    /// Callers should warn once per key and continue.
    pub unknown_keys: Vec<String>,
    /// Soft warnings for recognised keys that fall back instead of aborting.
    /// Today only an invalid `[notebook] code` (expected `"open"` or
    /// `"collapsed"`). Callers should print each once and continue.
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path}: TOML parse error: {message}")]
    Parse { path: PathBuf, message: String },

    #[error("{path}: {key}: {message}")]
    Invalid {
        path: PathBuf,
        key: String,
        message: String,
    },
}

impl ConfigError {
    fn invalid(path: &Path, key: &str, message: impl Into<String>) -> Self {
        Self::Invalid {
            path: path.to_path_buf(),
            key: key.to_string(),
            message: message.into(),
        }
    }
}

/// File name of the project config.
pub const PROJECT_CONFIG: &str = "rustlab.toml";

/// Nearest `rustlab.toml`, walking up from `start` (a file's directory or
/// a directory). Stops at the first match or the filesystem root, and
/// does not go above `home` or the first directory with a `.git` entry
/// (a directory, or a file as in a worktree or submodule): that directory
/// itself is checked, its parents are not.
pub fn find_project_config(start: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let start = std::fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    let home = home.map(|h| std::fs::canonicalize(h).unwrap_or_else(|_| h.to_path_buf()));
    let mut dir: Option<&Path> = Some(start.as_path());
    while let Some(d) = dir {
        let candidate = d.join(PROJECT_CONFIG);
        if candidate.is_file() {
            return Some(candidate);
        }
        if home.as_deref() == Some(d) || d.join(".git").symlink_metadata().is_ok() {
            return None;
        }
        dir = d.parent();
    }
    None
}

/// `rustlab.toml`, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectConfig {
    /// The `rustlab.toml` that was read.
    pub path: PathBuf,
    /// Its directory: the project root.
    pub root: PathBuf,
    /// `[notebook] theme`, validated like the rc key.
    pub notebook_theme: Option<String>,
    /// `[notebook] theme_dir`, resolved against `root`. Must stay inside it.
    pub theme_dir: Option<PathBuf>,
    /// `[notebook] jail_root`: the notebook file-I/O jail, canonical. An
    /// existing directory at or below the canonical `root`; see
    /// [`project_jail_dir`].
    pub jail_root: Option<PathBuf>,
    /// Dotted keys this schema does not know. Callers warn once each.
    pub unknown_keys: Vec<String>,
}

/// Find and parse the project config for `start`. `Ok(None)` when there
/// is no `rustlab.toml` between `start` and `home` (or the root).
pub fn load_project(
    start: &Path,
    home: Option<&Path>,
) -> Result<Option<ProjectConfig>, ConfigError> {
    let Some(path) = find_project_config(start, home) else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&path).map_err(|source| ConfigError::Io {
        path: path.clone(),
        source,
    })?;
    parse_project_toml(&text, &path).map(Some)
}

/// Parse a `rustlab.toml` body that lives at `path`.
pub fn parse_project_toml(text: &str, path: &Path) -> Result<ProjectConfig, ConfigError> {
    let value: TomlValue = text
        .parse()
        .map_err(|e: toml::de::Error| ConfigError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
    let TomlValue::Table(table) = value else {
        return Err(ConfigError::Parse {
            path: path.to_path_buf(),
            message: "expected a TOML table at the top level".to_string(),
        });
    };
    let root = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut out = ProjectConfig {
        path: path.to_path_buf(),
        root: root.clone(),
        notebook_theme: None,
        theme_dir: None,
        jail_root: None,
        unknown_keys: Vec::new(),
    };
    for (key, val) in table {
        if key != "notebook" {
            out.unknown_keys.push(key);
            continue;
        }
        for (k, v) in expect_table(&val, path, "notebook")? {
            match k.as_str() {
                "theme" => {
                    let s = expect_string(v, path, "notebook.theme")?;
                    out.notebook_theme = Some(parse_theme_name(s, path, "notebook.theme")?);
                }
                "theme_dir" => {
                    let s = expect_string(v, path, "notebook.theme_dir")?;
                    out.theme_dir = Some(project_relative_dir(&root, s).ok_or_else(|| {
                        ConfigError::invalid(
                            path,
                            "notebook.theme_dir",
                            format!("\"{s}\" leaves the project root {}", root.display()),
                        )
                    })?);
                }
                "jail_root" => {
                    let s = expect_string(v, path, "notebook.jail_root")?;
                    out.jail_root = Some(
                        project_jail_dir(&root, s)
                            .map_err(|m| ConfigError::invalid(path, "notebook.jail_root", m))?,
                    );
                }
                other => out.unknown_keys.push(format!("notebook.{other}")),
            }
        }
    }
    out.unknown_keys.sort();
    out.unknown_keys.dedup();
    Ok(out)
}

/// `rel` joined to `root`, or `None` when it is absolute or climbs out.
/// An existing directory is also canonicalised, so a symlink out is refused.
fn project_relative_dir(root: &Path, rel: &str) -> Option<PathBuf> {
    use std::path::Component;
    let rel_path = Path::new(rel);
    let mut depth: i64 = 0;
    let mut joined = root.to_path_buf();
    for c in rel_path.components() {
        match c {
            Component::Normal(part) => {
                depth += 1;
                joined.push(part);
            }
            Component::CurDir => {}
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                joined.pop();
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if let (Ok(canon), Ok(canon_root)) =
        (std::fs::canonicalize(&joined), std::fs::canonicalize(root))
    {
        if !canon.starts_with(&canon_root) {
            return None;
        }
        return Some(canon);
    }
    Some(joined)
}

/// Resolve a `[notebook] jail_root` value against the directory holding
/// `rustlab.toml`. The value must be relative and must name an existing
/// directory whose canonical path is that directory or below it, so `..`,
/// an absolute path, and a symlink that leads out are all refused. Returns
/// the canonical path. A project file can only narrow to its own tree;
/// widening past it takes the `--jail-root` flag.
pub fn project_jail_dir(root: &Path, rel: &str) -> Result<PathBuf, String> {
    use std::path::Component;
    let canon_root = std::fs::canonicalize(root).map_err(|e| {
        format!(
            "cannot resolve the project directory {}: {e}",
            root.display()
        )
    })?;
    if rel.is_empty() {
        return Err(format!(
            "expected a directory relative to {} (\".\" is the project directory)",
            canon_root.display()
        ));
    }
    let rel_path = Path::new(rel);
    if rel_path.has_root() || rel_path.is_absolute() {
        return Err(format!(
            "\"{rel}\" must be relative to the project directory {}",
            canon_root.display()
        ));
    }
    let escapes = || {
        format!(
            "\"{rel}\" escapes the project directory {}",
            canon_root.display()
        )
    };
    let mut depth = 0usize;
    for c in rel_path.components() {
        match c {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir => depth = depth.checked_sub(1).ok_or_else(escapes)?,
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "\"{rel}\" must be relative to the project directory {}",
                    canon_root.display()
                ))
            }
        }
    }
    // Joined as written so the OS resolves `link/..` the way a later
    // open would; the canonical prefix check below is the authority.
    let canon = std::fs::canonicalize(canon_root.join(rel_path)).map_err(|e| {
        format!(
            "\"{rel}\" is not a directory under {}: {e}",
            canon_root.display()
        )
    })?;
    if !canon.starts_with(&canon_root) {
        return Err(escapes());
    }
    if !canon.is_dir() {
        return Err(format!(
            "\"{rel}\" is not a directory ({})",
            canon.display()
        ));
    }
    Ok(canon)
}

/// Locations consulted by [`load`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    pub xdg_config_file: PathBuf,
    pub rustlabrc: PathBuf,
}

impl ConfigPaths {
    /// Build paths from `HOME` and `XDG_CONFIG_HOME`.
    ///
    /// `XDG_CONFIG_HOME` defaults to `$HOME/.config` when unset or empty.
    /// If `HOME` is also unset, `~` is treated as the current directory so
    /// a missing file still yields defaults rather than an error.
    pub fn from_env() -> Self {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let xdg = env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        Self::from_dirs(home, xdg)
    }

    /// Build paths from an explicit home directory and optional XDG
    /// config home. When `xdg_config_home` is `None`, uses `$home/.config`.
    pub fn from_dirs(home: impl AsRef<Path>, xdg_config_home: Option<impl AsRef<Path>>) -> Self {
        let home = home.as_ref();
        let xdg = xdg_config_home
            .map(|p| p.as_ref().to_path_buf())
            .unwrap_or_else(|| home.join(".config"));
        Self {
            xdg_config_file: xdg.join("rustlab").join("config.toml"),
            rustlabrc: home.join(".rustlabrc"),
        }
    }
}

/// Load user settings from the process environment paths.
pub fn load() -> Result<LoadedConfig, ConfigError> {
    load_from_paths(&ConfigPaths::from_env())
}

/// Load user settings from explicit paths (tests inject temp dirs here).
///
/// XDG wins when that file exists; otherwise `~/.rustlabrc`; otherwise
/// built-in defaults. A missing file is never an error.
pub fn load_from_paths(paths: &ConfigPaths) -> Result<LoadedConfig, ConfigError> {
    if paths.xdg_config_file.is_file() {
        return parse_file(
            &paths.xdg_config_file,
            ConfigSource::Xdg(paths.xdg_config_file.clone()),
        );
    }
    if paths.rustlabrc.is_file() {
        return parse_file(
            &paths.rustlabrc,
            ConfigSource::Rustlabrc(paths.rustlabrc.clone()),
        );
    }
    Ok(LoadedConfig {
        settings: UserSettings::default(),
        source: ConfigSource::Defaults,
        unknown_keys: Vec::new(),
        warnings: Vec::new(),
    })
}

fn parse_file(path: &Path, source: ConfigSource) -> Result<LoadedConfig, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source_err| ConfigError::Io {
        path: path.to_path_buf(),
        source: source_err,
    })?;
    parse_toml(&text, path, source)
}

/// Parse a TOML body. Public so tests can feed strings without touching disk.
pub fn parse_toml(
    text: &str,
    path: &Path,
    source: ConfigSource,
) -> Result<LoadedConfig, ConfigError> {
    let value: TomlValue = text
        .parse()
        .map_err(|e: toml::de::Error| ConfigError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
    let table = match value {
        TomlValue::Table(t) => t,
        other => {
            return Err(ConfigError::Parse {
                path: path.to_path_buf(),
                message: format!(
                    "expected a TOML table at the top level, got {}",
                    type_name(&other)
                ),
            });
        }
    };

    let mut settings = UserSettings::default();
    let mut unknown = Vec::new();
    let mut warnings = Vec::new();

    for (key, val) in table {
        match key.as_str() {
            "display" => parse_display_section(&val, path, &mut settings, &mut unknown)?,
            "plot" => parse_plot_section(&val, path, &mut settings, &mut unknown)?,
            "viewer" => parse_viewer_section(&val, path, &mut settings, &mut unknown)?,
            "notebook" => {
                parse_notebook_section(&val, path, &mut settings, &mut unknown, &mut warnings)?
            }
            "repl" => parse_repl_section(&val, path, &mut settings, &mut unknown)?,
            "cache" => parse_cache_section(&val, path, &mut settings, &mut unknown)?,
            other => unknown.push(other.to_string()),
        }
    }

    unknown.sort();
    unknown.dedup();

    Ok(LoadedConfig {
        settings,
        source,
        unknown_keys: unknown,
        warnings,
    })
}

fn expect_table<'a>(
    val: &'a TomlValue,
    path: &Path,
    section: &str,
) -> Result<&'a toml::map::Map<String, TomlValue>, ConfigError> {
    match val {
        TomlValue::Table(t) => Ok(t),
        other => Err(ConfigError::invalid(
            path,
            section,
            format!("expected a table, got {}", type_name(other)),
        )),
    }
}

fn parse_display_section(
    val: &TomlValue,
    path: &Path,
    settings: &mut UserSettings,
    unknown: &mut Vec<String>,
) -> Result<(), ConfigError> {
    let table = expect_table(val, path, "display")?;
    for (key, v) in table {
        match key.as_str() {
            "format" => {
                let s = expect_string(v, path, "display.format")?;
                settings.display.format = Some(DisplayFormat::parse(s).ok_or_else(|| {
                    ConfigError::invalid(
                        path,
                        "display.format",
                        format!(
                            "expected \"short\", \"long\", \"hex\", or \"commas\"; got \"{s}\""
                        ),
                    )
                })?);
            }
            other => unknown.push(format!("display.{other}")),
        }
    }
    Ok(())
}

fn parse_plot_section(
    val: &TomlValue,
    path: &Path,
    settings: &mut UserSettings,
    unknown: &mut Vec<String>,
) -> Result<(), ConfigError> {
    let table = expect_table(val, path, "plot")?;
    for (key, v) in table {
        match key.as_str() {
            "theme" => {
                let s = expect_string(v, path, "plot.theme")?;
                settings.plot.theme = Some(ColorTheme::parse(s).ok_or_else(|| {
                    ConfigError::invalid(
                        path,
                        "plot.theme",
                        format!("expected one of {}; got \"{s}\"", ColorTheme::NAMES),
                    )
                })?);
            }
            "default_axis" => {
                let s = expect_string(v, path, "plot.default_axis")?;
                settings.plot.default_axis = Some(DefaultAxis::parse(s).ok_or_else(|| {
                    ConfigError::invalid(
                        path,
                        "plot.default_axis",
                        format!("expected \"ij\" or \"xy\"; got \"{s}\""),
                    )
                })?);
            }
            other => unknown.push(format!("plot.{other}")),
        }
    }
    Ok(())
}

fn parse_viewer_section(
    val: &TomlValue,
    path: &Path,
    settings: &mut UserSettings,
    unknown: &mut Vec<String>,
) -> Result<(), ConfigError> {
    let table = expect_table(val, path, "viewer")?;
    for (key, v) in table {
        match key.as_str() {
            "auto_connect" => {
                settings.viewer.auto_connect = Some(expect_bool(v, path, "viewer.auto_connect")?);
            }
            "name" => {
                let s = expect_string(v, path, "viewer.name")?;
                if s.is_empty() {
                    return Err(ConfigError::invalid(
                        path,
                        "viewer.name",
                        "expected a non-empty session name",
                    ));
                }
                settings.viewer.name = Some(s.to_string());
            }
            other => unknown.push(format!("viewer.{other}")),
        }
    }
    Ok(())
}

/// `true` for a name a theme file may have: lowercase ASCII letters,
/// digits, `-`, `_`, starting with a letter or digit, at most 64 bytes.
/// Names are looked up as `<dir>/<name>.toml`, so this also keeps a name
/// from being a path.
pub fn is_theme_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-' || *c == b'_')
}

/// A builtin (any case or alias, stored canonical) or a theme-file name.
fn parse_theme_name(s: &str, path: &Path, key: &str) -> Result<String, ConfigError> {
    if let Some(t) = ColorTheme::parse(s) {
        return Ok(t.as_str().to_string());
    }
    let name = s.trim().to_ascii_lowercase();
    if is_theme_name(&name) {
        Ok(name)
    } else {
        Err(ConfigError::invalid(
            path,
            key,
            format!(
                "expected a theme name (a builtin: {}, or a theme file name of lowercase letters, digits, - and _); got \"{s}\"",
                ColorTheme::NAMES
            ),
        ))
    }
}

fn parse_notebook_section(
    val: &TomlValue,
    path: &Path,
    settings: &mut UserSettings,
    unknown: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<(), ConfigError> {
    let table = expect_table(val, path, "notebook")?;
    for (key, v) in table {
        match key.as_str() {
            "theme" => {
                let s = expect_string(v, path, "notebook.theme")?;
                settings.notebook.theme = Some(parse_theme_name(s, path, "notebook.theme")?);
            }
            // Soft-fail: a bad fold value must not abort startup. The
            // built-in default (open) applies when `code` stays unset.
            "code" => match v {
                TomlValue::String(s) => match CodeFold::parse(s) {
                    Some(fold) => settings.notebook.code = Some(fold),
                    None => warnings.push(format!(
                        "notebook.code: expected \"open\" or \"collapsed\"; got \"{s}\" (using open)"
                    )),
                },
                other => warnings.push(format!(
                    "notebook.code: expected a string, got {} (using open)",
                    type_name(other)
                )),
            },
            "comments" => {
                let s = expect_string(v, path, "notebook.comments")?;
                settings.notebook.comments = Some(match s {
                    "on" => true,
                    "off" => false,
                    _ => {
                        return Err(ConfigError::invalid(
                            path,
                            "notebook.comments",
                            format!("expected \"on\" or \"off\"; got \"{s}\""),
                        ));
                    }
                });
            }
            "git_diff" => {
                settings.notebook.git_diff = Some(expect_bool(v, path, "notebook.git_diff")?);
            }
            other => unknown.push(format!("notebook.{other}")),
        }
    }
    Ok(())
}

fn parse_repl_section(
    val: &TomlValue,
    path: &Path,
    settings: &mut UserSettings,
    unknown: &mut Vec<String>,
) -> Result<(), ConfigError> {
    let table = expect_table(val, path, "repl")?;
    for (key, v) in table {
        match key.as_str() {
            "history_limit" => {
                let n = expect_usize(v, path, "repl.history_limit")?;
                if n == 0 {
                    return Err(ConfigError::invalid(
                        path,
                        "repl.history_limit",
                        "expected a positive integer",
                    ));
                }
                settings.repl.history_limit = Some(n);
            }
            "history" => {
                settings.repl.history = Some(expect_bool(v, path, "repl.history")?);
            }
            other => unknown.push(format!("repl.{other}")),
        }
    }
    Ok(())
}

fn parse_cache_section(
    val: &TomlValue,
    path: &Path,
    settings: &mut UserSettings,
    unknown: &mut Vec<String>,
) -> Result<(), ConfigError> {
    let table = expect_table(val, path, "cache")?;
    for (key, v) in table {
        match key.as_str() {
            "enabled" => {
                settings.cache.enabled = Some(expect_bool(v, path, "cache.enabled")?);
            }
            other => unknown.push(format!("cache.{other}")),
        }
    }
    Ok(())
}

fn expect_string<'a>(val: &'a TomlValue, path: &Path, key: &str) -> Result<&'a str, ConfigError> {
    match val {
        TomlValue::String(s) => Ok(s),
        other => Err(ConfigError::invalid(
            path,
            key,
            format!("expected a string, got {}", type_name(other)),
        )),
    }
}

fn expect_bool(val: &TomlValue, path: &Path, key: &str) -> Result<bool, ConfigError> {
    match val {
        TomlValue::Boolean(b) => Ok(*b),
        other => Err(ConfigError::invalid(
            path,
            key,
            format!("expected a boolean, got {}", type_name(other)),
        )),
    }
}

fn expect_usize(val: &TomlValue, path: &Path, key: &str) -> Result<usize, ConfigError> {
    match val {
        TomlValue::Integer(n) if *n >= 0 => usize::try_from(*n).map_err(|_| {
            ConfigError::invalid(path, key, format!("integer {n} does not fit in usize"))
        }),
        TomlValue::Integer(n) => Err(ConfigError::invalid(
            path,
            key,
            format!("expected a non-negative integer, got {n}"),
        )),
        other => Err(ConfigError::invalid(
            path,
            key,
            format!("expected an integer, got {}", type_name(other)),
        )),
    }
}

fn type_name(val: &TomlValue) -> &'static str {
    match val {
        TomlValue::String(_) => "string",
        TomlValue::Integer(_) => "integer",
        TomlValue::Float(_) => "float",
        TomlValue::Boolean(_) => "boolean",
        TomlValue::Datetime(_) => "datetime",
        TomlValue::Array(_) => "array",
        TomlValue::Table(_) => "table",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn paths_in(dir: &Path) -> ConfigPaths {
        ConfigPaths {
            xdg_config_file: dir.join("xdg").join("rustlab").join("config.toml"),
            rustlabrc: dir.join("home").join(".rustlabrc"),
        }
    }

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn missing_file_is_defaults() {
        let dir = TempDir::new().unwrap();
        let loaded = load_from_paths(&paths_in(dir.path())).unwrap();
        assert_eq!(loaded.source, ConfigSource::Defaults);
        assert!(loaded.unknown_keys.is_empty());
        assert_eq!(loaded.settings, UserSettings::default());
        assert_eq!(loaded.settings.display_format(), DisplayFormat::Short);
        assert_eq!(loaded.settings.plot_theme(), ColorTheme::Mocha);
        assert_eq!(loaded.settings.default_axis(), DefaultAxis::Ij);
        assert_eq!(loaded.settings.notebook_theme(), "mocha");
        assert_eq!(loaded.settings.history_limit(), DEFAULT_HISTORY_LIMIT);
        assert!(!loaded.settings.viewer_auto_connect());
    }

    #[test]
    fn rustlabrc_used_when_no_xdg() {
        let dir = TempDir::new().unwrap();
        let paths = paths_in(dir.path());
        write(
            &paths.rustlabrc,
            r#"
            [display]
            format = "commas"
            "#,
        );
        let loaded = load_from_paths(&paths).unwrap();
        assert!(matches!(loaded.source, ConfigSource::Rustlabrc(_)));
        assert_eq!(loaded.source.path(), Some(paths.rustlabrc.as_path()));
        assert_eq!(loaded.settings.display.format, Some(DisplayFormat::Commas));
    }

    #[test]
    fn xdg_wins_over_rustlabrc() {
        let dir = TempDir::new().unwrap();
        let paths = paths_in(dir.path());
        write(
            &paths.rustlabrc,
            r#"
            [display]
            format = "commas"
            "#,
        );
        write(
            &paths.xdg_config_file,
            r#"
            [display]
            format = "long"
            "#,
        );
        let loaded = load_from_paths(&paths).unwrap();
        assert!(matches!(loaded.source, ConfigSource::Xdg(_)));
        assert_eq!(loaded.settings.display.format, Some(DisplayFormat::Long));
    }

    #[test]
    fn unknown_keys_warn_do_not_fail() {
        let dir = TempDir::new().unwrap();
        let paths = paths_in(dir.path());
        write(
            &paths.rustlabrc,
            r#"
            extra = 1
            [display]
            format = "short"
            mystery = true
            [plot]
            theme = "dark"
            flavour = "mocha"
            "#,
        );
        let loaded = load_from_paths(&paths).unwrap();
        assert_eq!(
            loaded.unknown_keys,
            vec![
                "display.mystery".to_string(),
                "extra".to_string(),
                "plot.flavour".to_string(),
            ]
        );
        assert_eq!(loaded.settings.display.format, Some(DisplayFormat::Short));
        assert_eq!(loaded.settings.plot.theme, Some(ColorTheme::Mocha));
    }

    #[test]
    fn invalid_format_errors_with_path_and_key() {
        let dir = TempDir::new().unwrap();
        let paths = paths_in(dir.path());
        write(
            &paths.rustlabrc,
            r#"
            [display]
            format = "banana"
            "#,
        );
        let err = load_from_paths(&paths).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains(paths.rustlabrc.to_string_lossy().as_ref()),
            "error should name the file: {msg}"
        );
        assert!(
            msg.contains("display.format"),
            "error should name the key: {msg}"
        );
        assert!(
            msg.contains("banana"),
            "error should quote the bad value: {msg}"
        );
    }

    #[test]
    fn invalid_theme_errors() {
        let err = parse_toml(
            "[plot]\ntheme = \"sepia\"\n",
            Path::new("/tmp/rc"),
            ConfigSource::Defaults,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("/tmp/rc"));
        assert!(msg.contains("plot.theme"));
        assert!(msg.contains("sepia"));
    }

    #[test]
    fn invalid_axis_errors() {
        let err = parse_toml(
            "[plot]\ndefault_axis = \"up\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap_err();
        assert!(err.to_string().contains("plot.default_axis"));
    }

    #[test]
    fn wrong_type_errors() {
        let err = parse_toml(
            "[display]\nformat = 3\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("display.format"));
        assert!(msg.contains("string"));
    }

    #[test]
    fn repl_history_flag_parses_and_rejects_a_string() {
        let off = parse_toml(
            "[repl]\nhistory = false\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert_eq!(off.settings.repl.history, Some(false));
        assert!(!off.settings.repl_history_enabled());
        let on = parse_toml(
            "[repl]\nhistory = true\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert!(on.settings.repl_history_enabled());
        let err = parse_toml(
            "[repl]\nhistory = \"no\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap_err();
        assert!(err.to_string().contains("repl.history"));
    }

    #[test]
    fn history_limit_zero_is_invalid() {
        let err = parse_toml(
            "[repl]\nhistory_limit = 0\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap_err();
        assert!(err.to_string().contains("repl.history_limit"));
    }

    #[test]
    fn full_schema_round_trip() {
        let text = r#"
            [display]
            format = "hex"

            [plot]
            theme = "light"
            default_axis = "xy"

            [viewer]
            auto_connect = true
            name = "work"

            [notebook]
            theme = "dark"

            [repl]
            history_limit = 250
        "#;
        let loaded = parse_toml(text, Path::new("example"), ConfigSource::Defaults).unwrap();
        assert!(loaded.unknown_keys.is_empty());
        assert_eq!(loaded.settings.display.format, Some(DisplayFormat::Hex));
        assert_eq!(loaded.settings.plot.theme, Some(ColorTheme::Latte));
        assert_eq!(loaded.settings.plot.default_axis, Some(DefaultAxis::Xy));
        assert_eq!(loaded.settings.viewer.auto_connect, Some(true));
        assert_eq!(loaded.settings.viewer.name.as_deref(), Some("work"));
        assert_eq!(loaded.settings.notebook.theme.as_deref(), Some("mocha"));
        assert_eq!(loaded.settings.repl.history_limit, Some(250));
        // notebook.theme wins over plot.theme
        assert_eq!(loaded.settings.notebook_theme(), "mocha");
        assert_eq!(loaded.settings.history_limit(), 250);
        assert!(loaded.settings.notebook_code_open());
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn notebook_code_collapsed_and_open() {
        let collapsed = parse_toml(
            "[notebook]\ncode = \"collapsed\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert_eq!(collapsed.settings.notebook.code, Some(CodeFold::Collapsed));
        assert!(!collapsed.settings.notebook_code_open());
        assert!(collapsed.warnings.is_empty());

        let open = parse_toml(
            "[notebook]\ncode = \"OPEN\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert_eq!(open.settings.notebook.code, Some(CodeFold::Open));
        assert!(open.settings.notebook_code_open());
    }

    #[test]
    fn notebook_comments_on_off_and_invalid() {
        let on = parse_toml(
            "[notebook]\ncomments = \"on\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert_eq!(on.settings.notebook_comments(), Some(true));
        let off = parse_toml(
            "[notebook]\ncomments = \"off\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert_eq!(off.settings.notebook_comments(), Some(false));
        let bad = parse_toml(
            "[notebook]\ncomments = \"maybe\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        );
        assert!(bad.is_err());
    }

    #[test]
    fn cache_enabled_defaults_off_and_parses() {
        let path = Path::new("rc");
        let missing = parse_toml("", path, ConfigSource::Defaults).unwrap();
        assert!(missing.settings.cache.enabled.is_none());
        assert!(!missing.settings.cache_enabled());

        let on = parse_toml("[cache]\nenabled = true\n", path, ConfigSource::Defaults).unwrap();
        assert_eq!(on.settings.cache.enabled, Some(true));
        assert!(on.settings.cache_enabled());
        assert!(on.unknown_keys.is_empty());

        let off = parse_toml("[cache]\nenabled = false\n", path, ConfigSource::Defaults).unwrap();
        assert_eq!(off.settings.cache.enabled, Some(false));
        assert!(!off.settings.cache_enabled());

        let err =
            parse_toml("[cache]\nenabled = \"yes\"\n", path, ConfigSource::Defaults).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("cache.enabled"), "{msg}");
    }

    #[test]
    fn notebook_git_diff_defaults_off() {
        let off = parse_toml("", Path::new("rc"), ConfigSource::Defaults).unwrap();
        assert!(!off.settings.notebook_git_diff());
        let on = parse_toml(
            "[notebook]\ngit_diff = true\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert!(on.settings.notebook_git_diff());
        let bad = parse_toml(
            "[notebook]\ngit_diff = \"yes\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        );
        assert!(bad.is_err());
    }

    #[test]
    fn notebook_code_invalid_warns_and_stays_open() {
        let loaded = parse_toml(
            "[notebook]\ncode = \"folded\"\ntheme = \"dark\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert!(loaded.settings.notebook.code.is_none());
        assert!(loaded.settings.notebook_code_open());
        assert_eq!(loaded.settings.notebook.theme.as_deref(), Some("mocha"));
        assert!(loaded.unknown_keys.is_empty());
        assert_eq!(loaded.warnings.len(), 1);
        assert!(loaded.warnings[0].contains("folded"));
        assert!(loaded.warnings[0].contains("using open"));

        let bad_type = parse_toml(
            "[notebook]\ncode = 1\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert!(bad_type.settings.notebook_code_open());
        assert_eq!(bad_type.warnings.len(), 1);
        assert!(bad_type.warnings[0].contains("expected a string"));
    }

    #[test]
    fn theme_keys_accept_four_names_and_aliases() {
        for (src, want) in [
            ("mocha", ColorTheme::Mocha),
            ("DARK", ColorTheme::Mocha),
            ("macchiato", ColorTheme::Macchiato),
            ("frappe", ColorTheme::Frappe),
            ("latte", ColorTheme::Latte),
            ("light", ColorTheme::Latte),
        ] {
            let loaded = parse_toml(
                &format!("[plot]\ntheme = \"{src}\"\n[notebook]\ntheme = \"{src}\"\n"),
                Path::new("rc"),
                ConfigSource::Defaults,
            )
            .unwrap();
            assert_eq!(loaded.settings.plot_theme(), want, "{src}");
            assert_eq!(loaded.settings.notebook_theme(), want.as_str(), "{src}");
        }
        // as_str is canonical, never the alias
        assert_eq!(ColorTheme::parse("dark").unwrap().as_str(), "mocha");
        assert_eq!(ColorTheme::parse("light").unwrap().as_str(), "latte");
        // [plot] theme stays the four builtins.
        let err = parse_toml(
            "[plot]\ntheme = \"solarized\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("macchiato"), "{err}");
    }

    #[test]
    fn notebook_theme_falls_back_to_plot_theme() {
        let loaded = parse_toml(
            "[plot]\ntheme = \"light\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert_eq!(loaded.settings.notebook_theme(), "latte");
        assert_eq!(loaded.settings.plot_theme(), ColorTheme::Latte);
    }

    #[test]
    fn format_default_alias_and_case() {
        let loaded = parse_toml(
            "[display]\nformat = \"DEFAULT\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert_eq!(loaded.settings.display.format, Some(DisplayFormat::Short));
    }

    #[test]
    fn empty_file_is_defaults() {
        let loaded = parse_toml("", Path::new("rc"), ConfigSource::Defaults).unwrap();
        assert_eq!(loaded.settings, UserSettings::default());
    }

    #[test]
    fn comments_only_is_defaults() {
        let loaded = parse_toml(
            "# just a comment\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert_eq!(loaded.settings, UserSettings::default());
    }

    #[test]
    fn toml_syntax_error_names_path() {
        let err = parse_toml("[[[", Path::new("/bad/rc"), ConfigSource::Defaults).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("/bad/rc"));
        assert!(msg.contains("TOML parse error"));
    }

    #[test]
    fn from_dirs_joins_xdg_and_home() {
        let with_xdg = ConfigPaths::from_dirs("/home/me", Some("/xdg"));
        assert_eq!(
            with_xdg.xdg_config_file,
            PathBuf::from("/xdg/rustlab/config.toml")
        );
        assert_eq!(with_xdg.rustlabrc, PathBuf::from("/home/me/.rustlabrc"));

        let no_xdg: ConfigPaths = ConfigPaths::from_dirs("/home/me", None::<&Path>);
        assert_eq!(
            no_xdg.xdg_config_file,
            PathBuf::from("/home/me/.config/rustlab/config.toml")
        );
    }

    #[test]
    fn notebook_theme_accepts_any_theme_file_name() {
        let loaded = parse_toml(
            "[notebook]\ntheme = \"Course-Dark_2\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert_eq!(loaded.settings.notebook_theme(), "course-dark_2");
        for bad in ["../evil", "a/b", "x.toml", "", "-lead", "red;}"] {
            let err = parse_toml(
                &format!("[notebook]\ntheme = \"{bad}\"\n"),
                Path::new("rc"),
                ConfigSource::Defaults,
            )
            .unwrap_err()
            .to_string();
            assert!(err.contains("notebook.theme"), "{bad}: {err}");
        }
        assert!(is_theme_name("course"));
        assert!(!is_theme_name("Course"));
        assert!(!is_theme_name(&"a".repeat(65)));
    }

    fn tempdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rl-config-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }

    #[test]
    fn project_config_is_the_nearest_ancestor() {
        let root = tempdir("ancestor");
        let outer = root.join("course");
        let inner = outer.join("unit1/notebooks");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(
            outer.join(PROJECT_CONFIG),
            "[notebook]\ntheme = \"outer\"\n",
        )
        .unwrap();
        assert_eq!(
            find_project_config(&inner, None),
            Some(outer.join(PROJECT_CONFIG))
        );
        // A nearer file wins.
        std::fs::write(
            outer.join("unit1").join(PROJECT_CONFIG),
            "[notebook]\ntheme = \"inner\"\n",
        )
        .unwrap();
        let p = load_project(&inner, None).unwrap().unwrap();
        assert_eq!(p.notebook_theme.as_deref(), Some("inner"));
        assert_eq!(p.root, outer.join("unit1"));
        // None found below the home stop.
        let lonely = root.join("elsewhere/deep");
        std::fs::create_dir_all(&lonely).unwrap();
        assert_eq!(
            find_project_config(&lonely, Some(&root.join("elsewhere"))),
            None
        );
        // The walk does not climb above home even when an ancestor has one.
        std::fs::write(root.join(PROJECT_CONFIG), "").unwrap();
        assert_eq!(
            find_project_config(&lonely, Some(&root.join("elsewhere"))),
            None
        );
        assert_eq!(
            find_project_config(&lonely, None),
            Some(root.join(PROJECT_CONFIG))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_config_keys_and_theme_dir() {
        let root = tempdir("keys");
        std::fs::create_dir_all(root.join("styles/themes")).unwrap();
        let path = root.join(PROJECT_CONFIG);
        let p = parse_project_toml(
            "[notebook]\ntheme = \"Latte\"\ntheme_dir = \"styles/themes\"\ncode = \"open\"\n[plot]\ntheme = \"dark\"\n",
            &path,
        )
        .unwrap();
        assert_eq!(p.notebook_theme.as_deref(), Some("latte"));
        assert_eq!(p.theme_dir, Some(root.join("styles/themes")));
        assert_eq!(
            p.unknown_keys,
            vec!["notebook.code".to_string(), "plot".to_string()]
        );
        for escape in ["..", "../x", "a/../../x", "/etc"] {
            let err = parse_project_toml(&format!("[notebook]\ntheme_dir = \"{escape}\"\n"), &path)
                .unwrap_err()
                .to_string();
            assert!(err.contains("leaves the project root"), "{escape}: {err}");
        }
        #[cfg(unix)]
        {
            let outside = tempdir("outside");
            std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
            let err = parse_project_toml("[notebook]\ntheme_dir = \"link\"\n", &path)
                .unwrap_err()
                .to_string();
            assert!(err.contains("leaves the project root"), "{err}");
            let _ = std::fs::remove_dir_all(&outside);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_config_walk_stops_at_the_git_root() {
        let root = tempdir("git-stop");
        let repo = root.join("repo");
        let inner = repo.join("lessons/unit1");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(root.join(PROJECT_CONFIG), "").unwrap();
        // No .git anywhere: the walk reaches the outer file as before.
        assert_eq!(
            find_project_config(&inner, None),
            Some(root.join(PROJECT_CONFIG))
        );
        // A .git directory marks the repository root: nothing above it.
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        assert_eq!(find_project_config(&inner, None), None);
        assert!(load_project(&inner, None).unwrap().is_none());
        // The .git root itself is still checked.
        std::fs::write(repo.join(PROJECT_CONFIG), "").unwrap();
        assert_eq!(
            find_project_config(&inner, None),
            Some(repo.join(PROJECT_CONFIG))
        );
        // Starting at the .git root finds its own file.
        assert_eq!(
            find_project_config(&repo, None),
            Some(repo.join(PROJECT_CONFIG))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_config_walk_stops_at_a_git_file() {
        // Worktrees and submodules have a `.git` file, not a directory.
        let root = tempdir("git-file");
        let sub = root.join("sub");
        let inner = sub.join("notes");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(root.join(PROJECT_CONFIG), "").unwrap();
        std::fs::write(sub.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();
        assert_eq!(find_project_config(&inner, None), None);
        std::fs::write(sub.join(PROJECT_CONFIG), "").unwrap();
        assert_eq!(
            find_project_config(&inner, None),
            Some(sub.join(PROJECT_CONFIG))
        );
        // A nearer .git wins over an outer one.
        std::fs::create_dir_all(root.join(".git")).unwrap();
        assert_eq!(
            find_project_config(&inner, None),
            Some(sub.join(PROJECT_CONFIG))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn jail_err(root: &Path, value: &str) -> String {
        parse_project_toml(
            &format!("[notebook]\njail_root = \"{value}\"\n"),
            &root.join(PROJECT_CONFIG),
        )
        .unwrap_err()
        .to_string()
    }

    #[test]
    fn project_jail_root_resolves_inside_the_toml_directory() {
        let root = tempdir("jail-ok");
        std::fs::create_dir_all(root.join("lib/deep")).unwrap();
        let path = root.join(PROJECT_CONFIG);
        let p = parse_project_toml("[notebook]\njail_root = \".\"\n", &path).unwrap();
        assert_eq!(p.jail_root, Some(root.clone()));
        assert!(p.unknown_keys.is_empty());
        let p = parse_project_toml("[notebook]\njail_root = \"lib/./deep/..\"\n", &path).unwrap();
        assert_eq!(p.jail_root, Some(root.join("lib")));
        let p = parse_project_toml("[notebook]\ntheme = \"latte\"\n", &path).unwrap();
        assert_eq!(p.jail_root, None);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("lib/deep"), root.join("inner")).unwrap();
            let p = parse_project_toml("[notebook]\njail_root = \"inner\"\n", &path).unwrap();
            assert_eq!(p.jail_root, Some(root.join("lib/deep")));
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_jail_root_refuses_to_escape() {
        let root = tempdir("jail-escape");
        std::fs::create_dir_all(root.join("lib")).unwrap();
        for escape in ["..", "../", "../x", "lib/../..", "lib/../../x"] {
            let err = jail_err(&root, escape);
            assert!(err.contains("notebook.jail_root"), "{escape}: {err}");
            assert!(err.contains("escapes"), "{escape}: {err}");
        }
        let abs_inside = root.join("lib").display().to_string();
        for abs in ["/", "/etc", abs_inside.as_str()] {
            let err = jail_err(&root, abs);
            assert!(err.contains("notebook.jail_root"), "{abs}: {err}");
            assert!(err.contains("must be relative"), "{abs}: {err}");
        }
        let err = jail_err(&root, "");
        assert!(err.contains("notebook.jail_root"), "{err}");
        let err = jail_err(&root, "missing");
        assert!(err.contains("not a directory"), "{err}");
        std::fs::write(root.join("file.txt"), "x").unwrap();
        let err = jail_err(&root, "file.txt");
        assert!(err.contains("not a directory"), "{err}");
        let err = parse_project_toml("[notebook]\njail_root = 1\n", &root.join(PROJECT_CONFIG))
            .unwrap_err()
            .to_string();
        assert!(err.contains("expected a string"), "{err}");
        #[cfg(unix)]
        {
            let outside = tempdir("jail-outside");
            std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
            std::os::unix::fs::symlink(root.parent().unwrap(), root.join("lib/up")).unwrap();
            for via_link in ["link", "link/.", "lib/up"] {
                let err = jail_err(&root, via_link);
                assert!(err.contains("escapes"), "{via_link}: {err}");
            }
            let _ = std::fs::remove_dir_all(&outside);
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
