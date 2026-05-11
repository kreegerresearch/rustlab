use std::collections::{HashMap, HashSet};

/// Per-function accumulated statistics.
#[derive(Default, Clone)]
pub struct FnStats {
    pub call_count: u64,
    pub total_ns: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
}

/// Runtime profiler embedded in the Evaluator.
/// Zero overhead when disabled — all hot paths short-circuit on `enabled`.
#[derive(Clone)]
pub struct Profiler {
    enabled: bool,
    /// None = track all; Some(set) = track only these names.
    whitelist: Option<HashSet<String>>,
    /// While > 0, inner calls (lambdas/fns invoked as callbacks) are not recorded.
    higher_order_depth: u32,
    stats: HashMap<String, FnStats>,
}

impl Default for Profiler {
    fn default() -> Self {
        Self {
            enabled: false,
            whitelist: None,
            higher_order_depth: 0,
            stats: HashMap::new(),
        }
    }
}

impl Profiler {
    /// Activate profiling. `names = None` tracks all; `Some(v)` tracks only the named functions.
    pub fn enable(&mut self, names: Option<Vec<String>>) {
        self.enabled = true;
        self.whitelist = names.map(|v| v.into_iter().collect());
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    pub fn has_data(&self) -> bool {
        !self.stats.is_empty()
    }

    /// True when the next call to `record` for `name` should actually be stored.
    /// Returns false immediately when profiling is disabled — no overhead.
    pub fn should_track(&self, name: &str) -> bool {
        self.enabled
            && self.higher_order_depth == 0
            && self.whitelist.as_ref().map_or(true, |s| s.contains(name))
    }

    /// Signal that we are entering a higher-order call (arrayfun, user fn, lambda).
    /// Inner function calls will be suppressed until the matching `exit_higher_order`.
    pub fn enter_higher_order(&mut self) {
        self.higher_order_depth = self.higher_order_depth.saturating_add(1);
    }

    pub fn exit_higher_order(&mut self) {
        self.higher_order_depth = self.higher_order_depth.saturating_sub(1);
    }

    pub fn record(&mut self, name: &str, elapsed_ns: u64, in_bytes: u64, out_bytes: u64) {
        let s = self.stats.entry(name.to_string()).or_default();
        s.call_count += 1;
        s.total_ns += elapsed_ns;
        s.input_bytes += in_bytes;
        s.output_bytes += out_bytes;
    }

    /// Drain the stats and return rows sorted by total time descending.
    pub fn take_report(&mut self) -> Vec<(String, FnStats)> {
        let mut rows: Vec<_> = std::mem::take(&mut self.stats).into_iter().collect();
        rows.sort_by(|a, b| b.1.total_ns.cmp(&a.1.total_ns));
        rows
    }
}

/// Print a profiling report to stderr.
/// Called automatically at script end if any data was collected.
pub fn print_report(rows: &[(String, FnStats)]) {
    let non_zero: Vec<_> = rows.iter().filter(|(_, s)| s.total_ns > 0).collect();
    if non_zero.is_empty() {
        return;
    }

    let fn_col = non_zero
        .iter()
        .map(|(n, _)| n.len())
        .max()
        .unwrap_or(8)
        .max(8);

    // Header
    eprintln!();
    eprintln!(
        "  {:<fn_col$}  {:>6}  {:>12}  {:>10}  {:>10}  {:>10}  {:>10}",
        "Function",
        "Calls",
        "Total (ms)",
        "Avg (µs)",
        "In (KB)",
        "Out (KB)",
        "Mbit/s",
        fn_col = fn_col
    );
    let sep_len = fn_col + 2 + 6 + 2 + 12 + 2 + 10 + 2 + 10 + 2 + 10 + 2 + 10 + 4;
    let sep = "─".repeat(sep_len);
    eprintln!("  {sep}");

    let mut tot_calls = 0u64;
    let mut tot_ns = 0u64;
    let mut tot_in = 0u64;
    let mut tot_out = 0u64;

    for (name, s) in &non_zero {
        let total_ms = s.total_ns as f64 / 1_000_000.0;
        let avg_us = s.total_ns as f64 / s.call_count as f64 / 1_000.0;
        let in_kb = s.input_bytes as f64 / 1024.0;
        let out_kb = s.output_bytes as f64 / 1024.0;
        let secs = s.total_ns as f64 / 1_000_000_000.0;
        let mbits = if secs > 0.0 {
            (s.input_bytes + s.output_bytes) as f64 * 8.0 / secs / 1_000_000.0
        } else {
            0.0
        };
        eprintln!(
            "  {:<fn_col$}  {:>6}  {:>12.3}  {:>10.3}  {:>10.2}  {:>10.2}  {:>10.1}",
            name,
            s.call_count,
            total_ms,
            avg_us,
            in_kb,
            out_kb,
            mbits,
            fn_col = fn_col
        );
        tot_calls += s.call_count;
        tot_ns += s.total_ns;
        tot_in += s.input_bytes;
        tot_out += s.output_bytes;
    }

    eprintln!("  {sep}");
    let tot_ms = tot_ns as f64 / 1_000_000.0;
    let tot_secs = tot_ns as f64 / 1_000_000_000.0;
    let tot_mbits = if tot_secs > 0.0 {
        (tot_in + tot_out) as f64 * 8.0 / tot_secs / 1_000_000.0
    } else {
        0.0
    };
    eprintln!(
        "  {:<fn_col$}  {:>6}  {:>12.3}  {:>10}  {:>10.2}  {:>10.2}  {:>10.1}",
        "TOTAL",
        tot_calls,
        tot_ms,
        "",
        tot_in as f64 / 1024.0,
        tot_out as f64 / 1024.0,
        tot_mbits,
        fn_col = fn_col
    );
    eprintln!();
}
