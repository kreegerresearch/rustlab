# Examples & Notebooks Bug Hunt — Test Plan

---

## ROUND 3 — plotting/rendering + I/O layer hunt (2026-06-17→18, FIXED)

**Resolution (2026-06-18):**
- **IO1/IO2 (P0)** NPY `fortran_order` + big-endian now honored (`parse_npy_bytes`/
  `array_to_value`); **IO3 (P1)** TOML Matrix round-trips as a Matrix; **IO4 (P1)** CSV
  scientific-notation complex parses; NPY v2 header also handled. Tests:
  `round3_io_regressions` (4).
- **PL2 (P0)** `escape_js` now escapes `<` (blocks `</script>` breakout/injection;
  `>`/`&` left so labels like `|00>` aren't churned). **PL3 (P1)** heatmap auto-range is
  finite-only (Inf no longer collapses the scale; NaN → neutral gray in SVG / transparent
  in the RGBA viewer path). **PL4 (P1)** a 2-D plot now clears a stale `surface`
  (`clear_2d_overlays`), so `surf` no longer shadows later plots. **PL5a (P1)** constant-Y
  axis opens a real window instead of all-identical ticks.
- **PL1 (P0)** heatmap SVG bloat: the intended fix (embed a raster `<image>`) is **not
  possible** — plotters' SVG backend has no raster embed and `BitMapElement` falls back to
  one rect *per pixel* (worse). Instead the renderer now **coarsens dense heatmaps in both
  dimensions** to a cell budget (`HEATMAP_CELL_BUDGET = 120k`), bounding every heatmap SVG
  to ≤~8 MB (was: scalogram 109 MB, 256-scale 33 MB, mandelbrot 40 MB) while preserving
  orientation/coverage; the returned matrix is untouched. The Round-2b column-only
  decimation in `push_db_heatmap` was reverted in favour of this both-dimension cap.
- **Deferred:** PL5b (sci-notation tick labels for 1e±300 data — exotic cosmetic). True
  full-resolution heatmap rasters would need either a PNG figure-output path or manual
  SVG `<image>` injection (SVG-backend-specific) — a separate, larger effort.
- **Resolved 2026-07-20** (branch `bug-run/1xn-plot-args`): **PL6** — plot/scatter/bar
  now error on x/y length mismatch instead of silently zipping to the shorter length.
  **IO6** — `save` refuses struct fields named `__tensor3_*`; `load` only rebuilds a
  Tensor3 from a table containing *exactly* the two reserved keys (extra keys → plain
  struct, nothing dropped; malformed payloads no longer lose the reserved fields).
  Bonus regression found while sweeping examples: the Round-2 **E12** strict-p fix broke
  `norm(M, "fro")` (bench_parmap.rlab) — `norm` now accepts `"fro"`/`"inf"` strings on
  all value shapes.

### Original findings (for reference; NOT yet fixed at time of writing)

Audited the two least-covered implementation layers (the bug behind the time_frequency
gallery item lived here). All findings VERIFIED with repros.

### Plotting / rendering
- **PL1 (P0)** — heatmap SVG export draws **one `<rect>` per cell** (O(N·M)): `imagesc(rand(300,300))`
  → 7.7 MB / 90k rects; mandelbrot gallery plots are 40 MB each. The Round-2b time_frequency
  fix only decimated *columns* (not rows), so `scalogram(x,fs,"morlet",256)` still makes a
  33 MB / 384k-rect SVG. The RGBA raster path `render_heatmap_cells_to_rgba` (file.rs:192)
  already exists but the SVG/HTML export never uses it. **Proper fix:** embed heatmaps as a
  single rasterized `<image>` (needs a small PNG-encoder dep); then revert the decimation hack.
  Affects imagesc/heatmap/image/spectrogram/scalogram/surf/contourf.
- **PL2 (P0)** — HTML/Plotly backend doesn't HTML-escape `<`/`&` in title/label/legend; a
  `</script>` breaks the inline `<script>` (chart never renders + stored-injection vector).
  `escape_js` (html.rs:764) only handles `\ " \n`. (SVG backend escapes correctly.)
- **PL3 (P1)** — NaN heatmap cells paint as the colormap MAX color (look like real maxima);
  a single Inf collapses every finite cell to the MIN color. Auto-range uses `f64::min/max`
  (Inf-propagating) and `colormap_rgb(NaN)` falls through to the t=1 endpoint. (HTML correct.)
- **PL4 (P1)** — a 3-D `surf()` permanently shadows later 2-D plots on the same subplot
  (`SurfaceData` never cleared on replace).
- **PL5 (P1)** — constant-Y data (`plot([1,2,3],[5,5,5])`) collapses the SVG Y axis (all ticks
  read 5.0); huge-magnitude values render ~300-digit fixed-point tick labels (no sci-notation).
- **PL6 (P2)** — plot/scatter silently zip mismatched x/y to the shorter length.

### I/O / serialization
- **IO1 (P0)** — NPY reader ignores `fortran_order`: a numpy-saved F-order array loads
  transposed (`[[1,2],[3,4]]`→`[[1,3],[2,4]]`). `parse_npy_bytes`/`array_to_value`
  (builtins.rs:5682,5707). Self-round-trips are safe (writer only emits C-order).
- **IO2 (P0)** — NPY reader accepts big-endian dtype (`>f8`/`>c16`) but decodes with
  `from_le_bytes` → garbage, no error (builtins.rs:5717-5744).
- **IO3 (P1)** — TOML round-trip turns a Matrix into a Tuple of vectors (`size` then errors).
  `value_to_toml` writes nested arrays; `array_to_value` only rebuilds Matrix from a flat
  array (toml_io.rs:63,216). Tensor3 round-trips fine (asymmetry).
- **IO4 (P1)** — CSV complex parser mis-splits scientific notation (`1.5e-3+2.5e-4i` →
  "invalid real part"): the real/imag sign scan catches the exponent's `-` (builtins.rs:5779).
- **IO5 (P2)** — NPY v2.0+ headers unsupported (rare, clean failure). **IO6 (P2)** — TOML
  reserved-key (`__tensor3_*`) collision silently rebuilds a user struct as a Tensor3.

Verified clean: Touchstone .sNp (all formats/units/ports/noise), NPY writer (spec-conformant),
NPZ multi-array, CSV real round-trips, TOML scalars/arrays/nested/Tensor3; polar negative-r,
loglog/semilog ≤0 rejection, surf/contour z-orientation, contour levels, quiver, meshgrid,
colormap endpoints, RGB clamping, SVG label escaping, degenerate-input crash probes.

---

**Goal:** verify every calculation and every plot/figure across all `.rlab` examples
and all notebook `.md` files is accurate. Catalogue issues with a minimal repro so
fixes can be reviewed and applied area-by-area.

---

## ROUND 2 — implementation-level bug hunt (2026-06-16, ALL FIXED)

The first round audited example/notebook OUTPUTS. Round 2 audited the **builtin
implementations** and **edge cases** that no example exercises. All findings below were
VERIFIED (reproduced + cross-checked against textbook truth) and are now **FIXED** on
`fix/examples-notebooks-bug-hunt`, each with a regression test. The whole workspace test
suite is green except the same 7 pre-existing environmental failures (TCP-port binding +
missing `tidy`/`chktex`). Resolution summary:

- **E1 (P0)** eig/roots `xⁿ−c` → EISPACK exceptional shift in `eig_hessenberg`
  (`builtins.rs`); `roots(x³−1)` now returns the cube roots of unity.
- **E4** `eigs("sm")` → shift-and-invert via `SparseLU` (`sparse_eig/mod.rs`,
  `run_shift_invert_sm`); 100-dim Laplacian smallest now exact (0.000967…), residual
  surfaced.
- **E5** complex SVD via the Hermitian `AᴴA` real embedding (`builtins.rs`,
  `svd_complex`); `svd([1,i;-i,1])` → σ=[2,0], reconstruction ~1e-16.
- **E6** complex-Hermitian sparse `eigs` now routes to Arnoldi (computes, no internal
  error).
- **E2/E3/E16** element-wise array comparisons, logical-mask indexing (`v(v>2)`,
  `M(M>2)`, masked assignment), bool→numeric coercion (`value.rs`).
- **E7** quantize NaN guard · **E8** legendre `|x|>1` · **E9** gainmax `S12=0` ·
  **E10** scalar indexing · **E11** huge-range guard · **E12** dense `norm(M,p)` ·
  **E13** `rank` of zero vector · **E14** strict integer index.

Regression tests: `round2_regressions` (16) in `rustlab-script/src/tests.rs`, plus
`eigs_smallest_shift_invert_on_larger_grid` in `rustlab-core/src/sparse_eig/tests.rs`.

### Round-2b — time_frequency gallery wavelet plots (2026-06-17, FIXED)

The `gallery/time_frequency` item showed **no wavelet plots** (CWT/scalogram). Root
cause: a scalogram heatmap was emitted at full time resolution (64 scales × 20000
samples ≈ 1.3M per-cell SVG `<rect>`s → ~110 MB SVGs). Those files exceed GitHub's 100 MB
limit and had been explicitly `.gitignore`d, so the committed `.md` referenced files that
weren't in the repo → broken/missing images. Fix: `push_db_heatmap` now decimates the
display heatmap's time axis to ≤1500 columns (spectrograms with ≤153 frames are
unaffected; the returned CWT matrix is untouched). Wavelet SVGs dropped 110 MB → 7.5 MB,
the `.gitignore` workaround was removed, and `gallery/time_frequency.{md,plots}` were
regenerated and committed. Test: `scalogram_svg_export_cell_count_is_bounded` (Round 3
reverted the in-figure column cap for an export-time cell-budget cap, so the test now
asserts the rendered SVG's `<rect>` count is bounded rather than the figure's column count).
(Remaining: the *other* fixed notebooks' gallery entries are still stale — run
`make notebooks` to refresh the whole gallery in a deliberate commit.)

