//! One stderr line before a large viewer plot spends time or bandwidth.
//!
//! Thresholds are fixed. A notebook render does not print these: the page
//! captures script output, and notebook context never sends to the viewer.

use std::cell::Cell;

use crate::decimate::{VIEWER_ENVELOPE_COLUMNS, VIEWER_ENVELOPE_MAX_POINTS};
use crate::figure::{plot_context, PlotContext};
use rustlab_proto::MAX_MSG_LEN;

/// Unreduced line payload at or above this gets one warning before the
/// envelope scan. 8 MiB is about 470_000 xy samples. Smaller reductions
/// stay quiet.
pub(crate) const VIEWER_LARGE_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

/// Bar, heatmap, and surface frames at or above this (half of the 32 MiB
/// cap) get one warning. Anything over the cap always warns.
pub(crate) const VIEWER_NEAR_CAP_BYTES: usize = (MAX_MSG_LEN as usize) / 2;

/// Envelope scan cost used only in the warning. A release run reduced 1e8
/// samples in 347 ms; 4 ns/sample rounds that to about 400 ms.
const ENVELOPE_NANOS_PER_SAMPLE: u128 = 4;

/// Skip the time clause when the scan should be quicker than this.
const VIEWER_TIME_HINT_MIN_MS: u128 = 100;

/// Text of the large-line warning, or `None` when the unreduced payload is
/// under [`VIEWER_LARGE_PAYLOAD_BYTES`]. `sent_bytes` is the estimate of
/// what will actually be written after the envelope.
pub(crate) fn envelope_warning(
    samples: usize,
    raw_bytes: usize,
    sent_bytes: usize,
) -> Option<String> {
    if raw_bytes < VIEWER_LARGE_PAYLOAD_BYTES {
        return None;
    }
    let raw = format_bytes(raw_bytes);
    let sent = format_bytes(sent_bytes);
    let mut line = format!(
        "viewer: warning: {samples} samples would be about {raw}; reducing each line to a min/max envelope of at most {VIEWER_ENVELOPE_MAX_POINTS} samples ({VIEWER_ENVELOPE_COLUMNS} columns, about {sent} sent) before sending."
    );
    let ms = (samples as u128).saturating_mul(ENVELOPE_NANOS_PER_SAMPLE) / 1_000_000;
    if ms >= VIEWER_TIME_HINT_MIN_MS {
        line.push(' ');
        line.push_str(&format!(
            "One pass over the samples, about {}.",
            format_duration(ms)
        ));
    }
    Some(line)
}

/// Text of the bar / heatmap / surface warning, or `None` when `bytes` is
/// under [`VIEWER_NEAR_CAP_BYTES`] and still fits in one frame.
///
/// `fallback_terminal` adds the terminal-draw sentence. Live heatmaps pass
/// false: they are not drawn in the terminal when refused.
pub(crate) fn bulk_warning(what: &str, bytes: usize, fallback_terminal: bool) -> Option<String> {
    let over = bytes > MAX_MSG_LEN as usize;
    if !over && bytes < VIEWER_NEAR_CAP_BYTES {
        return None;
    }
    let size = format_bytes(bytes);
    if over {
        if fallback_terminal {
            Some(format!(
                "viewer: warning: {what} is about {size}, over the 32 MiB viewer limit — not sending; drawing this figure in the terminal. The viewer stays connected."
            ))
        } else {
            Some(format!(
                "viewer: warning: {what} is about {size}, over the 32 MiB viewer limit — not sending."
            ))
        }
    } else {
        Some(format!(
            "viewer: warning: {what} is about {size} (32 MiB limit) — sending it whole, not reduced to an envelope."
        ))
    }
}

/// One warning per slot. Notebook context returns `None` and does not
/// consume the slot, so a later terminal plot on the same thread can warn.
/// The caller prints the returned line. A second call in the same slot
/// returns `None`.
pub(crate) fn take_warning(slot: &Cell<bool>, line: Option<String>) -> Option<String> {
    let line = line?;
    if plot_context() == PlotContext::Notebook {
        return None;
    }
    if slot.get() {
        return None;
    }
    slot.set(true);
    Some(line)
}

