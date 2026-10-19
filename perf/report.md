# Performance Report

| | |
|---|---|
| **Generated** | 2026-10-18 19:12:07 |
| **Version** | 0.3.10 |
| **Platform** | Darwin 25.6.0 arm64 |
| **Binary** | `target/release/rustlab` |

---

## Binary Size

| Build | Size |
|---|---|
| Release (with debug symbols) | 12M |
| After `strip` | 12M |

**Section sizes:**
```
__TEXT	__DATA	__OBJC	others	dec	hex
12091392	32768	0	4295344128	4307468288	100bec000
```

---

## Benchmark Results

### `bench_builtins` — builtins

| | |
|---|---|
| **Wall time** | 342 ms |
| **Status** | PASS |

```
rustlab 0.3.10 — interpreting /Users/mike/projects/2026/rustlab/perf/bench_builtins.rlab (.rlab)
N = 100000
abs   n=100000
  out:  100000
exp   n=100000
  out:  100000
log   n=100000 (positive input)
  out:  100000
sqrt  n=100000 (positive input)
  out:  100000
sin   n=100000
  out:  100000
cos   n=100000
  out:  100000
tanh  n=100000
  out:  100000
sum   n=100000
  sum:  0.06937722703459315
mean  n=100000
  mean:  0.0000006937722703459315
std   n=100000
  std:  1.0039273733430256
sort  n=100000
  out:  100000
done
```

### `bench_convolve` — convolve

| | |
|---|---|
| **Wall time** | 42 ms |
| **Status** | PASS |

```
rustlab 0.3.10 — interpreting /Users/mike/projects/2026/rustlab/perf/bench_convolve.rlab (.rlab)
convolve 256 * 64
  output length:  319
convolve 4096 * 256
  output length:  4351
convolve 48000 * 64
  output length:  48063
convolve 48000 * 512
  output length:  48511
done
```

### `bench_fft` — fft

| | |
|---|---|
| **Wall time** | 81 ms |
| **Status** | PASS |

```
rustlab 0.3.10 — interpreting /Users/mike/projects/2026/rustlab/perf/bench_fft.rlab (.rlab)
sr = 44100
n1 = 1024
FFT/IFFT n=1024
  round-trip length:  1024
n2 = 16384
FFT/IFFT n=16384
  round-trip length:  16384
n3 = 131072
FFT/IFFT n=131072
  round-trip length:  131072
n4 = 44100
FFT/IFFT n=44100 (non-pow2)
  round-trip length:  44100
n5 = 100003
FFT/IFFT n=100003 (prime)
  round-trip length:  100003
done
```

### `bench_filter_design` — filter design

| | |
|---|---|
| **Wall time** | 23 ms |
| **Status** | PASS |

```
rustlab 0.3.10 — interpreting /Users/mike/projects/2026/rustlab/perf/bench_filter_design.rlab (.rlab)
sr = 48000
fir_lowpass 64-tap hann
  taps:  64
fir_lowpass 512-tap hann
  taps:  512
fir_lowpass 1024-tap hann
  taps:  1024
fir_lowpass_kaiser 60dB
  taps:  175
fir_lowpass_kaiser 80dB
  taps:  243
firpm 63-tap lowpass
  taps:  63
firpm 127-tap lowpass
  taps:  127
butterworth_lowpass order 4
  coeff count:  5
butterworth_lowpass order 8
  coeff count:  9
done
```

### `bench_interpreter` — interpreter

| | |
|---|---|
| **Wall time** | 24 ms |
| **Status** | PASS |

```
rustlab 0.3.10 — interpreting /Users/mike/projects/2026/rustlab/perf/bench_interpreter.rlab (.rlab)
scalar loop 10000 iterations
  result:  50005000
indexed assign build n=1000
  v(1000):  2000
deep expression chain n=500
  out:  500
1000 calls to len()
  total:  64000
done
```

### `bench_linalg` — linalg

| | |
|---|---|
| **Wall time** | 25 ms |
| **Status** | PASS |

```
rustlab 0.3.10 — interpreting /Users/mike/projects/2026/rustlab/perf/bench_linalg.rlab (.rlab)
Matrix multiply 64x64
  result size:  [1×2]  64.000000  64.000000
Matrix multiply 256x256
  result size:  [1×2]  256.000000  256.000000
inv 64x64
  result size:  [1×2]  64.000000  64.000000
eig 32x32
  eigenvalue count:  32
done
```

### `bench_upfirdn` — upfirdn

| | |
|---|---|
| **Wall time** | 28 ms |
| **Status** | PASS |

