//! Interactive notebook widgets — declaration parsing and value coercion.
//!
//! A `rustlab-widget` fenced code block carries a small TOML body declaring
//! one control (Phase 1: `slider` only). The declaration is parsed here into
//! a [`WidgetDecl`]; the notebook executor seeds a value table from each
//! widget's declared default, and the `widget(name)` rlab builtin reads the
//! current value back. The interactive server overlays live values (slider
//! drags) and validates them through [`WidgetDecl::coerce`].
//!
//! See `dev/plans/notebook_interactive_widgets.md`. Phase 1 ships `slider`;
//! `option` and `number` are reserved for Phase 2 and currently parse-error
//! with a clear "not supported yet" message.

use rustlab_script::WidgetValue;
use serde::Deserialize;

/// A parsed widget declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetDecl {
    /// The name passed to `widget("name")` in code blocks. Unique per notebook.
    pub name: String,
    /// Human label shown next to the control. Falls back to `name` when absent.
    pub label: Option<String>,
    pub kind: WidgetKind,
}

/// The control type and its type-specific parameters.
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetKind {
    /// Continuous range control. `min`/`max` are required.
    Slider {
        min: f64,
        max: f64,
        step: Option<f64>,
        default: f64,
    },
    /// Numeric text input. `min`/`max` are optional bounds (unbounded when
    /// absent); when present, incoming values clamp to them.
    Number {
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
        default: f64,
    },
    /// One-of-N string choice (rendered as a radio / segmented group).
    Option {
        choices: Vec<String>,
        default: String,
    },
}

impl WidgetDecl {
    /// The declared default, used to seed the value table so `widget()`
    /// resolves before the user touches any control (and in batch
    /// `notebook render`, which never installs live values).
    pub fn default_value(&self) -> WidgetValue {
        match &self.kind {
            WidgetKind::Slider { default, .. } | WidgetKind::Number { default, .. } => {
                WidgetValue::Number(*default)
            }
            WidgetKind::Option { default, .. } => WidgetValue::Text(default.clone()),
        }
    }

    /// Validate a value arriving from the browser (or carried over from a
    /// prior render) against this declaration, returning the coerced value
    /// or `None` when it is incompatible. Numeric controls clamp to their
    /// bounds and reject non-finite / non-numeric input; `option` accepts
    /// only a declared choice. `None` tells the caller to fall back to the
    /// declared default — that's how a `.md` reload that changes a widget's
    /// type or range resets the live value (locked-in carry-over rule).
    pub fn coerce(&self, value: &WidgetValue) -> Option<WidgetValue> {
        match &self.kind {
            WidgetKind::Slider { min, max, .. } => {
                let n = value.as_number()?;
                if !n.is_finite() {
                    return None;
                }
                Some(WidgetValue::Number(n.clamp(*min, *max)))
            }
            WidgetKind::Number { min, max, .. } => {
                let mut n = value.as_number()?;
                if !n.is_finite() {
                    return None;
                }
                if let Some(lo) = min {
                    n = n.max(*lo);
                }
                if let Some(hi) = max {
                    n = n.min(*hi);
                }
                Some(WidgetValue::Number(n))
            }
            WidgetKind::Option { choices, .. } => {
                let s = value.as_text()?;
                if choices.iter().any(|c| c == &s) {
                    Some(WidgetValue::Text(s))
                } else {
                    None
                }
            }
        }
    }
}

/// Raw TOML shape; the union of every widget type's keys. Validated into a
/// typed [`WidgetDecl`] by [`parse_widget`].
#[derive(Debug, Deserialize)]
struct RawWidget {
    name: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
    /// Loosely typed so `type` dispatch happens before the per-type check:
    /// a slider's default is a number, but an `option`'s is a string. The
    /// numeric branches coerce it to f64; type validation reports the right
    /// error rather than a generic TOML type mismatch.
    default: Option<toml::Value>,
    choices: Option<Vec<String>>,
    label: Option<String>,
}