Original findings (for reference) below.

### P0 — silent wrong result
- **E1 `eig`/`roots` return all-zeros for circulant/companion matrices of `xⁿ−c` (n≥3).**
  `eig([0,0,1;1,0,0;0,1,0])` → `[0,0,0]` (true: cube roots of unity); `roots([1,0,0,-1])`
  and `roots([1,0,0,-8])` → `[0,0,0]`. Breaks **every** `roots(xⁿ−c)`, n≥3. Root cause:
  `builtins.rs:7383-7446` `eig_hessenberg` — Wilkinson shift is exactly 0 when the
  trailing 2×2 has `tr2==0 && det2==0` (true for these zero-trace, equal-modulus spectra);
  zero-shift QR never deflates, force-deflation emits 0. Needs an exceptional/ad-hoc shift.

### P1 — wrong result / broken on common or edge inputs
- **E2 Logical/mask indexing `v(v>2)` is unusable** (errors `index 0 is invalid`); masked
  assignment `v(v>3)=0` too. Comparisons yield a numeric 0/1 vector and there is no
  logical-indexing branch. `value.rs` index path.
- **E3 Element-wise comparison missing** for `vector⊗vector` (`[1,2,3]==[1,0,3]`) and
  `matrix⊗scalar` (`[1,2;3,4]>2`) — both error. `value.rs:1034-1101` (only scalar/vector-op-scalar).