```
rustlab 0.3.10 — interpreting /Users/mike/projects/2026/rustlab/perf/bench_upfirdn.rlab (.rlab)
sr = 48000
n1 = 256
Workload 1: n=256, h=512-tap, 4x interp
  output length:  1532
n2 = 48000
Workload 2: n=48000 (1s), h=64-tap, 3x decimate
  output length:  16021
n3 = 44100
Workload 3: n=44100 (1s at 44.1kHz), h=128-tap, SRC 3/2
  output length:  66213
done
```

---

## Dependency Summary

| | |
|---|---|
| Direct dependencies (rustlab-cli) | 14 |
| Total transitive dependency lines | 358 |

```
rustlab-cli v0.3.10 (/Users/mike/projects/2026/rustlab/crates/rustlab-cli)
├── anyhow v1.0.102
├── clap v4.6.0
├── ndarray v0.16.1
├── num-complex v0.4.6
├── rustlab-cache v0.3.10 (/Users/mike/projects/2026/rustlab/crates/rustlab-cache)
├── rustlab-config v0.3.10 (/Users/mike/projects/2026/rustlab/crates/rustlab-config)
├── rustlab-core v0.3.10 (/Users/mike/projects/2026/rustlab/crates/rustlab-core)
├── rustlab-dsp v0.3.10 (/Users/mike/projects/2026/rustlab/crates/rustlab-dsp)
├── rustlab-plot v0.3.10 (/Users/mike/projects/2026/rustlab/crates/rustlab-plot)
├── rustlab-script v0.3.10 (/Users/mike/projects/2026/rustlab/crates/rustlab-script)
├── rustyline v14.0.0
├── serde v1.0.228
└── serde_json v1.0.149
[dev-dependencies]
└── tempfile v3.27.0
```

---

## Known Optimisation Opportunities

These were identified during the initial audit in `perf/performance.md`.
An AI agent reviewing this report should check whether each item is still
open, estimate its impact given the numbers above, and prioritise them.

| ID | Change | Expected saving | Status |
|----|--------|----------------|--------|
| OPT-1 | `zip`: `default-features = false, features = ["deflate"]` | ~300–500 KB | **open** |
| OPT-2 | `ndarray`: remove `features = ["rayon"]` (unused) | ~150–250 KB | **open** |
| OPT-3 | `[profile.release]` — `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"` | ~400–800 KB + runtime | **open** |

---

<!-- AI_ANALYSIS

You are an AI assistant reviewing a rustlab performance report.
Produce a section headed "## AI Analysis" and append it to this file.

Work through these tasks in order:

1. BINARY SIZE
   - Current unstripped size: 12M  stripped: 12M
   - Are OPT-1, OPT-2, OPT-3 still open?  Apply whichever are still pending
     and re-measure, or estimate the new size if you cannot rebuild.
   - Flag any new dependencies added since the last report that have not
     been audited for unnecessary default features.

2. BENCHMARK TIMINGS
   Thresholds for concern (flag if exceeded):
   - bench_upfirdn total:        > 500 ms
   - bench_fft total:            > 100 ms
   - bench_linalg total:         > 200 ms
   - bench_convolve total:       > 800 ms
   - bench_filter_design total:  > 600 ms
   - bench_builtins total:       > 300 ms
   - bench_interpreter total:    > 2000 ms
   For each benchmark that exceeds its threshold, suggest a specific next
   profiling step (e.g. samply, flamegraph, criterion micro-benchmark).

3. DEPENDENCY GROWTH
   Total transitive dependency lines: 358
   If this is > 400 lines, identify the largest contributor among direct
   deps and suggest whether its feature flags can be trimmed further.

4. ALGORITHMIC SUGGESTIONS
   Review the bench script outputs above.  If any workload output looks
   wrong (e.g. unexpected lengths, NaN values) flag it.  Otherwise suggest
   one concrete algorithmic improvement for the slowest benchmark.
   For bench_interpreter specifically: if the scalar loop time is more than
   10x the equivalent C loop time (estimate ~1ms for 10K additions), flag
   interpreter dispatch overhead and suggest a JIT or bytecode compilation path.

5. SUMMARY TABLE
   End with a markdown table:
   | Recommendation | Priority | Effort | Expected gain |
   listing all findings from steps 1-4, highest priority first.

-->

## AI Analysis

