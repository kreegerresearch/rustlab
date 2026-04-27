# Performance & Binary Size

Audit and recommendations document. Numbers in `## Current binary size`
and `## Expected impact` are baseline-era estimates kept for historical
context — the live, regenerated measurements live in
[`report.md`](report.md), produced by `make perf` (or
`bash perf/run_perf.sh`).

| | |
|---|---|
| **Document scope** | dependency audit + release-profile recommendations |
| **Audit baseline** | v0.1.1, Apple M-series arm64 |
| **Current version** | 0.1.12 |
| **Latest live numbers** | see [`perf/report.md`](report.md) |

---

## Running the benchmarks

All benchmarks are `.r` scripts in this directory. The recommended path
is `make perf`, which builds release, runs every `bench_*.r`, measures
binary size, and rewrites `perf/report.md`:

```sh
make perf
```

To run an individual workload by hand:

```sh
cargo build --release
time ./target/release/rustlab run perf/bench_upfirdn.r
time ./target/release/rustlab run perf/bench_fft.r
time ./target/release/rustlab run perf/bench_linalg.r
```

For more detailed profiling on macOS use `Instruments` or `samply`:

```sh
# samply (cargo install samply)
samply record ./target/release/rustlab run perf/bench_upfirdn.r

# hyperfine for statistical wall-time (cargo install hyperfine)
hyperfine --warmup 3 './target/release/rustlab run perf/bench_upfirdn.r'
```

---

## Benchmark scripts

| Script | What it measures |
|---|---|
| `bench_builtins.r` | element-wise builtins (`abs`, `exp`, `log`, `sqrt`, `sin`, `cos`, `tanh`, `sum`, `mean`, `std`, `sort`) at n=100 000 |
| `bench_convolve.r` | direct convolution at four signal × kernel sizes |
| `bench_fft.r` | FFT/IFFT round-trip — 1 K, 16 K, 128 K points |
| `bench_filter_design.r` | FIR (`hann`, Kaiser, Parks–McClellan) and IIR Butterworth design |
| `bench_interpreter.r` | scalar loop, indexed assignment, deep expression chain, function-call overhead |
| `bench_linalg.r` | matrix multiply, inverse, and eigenvalues at 32–256 size |
| `bench_upfirdn.r` | polyphase upfirdn — three signal sizes and rate ratios |

> The DSP-heavy `Tensor3` work added in v0.1.11 / v0.1.12 is captured in
> `baseline_pre_tensor3.md`, `post_phase4.md`, and `post_phase7.md`.
> Compare those snapshots if you want to track regressions across the
> tensor3 sweep.

---

## Current binary size

| Build | Size |
|---|---|
| `cargo build --release` (audit baseline, v0.1.1) | **4.3 MB** |
| After `strip` (audit baseline) | **3.7 MB** |

For up-to-date sizes (after OPT-1/2/3 below) see
[`report.md`](report.md) → "Binary Size".

---

## Dependency audit

### `zip` — biggest avoidable cost (✅ applied)

`zip`'s default feature set includes every compression codec it supports:

```
default = [aes-crypto, bzip2, deflate64, deflate, lzma, time, zstd, xz]
```

The codebase uses **only `Stored` (no compression) for writing** and needs
**only deflate for reading** Python-generated `.npz` files (NumPy uses
`Compression.ZIP_DEFLATED`). Everything else is dead weight:

| Pulled in by zip defaults | Used? |
|---|---|
| `aes-crypto` (AES, HMAC, PBKDF2, SHA-1) | No |
| `bzip2` | No |
| `deflate64` | No |
| `lzma` / `xz` | No |
| `zstd` | No |
| `deflate` (flate2) | **Yes** — for reading .npz |
| `time` | No |

**Fix:** pin zip to `default-features = false, features = ["deflate"]`.
**Status:** applied in `Cargo.toml` (workspace dependency).

### `rayon` — was enabled but unused (✅ removed)

`ndarray` was previously declared with `features = ["rayon"]`, which
compiled the full `rayon` thread-pool runtime into every crate that
depends on `ndarray`. No code in the project calls `par_iter`,
`par_azip`, or any other parallel ndarray operation.

**Fix:** remove the `rayon` feature from the `ndarray` workspace dependency.
**Status:** applied — `ndarray = "0.16"` (no features) in `Cargo.toml`.

### Release profile (✅ applied)

The workspace originally had no `[profile.release]` section, so it used
Rust defaults: `opt-level = 3`, `lto = false`, `codegen-units = 16`, no
stripping. Without LTO the linker cannot inline or dead-strip across
crate boundaries.

The current workspace `Cargo.toml`:

```toml
[profile.release]
opt-level     = 3
lto           = "thin"
codegen-units = 1
strip         = "symbols"
```

**Status:** applied.

