//! One stderr line before a viewer plot spends noticeable time or memory.
//!
//! The estimate is a few named rates, not a fixed payload size. A release
//! run on this tree (2026-10-26) is what they are rounded from:
//!
//! | work | measured | model |
//! |---|---|---|
//! | envelope scan, 1e6 samples | 2.22 ms | 4 ms (`SCAN_NANOS_PER_SAMPLE`) |
//! | envelope scan, 1e7 samples | 23.6 ms | 40 ms |
//! | envelope scan, 1e8 samples | 339 ms | 400 ms |
//! | msgpack of 1e6 xy pairs (18.0 MB) | 23.1 ms | 36 ms (`ENCODE_NANOS_PER_BYTE` = 2) |
//! | unix-socket write of 18 MB | 2.6–3.9 ms | 18 ms (`TRANSFER_NANOS_PER_BYTE` = 1) |
//! | heatmap raster, 1000×1000 | 12.2 ms | 12 ms (`RASTER_NANOS_PER_PIXEL`) |
//!
//! Extra memory is the allocation on top of the arrays the script already
//! holds: the envelope or wire copy, plus the msgpack buffer, which are
//! both live while the frame is written. The 32 MiB IPC cap is a protocol
//! limit, not a warning threshold.
//!
//! A notebook render does not print these lines.

use std::cell::Cell;

use crate::decimate::{VIEWER_ENVELOPE_COLUMNS, VIEWER_ENVELOPE_MAX_POINTS};
use crate::figure::{plot_context, PlotContext};
use rustlab_proto::MAX_MSG_LEN;

/// Envelope scan. 1e8 samples took 339 ms; 4 ns/sample rounds that to 0.4 s.
pub(crate) const SCAN_NANOS_PER_SAMPLE: u128 = 4;

/// msgpack of a numeric frame. 18 MB of xy pairs took 23 ms; 2 ns/byte
/// rounds that up.
pub(crate) const ENCODE_NANOS_PER_BYTE: u128 = 2;

/// Local socket write. A large frame is about 0.2 ns/byte; 1 ns/byte
/// covers the syscall on a small frame too.
pub(crate) const TRANSFER_NANOS_PER_BYTE: u128 = 1;

/// `render_heatmap_cells_to_rgba`. 1e6 pixels took 12.2 ms.
pub(crate) const RASTER_NANOS_PER_PIXEL: u128 = 12;

/// Warn when the estimated reduction, encode, and send reach this.
/// A 1e8-point line estimates 0.4 s, so it warns. A 1e7-point line
/// estimates 40 ms and stays quiet.
pub(crate) const VIEWER_WARN_MILLIS: u128 = 250;

/// Extra-memory gate when `MemAvailable` cannot be read.
pub(crate) const VIEWER_WARN_EXTRA_BYTES: usize = 64 * 1024 * 1024;

/// Warn when extra memory reaches this fraction of available memory.
pub(crate) const VIEWER_MEM_FRACTION: usize = 8;

/// Estimated wall time and peak extra bytes for one plot send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlotCost {
    pub millis: u128,
    pub extra_bytes: usize,
}

/// Cost of reducing `scan_samples` and then encoding `sent_bytes`.
/// `sent_points` is the number of f64 pairs kept (the envelope, or the
/// whole series when it is already short).
pub(crate) fn line_cost(scan_samples: usize, sent_points: usize, sent_bytes: usize) -> PlotCost {
    let scan = (scan_samples as u128).saturating_mul(SCAN_NANOS_PER_SAMPLE);
    let io = (sent_bytes as u128).saturating_mul(ENCODE_NANOS_PER_BYTE + TRANSFER_NANOS_PER_BYTE);
    PlotCost {
        millis: scan.saturating_add(io) / 1_000_000,
        extra_bytes: sent_points.saturating_mul(16).saturating_add(sent_bytes),
    }
}

/// Cost of building a bar, heatmap, or surface frame.
/// `work_nanos` is the raster (or zero when the bytes already exist).
pub(crate) fn payload_cost(payload_bytes: usize, extra_bytes: usize, work_nanos: u128) -> PlotCost {
    let io =
        (payload_bytes as u128).saturating_mul(ENCODE_NANOS_PER_BYTE + TRANSFER_NANOS_PER_BYTE);
    PlotCost {
        millis: work_nanos.saturating_add(io) / 1_000_000,
        extra_bytes,
    }
}

