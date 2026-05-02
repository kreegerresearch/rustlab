# Development Plan: Function Call Profiling

**Status:** Complete (both phases implemented in commit 6cff8e5)
**Primary trigger:** `profile(fft, myfun)` call in script
**Secondary trigger:** `rustlab run --profile script.rlab` CLI flag

---

## Overview

Add opt-in profiling that tracks, for a named set of functions (or all functions):
- Call count
- Total and average wall-clock time
- Input and output data volume (bytes)
- IO throughput (Mbit/s)

The report prints to stderr at end of script execution (or on-demand via
`profile_report()`).

```
─────────────────────────────────────────────────────────────────────────────
  Function     Calls    Total (ms)    Avg (µs)    In (KB)    Out (KB)    Mbit/s
─────────────────────────────────────────────────────────────────────────────
  fft             50      125.300    2 506.000    6 400.0    6 400.0     819.2
  normalize        5        0.120       24.000      500.0      500.0     333.3
  my_filter        1        0.040       40.000       64.0       64.0      25.6
─────────────────────────────────────────────────────────────────────────────
  TOTAL           56      125.460                                        819.0
─────────────────────────────────────────────────────────────────────────────
```

---

## Performance Impact Analysis

### Cost of recording one call
| Operation                      | Approx cost (macOS) |
|-------------------------------|---------------------|
| `std::time::Instant::now()`   | 10–20 ns            |
| Two Instant calls per function | 20–40 ns            |
| HashMap lookup + update        | 20–50 ns            |
| `value_bytes` sizing           | 2–5 ns              |
| **Total overhead per call**    | **~45–100 ns**      |

### Impact by function type
| Function           | Typical runtime | Overhead fraction |
|-------------------|-----------------|-------------------|
| `sin(scalar)`      | ~5 ns           | **10–20×** — severe |
| `abs(vector, n=64)`| ~300 ns         | ~25% — noticeable |
| `fft(n=1024)`      | ~50 µs          | 0.1% — negligible |
| User fn (moderate) | ~10 µs          | <1% — negligible  |

### Verdict
Tracking every function always is too expensive for tight loops.  The selective
`profile(fft, myfun)` approach solves this: only the named functions pay the
timing overhead.  `profile()` with no args opts into tracking everything — the
user accepts the cost.

---

## Design Decisions

### 1. `profile(fn1, fn2, ...)` — in-script selective profiling

`profile` is handled as an evaluator-level special case (like `arrayfun`),
not a registered builtin, because it modifies evaluator state.

```
profile(fft, myfun)     % track only these two
profile()               % track all function calls
profile_report()        % print current stats mid-script (optional)
```

Argument form: bare names (`Expr::Var`) or strings (`Expr::Str`) — both
accepted.  `profile(fft)` and `profile("fft")` are equivalent.

When `profile()` is called, profiling activates immediately for subsequent
calls.  The report prints automatically at script end if any profiling
happened; `profile_report()` can also print mid-script (and resets counters).

### 2. Is `--profile` still needed?

Yes — for running a script you don't want to touch.  But it is lightweight:
the CLI flag simply calls `ev.enable_profiling(None)` (track everything)
before running the script.  Most of the work is already in the evaluator.
Estimated additional CLI code: ~10 lines.

Keep `--profile`.

### 3. Lambda timing — track by variable name, suppress callbacks

**Direct lambda call** — `f = @(x) x^2; f(3)`:
- The call site is `Expr::Call { name: "f", args }` where `env["f"]` is a
  Lambda.  We already know the name `"f"` at dispatch time.
- Track under `"f"`, not `"<lambda>"`.  This is far more useful in the report.

**Lambda as callback** — `arrayfun(@sin, v)` or `apply_twice(sq, 2)`:
- The inner function (`sin`, `sq`) is invoked from `call_callable` or
  `eval_user_fn`, not from a direct `Expr::Call`.
- These inner calls must **not** be tracked separately — the outer function's
  wall time already includes them, and recording N individual `sin` calls
  would flood the report with noise.
- Implement via a `higher_order_depth: u32` counter in `Profiler`.  When
  any higher-order function (`arrayfun`, `eval_user_fn`, `eval_lambda_call`)
  is entered, it increments the counter.  Inner recordings are suppressed when
  depth > 0.  The outer function is recorded normally (it ran before
  incrementing depth from outside).