*2026-10-18, on `feature/perf-iter01` (0.3.10, PR #77), Apple M4 Max. The report above is the second of two `make perf` runs. All seven benchmarks PASS on both runs, and every output checks out (step 4).*

### 1. Binary size

| Build | Size |
|---|---:|
| `main` (0.3.9, `65d6507`) | 11,905,008 B (11.91 MB) |
| This branch | 12,500,960 B (12.50 MB) |
| Difference | **+595,952 B (+5.0 %)** |

- **The OPT items are not open.** All three are already applied in `Cargo.toml`. `zip` uses `default-features = false, features = ["deflate"]` (OPT-1). `ndarray` has no `rayon` feature (OPT-2). `[profile.release]` sets `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"` (OPT-3). The table above still says **open** because `perf/run_perf.sh` hardcodes it; that table should be updated or removed. "Stripped = unstripped = 12M" follows from OPT-3: the release binary is already stripped.
- **No new dependencies.** Between `main` and this branch, the only manifest change is the version bump. `matrixmultiply` was already linked through `ndarray`; using its `dgemm` alongside `zgemm` adds 14 KB (`cargo bloat --crates`).
- **Where the growth comes from:** this branch's own code in `rustlab_script`. The largest visible piece is the elementwise kernels (`elementwise::zip_arrays` 69 KB, `elementwise::binop` 40 KB, plus what is inlined into callers). They are monomorphized per operator × operand ownership × {Vector, Matrix} × {sequential `Zip`, parallel slice}. The call-stack profiler, the borrowed-index path and `gelu_grad` add the rest. Per the "keep the rustlab binary small" rule, this deserves a follow-up. Using the slice-chunk loop for small contiguous arrays too, run sequentially, would remove the separate `Zip` copies. A `cargo bloat` diff against `main` should guide it.

### 2. Benchmark timings

| Benchmark | Run 1 | Run 2 | Threshold | Warm, `main` | Warm, branch |
|---|---:|---:|---:|---:|---:|
| `bench_builtins` | 335 ms | 342 ms | 300 ms | 13.8 ms | 13.8 ms |
| `bench_convolve` | 41 ms | 42 ms | 800 ms | 23.1 ms | 23.4 ms |
| `bench_fft` | 32 ms | 81 ms | 100 ms | 62.9 ms | 63.9 ms |
| `bench_filter_design` | 21 ms | 23 ms | 600 ms | 4.0 ms | 3.9 ms |
| `bench_interpreter` | 22 ms | 24 ms | 2000 ms | 5.6 ms | 5.7 ms |
| `bench_linalg` | 25 ms | 25 ms | 200 ms | 9.1 ms | 6.7 ms |
| `bench_upfirdn` | 20 ms | 28 ms | 500 ms | 7.2 ms | 6.9 ms |

("Warm" is the median of 7 runs per binary after a warm-up launch, both binaries timed side by side.)

- **`bench_builtins` exceeds its threshold on both runs, but it is a harness artifact, not a regression.** It is the first benchmark `run_perf.sh` runs after `cargo build` writes a new binary. On macOS the first launch of a newly written binary costs about 310 ms (a fresh copy measured 325 ms on first launch, 16 ms on the second). The script itself takes 14 ms warm on both `main` and this branch.
  - **Next step:** fix the harness, not the code. Add a warm-up launch (`"$BINARY" --version >/dev/null`) after the build in `run_perf.sh`, or time the median of several runs. Until then, a fresh-build `make perf` will always put `bench_builtins` near or over 300 ms.
- **The other six are well under their thresholds.** `bench_fft` varies (32 vs 81 ms) with machine state; warm it is 63–64 ms on both binaries.
- **Branch vs `main`:** no regression. `bench_linalg` is 26 % faster (the real `dgemm` path for `A * B`, R1). Everything else is within ±5 %, which is noise at these durations.

### 3. Dependency growth

358 transitive dependency lines, under the 400-line trigger. No direct dependency was added. Nothing to trim.

### 4. Correctness of outputs, and an algorithmic suggestion

All outputs check out:
- **`convolve`:** `len(x) + len(h) − 1` in all four cases (319, 4351, 48063, 48511).
- **FFT:** every round trip returns its input length, including 44,100 and the prime 100,003.
- **`upfirdn`:** lengths match `⌈((n − 1)·p + len(h)) / q⌉` (1532, 16021, 66213).
- **Interpreter scalar loop:** Σ(1..10000) = 50005000.
- **Statistics:** `randn` mean ≈ 7e−7 and std ≈ 1.004 at n = 1e5, as expected.
- **Sizes and counts:** the `eig` count, `firpm` / Kaiser tap counts and matrix sizes are as expected.

**The slowest benchmark is `bench_fft`, and its cost is the non-power-of-two lengths:**

| n | `fft` time | ns per n·log₂n |
|---:|---:|---:|
| 1,024 | 0.013 ms | 1.3 |
| 16,384 | 0.267 ms | 1.2 |
| 131,072 | 2.60 ms | 1.2 |
| 44,100 | 7.85 ms | 11.5 |
| 100,003 (prime) | 16.8 ms | 10.1 |

Both non-power-of-two cases use Bluestein. It pads to the next power of two ≥ 2n − 1 (131,072 and 262,144) and runs three FFTs of that size, so it costs about 10× a radix-2 transform per n·log₂n.

**Suggestion:** hand-roll a mixed-radix Cooley–Tukey for 2, 3, 5 and 7 factors, keeping it pure Rust per Rule 10. It would handle smooth lengths directly. 44,100 = 2²·3²·5²·7² is the common audio case; it should drop from about 7.8 ms to about 1 ms. Keep Bluestein for lengths with a large prime factor, and cache each length's chirp and the chirp's FFT, which is one of the three transforms and is recomputed every call. That saves about a third for repeated prime-length calls.

**`bench_interpreter`:** by the 10K-iteration rule it is **not flagged**. The whole script takes 5.7 ms warm, including process start, against the ~10 ms that "10 × C" allows. The `rustlab_llm` harness's `prim.scalar_loop_1e5` (17.4 ms per 1e5 iterations, about 170 ns per `acc = acc + k`) shows the same overhead at a larger scale. The first step is the deferred value-borrow refactor (roadmap C11). The obvious remaining costs are per-statement `Value` clones and hash-map variable lookups. A bytecode path is the larger option, worth weighing only after C11 is measured. This branch's R2 profiler (`--profile=ops`) can now attribute that time.

### 5. Summary

| Recommendation | Priority | Effort | Expected gain |
|---|---|---|---|
| Warm-up launch (or median of N) in `run_perf.sh` | High | Trivial | Removes the ~310 ms first-launch artifact that puts `bench_builtins` over its 300 ms threshold on every fresh build |
| Update the stale OPT-1/2/3 "open" table in `run_perf.sh` | High | Trivial | The report stops asking for work that is already done |
| Shrink elementwise monomorphization (`cargo bloat` diff against `main`) | Medium | Small | Recover part of the +596 KB (+5.0 %) binary growth |
| Mixed-radix (2/3/5/7) FFT; cache Bluestein chirp plans | Medium | Medium | `fft(44100)` ~7.8 → ~1 ms; prime lengths ~⅓ faster when repeated |
| Roadmap C11 value-borrow refactor, measured with `--profile=ops` | Medium | Large | Interpreter dispatch (~170 ns per scalar statement) and parmap copy overhead |

### Addendum: harness fixes (same day)

The first two summary rows are done in `perf/run_perf.sh`:
- **Warm-up launch:** `"$BINARY" --version` runs once after the build, before any benchmark is timed.
- **OPT table:** OPT-1/2/3 are marked done, and step 1 of the analysis prompt now asks to confirm they are still applied and to attribute size growth with `cargo bloat`.
- **Strip copy:** the temp file is now made with `mktemp` under `$TMPDIR`. The hardcoded `/tmp` path failed under a sandboxed shell.

**Check:** I replaced `target/release/rustlab` with a fresh copy, which reproduces what a rebuild does, then ran `make perf`. `bench_builtins` read **31 ms**, against 335–342 ms in the two runs above; the other six were in the same range as before. The tables above are from the script before the fix and are kept as the record this analysis describes. The next `make perf` will regenerate this file.

### Addendum: binary size growth reduced (same day)

The third summary row (shrink the elementwise monomorphization) is done. `cargo bloat`, comparing `main` with this branch function by function, put **309 KB of the growth in rayon itself**: `rayon_core` +235 KB, `rayon` +74 KB. Every parallel loop with its own closure type got its own copy of rayon's job, join and bridge code, and the kernels had one closure per operator × operand ownership × {Vector, Matrix}. Another ~110 KB was the sequential `Zip` copies of the same kernels (`zip_arrays` 71 KB, `binop` 41 KB).

`eval/elementwise.rs` now passes every kernel as a `&dyn Fn` over one chunk to one of two non-generic drivers. The drivers run the whole slice when the array is small and use rayon chunks when it is large. There is one kernel body per operator, with no sequential duplicate, and arrays not in row-major layout fall back to `Value::binop`.

| Binary | Size | vs `main` |
|---|---:|---:|
| `main` (0.3.9) | 11,905,008 B | — |
| Branch before | 12,500,960 B | +595,952 B (+5.0 %) |
| Branch after | 12,054,256 B | **+149,248 B (+1.3 %)** |

After the change, rayon accounts for +12 KB of the growth (was +309 KB) and the elementwise kernels for about 36 KB. The remaining ~117 KB of `.text` is new functionality: the call-stack profiler, the borrowed-index path, `gelu_grad` and the real matmul, plus ~13 KB of extra inlining from moving the index-assign arm into its own function.

Speed is unchanged, with the gate passing: training step 8,395–8,679 tok/s, `flat.adamw_811k` 4.5 ms, `flat.axpy_811k` 2,928–3,019 Melem/s. The bit-exact operator tests pass, plus a new one for the non-contiguous fallback.
