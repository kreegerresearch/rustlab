//! Tiny duration parser shared by the `cache prune older=...` script
//! directive and the `rustlab cache prune --older-than` CLI flag.
//!
//! Grammar: `<integer><unit>` with no whitespace between the two
//! pieces. Units: `ms`, `s` (default if absent), `m`, `h`, `d`, `w`.
//! `500ms` truncates to 0 seconds — sub-second pruning isn't useful
//! but doesn't error either. Returns seconds so callers can pass
//! straight into `Store::prune_older_than`.

/// Parser errors. Surfaced verbatim in CLI / REPL error messages so
/// the user sees exactly what went wrong.
#[derive(Debug, thiserror::Error)]
pub enum DurationParseError {
    #[error("empty duration")]
    Empty,
    #[error("duration '{0}' missing numeric component")]
    MissingNumber(String),
    #[error("duration '{0}': invalid number")]
    InvalidNumber(String),
    #[error("duration '{input}': unknown unit '{unit}' (use ms, s, m, h, d, w)")]
    UnknownUnit { input: String, unit: String },
}

/// Parse a `<number><unit>` string into whole seconds. See module
/// docs for the grammar and unit list.
pub fn parse_duration_secs(s: &str) -> Result<u64, DurationParseError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(DurationParseError::Empty);
    }
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (num_str, unit) = s.split_at(split);
    if num_str.is_empty() {
        return Err(DurationParseError::MissingNumber(s.to_string()));
    }
    let n: u64 = num_str
        .parse()
        .map_err(|_| DurationParseError::InvalidNumber(s.to_string()))?;
    let secs = match unit.trim() {
        "" | "s" => n,
        "ms" => n / 1000,
        "m" => n * 60,
        "h" => n * 60 * 60,
        "d" => n * 60 * 60 * 24,
        "w" => n * 60 * 60 * 24 * 7,
        other => {
            return Err(DurationParseError::UnknownUnit {
                input: s.to_string(),
                unit: other.to_string(),
            });
        }
    };
    Ok(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_units() {
        assert_eq!(parse_duration_secs("500ms").unwrap(), 0);
        assert_eq!(parse_duration_secs("30").unwrap(), 30);
        assert_eq!(parse_duration_secs("30s").unwrap(), 30);
        assert_eq!(parse_duration_secs("5m").unwrap(), 300);
        assert_eq!(parse_duration_secs("12h").unwrap(), 12 * 3600);
        assert_eq!(parse_duration_secs("30d").unwrap(), 30 * 24 * 3600);
        assert_eq!(parse_duration_secs("2w").unwrap(), 2 * 7 * 24 * 3600);
    }

    #[test]
    fn rejects_empty() {
        assert!(matches!(
            parse_duration_secs(""),
            Err(DurationParseError::Empty)
        ));
        assert!(matches!(
            parse_duration_secs("   "),
            Err(DurationParseError::Empty)
        ));
    }

    #[test]
    fn rejects_bare_unit() {
        assert!(matches!(
            parse_duration_secs("d"),
            Err(DurationParseError::MissingNumber(_))
        ));
    }

    #[test]
    fn rejects_unknown_unit() {
        assert!(matches!(
            parse_duration_secs("30q"),
            Err(DurationParseError::UnknownUnit { .. })
        ));
    }
}