### 4. Selective vs full tracking

`Profiler` holds an `Option<HashSet<String>>` whitelist:
- `None` → track all functions
- `Some(set)` → track only named functions

When a call site is about to record, it first checks:
```rust
fn should_track(&self, name: &str) -> bool {
    self.enabled
        && self.higher_order_depth == 0
        && self.whitelist.as_ref().map_or(true, |s| s.contains(name))
}
```

---

## Architecture

### New file: `crates/rustlab-script/src/eval/profile.rs`

```rust
use std::collections::{HashMap, HashSet};

#[derive(Default, Clone)]
pub struct FnStats {
    pub call_count:   u64,
    pub total_ns:     u64,
    pub input_bytes:  u64,
    pub output_bytes: u64,
}

pub struct Profiler {
    enabled:            bool,
    whitelist:          Option<HashSet<String>>,  // None = all
    higher_order_depth: u32,
    stats:              HashMap<String, FnStats>,
}

impl Default for Profiler {
    fn default() -> Self {
        Self { enabled: false, whitelist: None, higher_order_depth: 0, stats: HashMap::new() }
    }
}

impl Profiler {
    /// Enable tracking. `names` = None means track all; Some(names) = whitelist.
    pub fn enable(&mut self, names: Option<Vec<String>>) {
        self.enabled   = true;
        self.whitelist = names.map(|v| v.into_iter().collect());
    }

    pub fn is_enabled(&self) -> bool { self.enabled }

    /// True when this call should be recorded.
    pub fn should_track(&self, name: &str) -> bool {
        self.enabled
            && self.higher_order_depth == 0
            && self.whitelist.as_ref().map_or(true, |s| s.contains(name))
    }

    pub fn enter_higher_order(&mut self) { self.higher_order_depth += 1; }
    pub fn exit_higher_order(&mut self)  { self.higher_order_depth  = self.higher_order_depth.saturating_sub(1); }

    pub fn record(&mut self, name: &str, elapsed_ns: u64, in_bytes: u64, out_bytes: u64) {
        let s = self.stats.entry(name.to_string()).or_default();
        s.call_count   += 1;
        s.total_ns     += elapsed_ns;
        s.input_bytes  += in_bytes;
        s.output_bytes += out_bytes;
    }

    /// Returns rows sorted by total time descending.  Resets stats.
    pub fn take_report(&mut self) -> Vec<(String, FnStats)> {
        let mut rows: Vec<_> = std::mem::take(&mut self.stats).into_iter().collect();
        rows.sort_by(|a, b| b.1.total_ns.cmp(&a.1.total_ns));
        rows
    }

    pub fn has_data(&self) -> bool { !self.stats.is_empty() }
}
```

### Changes to `eval/mod.rs`

**New field on `Evaluator`:**
```rust
profiler: profile::Profiler,   // always present, zero-cost when disabled
```

**New field on `UserFn`:**
```rust
name: String,   // populated from Stmt::FunctionDef so eval_user_fn can record it
```

**Public API additions:**
```rust
/// Enable profiling for all functions, or for a named subset.
pub fn enable_profiling(&mut self, names: Option<Vec<String>>) {
    self.profiler.enable(names);
}

/// True if profiling gathered any data (used by CLI to decide whether to print).
pub fn has_profile_data(&self) -> bool { self.profiler.has_data() }

/// Consume profiler data and return the report rows.
pub fn take_profile(&mut self) -> Vec<(String, profile::FnStats)> {
    self.profiler.take_report()
}
```

**Private helper:**
```rust
fn value_bytes(v: &Value) -> u64 {
    match v {
        Value::Scalar(_)  => 8,
        Value::Complex(_) => 16,
        Value::Vector(v)  => (v.len() * 16) as u64,
        Value::Matrix(m)  => (m.nrows() * m.ncols() * 16) as u64,
        _                 => 0,
    }
}
```

### Instrumented call sites

**1. Builtin calls** — extracted to a helper so both call sites share the logic:

