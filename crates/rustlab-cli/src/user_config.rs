//! Load `~/.rustlabrc` / XDG config and apply process-wide defaults.

use anyhow::{Context, Result};
use rustlab_config::{
    ColorTheme, ConfigSource, DefaultAxis, DisplayFormat, LoadedConfig, UserSettings,
};
use rustlab_plot::{set_default_axis_y_direction, set_default_theme, AxisYDirection, Theme};
use rustlab_script::{set_default_number_format, NumberFormat};

/// Load the user rc, warn about unknown keys, apply process defaults.
///
/// A missing file is fine (built-in defaults). Invalid values abort, except
/// `[notebook] code`, which warns once and falls back to open.
pub fn load_and_apply() -> Result<UserSettings> {
    let loaded = rustlab_config::load().context("failed to load rustlab user settings")?;
    warn_unknown(&loaded);
    apply_process_defaults(&loaded.settings);
    Ok(loaded.settings)
}

pub fn apply_process_defaults(settings: &UserSettings) {
    set_default_number_format(match settings.display_format() {
        DisplayFormat::Short => NumberFormat::Short,
        DisplayFormat::Long => NumberFormat::Long,
        DisplayFormat::Hex => NumberFormat::Hex,
        DisplayFormat::Commas => NumberFormat::Commas,
    });
    set_default_axis_y_direction(match settings.default_axis() {
        DefaultAxis::Ij => AxisYDirection::Ij,
        DefaultAxis::Xy => AxisYDirection::Xy,
    });
    set_default_theme(match settings.plot_theme() {
        ColorTheme::Dark => Theme::Dark,
        ColorTheme::Light => Theme::Light,
    });
}

fn warn_unknown(loaded: &LoadedConfig) {
    if loaded.unknown_keys.is_empty() && loaded.warnings.is_empty() {
        return;
    }
    let where_ = match &loaded.source {
        ConfigSource::Xdg(p) | ConfigSource::Rustlabrc(p) => p.display().to_string(),
        ConfigSource::Defaults => "rustlab config".to_string(),
    };
    for key in &loaded.unknown_keys {
        eprintln!("warning: {where_}: unknown setting '{key}' (ignored)");
    }
    for warning in &loaded.warnings {
        eprintln!("warning: {where_}: {warning}");
    }
}
