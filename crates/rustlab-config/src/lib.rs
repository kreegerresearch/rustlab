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
//! Project-local config and `startup.rlab` are out of scope for v1.

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
    Dark,
    Light,
}

impl ColorTheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
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
    pub theme: Option<ColorTheme>,
    /// `[notebook] code`. Missing key → built-in default open.
    pub code: Option<CodeFold>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplSettings {
    /// rustyline history cap. Built-in default is 1000.
    pub history_limit: Option<usize>,
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
        self.plot.theme.unwrap_or(ColorTheme::Dark)
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

    /// Notebook page/plot theme: `[notebook] theme`, else `[plot] theme`,
    /// else dark.
    pub fn notebook_theme(&self) -> ColorTheme {
        self.notebook
            .theme
            .or(self.plot.theme)
            .unwrap_or(ColorTheme::Dark)
    }

    /// Effective REPL history cap.
    pub fn history_limit(&self) -> usize {
        self.repl.history_limit.unwrap_or(DEFAULT_HISTORY_LIMIT)
    }

    /// Effective viewer auto-connect flag.
    pub fn viewer_auto_connect(&self) -> bool {
        self.viewer.auto_connect.unwrap_or(false)
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
                        format!("expected \"dark\" or \"light\"; got \"{s}\""),
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
                settings.notebook.theme = Some(ColorTheme::parse(s).ok_or_else(|| {
                    ConfigError::invalid(
                        path,
                        "notebook.theme",
                        format!("expected \"dark\" or \"light\"; got \"{s}\""),
                    )
                })?);
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
            other => unknown.push(format!("repl.{other}")),
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
        assert_eq!(loaded.settings.plot_theme(), ColorTheme::Dark);
        assert_eq!(loaded.settings.default_axis(), DefaultAxis::Ij);
        assert_eq!(loaded.settings.notebook_theme(), ColorTheme::Dark);
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
        assert_eq!(loaded.settings.plot.theme, Some(ColorTheme::Dark));
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
        assert_eq!(loaded.settings.plot.theme, Some(ColorTheme::Light));
        assert_eq!(loaded.settings.plot.default_axis, Some(DefaultAxis::Xy));
        assert_eq!(loaded.settings.viewer.auto_connect, Some(true));
        assert_eq!(loaded.settings.viewer.name.as_deref(), Some("work"));
        assert_eq!(loaded.settings.notebook.theme, Some(ColorTheme::Dark));
        assert_eq!(loaded.settings.repl.history_limit, Some(250));
        // notebook.theme wins over plot.theme
        assert_eq!(loaded.settings.notebook_theme(), ColorTheme::Dark);
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
    fn notebook_code_invalid_warns_and_stays_open() {
        let loaded = parse_toml(
            "[notebook]\ncode = \"folded\"\ntheme = \"dark\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert!(loaded.settings.notebook.code.is_none());
        assert!(loaded.settings.notebook_code_open());
        assert_eq!(loaded.settings.notebook.theme, Some(ColorTheme::Dark));
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
    fn notebook_theme_falls_back_to_plot_theme() {
        let loaded = parse_toml(
            "[plot]\ntheme = \"light\"\n",
            Path::new("rc"),
            ConfigSource::Defaults,
        )
        .unwrap();
        assert_eq!(loaded.settings.notebook_theme(), ColorTheme::Light);
        assert_eq!(loaded.settings.plot_theme(), ColorTheme::Light);
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
}