/// `MemAvailable` from `/proc/meminfo`, or `None` when the file is absent.
pub(crate) fn parse_mem_available(text: &str) -> Option<usize> {
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("MemAvailable:") {
            let kb: usize = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb.saturating_mul(1024));
        }
    }
    None
}

fn mem_available_bytes() -> Option<usize> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    parse_mem_available(&text)
}

/// Extra memory that is worth a warning: 1/8 of available memory, or
/// [`VIEWER_WARN_EXTRA_BYTES`] when that figure is unknown.
pub(crate) fn memory_limit() -> usize {
    match mem_available_bytes() {
        Some(n) if n >= VIEWER_MEM_FRACTION => n / VIEWER_MEM_FRACTION,
        _ => VIEWER_WARN_EXTRA_BYTES,
    }
}

fn over_budget(cost: &PlotCost, time_limit_ms: u128, mem_limit: usize) -> bool {
    cost.millis >= time_limit_ms || cost.extra_bytes >= mem_limit
}

pub(crate) fn cost_warning(
    prefix: &str,
    tail: &str,
    cost: &PlotCost,
    time_limit_ms: u128,
    mem_limit: usize,
) -> Option<String> {
    if !over_budget(cost, time_limit_ms, mem_limit) {
        return None;
    }
    Some(format!(
        "viewer: warning: {prefix} will take about {} and about {} extra memory; {tail}",
        format_duration(cost.millis),
        format_bytes(cost.extra_bytes),
    ))
}

/// Line / scatter / stem warning, or `None` when the estimate is under both
/// gates. `scan_samples` is the count that will actually be walked; short
/// series that are copied unchanged contribute to `samples` and `sent_*`
/// only.
pub(crate) fn envelope_warning(
    samples: usize,
    scan_samples: usize,
    sent_points: usize,
    sent_bytes: usize,
) -> Option<String> {
    let cost = line_cost(scan_samples, sent_points, sent_bytes);
    let sent = format_bytes(sent_bytes);
    let tail = if scan_samples > 0 {
        format!(
            "reducing each line to a min/max envelope of at most {VIEWER_ENVELOPE_MAX_POINTS} samples ({VIEWER_ENVELOPE_COLUMNS} columns, about {sent} sent) before sending."
        )
    } else {
        format!("sending about {sent}.")
    };
    cost_warning(
        &format!("plotting {samples} samples"),
        &tail,
        &cost,
        VIEWER_WARN_MILLIS,
        memory_limit(),
    )
}

