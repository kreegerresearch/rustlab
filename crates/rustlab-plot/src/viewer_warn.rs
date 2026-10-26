//! One stderr line before a viewer plot spends noticeable time or memory.
//!
//! The first guess is a few named rates, from a release run on this tree
//! (2026-10-26). Each real send replaces that guess with a half-and-half
//! average of the previous estimate and the measurement, and the next
//! warning uses the updated numbers. Nothing is written to disk.
//!
//! | work | measured | prior |
//! |---|---|---|
//! | envelope scan, 1e6 samples | 2.22 ms | 4 ms (`SCAN_NANOS_PER_SAMPLE`) |
//! | envelope scan, 1e7 samples | 23.6 ms | 40 ms |
//! | envelope scan, 1e8 samples | 339 ms | 400 ms |
//! | msgpack of 1e6 xy pairs (18.0 MB) | 23.1 ms | 36 ms (`ENCODE_NANOS_PER_BYTE` = 2) |
//! | unix-socket write of 18 MB | 2.6–3.9 ms | 18 ms (`TRANSFER_NANOS_PER_BYTE` = 1) |
//! | heatmap raster, 1000×1000 | 12.2 ms | 12 ms (`RASTER_NANOS_PER_PIXEL`) |
//!
//! Scan and encode are one estimate for the process (they are CPU work).
//! Transfer rate and the fixed per-send latency live on the viewer
//! connection, so a remote viewer does not inherit a local socket's
//! numbers. A send smaller than [`MIN_RATE_BYTES`], and a scan shorter
//! than [`MIN_SCAN_SAMPLES`], updates that latency and not the rate: a
//! round trip must not be read as bandwidth. The connect `Ping` is
//! already that kind of small reply, so there is no separate probe.
//!
//! Extra memory is the allocation on top of the arrays the script already
//! holds: the envelope or wire copy, plus the msgpack buffer, which are
//! both live while the frame is written. The 32 MiB IPC cap is a protocol
//! limit, not a warning threshold.
//!
//! A notebook render does not print these lines.

use std::cell::Cell;
use std::sync::Mutex;

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
/// With the prior rates a 1e8-point line is about 0.4 s and stays quiet.
/// About 2.5×10^8 samples is 1.0 s and warns. A measured round trip can
/// push a smaller plot over this on the next warning.
pub(crate) const VIEWER_WARN_MILLIS: u128 = 1000;

/// Scans shorter than this teach latency, not the per-sample rate.
pub(crate) const MIN_SCAN_SAMPLES: u128 = 100_000;

/// Encodes and socket writes shorter than this teach latency, not the
/// per-byte rate. A label, a ping, or a reply is mostly fixed overhead.
pub(crate) const MIN_RATE_BYTES: u128 = 64 * 1024;

/// Extra-memory gate when `MemAvailable` cannot be read.
pub(crate) const VIEWER_WARN_EXTRA_BYTES: usize = 64 * 1024 * 1024;

/// Warn when extra memory reaches this fraction of available memory.
pub(crate) const VIEWER_MEM_FRACTION: usize = 8;

/// Nanoseconds per sample or per byte, plus a fixed overhead.
///
/// `nanos_per_unit` starts at a prior. [`RateModel::observe`] blends each
/// large measurement in with equal weight. A short measurement replaces
/// or blends `latency_ns` and leaves the rate alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RateModel {
    nanos_per_unit: u128,
    latency_ns: u128,
}

