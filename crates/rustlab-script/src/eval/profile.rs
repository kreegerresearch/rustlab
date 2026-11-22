//! Call profiler embedded in the [`Evaluator`](super::Evaluator).
//!
//! Every profiled call pushes a frame on a stack. When the frame pops, its
//! elapsed time is the call's *inclusive* time, and that time minus what its
//! children took is its *self* time. Nested user functions, the builtins
//! they call and callbacks inside `arrayfun` all get their own rows.
//!
//! Besides named calls there are pseudo-rows in brackets:
//! - `[top-level]` — script time outside any profiled call (statements,
//!   loops, assignments at the top level). With it, the self times of all
//!   main-thread rows add up to the wall time since profiling started.
//! - `[matmul]` — matrix products (`A * B`, including `A' * B`), always on
//!   while profiling. In/Out are the stored bytes of the two operands and
//!   of the product, so Mbit/s is the throughput of the multiply.
//! - With op profiling (`--profile=ops`): one row per operator (`[+]`,
//!   `[.*]`, …) and `[transpose]`, with the same operand/result bytes.
//!   `[index]` and `[index-assign]` time the subscript and leave In/Out at 0.
//! - `[widen]` and `[promote]` — complex views built from real storage, and
//!   real buffers converted to complex by a write (`eval/shared.rs`). Calls,
//!   time (in the total column; it is already inside the caller's self
//!   time, so self is 0) and KB: in is the real bytes read, out the complex
//!   bytes built. These rank the code paths still to convert.
//!
//! `parmap` workers profile into their own copies; their rows are merged
//! back as `parmap:<name>`. Those rows are CPU time summed across threads,
//! so they are reported but kept out of the wall-clock total.
//!
//! Zero overhead when disabled: every entry point short-circuits on
//! `enabled`.

use super::shared::{conversion_counts, set_timing, Conversions};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// Per-row accumulated statistics.
#[derive(Default, Clone, Debug)]
pub struct FnStats {
    pub call_count: u64,
    /// Inclusive time: the call and everything it called. A recursive
    /// function counts only its outermost activation, so this never
    /// exceeds wall time on one thread.
    pub total_ns: u64,
    /// Time in the call itself, excluding profiled children.
    pub self_ns: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    /// True for rows merged from `parmap` workers (CPU time, not wall time).
    pub parallel: bool,
}

#[derive(Clone)]
struct Frame {
    start: Instant,
    child_ns: u64,
}

/// Handle returned by [`Profiler::enter_op`]; pass it back to
/// [`Profiler::exit_op`]. `None` when nothing is being recorded.
pub type OpFrame = Option<&'static str>;

/// Name of the pseudo-row for script time outside any profiled call.
pub const TOP_LEVEL: &str = "[top-level]";

#[derive(Clone, Default)]
pub struct Profiler {
    enabled: bool,
    /// Record per-operator and indexing rows (`--profile=ops`).
    ops: bool,
    /// None = report all; Some(set) = report only these names.
    whitelist: Option<HashSet<String>>,
    stack: Vec<Frame>,
    /// Activations per name currently on the stack, for recursion.
    active: HashMap<String, u32>,
    /// Start of the `[top-level]` root and the time its children took.
    /// `None` for a parmap worker, whose root is the parent's parmap call.
    root: Option<Frame>,
    stats: HashMap<String, FnStats>,
    /// Conversion counters on this profiler's thread when they were last
    /// read; `None` until the first profiled call on that thread.
    conv_base: Option<(Conversions, Conversions)>,
}

impl Profiler {
    /// Activate profiling. `names = None` tracks all; `Some(v)` tracks only
    /// the named functions.
    pub fn enable(&mut self, names: Option<Vec<String>>) {
        self.enabled = true;
        self.whitelist = names.map(|v| v.into_iter().collect());
        if self.root.is_none() {
            self.root = Some(Frame { start: Instant::now(), child_ns: 0 });
        }
    }