pub(crate) fn print_warning(line: &str) {
    eprintln!("{line}");
}

fn format_bytes(n: usize) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let n = n as f64;
    if n >= GIB {
        format!("{:.2} GiB", n / GIB)
    } else if n >= MIB {
        format!("{:.1} MiB", n / MIB)
    } else if n >= KIB {
        format!("{:.0} KiB", n / KIB)
    } else {
        format!("{} bytes", n as usize)
    }
}

fn format_duration(ms: u128) -> String {
    if ms >= 1000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{ms} ms")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_line_is_quiet() {
        let n = 50_000usize;
        let raw = 106 + 18 * n;
        assert!(raw < VIEWER_LARGE_PAYLOAD_BYTES);
        assert!(envelope_warning(n, raw, raw).is_none());
    }

    #[test]
    fn one_e8_line_states_sizes_and_time() {
        let n = 100_000_000usize;
        let raw = 106 + 18 * n;
        let sent = 106 + 18 * VIEWER_ENVELOPE_MAX_POINTS;
        let line = envelope_warning(n, raw, sent).unwrap();
        assert_eq!(
            line,
            "viewer: warning: 100000000 samples would be about 1.68 GiB; reducing each line to a min/max envelope of at most 8192 samples (4096 columns, about 144 KiB sent) before sending. One pass over the samples, about 400 ms."
        );
    }

    #[test]
    fn bar_over_the_cap_names_the_terminal_fallback() {
        let n = 2_000_000usize;
        let bytes = 106 + 18 * n;
        let line = bulk_warning(&format!("bar series of {n} bars"), bytes, true).unwrap();
        assert_eq!(
            line,
            "viewer: warning: bar series of 2000000 bars is about 34.3 MiB, over the 32 MiB viewer limit — not sending; drawing this figure in the terminal. The viewer stays connected."
        );
    }

    #[test]
    fn bar_near_the_cap_says_it_is_sent_whole() {
        let n = 1_000_000usize;
        let bytes = 106 + 18 * n;
        assert!(bytes < MAX_MSG_LEN as usize);
        assert!(bytes >= VIEWER_NEAR_CAP_BYTES);
        let line = bulk_warning(&format!("bar series of {n} bars"), bytes, true).unwrap();
        assert_eq!(
            line,
            "viewer: warning: bar series of 1000000 bars is about 17.2 MiB (32 MiB limit) — sending it whole, not reduced to an envelope."
        );
    }

    #[test]
    fn heatmap_over_the_cap_without_a_terminal_fallback() {
        let bytes = 4096usize * 2048 * 4 + 64;
        assert!(bytes > MAX_MSG_LEN as usize);
        let line = bulk_warning("heatmap 4096 by 2048", bytes, false).unwrap();
        assert_eq!(
            line,
            "viewer: warning: heatmap 4096 by 2048 is about 32.0 MiB, over the 32 MiB viewer limit — not sending."
        );
    }

    #[test]
    fn modest_heatmap_is_quiet() {
        let bytes = 100usize * 100 * 4 + 64;
        assert!(bulk_warning("heatmap 100 by 100", bytes, true).is_none());
    }

    #[test]
    fn one_warning_per_slot_and_notebooks_stay_quiet() {
        let slot = Cell::new(false);
        let line = Some("viewer: warning: example".to_string());
        assert_eq!(
            take_warning(&slot, line.clone()).as_deref(),
            Some("viewer: warning: example")
        );
        assert!(take_warning(&slot, line.clone()).is_none());

        slot.set(false);
        let prev = plot_context();
        crate::figure::set_plot_context(PlotContext::Notebook);
        assert!(take_warning(&slot, line).is_none());
        assert!(
            !slot.get(),
            "a suppressed notebook warning must not consume the slot"
        );
        crate::figure::set_plot_context(prev);
    }
}