impl RateModel {
    pub(crate) const fn prior(nanos_per_unit: u128) -> Self {
        Self {
            nanos_per_unit,
            latency_ns: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn nanos_per_unit(self) -> u128 {
        self.nanos_per_unit
    }

    #[cfg(test)]
    pub(crate) fn latency_ns(self) -> u128 {
        self.latency_ns
    }

    /// `latency + units * nanos_per_unit`.
    pub(crate) fn predict(self, units: u128) -> u128 {
        self.latency_ns
            .saturating_add(units.saturating_mul(self.nanos_per_unit))
    }

    /// Fold one measurement into this model.
    ///
    /// `units` below `min_units` (including a ping or a label) updates
    /// latency only. A larger sample updates the rate from the time left
    /// after subtracting the latency learned so far. The new rate is the
    /// average of the previous rate and this sample.
    pub(crate) fn observe(&mut self, units: u128, elapsed_ns: u128, min_units: u128) {
        if elapsed_ns == 0 || units == 0 {
            return;
        }
        if units < min_units {
            self.observe_latency(elapsed_ns);
            return;
        }
        let variable = elapsed_ns.saturating_sub(self.latency_ns);
        if variable == 0 {
            return;
        }
        let sample = (variable / units).max(1);
        self.nanos_per_unit = ewma(self.nanos_per_unit, sample);
    }

    fn observe_latency(&mut self, elapsed_ns: u128) {
        if self.latency_ns == 0 {
            self.latency_ns = elapsed_ns;
        } else {
            self.latency_ns = ewma(self.latency_ns, elapsed_ns);
        }
    }
}

/// Half the previous value, half the new sample.
fn ewma(prev: u128, sample: u128) -> u128 {
    prev.saturating_add(sample) / 2
}

/// Rates used to price one warning. Scan and encode come from the process.
/// Transfer comes from the connection that will send the plot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CostRates {
    pub scan: RateModel,
    pub encode: RateModel,
    pub transfer: RateModel,
}

impl CostRates {
    #[cfg(test)]
    pub(crate) const fn prior() -> Self {
        Self {
            scan: RateModel::prior(SCAN_NANOS_PER_SAMPLE),
            encode: RateModel::prior(ENCODE_NANOS_PER_BYTE),
            transfer: RateModel::prior(TRANSFER_NANOS_PER_BYTE),
        }
    }

    /// Process scan and encode, plus this connection's transfer model.
    pub(crate) fn current(transfer: RateModel) -> Self {
        let cpu = cpu_lock();
        Self {
            scan: cpu.scan,
            encode: cpu.encode,
            transfer,
        }
    }
}

struct CpuRates {
    scan: RateModel,
    encode: RateModel,
}

static CPU: Mutex<CpuRates> = Mutex::new(CpuRates {
    scan: RateModel::prior(SCAN_NANOS_PER_SAMPLE),
    encode: RateModel::prior(ENCODE_NANOS_PER_BYTE),
});

fn cpu_lock() -> std::sync::MutexGuard<'static, CpuRates> {
    CPU.lock().unwrap_or_else(|err| err.into_inner())
}

/// Record one envelope scan. Short scans update latency only.
pub(crate) fn record_scan(samples: u128, elapsed_ns: u128) {
    cpu_lock()
        .scan
        .observe(samples, elapsed_ns, MIN_SCAN_SAMPLES);
}

/// Record one msgpack encode. Short frames update latency only.
pub(crate) fn record_encode(bytes: u128, elapsed_ns: u128) {
    cpu_lock().encode.observe(bytes, elapsed_ns, MIN_RATE_BYTES);
}

/// Estimated wall time and peak extra bytes for one plot send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlotCost {
    pub millis: u128,
    pub extra_bytes: usize,
}

/// Cost of reducing `scan_samples` and then encoding and sending `sent_bytes`.
/// `sent_points` is the number of f64 pairs kept (the envelope, or the
/// whole series when it is already short).
pub(crate) fn line_cost(
    scan_samples: usize,
    sent_points: usize,
    sent_bytes: usize,
    rates: &CostRates,
) -> PlotCost {
    let scan_ns = if scan_samples == 0 {
        0
    } else {
        rates.scan.predict(scan_samples as u128)
    };
    let sent = sent_bytes as u128;
    let io = rates
        .encode
        .predict(sent)
        .saturating_add(rates.transfer.predict(sent));
    PlotCost {
        millis: scan_ns.saturating_add(io) / 1_000_000,
        extra_bytes: sent_points.saturating_mul(16).saturating_add(sent_bytes),
    }
}