```rust
fn call_builtin_tracked(&mut self, name: &str, vals: Vec<Value>) -> Result<Value, ScriptError> {
    if !self.profiler.should_track(name) {
        return self.builtins.call(name, vals);
    }
    let in_bytes: u64 = vals.iter().map(Self::value_bytes).sum();
    let t0     = std::time::Instant::now();
    let result = self.builtins.call(name, vals);
    let ns     = t0.elapsed().as_nanos() as u64;
    if let Ok(ref v) = result {
        self.profiler.record(name, ns, in_bytes, Self::value_bytes(v));
    }
    result
}
```

**2. User function calls** — wrap `eval_user_fn`:

```rust
fn eval_user_fn(&mut self, func: UserFn, args: Vec<Value>) -> Result<Value, ScriptError> {
    let tracking = self.profiler.should_track(&func.name);
    let in_bytes = if tracking { args.iter().map(Self::value_bytes).sum() } else { 0 };
    let t0       = if tracking { Some(std::time::Instant::now()) } else { None };

    self.profiler.enter_higher_order(); // suppress inner recordings
    // ... existing body (unchanged) ...
    self.profiler.exit_higher_order();

    if let (true, Some(t0)) = (tracking, t0) {
        let ns = t0.elapsed().as_nanos() as u64;
        self.profiler.record(&func.name, ns, in_bytes, Self::value_bytes(&ret_val));
    }
    Ok(ret_val)
}
```

**3. Lambda calls** — track under the *call-site variable name* (passed through):

Change `eval_lambda_call` signature to accept `call_name: &str`:
```rust
fn eval_lambda_call(
    &mut self,
    call_name:    &str,   // name of variable holding the lambda, or "" for callbacks
    params:       &[String],
    body:         &Expr,
    captured_env: HashMap<String, Value>,
    args:         Vec<Value>,
) -> Result<Value, ScriptError>
```

In `Expr::Call` dispatch for Lambda values, pass `name.as_str()` as `call_name`.
In `call_callable` (used by arrayfun), pass `""` as `call_name`.

Inside `eval_lambda_call`:
```rust
let tracking = !call_name.is_empty() && self.profiler.should_track(call_name);
let in_bytes = if tracking { args.iter().map(Self::value_bytes).sum() } else { 0 };
let t0       = if tracking { Some(std::time::Instant::now()) } else { None };
self.profiler.enter_higher_order();
// ... existing body ...
self.profiler.exit_higher_order();
if let (true, Some(t0)) = (tracking, t0) {
    self.profiler.record(call_name, t0.elapsed().as_nanos() as u64, in_bytes, Self::value_bytes(&result));
}
```

**4. `arrayfun` itself** — timed as a whole, before `enter_higher_order` is
called by `eval_lambda_call`/`eval_feval` on each element:

```rust
fn eval_arrayfun(&mut self, func: Value, input: Value) -> Result<Value, ScriptError> {
    let tracking = self.profiler.should_track("arrayfun");
    let in_bytes = if tracking { Self::value_bytes(&input) } else { 0 };
    let t0       = if tracking { Some(std::time::Instant::now()) } else { None };
    // ... existing body (inner calls go through call_callable with call_name="") ...
    if let (true, Some(t0)) = (tracking, t0) {
        self.profiler.record("arrayfun", t0.elapsed().as_nanos() as u64, in_bytes, Self::value_bytes(&result));
    }
    result
}
```

### `profile()` and `profile_report()` — evaluator-level special cases

In `eval_expr`, `Expr::Call` handler, before all other dispatch:

```rust
if name == "profile" {
    let names: Vec<String> = args.iter().map(|a| match a {
        Expr::Var(n) | Expr::Str(n) => Ok(n.clone()),
        _ => Err(ScriptError::Runtime("profile: expected function names".to_string())),
    }).collect::<Result<_, _>>()?;
    let whitelist = if names.is_empty() { None } else { Some(names) };
    self.profiler.enable(whitelist);
    return Ok(Value::None);
}
if name == "profile_report" && args.is_empty() {
    print_profile_report(&self.profiler.take_report());
    return Ok(Value::None);
}
```

`print_profile_report` is a free function (also used by the CLI) that formats
and writes the table to stderr.

---

## Phase 1 — Profiler Core

### Checklist

- [x] **1a.** Create `crates/rustlab-script/src/eval/profile.rs` with `FnStats`
  and `Profiler` as shown above.