    /// Also record operator and indexing rows.
    pub fn enable_ops(&mut self) {
        self.ops = true;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn has_data(&self) -> bool {
        !self.stats.is_empty()
    }

    /// Copy for a `parmap` worker: same settings, no data, no root.
    pub fn for_worker(&self) -> Profiler {
        Profiler {
            enabled: self.enabled,
            ops: self.ops,
            whitelist: self.whitelist.clone(),
            ..Profiler::default()
        }
    }

    /// Whether `name` would appear in the report.
    fn reports(&self, name: &str) -> bool {
        self.whitelist.as_ref().map_or(true, |s| s.contains(name))
    }

    /// Start a profiled call. Returns false (and does nothing) when
    /// profiling is off; the caller then skips [`exit`](Self::exit).
    pub fn enter(&mut self, name: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if self.conv_base.is_none() {
            set_timing(true);
            self.conv_base = Some(conversion_counts());
        }
        self.stack.push(Frame { start: Instant::now(), child_ns: 0 });
        match self.active.get_mut(name) {
            Some(n) => *n += 1,
            None => {
                self.active.insert(name.to_string(), 1);
            }
        }
        true
    }

    /// Finish the call started by the matching [`enter`](Self::enter).
    pub fn exit(&mut self, name: &str, in_bytes: u64, out_bytes: u64) {
        let Some(frame) = self.stack.pop() else { return };
        let elapsed = frame.start.elapsed().as_nanos() as u64;
        let self_ns = elapsed.saturating_sub(frame.child_ns);
        match self.stack.last_mut() {
            Some(parent) => parent.child_ns += elapsed,
            None => {
                if let Some(root) = self.root.as_mut() {
                    root.child_ns += elapsed;
                }
            }
        }
        let outermost = match self.active.get_mut(name) {
            Some(n) => {
                *n -= 1;
                *n == 0
            }
            None => true,
        };
        if self.reports(name) {
            let s = self.row(name);
            s.call_count += 1;
            if outermost {
                s.total_ns += elapsed;
            }
            s.self_ns += self_ns;
            s.input_bytes += in_bytes;
            s.output_bytes += out_bytes;
        }
    }

    /// Start a pseudo-row for an operator. `[matmul]` is always recorded
    /// while profiling; other operators only with op profiling on.
    pub fn enter_op(&mut self, name: &'static str) -> OpFrame {
        if !self.enabled || (!self.ops && name != "[matmul]") {
            return None;
        }
        self.enter(name);
        Some(name)
    }

    /// Finish the operator row started by [`enter_op`].
    ///
    /// `in_bytes` and `out_bytes` are the stored sizes of the operands and
    /// the result, the same accounting as a function call. They are ignored
    /// when `frame` is `None`. `[index]` and `[index-assign]` pass 0: a
    /// subscript is not a bulk copy of the whole container.
    pub fn exit_op(&mut self, frame: OpFrame, in_bytes: u64, out_bytes: u64) {
        if let Some(name) = frame {
            self.exit(name, in_bytes, out_bytes);
        }
    }

    fn row(&mut self, name: &str) -> &mut FnStats {
        if !self.stats.contains_key(name) {
            self.stats.insert(name.to_string(), FnStats::default());
        }
        self.stats.get_mut(name).expect("inserted above")
    }

    /// Add the conversions since the last read to `[widen]` / `[promote]`.
    fn flush_conversions(&mut self) {
        let Some(base) = self.conv_base else { return };
        let now = conversion_counts();
        self.conv_base = Some(now);
        for (name, d) in [("[widen]", now.0 - base.0), ("[promote]", now.1 - base.1)] {
            if d.calls == 0 || !self.reports(name) {
                continue;
            }
            let s = self.row(name);
            s.call_count += d.calls;
            s.total_ns += d.ns.max(1);
            s.input_bytes += d.elements * 8;
            s.output_bytes += d.elements * 16;
        }
    }

    /// Drain a worker's rows (used per `parmap` task).
    pub fn take_stats(&mut self) -> HashMap<String, FnStats> {
        self.flush_conversions();
        std::mem::take(&mut self.stats)
    }

    /// Merge a `parmap` worker's rows as `parmap:<name>` parallel rows.
    pub fn merge_parallel(&mut self, worker: HashMap<String, FnStats>) {
        for (name, w) in worker {
            let key = format!("parmap:{name}");
            if !self.reports(&name) && !self.reports(&key) {
                continue;
            }
            let s = self.row(&key);
            s.call_count += w.call_count;
            s.total_ns += w.total_ns;
            s.self_ns += w.self_ns;
            s.input_bytes += w.input_bytes;
            s.output_bytes += w.output_bytes;
            s.parallel = true;
        }
    }

    /// Drain the stats and return rows sorted by inclusive time, descending.
    /// Closes the `[top-level]` row up to now and restarts it, so a
    /// mid-script `profile_report()` and the final report do not overlap.
    pub fn take_report(&mut self) -> Vec<(String, FnStats)> {
        self.flush_conversions();
        if self.whitelist.is_none() {
            if let Some(root) = self.root.as_mut() {
                let elapsed = root.start.elapsed().as_nanos() as u64;
                // A report from inside a call: the outermost open call is not
                // in `child_ns` yet, and its time so far is not top-level.
                let open_start = self.stack.first().map(|f| f.start);
                let open = open_start.map_or(0, |t| t.elapsed().as_nanos() as u64);
                let self_ns = elapsed.saturating_sub(root.child_ns + open);
                // Restart the root. If a call is open, start the root where
                // that call started: when it exits it adds its whole time to
                // `child_ns`, which then cancels exactly.
                *root = Frame { start: open_start.unwrap_or_else(Instant::now), child_ns: 0 };
                let s = self.row(TOP_LEVEL);
                s.call_count = 1;
                s.total_ns += elapsed;
                s.self_ns += self_ns;
            }
        }
        let mut rows: Vec<_> = std::mem::take(&mut self.stats).into_iter().collect();
        rows.sort_by(|a, b| b.1.total_ns.cmp(&a.1.total_ns).then_with(|| a.0.cmp(&b.0)));
        rows
    }
}

/// Print a profiling report to stderr.
/// Called automatically at script end if any data was collected.
pub fn print_report(rows: &[(String, FnStats)]) {
    let shown: Vec<_> = rows.iter().filter(|(_, s)| s.total_ns > 0 || s.self_ns > 0).collect();
    if shown.is_empty() {
        return;
    }

    let fn_col = shown.iter().map(|(n, _)| n.chars().count()).max().unwrap_or(8).max(8);

    eprintln!();
    eprintln!(
        "  {:<fn_col$}  {:>7}  {:>12}  {:>12}  {:>10}  {:>10}  {:>10}  {:>10}",
        "Function",
        "Calls",
        "Total (ms)",
        "Self (ms)",
        "Avg (µs)",
        "In (KB)",
        "Out (KB)",
        "Mbit/s",
        fn_col = fn_col
    );
    let sep = "─".repeat(fn_col + 2 + 7 + 2 + 12 + 2 + 12 + 2 + 10 + 2 + 10 + 2 + 10 + 2 + 10);
    eprintln!("  {sep}");

    let (mut calls, mut self_ns, mut bytes_in, mut bytes_out) = (0u64, 0u64, 0u64, 0u64);
    let mut any_parallel = false;
    for (name, s) in &shown {
        let avg_us = s.total_ns as f64 / s.call_count.max(1) as f64 / 1_000.0;
        let secs = s.total_ns as f64 / 1e9;
        let mbits = if secs > 0.0 {
            (s.input_bytes + s.output_bytes) as f64 * 8.0 / secs / 1e6
        } else {
            0.0
        };
        eprintln!(
            "  {:<fn_col$}  {:>7}  {:>12.3}  {:>12.3}  {:>10.3}  {:>10.2}  {:>10.2}  {:>10.1}",
            name,
            s.call_count,
            s.total_ns as f64 / 1e6,
            s.self_ns as f64 / 1e6,
            avg_us,
            s.input_bytes as f64 / 1024.0,
            s.output_bytes as f64 / 1024.0,
            mbits,
            fn_col = fn_col
        );
        if s.parallel {
            any_parallel = true;
            continue;
        }
        calls += s.call_count;
        self_ns += s.self_ns;
        bytes_in += s.input_bytes;
        bytes_out += s.output_bytes;
    }

    eprintln!("  {sep}");
    // The total is the sum of main-thread self times: the wall time covered
    // by the rows above when `[top-level]` is present.
    let secs = self_ns as f64 / 1e9;
    let mbits = if secs > 0.0 {
        (bytes_in + bytes_out) as f64 * 8.0 / secs / 1e6
    } else {
        0.0
    };
    eprintln!(
        "  {:<fn_col$}  {:>7}  {:>12.3}  {:>12.3}  {:>10}  {:>10.2}  {:>10.2}  {:>10.1}",
        "TOTAL",
        calls,
        self_ns as f64 / 1e6,
        self_ns as f64 / 1e6,
        "",
        bytes_in as f64 / 1024.0,
        bytes_out as f64 / 1024.0,
        mbits,
        fn_col = fn_col
    );
    if any_parallel {
        eprintln!("  parmap:* rows are CPU time summed across worker threads; TOTAL excludes them.");
    }
    eprintln!();
}