/// Coerce a TOML scalar (integer or float) to f64. Returns `None` for
/// strings, booleans, etc.
fn toml_to_f64(v: &toml::Value) -> Option<f64> {
    match v {
        toml::Value::Float(f) => Some(*f),
        toml::Value::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

/// Parse the TOML body of a `rustlab-widget` fence into a [`WidgetDecl`].
///
/// Returns a human-readable error (rendered as an inline `[!CAUTION]`
/// callout by the notebook renderer) on malformed TOML, a missing/unknown
/// `type`, missing required keys, or an inconsistent range.
pub fn parse_widget(toml_src: &str) -> Result<WidgetDecl, String> {
    let raw: RawWidget =
        toml::from_str(toml_src).map_err(|e| format!("invalid widget TOML: {e}"))?;

    let name = raw
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .ok_or("widget requires a non-empty `name`")?;

    let kind = raw
        .kind
        .as_deref()
        .ok_or_else(|| format!("widget '{name}' requires a `type` (e.g. type = \"slider\")"))?;

    match kind {
        "slider" => {
            let min = raw
                .min
                .ok_or_else(|| format!("slider '{name}' requires `min`"))?;
            let max = raw
                .max
                .ok_or_else(|| format!("slider '{name}' requires `max`"))?;
            let default = raw
                .default
                .as_ref()
                .and_then(toml_to_f64)
                .ok_or_else(|| format!("slider '{name}' requires a numeric `default`"))?;
            if !(min < max) {
                return Err(format!(
                    "slider '{name}': `min` ({min}) must be less than `max` ({max})"
                ));
            }
            if let Some(step) = raw.step {
                if !(step > 0.0) {
                    return Err(format!("slider '{name}': `step` ({step}) must be positive"));
                }
            }
            if default < min || default > max {
                return Err(format!(
                    "slider '{name}': `default` ({default}) must be within [{min}, {max}]"
                ));
            }
            Ok(WidgetDecl {
                name,
                label: raw.label,
                kind: WidgetKind::Slider {
                    min,
                    max,
                    step: raw.step,
                    default,
                },
            })
        }
        "number" => {
            let default = raw
                .default
                .as_ref()
                .and_then(toml_to_f64)
                .ok_or_else(|| format!("number '{name}' requires a numeric `default`"))?;
            if let (Some(lo), Some(hi)) = (raw.min, raw.max) {
                if !(lo <= hi) {
                    return Err(format!(
                        "number '{name}': `min` ({lo}) must not exceed `max` ({hi})"
                    ));
                }
            }
            if let Some(lo) = raw.min {
                if default < lo {
                    return Err(format!(
                        "number '{name}': `default` ({default}) is below `min` ({lo})"
                    ));
                }
            }
            if let Some(hi) = raw.max {
                if default > hi {
                    return Err(format!(
                        "number '{name}': `default` ({default}) is above `max` ({hi})"
                    ));
                }
            }
            Ok(WidgetDecl {
                name,
                label: raw.label,
                kind: WidgetKind::Number {
                    min: raw.min,
                    max: raw.max,
                    step: raw.step,
                    default,
                },
            })
        }
        "option" => {
            let choices = raw
                .choices
                .filter(|c| !c.is_empty())
                .ok_or_else(|| format!("option '{name}' requires a non-empty `choices` list"))?;
            let default = raw
                .default
                .as_ref()
                .and_then(|v| v.as_str().map(str::to_string))
                .ok_or_else(|| format!("option '{name}' requires a string `default`"))?;
            if !choices.contains(&default) {
                return Err(format!(
                    "option '{name}': `default` ({default:?}) must be one of `choices`"
                ));
            }
            Ok(WidgetDecl {
                name,
                label: raw.label,
                kind: WidgetKind::Option { choices, default },
            })
        }
        other => Err(format!(
            "widget '{name}': unknown type `{other}` (expected `slider`, `number`, or `option`)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_slider() {
        let decl = parse_widget(
            "name = \"cutoff\"\ntype = \"slider\"\nmin = 0.1\nmax = 10.0\nstep = 0.05\ndefault = 1.0\nlabel = \"Cutoff (Hz)\"",
        )
        .unwrap();
        assert_eq!(decl.name, "cutoff");
        assert_eq!(decl.label.as_deref(), Some("Cutoff (Hz)"));
        assert_eq!(
            decl.kind,
            WidgetKind::Slider {
                min: 0.1,
                max: 10.0,
                step: Some(0.05),
                default: 1.0
            }
        );
        assert_eq!(decl.default_value(), WidgetValue::Number(1.0));
    }

    #[test]
    fn slider_step_is_optional() {
        let decl =
            parse_widget("name = \"g\"\ntype = \"slider\"\nmin = 0\nmax = 5\ndefault = 2").unwrap();
        assert!(matches!(
            decl.kind,
            WidgetKind::Slider { step: None, .. }
        ));
    }

    #[test]
    fn missing_name_errors() {
        let err = parse_widget("type = \"slider\"\nmin = 0\nmax = 1\ndefault = 0").unwrap_err();
        assert!(err.contains("name"), "got: {err}");
    }

    #[test]
    fn missing_required_key_errors() {
        let err = parse_widget("name = \"a\"\ntype = \"slider\"\nmin = 0\ndefault = 0").unwrap_err();
        assert!(err.contains("`max`"), "got: {err}");
    }

    #[test]
    fn min_must_be_below_max() {
        let err =
            parse_widget("name = \"a\"\ntype = \"slider\"\nmin = 5\nmax = 1\ndefault = 2").unwrap_err();
        assert!(err.contains("less than"), "got: {err}");
    }

    #[test]
    fn default_out_of_range_errors() {
        let err =
            parse_widget("name = \"a\"\ntype = \"slider\"\nmin = 0\nmax = 1\ndefault = 9").unwrap_err();
        assert!(err.contains("within"), "got: {err}");
    }

    #[test]
    fn unknown_type_errors() {
        let err =
            parse_widget("name = \"w\"\ntype = \"dial\"\nmin = 0\nmax = 1\ndefault = 0").unwrap_err();
        assert!(err.contains("unknown type"), "got: {err}");
    }

    #[test]
    fn malformed_toml_errors() {
        let err = parse_widget("name = = =").unwrap_err();
        assert!(err.contains("invalid widget TOML"), "got: {err}");
    }

    #[test]
    fn slider_coerce_clamps_to_range() {
        let decl =
            parse_widget("name = \"a\"\ntype = \"slider\"\nmin = 0\nmax = 10\ndefault = 5").unwrap();
        assert_eq!(
            decl.coerce(&WidgetValue::Number(20.0)),
            Some(WidgetValue::Number(10.0))
        );
        assert_eq!(
            decl.coerce(&WidgetValue::Number(-3.0)),
            Some(WidgetValue::Number(0.0))
        );
        assert_eq!(
            decl.coerce(&WidgetValue::Number(4.0)),
            Some(WidgetValue::Number(4.0))
        );
        assert_eq!(decl.coerce(&WidgetValue::Number(f64::NAN)), None);
        // A string value for a numeric widget is incompatible.
        assert_eq!(decl.coerce(&WidgetValue::Text("x".into())), None);
    }

    #[test]
    fn parses_a_number_widget() {
        let decl = parse_widget(
            "name = \"order\"\ntype = \"number\"\nmin = 1\nmax = 64\ndefault = 8\nlabel = \"Order\"",
        )
        .unwrap();
        assert_eq!(
            decl.kind,
            WidgetKind::Number {
                min: Some(1.0),
                max: Some(64.0),
                step: None,
                default: 8.0
            }
        );
        assert_eq!(decl.default_value(), WidgetValue::Number(8.0));
    }

    #[test]
    fn number_bounds_are_optional() {
        let decl = parse_widget("name = \"n\"\ntype = \"number\"\ndefault = 3").unwrap();
        assert_eq!(
            decl.kind,
            WidgetKind::Number {
                min: None,
                max: None,
                step: None,
                default: 3.0
            }
        );
        // Unbounded → no clamping.
        assert_eq!(
            decl.coerce(&WidgetValue::Number(1e6)),
            Some(WidgetValue::Number(1e6))
        );
    }

    #[test]
    fn number_coerce_clamps_only_present_bounds() {
        let decl =
            parse_widget("name = \"n\"\ntype = \"number\"\nmin = 1\ndefault = 4").unwrap();
        assert_eq!(
            decl.coerce(&WidgetValue::Number(0.0)),
            Some(WidgetValue::Number(1.0))
        );
        assert_eq!(
            decl.coerce(&WidgetValue::Number(999.0)),
            Some(WidgetValue::Number(999.0))
        );
    }

    #[test]
    fn number_default_out_of_range_errors() {
        let err =
            parse_widget("name = \"n\"\ntype = \"number\"\nmin = 1\nmax = 10\ndefault = 50")
                .unwrap_err();
        assert!(err.contains("above"), "got: {err}");
    }

    #[test]
    fn parses_an_option_widget() {
        let decl = parse_widget(
            "name = \"window\"\ntype = \"option\"\nchoices = [\"hamming\", \"hann\", \"blackman\"]\ndefault = \"hann\"",
        )
        .unwrap();
        assert_eq!(
            decl.kind,
            WidgetKind::Option {
                choices: vec!["hamming".into(), "hann".into(), "blackman".into()],
                default: "hann".into()
            }
        );
        assert_eq!(decl.default_value(), WidgetValue::Text("hann".into()));
    }

    #[test]
    fn option_coerce_accepts_only_declared_choices() {
        let decl = parse_widget(
            "name = \"w\"\ntype = \"option\"\nchoices = [\"a\", \"b\"]\ndefault = \"a\"",
        )
        .unwrap();
        assert_eq!(
            decl.coerce(&WidgetValue::Text("b".into())),
            Some(WidgetValue::Text("b".into()))
        );
        assert_eq!(decl.coerce(&WidgetValue::Text("z".into())), None);
        // A numeric value for an option is incompatible.
        assert_eq!(decl.coerce(&WidgetValue::Number(1.0)), None);
    }

    #[test]
    fn option_requires_choices_and_valid_default() {
        let no_choices =
            parse_widget("name = \"w\"\ntype = \"option\"\ndefault = \"a\"").unwrap_err();
        assert!(no_choices.contains("choices"), "got: {no_choices}");
        let bad_default = parse_widget(
            "name = \"w\"\ntype = \"option\"\nchoices = [\"a\", \"b\"]\ndefault = \"c\"",
        )
        .unwrap_err();
        assert!(bad_default.contains("one of"), "got: {bad_default}");
    }
}