- [x] **1b.** Add `pub mod profile;` and `profiler: profile::Profiler` field to
  `Evaluator`.  Add `UserFn.name: String`.  Populate name in `Stmt::FunctionDef`
  execution.

- [x] **1c.** Add `value_bytes(v: &Value) -> u64` private static method.

- [x] **1d.** Extract `call_builtin_tracked` wrapper; replace both bare
  `self.builtins.call(name, vals)` sites in `Expr::Call` and the one in
  `eval_feval`.

- [x] **1e.** Instrument `eval_user_fn` with enter/exit higher-order and timing.

- [x] **1f.** Add `call_name: &str` to `eval_lambda_call`; update all call sites
  (direct call passes variable name; `call_callable` passes `""`).  Add timing.

- [x] **1g.** Instrument `eval_arrayfun` with outer timing.

- [x] **1h.** Add `profile(...)` and `profile_report()` special-case dispatch at
  top of `Expr::Call`.

- [x] **1i.** Add public API: `enable_profiling`, `has_profile_data`,
  `take_profile`.

- [x] **1j.** Add `pub fn print_profile_report(rows: &[(String, FnStats)])` as a
  free function in `profile.rs`, writing to stderr.  Columns: Function | Calls |
  Total (ms) | Avg (µs) | In (KB) | Out (KB) | Mbit/s.  Omit zero-time rows.
  Include TOTAL row.

- [x] **1k.** In `Evaluator::run` (or the caller), auto-print the report at
  script end if `profiler.has_data()`.  This makes `profile(fft)` in a script
  self-contained without needing `profile_report()`.

- [x] **1l.** Unit tests in `tests.rs`:
  - `profile(fft)` in a script → `take_profile()` contains `fft` with correct
    call count and `total_ns > 0`.
  - `profile(fft)` does **not** appear for `sin` calls in the same script.
  - `profile()` tracks both `fft` and `sin`.
  - `f = @(x) x^2; f(3)` with `profile(f)` → tracked under `"f"`.
  - `arrayfun(@sin, v)` with `profile(arrayfun)` → `arrayfun` tracked;
    `sin` not tracked separately.
  - No overhead: default `Evaluator::new()` runs without profiling touching stats.

---

## Phase 2 — CLI Integration

### Checklist

- [x] **2a.** Add `--profile` flag to `RunArgs` in
  `crates/rustlab-cli/src/commands/run.rs`:
  ```rust
  #[arg(long, help = "Profile all function calls and print report on exit")]
  pub profile: bool,
  ```

- [x] **2b.** In the `run` command handler: if `args.profile`, call
  `ev.enable_profiling(None)` before running the script.

- [x] **2c.** After `ev.run(...)`, the auto-print in Phase 1k handles report
  output.  CLI adds nothing extra here — both `profile()` in-script and
  `--profile` share the same print path.

- [x] **2d.** Smoke-test: `rustlab run --profile examples/fft.rlab` — verify table
  appears on stderr, `fft` is in the top rows.

---

## Files Modified

| File | Phase | Change |
|---|---|---|
| `crates/rustlab-script/src/eval/profile.rs` *(new)* | 1 | `FnStats`, `Profiler`, `print_profile_report` |
| `crates/rustlab-script/src/eval/mod.rs` | 1 | field; instrumentation; `UserFn.name`; `profile()`/`profile_report()` dispatch; auto-print |
| `crates/rustlab-script/src/tests.rs` | 1 | Profiling tests |
| `crates/rustlab-cli/src/commands/run.rs` | 2 | `--profile` flag + `enable_profiling(None)` call |

---

## Summary of Design Decisions

| Question | Answer |
|---|---|
| Is `--profile` needed? | Yes — useful for un-modified scripts; ~10 lines of CLI code |
| Lambda tracking name | Variable name at call site (`f`, `myfun`), not `<lambda>` |
| Lambda as callback | Not tracked separately; outer function captures total time |
| How is suppression implemented | `higher_order_depth` counter in Profiler |
| Selective tracking | `Option<HashSet<String>>` whitelist; `None` = all |
| Report destination | stderr, auto-printed at script end if any data exists |
| `profile_report()` | Optional mid-script print + counter reset |
| Zero overhead when idle | `should_track()` short-circuits on `!enabled` |