/// Bar / heatmap / surface warning.
///
/// A payload over [`MAX_MSG_LEN`] always returns the hard-limit line. That
/// cap is the protocol, not a cost threshold. Under the cap, the line is
/// the time and memory estimate, and only when one of those gates trips.
/// `fallback_terminal` adds the terminal-draw sentence for a static figure.
/// Live heatmaps pass false.
pub(crate) fn bulk_warning(
    what: &str,
    payload_bytes: usize,
    extra_bytes: usize,
    work_nanos: u128,
    fallback_terminal: bool,
) -> Option<String> {
    if payload_bytes > MAX_MSG_LEN as usize {
        let size = format_bytes(payload_bytes);
        return Some(if fallback_terminal {
            format!(
                "viewer: warning: {what} is about {size}, over the 32 MiB viewer limit — not sending; drawing this figure in the terminal. The viewer stays connected."
            )
        } else {
            format!(
                "viewer: warning: {what} is about {size}, over the 32 MiB viewer limit — not sending."
            )
        });
    }
    let cost = payload_cost(payload_bytes, extra_bytes, work_nanos);
    cost_warning(
        &format!("plotting {what}"),
        &format!("sending it whole, about {}.", format_bytes(payload_bytes)),
        &cost,
        VIEWER_WARN_MILLIS,
        memory_limit(),
    )
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
    if ms >= 100 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{ms} ms")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent_xy(n: usize) -> usize {
        106 + 18 * n
    }

    #[test]
    fn small_line_is_quiet() {
        let n = 50_000usize;
        let sent = sent_xy(VIEWER_ENVELOPE_MAX_POINTS);
        let cost = line_cost(n, VIEWER_ENVELOPE_MAX_POINTS, sent);
        assert!(cost.millis < VIEWER_WARN_MILLIS, "{cost:?}");
        assert!(cost_warning(
            "plotting",
            "reducing.",
            &cost,
            VIEWER_WARN_MILLIS,
            usize::MAX,
        )
        .is_none());
    }

    #[test]
    fn one_e8_line_leads_with_time_and_extra_memory() {
        let n = 100_000_000usize;
        let sent_points = VIEWER_ENVELOPE_MAX_POINTS;
        let sent = sent_xy(sent_points);
        let line = cost_warning(
            &format!("plotting {n} samples"),
            &format!(
                "reducing each line to a min/max envelope of at most {VIEWER_ENVELOPE_MAX_POINTS} samples ({VIEWER_ENVELOPE_COLUMNS} columns, about {} sent) before sending.",
                format_bytes(sent)
            ),
            &line_cost(n, sent_points, sent),
            VIEWER_WARN_MILLIS,
            usize::MAX,
        )
        .unwrap();
        assert_eq!(
            line,
            "viewer: warning: plotting 100000000 samples will take about 0.4 s and about 272 KiB extra memory; reducing each line to a min/max envelope of at most 8192 samples (4096 columns, about 144 KiB sent) before sending."
        );
        assert_eq!(
            envelope_warning(n, n, sent_points, sent).as_deref(),
            Some(line.as_str())
        );
    }

    #[test]
    fn ten_million_points_stay_under_the_time_gate() {
        let n = 10_000_000usize;
        let sent = sent_xy(VIEWER_ENVELOPE_MAX_POINTS);
        let cost = line_cost(n, VIEWER_ENVELOPE_MAX_POINTS, sent);
        assert_eq!(cost.millis, 40, "{cost:?}");
        assert!(cost.millis < VIEWER_WARN_MILLIS);
        assert!(envelope_warning(n, n, VIEWER_ENVELOPE_MAX_POINTS, sent).is_none());
    }

    #[test]
    fn bar_over_the_cap_keeps_the_hard_limit_line() {
        let n = 2_000_000usize;
        let bytes = sent_xy(n);
        let line = bulk_warning(&format!("bar series of {n} bars"), bytes, 0, 0, true).unwrap();
        assert_eq!(
            line,
            "viewer: warning: bar series of 2000000 bars is about 34.3 MiB, over the 32 MiB viewer limit — not sending; drawing this figure in the terminal. The viewer stays connected."
        );
    }

    #[test]
    fn bar_under_the_cap_warns_only_on_time_or_memory() {
        let n = 1_000_000usize;
        let bytes = sent_xy(n);
        let extra = n * 16 + bytes;
        assert!(bytes < MAX_MSG_LEN as usize);
        let cost = payload_cost(bytes, extra, 0);
        assert!(cost.millis < VIEWER_WARN_MILLIS, "{cost:?}");
        assert!(
            cost_warning(
                "plotting",
                "sending.",
                &cost,
                VIEWER_WARN_MILLIS,
                usize::MAX
            )
            .is_none(),
            "a frame under the cap is quiet when time and memory are both under the gates"
        );
        let line = cost_warning(
            &format!("plotting bar series of {n} bars"),
            &format!("sending it whole, about {}.", format_bytes(bytes)),
            &cost,
            VIEWER_WARN_MILLIS,
            1,
        )
        .unwrap();
        assert_eq!(
            line,
            "viewer: warning: plotting bar series of 1000000 bars will take about 54 ms and about 32.4 MiB extra memory; sending it whole, about 17.2 MiB."
        );
    }

    #[test]
    fn heatmap_over_the_cap_without_a_terminal_fallback() {
        let bytes = 4096usize * 2048 * 4 + 64;
        assert!(bytes > MAX_MSG_LEN as usize);
        let line = bulk_warning("heatmap 4096 by 2048", bytes, 0, 0, false).unwrap();
        assert_eq!(
            line,
            "viewer: warning: heatmap 4096 by 2048 is about 32.0 MiB, over the 32 MiB viewer limit — not sending."
        );
    }

    #[test]
    fn modest_heatmap_is_quiet() {
        let pixels = 100usize * 100;
        let rgba = pixels * 4;
        let bytes = rgba + 64;
        let work = (pixels as u128) * RASTER_NANOS_PER_PIXEL;
        assert!(bulk_warning("heatmap 100 by 100", bytes, rgba + bytes, work, true,).is_none());
    }

    #[test]
    fn mem_available_parses_kilobytes() {
        let text = "MemTotal: 100000 kB\nMemAvailable:    524288 kB\nMemFree: 1 kB\n";
        assert_eq!(parse_mem_available(text), Some(524_288 * 1024));
        assert_eq!(parse_mem_available("MemFree: 1 kB\n"), None);
        let limit = 524_288 * 1024 / VIEWER_MEM_FRACTION;
        assert_eq!(limit, 64 * 1024 * 1024);
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