/// Cost of building a bar, heatmap, or surface frame.
/// `work_nanos` is the raster (or zero when the bytes already exist).
/// The raster rate stays the prior; encode and transfer use `rates`.
pub(crate) fn payload_cost(
    payload_bytes: usize,
    extra_bytes: usize,
    work_nanos: u128,
    rates: &CostRates,
) -> PlotCost {
    let n = payload_bytes as u128;
    let io = rates
        .encode
        .predict(n)
        .saturating_add(rates.transfer.predict(n));
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
        "viewer: warning: {prefix} will take an estimated {} and an estimated {} extra memory; {tail}",
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
    rates: &CostRates,
) -> Option<String> {
    let cost = line_cost(scan_samples, sent_points, sent_bytes, rates);
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
    rates: &CostRates,
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
    let cost = payload_cost(payload_bytes, extra_bytes, work_nanos, rates);
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

    fn prior() -> CostRates {
        CostRates::prior()
    }

    #[test]
    fn small_line_is_quiet() {
        let n = 50_000usize;
        let sent = sent_xy(VIEWER_ENVELOPE_MAX_POINTS);
        let rates = prior();
        let cost = line_cost(n, VIEWER_ENVELOPE_MAX_POINTS, sent, &rates);
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
    fn one_e8_line_stays_under_one_second() {
        let n = 100_000_000usize;
        let sent_points = VIEWER_ENVELOPE_MAX_POINTS;
        let sent = sent_xy(sent_points);
        let rates = prior();
        let cost = line_cost(n, sent_points, sent, &rates);
        assert_eq!(cost.millis, 400, "{cost:?}");
        assert!(cost.millis < VIEWER_WARN_MILLIS);
        assert!(cost_warning(
            &format!("plotting {n} samples"),
            "reducing.",
            &cost,
            VIEWER_WARN_MILLIS,
            usize::MAX,
        )
        .is_none());
        assert!(envelope_warning(n, n, sent_points, sent, &rates).is_none());
    }

    #[test]
    fn line_at_one_second_leads_with_time_and_extra_memory() {
        let n = 250_000_000usize;
        let sent_points = VIEWER_ENVELOPE_MAX_POINTS;
        let sent = sent_xy(sent_points);
        let rates = prior();
        let cost = line_cost(n, sent_points, sent, &rates);
        assert_eq!(cost.millis, VIEWER_WARN_MILLIS, "{cost:?}");
        let line = envelope_warning(n, n, sent_points, sent, &rates).unwrap();
        assert_eq!(
            line,
            "viewer: warning: plotting 250000000 samples will take an estimated 1.0 s and an estimated 272 KiB extra memory; reducing each line to a min/max envelope of at most 8192 samples (4096 columns, about 144 KiB sent) before sending."
        );
        // 4 ns/sample plus the envelope encode. One sample below stays quiet.
        let below = 249_889_328usize;
        let at = 249_889_329usize;
        assert_eq!(
            line_cost(below, sent_points, sent, &rates).millis,
            VIEWER_WARN_MILLIS - 1
        );
        assert_eq!(
            line_cost(at, sent_points, sent, &rates).millis,
            VIEWER_WARN_MILLIS
        );
        assert!(cost_warning(
            "plotting",
            "reducing.",
            &line_cost(below, sent_points, sent, &rates),
            VIEWER_WARN_MILLIS,
            usize::MAX
        )
        .is_none());
        assert!(cost_warning(
            "plotting",
            "reducing.",
            &line_cost(at, sent_points, sent, &rates),
            VIEWER_WARN_MILLIS,
            usize::MAX
        )
        .is_some());
    }

    #[test]
    fn ten_million_points_stay_under_the_time_gate() {
        let n = 10_000_000usize;
        let sent = sent_xy(VIEWER_ENVELOPE_MAX_POINTS);
        let rates = prior();
        let cost = line_cost(n, VIEWER_ENVELOPE_MAX_POINTS, sent, &rates);
        assert_eq!(cost.millis, 40, "{cost:?}");
        assert!(cost.millis < VIEWER_WARN_MILLIS);
        assert!(envelope_warning(n, n, VIEWER_ENVELOPE_MAX_POINTS, sent, &rates).is_none());
    }

    #[test]
    fn bar_over_the_cap_keeps_the_hard_limit_line() {
        let n = 2_000_000usize;
        let bytes = sent_xy(n);
        let line = bulk_warning(
            &format!("bar series of {n} bars"),
            bytes,
            0,
            0,
            true,
            &prior(),
        )
        .unwrap();
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
        let rates = prior();
        assert!(bytes < MAX_MSG_LEN as usize);
        let cost = payload_cost(bytes, extra, 0, &rates);
        assert_eq!(cost.millis, 54, "{cost:?}");
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
            "viewer: warning: plotting bar series of 1000000 bars will take an estimated 54 ms and an estimated 32.4 MiB extra memory; sending it whole, about 17.2 MiB."
        );
    }

    #[test]
    fn heatmap_over_the_cap_without_a_terminal_fallback() {
        let bytes = 4096usize * 2048 * 4 + 64;
        assert!(bytes > MAX_MSG_LEN as usize);
        let line = bulk_warning("heatmap 4096 by 2048", bytes, 0, 0, false, &prior()).unwrap();
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
        assert!(bulk_warning(
            "heatmap 100 by 100",
            bytes,
            rgba + bytes,
            work,
            true,
            &prior(),
        )
        .is_none());
    }

    #[test]
    fn slow_link_updates_one_connection_and_the_next_warning() {
        // Injected timings, not a clock. A 200-byte reply in 40 ms is
        // latency. Three 8 MB writes of 200 ms then walk the per-byte
        // rate from the 1 ns prior through 10, 15, and 17.
        let mut link = RateModel::prior(TRANSFER_NANOS_PER_BYTE);
        link.observe(200, 40_000_000, MIN_RATE_BYTES);
        assert_eq!(link.latency_ns(), 40_000_000);
        assert_eq!(link.nanos_per_unit(), 1);
        for expect in [10, 15, 17] {
            link.observe(8_000_000, 200_000_000, MIN_RATE_BYTES);
            assert_eq!(link.nanos_per_unit(), expect);
            assert_eq!(link.latency_ns(), 40_000_000);
        }
        let untouched = RateModel::prior(TRANSFER_NANOS_PER_BYTE);
        assert_eq!(untouched.nanos_per_unit(), 1);
        assert_eq!(untouched.latency_ns(), 0);

        let n = 100_000_000usize;
        let sent_points = VIEWER_ENVELOPE_MAX_POINTS;
        let sent = sent_xy(sent_points);
        let mut rates = prior();
        rates.transfer = link;
        let cost = line_cost(n, sent_points, sent, &rates);
        assert_eq!(cost.millis, 442, "{cost:?}");
        assert!(envelope_warning(n, n, sent_points, sent, &rates).is_none());

        // An 800 ms round trip, with the rate still at the prior, puts the
        // next 1e8-point line over 1 s (it was 0.4 s before that sample).
        let mut remote = RateModel::prior(TRANSFER_NANOS_PER_BYTE);
        remote.observe(200, 800_000_000, MIN_RATE_BYTES);
        assert_eq!(remote.nanos_per_unit(), 1);
        assert_eq!(remote.latency_ns(), 800_000_000);
        rates.transfer = remote;
        let cost = line_cost(n, sent_points, sent, &rates);
        assert_eq!(cost.millis, 1200, "{cost:?}");
        let line = envelope_warning(n, n, sent_points, sent, &rates).unwrap();
        assert_eq!(
            line,
            "viewer: warning: plotting 100000000 samples will take an estimated 1.2 s and an estimated 272 KiB extra memory; reducing each line to a min/max envelope of at most 8192 samples (4096 columns, about 144 KiB sent) before sending."
        );
    }

    #[test]
    fn tiny_or_short_samples_do_not_move_the_rate() {
        let mut model = RateModel::prior(SCAN_NANOS_PER_SAMPLE);
        model.observe(1_000, 50_000, MIN_SCAN_SAMPLES);
        assert_eq!(model.nanos_per_unit(), 4);
        assert_eq!(model.latency_ns(), 50_000);
        // A large sample that finishes inside the latency already learned
        // does not change the rate.
        model.observe(1_000_000, 10_000, MIN_SCAN_SAMPLES);
        assert_eq!(model.nanos_per_unit(), 4);
        // A second short sample blends the latency. The first sample
        // above was 50 µs.
        model.observe(1_000, 80_000_000, MIN_SCAN_SAMPLES);
        assert_eq!(model.latency_ns(), ewma(50_000, 80_000_000));
        assert_eq!(model.nanos_per_unit(), 4);

        // First short sample replaces latency; the next one averages.
        // 40 ms then 80 ms is 60 ms, and the rate stays at the prior.
        let mut latency = RateModel::prior(TRANSFER_NANOS_PER_BYTE);
        latency.observe(200, 40_000_000, MIN_RATE_BYTES);
        latency.observe(200, 80_000_000, MIN_RATE_BYTES);
        assert_eq!(latency.latency_ns(), 60_000_000);
        assert_eq!(latency.nanos_per_unit(), 1);

        let mut scan = RateModel::prior(SCAN_NANOS_PER_SAMPLE);
        scan.observe(100_000_000, 800_000_000, MIN_SCAN_SAMPLES);
        assert_eq!(scan.nanos_per_unit(), 6);
        assert_eq!(scan.latency_ns(), 0);
        scan.observe(1_000, 50_000, MIN_SCAN_SAMPLES);
        assert_eq!(scan.nanos_per_unit(), 6);
        assert_eq!(scan.latency_ns(), 50_000);

        let mut zero = RateModel::prior(4);
        zero.observe(1_000_000, 0, MIN_SCAN_SAMPLES);
        zero.observe(0, 1_000, MIN_SCAN_SAMPLES);
        assert_eq!(zero, RateModel::prior(4));
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
