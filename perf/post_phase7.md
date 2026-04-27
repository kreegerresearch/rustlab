# Performance Report

| | |
|---|---|
| **Generated** | 2026-04-22 20:52:37 |
| **Version** | 0.1.9 |
| **Platform** | Darwin 25.3.0 arm64 |
| **Binary** | `target/release/rustlab` |

---

## Binary Size

| Build | Size |
|---|---|
| Release (with debug symbols) | 3.9M |
| After `strip` | 3.9M |

**Section sizes:**
```
__TEXT	__DATA	__OBJC	others	dec	hex
3817472	16384	0	4295278592	4299112448	1003f4000
```

---

## Benchmark Results

### `bench_builtins` — builtins

| | |
|---|---|
| **Wall time** | 270 ms |
| **Status** | PASS |

```
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
  sum:  275.59492083794964
mean  n=100000
  mean:  0.0027559492083794965
std   n=100000
  std:  0.9965547482715092
sort  n=100000
  out:  100000
done
```

### `bench_convolve` — convolve

| | |
|---|---|
| **Wall time** | 40 ms |
| **Status** | PASS |

```
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
| **Wall time** | 26 ms |
| **Status** | PASS |

```
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
done
```

### `bench_filter_design` — filter design

| | |
|---|---|
| **Wall time** | 23 ms |
| **Status** | PASS |

```
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
| **Wall time** | 22 ms |
| **Status** | PASS |

```
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
| **Wall time** | 24 ms |
| **Status** | PASS |

```
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
| **Wall time** | 21 ms |
| **Status** | PASS |

```
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
| Direct dependencies (rustlab-cli) | 11 |
| Total transitive dependency lines | 268 |

```
rustlab-cli v0.1.9 (/Users/mike/projects/2026/rustlab/crates/rustlab-cli)
├── anyhow v1.0.102
├── clap v4.6.0
├── ndarray v0.16.1
├── num-complex v0.4.6
├── rustlab-core v0.1.9 (/Users/mike/projects/2026/rustlab/crates/rustlab-core)
├── rustlab-dsp v0.1.9 (/Users/mike/projects/2026/rustlab/crates/rustlab-dsp)
├── rustlab-notebook v0.1.9 (/Users/mike/projects/2026/rustlab/crates/rustlab-notebook)
├── rustlab-plot v0.1.9 (/Users/mike/projects/2026/rustlab/crates/rustlab-plot)
├── rustlab-script v0.1.9 (/Users/mike/projects/2026/rustlab/crates/rustlab-script)
└── rustyline v14.0.0
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
   - Current unstripped size: 3.9M  stripped: 3.9M
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
   Total transitive dependency lines: 268
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