---

## Recommended changes (status)

| ID | Change | Status |
|----|--------|--------|
| OPT-1 | `zip`: `default-features = false, features = ["deflate"]` | **applied** |
| OPT-2 | `ndarray`: remove `features = ["rayon"]` (unused) | **applied** |
| OPT-3 | `[profile.release]` — `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"` | **applied** |

Historical guidance below is kept for the next time we trim deps.

### 1. Trim `zip` features

In `Cargo.toml` (`[workspace.dependencies]`):

```toml
# Before
zip = "2"

# After
zip = { version = "2", default-features = false, features = ["deflate"] }
```

Removes: `aes`, `bzip2`, `deflate64`, `hmac`, `lzma`, `pbkdf2`, `sha1`,
`xz`, `zstd`, `zopfli`, `getrandom`, `zeroize` and their transitive deps.

### 2. Remove unused `rayon` from `ndarray`

In `Cargo.toml` (`[workspace.dependencies]`):

```toml
# Before
ndarray = { version = "0.16", features = ["rayon"] }

# After
ndarray = "0.16"
```

Removes the `rayon` thread-pool from all crates that depend on `ndarray`.
If parallel matrix operations are added in future, re-enable it only in
the crate that needs it.

### 3. Add a release profile

In the workspace `Cargo.toml`:

```toml
[profile.release]
opt-level     = 3       # already the default, explicit for clarity
lto           = "thin"  # cross-crate dead-code elimination and inlining
codegen-units = 1       # single CGU lets LLVM see the whole program
strip         = "symbols"  # drop debug symbols from the installed binary
```

`lto = "thin"` gives most of the binary-size and speed benefit of full
LTO with much faster link times. Use `lto = true` (fat LTO) for the
smallest possible binary at the cost of longer release builds.

`panic = "abort"` can also be added to remove the unwinding machinery
(saves ~50–100 KB), but check that any code relying on `catch_unwind`
still works — `rustyline` uses it internally for the REPL, so leave this
out unless you verify it is safe.

### 4. Install-time stripping (`make install`)

The Makefile copies the binary; `strip = "symbols"` in the profile
handles this automatically. Stripping in the Makefile is therefore
unnecessary, but the macOS `codesign --sign - --force` step is still
useful so the binary launches cleanly from `~/.local/bin/`.

---

## Expected impact (audit-era estimates)

These were estimates based on the dependency tree audit at v0.1.1 and
have all been applied. For current measurements compare
`perf/report.md` against the v0.1.1 baseline numbers (4.3 MB unstripped /
3.7 MB stripped).

| Change | Expected size reduction |
|---|---|
| Trim `zip` features | ~300–500 KB |
| Remove `rayon` | ~150–250 KB |
| Add `lto = "thin"` | ~200–400 KB (also improves runtime) |
| `strip = "symbols"` | ~600 KB |
| All four combined | **~1.3–1.7 MB** off the v0.1.1 4.3 MB |

Audit-era projection: **~2.6–3.0 MB** installed binary.

---

## New work since the original audit (v0.1.2 → v0.1.12)

The benchmark suite has grown well beyond the three original workloads.
Each major sweep landed with a snapshot in this directory:

| Snapshot | Sweep |
|---|---|
| `sparse_solve_phase1to4.md` | hand-rolled sparse Cholesky + sparse LU + AMD ordering (`em_requests` Item 2) |
| `baseline_pre_tensor3.md` | reference run before the `Tensor3` plan |
| `post_phase4.md` | `Tensor3` Phase 4 (`laplacian_3d`, `ij2k`/`k2ij` index helpers) |
| `post_phase7.md` | `Tensor3` Phase 7 closing report |

When a sweep adds new builtins, append them to `bench_*.r` (or add a new
`bench_<feature>.r`) so the next `make perf` picks them up automatically.

---

## Future considerations

- **`clap`** pulls in a formatting and help-generation system; if the CLI
  surface grows, consider `clap` with `default-features = false` and only
  the features you use (`derive`, `help`, `error-context`).
- **`plotters`** is already pared down with `default-features = false` — good.
- **`ratatui` / `crossterm`** are the terminal-rendering stack; they are
  well-scoped and unlikely to be a size issue.
- Pure-Rust hand-rolled sparse solvers (`crates/rustlab-core/src/sparse_solve/`)
  and sparse eigensolvers (`crates/rustlab-core/src/sparse_eig/`) keep us
  off `ndarray-linalg` / BLAS / LAPACK. Per AGENTS.md Rule 9, any future
  proposal to bring those in needs an explicit trade-off study.
- If `bench_interpreter`'s scalar-loop time creeps past ~1 ms / 10 K
  additions, that is the signal to revisit interpreter dispatch (bytecode
  compile pass, JIT, or specialised numeric fast path).