- **E4 `eigs(…,"sm")` (the DEFAULT) returns wrong/unconverged eigenvalues** once the matrix
  exceeds the Krylov dimension (no shift-invert). 100×100 1-D Laplacian: smallest returned
  0.006165 vs true 0.000967 (~6× off, residual ≈0.018). `builtin_eigs` also discards
  `EigPairs.residual`, so it's silent. (The Round-1 `eigs_gen` Krylov fix was a partial
  mitigation of the same root limitation.)
- **E5 `svd` of a complex matrix computes the SVD of the real part** — `svd([1,i;-i,1])`
  → `[1,1]` (true `[2,0]`); only an `eprintln!` warning. `builtins.rs:10505-10519`.
- **E6 `eigs` on a complex-Hermitian sparse matrix leaks an internal error**
  ("complex Hermitian Lanczos not yet implemented") instead of computing/erroring cleanly.
  `mod.rs:162-170, 380-384` (comment claims an Arnoldi fallback that doesn't happen).
- **E7 `quantize`/`qconv`/`qadd`/`qmul` silently turn NaN into 0.0** (`f64::NAN as i64 == 0`).
  `fixed.rs:71-94` `apply_round`; a NaN guard in `quantize_f64` fixes all four.
- **E8 `legendre(l,m,x)` returns 0 for m≥1 and |x|>1** — `(1−x²).max(0).sqrt()` clamps the
  `P_m^m` seed to 0. `legendre(2,1,1.5)` → -0 (true ≈5.03). `builtins.rs:7153`.
- **E9 `gainmax` (MAG/MSG) wrong at exactly `S12=0`** — unilateral branch drops the mismatch
  factors → 6.02 dB vs correct 9.21 dB (3.19 dB discontinuity). `sparam_analysis.rs:255-257`.
- **E10 Indexing a scalar var `s(1)` reports "undefined function 's'"** — Scalar/Complex/Bool
  omitted from the index-vs-call dispatch. `mod.rs:1693-1703`.
- **E11 `1:1e12` hangs ~40s then OOM-killed** — no guard on oversized range allocation.

### P2 — minor / misleading
- **E12 `norm(M, p)` ignores `p` for dense matrices** (always Frobenius): `norm([1,-2;-3,4],1)`
  → 5.477 (true 6); `Inf`-norm true 7. Sparse honors `p`. `builtins.rs:6638-6642`.
- **E13 `rank([0,0,0])` returns 1** (true 0). `builtins.rs:8089`.
- **E14 Fractional indices silently truncated** (`v(1.5)`→`v(1)`); should error. `value.rs:520-525`.
- **E15 No `~` NOT operator (lex error); `!` only on scalar bool** — no vectorized logical NOT.
- **E16 `true + 1` errors** (no bool→numeric coercion) — blocks `sum(a==b)` once E3 is fixed.
- **E17 display nits**: real assignment `v(5)=10` echoes `10+0j`; imaginary literals `1i`/`4i`
  don't lex (must write `1*i`).

### Missing builtins (gaps, not bugs unless docs claim them)
`var`, `mode`, `cov`, `corr`, and `bessel/gamma/erf/hermite/chebyshev/factorial` are not
implemented. (`legendre`, `laguerre`, `percentile` exist.)

### Round-2 suggested fix tiers
- **Tier A — safe, localized** (low risk, ~1 file each): E7, E8, E9, E10, E12, E13, E14, E11.
- **Tier B — deeper algorithmic/feature** (need care + broad tests): E1 (QR exceptional shift),
  E4 (eigs shift-invert + surface residual), E5 (complex SVD), E6 (complex Hermitian Lanczos),
  E2+E3 (logical indexing + element-wise comparison + E16 bool coercion).

**Status:** audit COMPLETE and ALL listed issues FIXED on branch
`fix/examples-notebooks-bug-hunt` (2026-06-16). Every fix was re-run/re-rendered to
verify. `cargo test` for the changed crates (core, dsp, script, cli) is fully green —
including the new `eigs_gen` regression test and the seeded `example_fixed_point`
monotonicity test. (The 7 workspace failures that remain are environmental: 3 notebook
`server::tests` need to bind TCP ports the sandbox blocks; 4 `validate::tests` need the
external `tidy`/`chktex` linters, which aren't installed.)

## Resolution (2026-06-16)

- **P0 ×2 fixed:** PDE1 (dielectric RHS sign → positive charge gives positive potential,
  in both `.rlab` and `.md`); SP1 (`eigs_gen` Krylov sizing in
  `crates/rustlab-core/src/sparse_eig/mod.rs` + regression test on a 12×8 grid — the
  generalized smallest now returns 35.746 = D(1)/2, was 69.94).
- **P1 ×14 fixed** (incl. NB2 streaming 149→153) and **P2 ×16 fixed** — see the master
  list; every item verified by re-running the example or re-rendering the notebook.
- **POL1:** purged MATLAB from the **DSP/spectral user-facing surface** — `pwelch`/`stft`
  help text (`repl.rs`), `welch.rs`/`builtins.rs` doc-comments, and
  `spectral_estimation.md` / `waterfall.md` prose.

### Follow-ups still open (NOT done in this pass)

1. **Gallery regeneration** — the committed `gallery/` is now stale for the fixed
   notebooks. Run `make notebooks` to refresh it. NOTE: this also rewrites unrelated
   plot SVGs for notebooks that use unseeded `randn()` (known churn, per the Makefile),
   so it's left for a deliberate, separate commit.
2. **Broader MATLAB sweep** — ~40 internal references remain (Rust code comments in
   `lexer.rs`/`ast.rs`/`builtins.rs`, ~25 in `tests.rs`, and "Matlab convention" notes in
   `eig.rlab`, `functions.rlab`, `language_v0_3*.{rlab,md}`, `language_v0_3_4.md`). Most
   document MATLAB-compatibility *conventions* as design context; per the strict project
   rule they should eventually be reworded, but it's a large, meaning-sensitive edit that
   deserves its own pass and the user's call.
3. **Audio + animation_wave** — still need live verification (audio device / animated GIF).

---

## How to use / resume this document

- Each **functional area** has a checklist. `[x]` = audited this pass, `[ ]` = still
  to do (fixes, or live-only tests we couldn't run headless).
- The **Master issue list** is the fix queue, sorted by severity. Each issue has an ID
  (e.g. `C1`, `PDE1`) referenced from its area section, which carries the full repro.
- An AI agent picking this up should: (1) read the Master issue list, (2) fix top-down
  by severity, (3) re-run the repro to confirm, (4) tick the area checklist + strike the
  issue. **Do not fix before the user approves the plan.**

## Tooling / setup

- Binaries (build once): `make release` →
  `target/release/rustlab` and `target/release/rustlab-notebook`.
- Run a script: `cd "$TMPDIR" && .../target/release/rustlab run <abs-path.rlab> 2>&1`
  — **always run from a temp dir**; `rustlab run` chdirs to the script's parent, so
  repo-relative `savefig` writes land in the repo otherwise.
- Render a notebook to inspectable markdown (inlines captured text output + figures):
  `rustlab-notebook render <file.md> -f markdown -o "$TMPDIR/out.md"`. Copy the whole
  `examples/notebooks/` tree to `$TMPDIR` first to avoid polluting `plots/`.
- `rustlab run` prints every assignment + `print()` result → compare against the
  `→ ≈ …` claims in comments and the prose claims in notebooks.
- Sandbox note: scripts that `savefig`/`save` to hard-coded `/tmp/...` fail with
  "Operation not permitted" under the sandbox — that's an environment artifact, **not**
  a script bug (compute still completes). Re-run with sandbox disabled to see plots.

## Severity legend

- **P0** — wrong numerical result or crash (silently misleads the user).
- **P1** — misleading: a comment/label/prose claim is contradicted by the (correct) output,
  or a teaching example doesn't demonstrate what it says.
- **P2** — cosmetic / stale comment / doc-policy / reproducibility nit.

---

## Master issue list (fix queue)

### P0 — wrong result / crash

| ID | Area | File:loc | Summary |
|----|------|----------|---------|
| **PDE1** | PDE | `examples/pde/dielectric.rlab:42` + `examples/notebooks/dielectric.md` | RHS sign flip: positive point charge yields a **negative** potential (max(V) = −199, well = −9.3e6). Fix `b = -1*rho(:)'/eps0` → `b = rho(:)'/eps0`. |
| **SP1** | Sparse / core engine | `crates/rustlab-core/src/sparse_eig/mod.rs` (eigs_gen) → surfaces in `examples/sparse/eigs.rlab:48`, `examples/notebooks/eigs.md` | Generalized `eigs(A,B,k,"sm")` returns the **wrong** smallest eigenvalue (69.94 vs correct 35.75 = 71.49/2). Default Krylov dim too small + plain Arnoldi can't resolve smallest-magnitude without shift-invert. |

### P1 — misleading (claim contradicted by correct output / broken teaching example)

| ID | Area | File:loc | Summary |
|----|------|----------|---------|
| **C1** | Controls | `examples/controls/design.rlab:110` | "open-loop \|H\| at ω=1 should be 1.0" indexes `w(100)`=3.1 rad/s on a log grid → prints 0.1035. ω=1 is index 67. |
| **C2** | Controls | `examples/controls/design.rlab:116` | "closed-loop \|H\| at ω=0.1 should be ≈1" → prints 0.316 (correct DC gain 1/√10); the "≈1" expectation is wrong. |
| **DSP1** | DSP | `examples/dsp/firpm.rlab:25` | Band-pass annotated "2.4–4 kHz" but 0.30–0.50 Nyquist @ fs=8k = **1.2–2 kHz** (−39 dB at 2.4 kHz). |
| **NB1** | Notebooks | `examples/notebooks/sparse_complex.md` (Hermitian block) | Residual computed with `x'` (conj-transpose) → displays **3.0145** (looks failed); true residual ~7e-16. Use `transpose(x)`. |
| **NB2** | Notebooks | `examples/notebooks/time_frequency.md` (streaming) | "column count matches batch (153)" but prints **149** — chunk loop `0:floor(n/chunk)-1` drops the last 544 samples. |
| **NB3** | Notebooks | `examples/notebooks/vector_calculus.md` (2-D gradient) | Comment "corner x=y=1 → gradient ≈2"; grid is `linspace(-2,2,41)` so corner is x=y=**2**, gradient = **4** (output 3.9999). |
| **NB4** | Notebooks | `examples/notebooks/filter_analysis.md` | 64-tap (even-length) symmetric FIR labeled "Type I"; even length ⇒ **Type II**. (τ=31.5 is the Type II signature.) |
| **PLOT1** | Plot | `examples/plot/log_polar.rlab:31,34` + `examples/notebooks/log_polar.md` | "Three-petal rose `r=1+0.3cos(3θ)`" — r∈[0.7,1.3], a perturbed circle, **no petals**. A rose needs r→0. |
| **NB5** | Notebooks | `examples/notebooks/persistent_cache_demo.md:127` | Prose states `heavy(50000)`=1.886e6; actual = **20327** (~93× off). |
| **NB6** | Notebooks | `examples/notebooks/cache_advanced_demo.md` §6 | "make_adder doesn't appear in the per-function table" — it **does** (0 hits/1 miss). |
| **NB7** | Notebooks | `examples/notebooks/mermaid_demo.md:58` | Code block `print("… ${N} …")` implies `${}` works inside rustlab; it's prose-only → output is literal `${N}`. Broken example. |
| **NB8** | Notebooks / engine | `examples/notebooks/seed.md:46,60` | Reproducibility values (`M`, `v`, `calibration`, `sample`) **never shown** — renderer drops auto-display of unsuppressed bare values; prose describes invisible evidence. |
| **MATH1** | Math | `examples/math/random.rlab:31` | Title "SNR ≈ 10 dB"; actual = **7.45 dB** (noise std 0.3 ⇒ 0.5/0.09). Need std≈0.224 for 10 dB. |

### P2 — cosmetic / stale comment / doc-policy / reproducibility

| ID | Area | File:loc | Summary |
|----|------|----------|---------|
| **POL1** | Project policy | `pwelch`/`stft`/`cwt` builtin docs; `spectral_estimation.md`; `time_frequency.md` | "MATLAB" references — violates the project's no-MATLAB-in-shipped-artefacts rule. |
| **C3** | Controls | `bode_plot.rlab:8`, `step_response.rlab:8`, `pid_controller.rlab:133` | `plot(v)` plots vs sample index; titles imply a frequency/time x-axis the chart lacks. Data is correct; dedicated `bode()`/`step()` render proper axes. |
| **C4** | Controls | `examples/controls/ode.rlab:90` | Pendulum "final angle should approach 0" prints 0.157 rad at t=6 s (still swinging; settles by t≈30). |
| **DSP2** | DSP | `examples/dsp/firpm.rlab:57` | "integer coeffs e.g. 127, −256" above an **8-bit** call; 8-bit signed can't be −256 (range −128…127; actual min −25). |
| **DSP3** | DSP | `examples/dsp/fixed_point.rlab:29,65` | Unseeded `randn` ⇒ SNR sweep not reproducible (all spectral scripts `seed(42)`); dead `n_out`/"trim" comment (no trimming happens). |
| **RF1** | RF | `examples/rf/polish_features.rlab:14` | "\|S21\| at 2.5 GHz" but `linspace(1e9,6e9,26)` index 8 = **2.40 GHz** (grid never hits 2.5). |
| **RF2** | RF | `examples/rf/polish_features.rlab:62` | Sdd21 "close to 1" but = **0.750**. Overstated wording; value correct. |
| **SP2** | Sparse | `examples/sparse/sparse_complex.rlab:25` | `nnz` comment "~14k"; actual **17760** (correct for 60×60 5-point). |
| **STAT1** | Stats | `examples/stats/all_any.rlab:14` | Comment claims `1`/`0`; scalar `all`/`any` print `true`/`false` while element-wise `>` prints `0/1` — inconsistent rendering. |
| **LIN1** | Linalg | `examples/linalg/tensor3.rlab:15` | Comment "A(i,j,k)=100i+10j+k"; code builds `reshape(1:24,…)` (plain 1..24). |
| **PLOT2** | Plot | `examples/plot/heatmap_image.rlab:70` | Prose swaps green/blue channel descriptions (green=x+y, blue=y in code). **Notebook version is correct** — script comment only. |
| **LANG1** | Language | `examples/language/profiling.rlab:34` | "peak ~512 for 1-sample/cycle sinusoid" — actual 472.6 (off-bin leakage); rationale also wrong (1000 Hz @44.1k ≈ 44 samp/cycle). |
| **NB9** | Notebooks | `examples/notebooks/time_frequency.md` (STFT) | Prose "hop of 128" vs code 4th-arg 384 (= noverlap; hop = 512−384). Pedagogically confusing. |
| **NB10** | Notebooks | `examples/notebooks/parallel_montecarlo.md` | Hand-pasted `text` output blocks in source are stale (nproc 12, π 3.141833…); render overwrites them — source-only nit. |
| **NB11** | Notebooks / engine | `examples/notebooks/animation.md`, `string_arrays.md` | Unsuppressed `figure()` leaks its numeric handle (`2`/`3`/`4`) into captured text output. |
| **NB12** | Notebooks | `examples/notebooks/dielectric.md` (sanity-check §) | Text-only block (no `clf`) emits a duplicate figure carried over from the prior cell. |

---

## Functional areas

### 1. Controls — `examples/controls/` (15 scripts)
- [x] All 15 scripts run clean (exit 0, no NaN/Inf).
- [x] Re-derived: controllability rank, pole/zero locations, gain/phase margins
      (Gm=∞, Pm=53.13°@4 rad/s), LQR/CARE/DARE residuals ~1e-14, `place` gains,
      ss observable-canonical realization, tf↔ss round-trips, Nyquist Ms=2.654@1.456 rad/s,
      dc-motor poles {0,−10} — all match output.
- Issues: **C1, C2** (P1), **C3, C4** (P2).
- [ ] Fix C1/C2 (`design.rlab` wrong index + wrong expectation), C3 (axis-bearing plots
      or retitle), C4 (lengthen horizon or soften wording).

### 2. DSP filters — `examples/dsp/` (6 scripts)
- [x] bandpass, kaiser_fir, lowpass, upfirdn fully verified (linear phase, DC gain,
      cutoffs at −6 dB, stopband ≥60 dB, upfirdn output lengths, Kaiser order formula).
- [x] fixed_point SNR trend (6 dB/bit, 12-bit first to clear 50 dB) holds.
- Issues: **DSP1** (P1), **DSP2, DSP3** (P2).
- [ ] Fix DSP1 (Hz annotation), DSP2 (8-bit coeff example), DSP3 (seed + dead code).

### 3. Spectral — `examples/spectral/` (6 scripts)
- [x] fft (peaks at 500/1500 Hz, mirror, DC-centered axis, round-trip), pwelch (peak
      0.1504, 8 segments @50%, default window floor(2N/9)), spectrogram_chirp (ridge
      tracks f(t) to <0.1%), waterfall_chirp/steps (orientation + tone freqs),
      cwt_chirp (ridge follows sweep) — all verified.
- Issues: none in the scripts. (`POL1` MATLAB wording lives in builtin docs / notebooks.)

### 4. RF / microwave — `examples/rf/` (6 scripts + .s2p data)
- [x] load_s2p, amplifier_stability (Rollett K, μ-params, VSWR, RL, MAG, Γms/Γml, stability
      circles), cascade_attenuator (S→Z→S round-trips ~5e-17, de-embed), measurement_review,
      smith_chart (markers + traces pixel-verified), .s2p parsing (MA/GHz/R50 + noise block)
      — all hand-verified correct.
- Issues: **RF1, RF2** (P2, both in polish_features).
- [ ] Fix RF1 (frequency label), RF2 (wording).

### 5. Linear algebra — `examples/linalg/` (6 scripts)
- [x] eig (incl. generalized + Vieta checks), linear_algebra (lyap, gramians, svd, cond),
      matrix_ops (reshape col-major, kron, expm rotation det=1, expm(−jHt)), rank, roots
      — all verified.
- Issues: **LIN1** (P2, stale tensor3 comment).
- [ ] Fix LIN1.

### 6. Sparse — `examples/sparse/` (4 scripts)
- [x] sparse (SpMV, tridiag solve), sparse_solve (x=[1/3,1/3], LU vs auto ~5e-18),
      sparse_complex (math correct).
- Issues: **SP1** (P0 — generalized eigs, core engine), **SP2** (P2 — nnz comment).
- [ ] **SP1 is the headline fix** — touches `crates/rustlab-core/src/sparse_eig/mod.rs`
      (raise default Krylov dim for `eigs_gen`, or use shift-invert for "sm"). Standard
      `eigs(...,"sm")` is fine; only the generalized B-matrix path is wrong. Add a
      regression test (`eigs(L, c*I, k, "sm") == eigs(L,k,"sm")/c`).
- [ ] Fix SP2 comment.

### 7. Stats — `examples/stats/` (2 scripts)
- [x] stats (energy=Σsegments, trapz cos≈0 / triangle=0.5, argmin/max, percentile),
      all_any (values correct).
- Issues: **STAT1** (P2, bool vs numeric rendering of reductions).
- [ ] Decide intended convention (true/false vs 1/0) and align comment + rendering.

### 8. Math / special functions — `examples/math/` (5 scripts)
- [x] complex_basics (abs/angle), mandelbrot (arrayfun==parmap, escape-time spot checks),
      ml_activations (relu/gelu/tanh(0)=0, softmax=1, sample-vs-pop std acknowledged),
      trig_special (acos/asin round-trip ~2e-16, asin+acos=π/2, Legendre ‖P₂‖²=0.4 / ∫P₁P₂=0,
      Laguerre L₁(2.5)=−1.5) — all verified.
- Issues: **MATH1** (P1, SNR label in random.rlab).
- [ ] Fix MATH1 (relabel ~7.5 dB or drop noise std to 0.224).

### 9. PDE / fields — `examples/pde/` (5 scripts)
- [x] electrostatics (quadrupole signs/symmetry correct, +charge→+V), laplacian
      (rel_err 1.4e-15), laplacian_bc (null residuals ~3e-12, nnz match), vector_calc
      (grad/div/curl, curl(grad)=0) — verified.
- Issues: **PDE1** (P0 — dielectric sign flip; also in dielectric.md).
- [ ] **PDE1 fix:** `dielectric.rlab:42` `b = rho(:)'/eps0` (drop the negation), then
      re-render `dielectric.md`. Confirm max(V)>0 at the charge.

### 10. Plotting — `examples/plot/` (9 scripts)
- [x] surf (z-sign), contour (level curves), heatmap_image (value→color), masks (π≈3.135),
      quiver (E=−∇V and vortex directions verified), multi_figure, log_polar transforms.
- [ ] `animation_wave.rlab` — **not run** (writes into `../gallery`); static-reviewed, logic
      sound. Live-verify the GIF separately.
- Issues: **PLOT1** (P1, log_polar rose), **PLOT2** (P2, heatmap script comment).
- [ ] Fix PLOT1 (relabel or pick a real rose `r=cos(3θ)`), PLOT2 (script comment).

### 11. Language / scripting — `examples/language/` (12 scripts)
- [x] if_else, disp_fprintf, lambda, functions, language_v0_3, lambda_pipeline, vectors,
      save_load (NPY/CSV/NPZ round-trip = 0 err), toml_io, toml_filter_chain, bench_parmap
      (parmap==arrayfun, 0 diff) — verified. (`vectors.rlab` `'` = conjugate transpose,
      correct.)
- Issues: **LANG1** (P2, profiling.rlab peak comment).
- [ ] Fix LANG1.

### 12. Audio — `examples/audio/` (5 `.rlab` + shell wrappers)
- [ ] **NOT run** — need a live audio device / TTY. Static review found no calculation
      bugs. Open: live-verify passthrough, filter, spectrum/spectrogram/waterfall monitors
      on a machine with audio I/O.

### 13. Notebook engine / renderer features
- [x] template_interpolation (all `${}` resolve), notebook_directives (hide/details/grid/
      callouts/exercises), embeds_demo (transclusion + block-id), widgets_demo (defaults +
      interpolation), multi_notebook (link rewriting + index), mermaid_demo (4 diagrams →
      SVG), seed reproducibility (seed(42) byte-identical across renders; unseeded differs),
      cache demos (hit/miss counters) — all functional.
- Engine-level findings to decide on:
  - **NB8** (P1): renderer drops auto-display of unsuppressed bare values (only captures
    `print`/`disp`/plots/directives). Either capture auto-display, or fix the notebooks
    (seed.md) to use `print`/`disp`.
  - **NB11** (P2): `figure()` returns an auto-displayed handle → leaks `2/3/4` into output.
    Suppress handle display, or `;`-terminate in the notebooks.

### 14. Notebooks — content (40 `.md` audited)
- [x] All 40 render successfully; every figure non-empty, no NaN/Inf.
- [x] Inheritance check: dielectric.md inherits **PDE1**; eigs.md inherits **SP1**;
      log_polar.md inherits **PLOT1**. heatmap_image.md does **NOT** inherit PLOT2 (its
      prose is correct — only the script comment is wrong).
- Notebook-specific issues: **NB1–NB12** (see master list).
- [ ] Fix the notebook-specific issues; re-render affected notebooks; confirm gallery
      regenerates clean.

### 15. Cross-cutting — project policy & reproducibility
- [ ] **POL1** (P2): purge "MATLAB" from `pwelch`/`stft`/`cwt` builtin docs and from
      `spectral_estimation.md` / `time_frequency.md` (project rule: no MATLAB in shipped
      artefacts; design-reference-only in plans).
- [ ] Reproducibility: `random.rlab` and `fixed_point.rlab` use unseeded `randn` →
      `seed(...)` them so printed numbers are stable (matches the rest of the suite).

---

## Suggested fix order (when approved)

1. **P0 first:** PDE1 (sign), SP1 (core eigs_gen + regression test).
2. **P1 batch:** the comment/label/prose corrections (C1, C2, DSP1, NB1–NB8, PLOT1, MATH1)
   — each is a small, isolated edit; re-run the repro after each.
3. **P2 sweep:** doc/comment/seed/policy nits (POL1 + the rest), grouped by file.
4. Re-render notebooks (`make notebooks`), re-run `cargo test --workspace`, spot-check the
   regenerated gallery.

## Coverage gaps (explicitly NOT verified this pass)

- `examples/audio/*` — needs live audio I/O.
- `examples/plot/animation_wave.rlab` and the animated GIFs — static-reviewed only.
- Timings/benchmarks quoted in notebooks (mandelbrot ms, sparse_scaling speedups) are
  hardware-specific and unfalsifiable headless — left as-is.
