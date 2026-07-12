# Changelog

Notable user-facing changes to rustlab, newest first. Versions are the
workspace version in `Cargo.toml`; entries under a version may land across
several PRs while that version is current. **Breaking / behavior changes**
get their own subsection with migration guidance — downstream script owners
should re-validate against those entries when upgrading.

## Unreleased

### Breaking / behavior changes
- **Single-output `svd` returns the singular values.** `s = svd(A)`
  now binds the singular-value vector (descending) — previously it
  bound the entire `(U, σ, V)` tuple, which was unusable as a single
  value (`size(s)` errored). `[U, S, V] = svd(A)` is unchanged.
  Migration: code that relied on the tuple binding should destructure
  explicitly.
- **`fft(x)` is now length-preserving.** It returns exactly `length(x)`
  bins instead of silently zero-padding to the next power of two;
  non-power-of-two lengths use a hand-rolled Bluestein (chirp-z) transform
  over the existing radix-2 kernel, and `ifft(X)` now accepts any length
  (previously a hard error off powers of two). New optional size argument:
  `fft(x, n)` / `ifft(X, n)` zero-pad or truncate to exactly `n` first.
  Migration: the axis idiom `f = fftfreq(length(X), fs)` is now always
  correct; scripts that relied on the implicit padding must request it
  explicitly, e.g. `fft(x, 1024)`. Windowed-frame estimators (`pwelch`,
  `stft`, `spectrogram`, `waterfall`, and their streaming forms)
  intentionally keep rounding their explicit `nfft` argument up to a power
  of two, as documented.

### Added
- Plot color names now include `gray`/`grey` and hex `"#RRGGBB"`
  everywhere a color string is accepted (`plot(..., "color", c)`,
  `hline`/`yline`, contour/quiver/streamplot color args).
- Plot argument validation is no longer silent: an unrecognized color
  name in a dedicated color slot (`hline(y, "dashed")`,
  `plot(..., "color", "chartreuse")`) prints a one-line stderr warning
  naming the accepted colors, and `heatmap`/`imagesc`/`bar` warn once
  per call when NaN/Inf values flow into plot data (renderers already
  handled them defensively, but silently — broken figures shipped with
  no hint at render time).
- `tic` / `toc` wall-clock stopwatch (bare or with parentheses): `tic`
  starts/restarts, `toc` returns elapsed seconds without clearing so
  repeated calls take split times; `toc` before any `tic` is an error.
  Thread-local (a `tic` inside a `parmap` worker times that worker).
- `ellipke(m)` — complete elliptic integrals K(m) and E(m) via the
  arithmetic-geometric mean. Parameter convention `m = k²`; domain
  [0, 1] with the exact limits `ellipke(1) → (Inf, 1)`; elementwise
  over vectors/matrices; `[K, E] = ellipke(m)` returns both.
- `pin_dirichlet(A, b, mask_or_indices, values) → [A, b]` — enforce
  Dirichlet boundary values on a linear system: pinned rows of `A`
  become identity rows and `b` gets the pinned values, so
  `spsolve(A, b)` reproduces the boundary potential exactly. Accepts a
  grid mask (column-major, matching `ij2k`/`ijk2k` and the
  `laplacian_*` builders) or a 1-based index list; sparse or dense
  square `A` (the sparse ordering hint survives).
- `trapz(M)` / `trapz(x, M)` — trapezoidal integration now handles
  matrices per column, returning a 1×ncols row (1-D-shaped inputs keep
  returning a scalar). Double integrals are two calls:
  `trapz(ys, trapz(xs, F))`.
- `quiver(..., "normalized")` — direction-only arrow plots: every
  vector is drawn at unit length × scale.

### Changed (plotting)
- **quiver auto-scaling is outlier-robust.** The auto-scale now keys on
  the 95th percentile of the field's nonzero magnitudes (was: the single
  longest arrow) and clamps outliers to one grid cell at draw time — a
  near-singular sample (e.g. a Biot-Savart field evaluated on the wire)
  no longer shrinks every other arrow to invisibility. Uniform fields
  are unchanged. Decimate dense grids with stride indexing
  (`U(1:5:end, 1:5:end)`), documented in `docs/functions.md`.
- **"not rendered to the terminal" warnings are deferred and
  savefig-aware.** Scripted runs that render vector plots and then save
  them (`quiver(...); savefig(...)`) no longer emit stderr noise; a plot
  that never reaches a file warns once, at the end of the run (or REPL
  line), as one combined message naming the plot kinds.

### Fixed
- Bare `figure()` and `histogram(v)` / `hist(v)` statements no longer
  echo their return values (a meaningless figure-handle integer above
  every plot in notebook output — churning with global figure count
  across a directory render — and a 2×n bin matrix, respectively).
  Statement-position builtin calls now carry `nargout = 0`; assigned
  forms (`h = figure()`, `b = histogram(v)`) still return their values.
  Other builtins are unaffected.
- `heatmap`, `imagesc`, `contour`/`contourf`, and `image` (grayscale /
  colormap modes) now color by **signed** value instead of magnitude.
  Previously complex-to-real collapse used |v| at ingest, so any signed
  matrix rendered wrong: `[-2, -1; 1, 3]` showed −2 at mid-scale and −1
  identical to +1, and large-magnitude negatives rendered *hot*. All
  static paths now match the live-viewer path (real part, `.re`). For
  genuinely complex inputs this means the real part is displayed —
  take `abs(Z)` explicitly to plot magnitudes. Spectrogram/scalogram dB
  displays are unchanged (magnitude there is intentional).
- Multi-panel `subplot` figures containing heatmaps (`heatmap`/`imagesc`)
  or 3-D surfaces (`surf`) now export **all** panels to SVG. Previously
  the file was finalized after the first heatmap/surface panel and every
  later panel was silently dropped (line-plot panels were unaffected;
  PNG was unaffected). Captured notebook figures had the same defect.
- `cache` is no longer a reserved word. The 0.3.6 cache statement had
  silently reserved the lowercase identifier `cache`, breaking scripts
  that use it as a variable (`cache = 5`, `function [y, cache] = f(x)`).
  It is now a soft keyword: `cache <subcommand>` / `cache "path"` /
  `cache path.rcache` still parse as cache statements, and every other
  use is an ordinary identifier. One corner changed: a bare `cache` line
  is now a variable reference (previously a parse error asking for a
  subcommand).
- `rustlab-viewer` no longer burns CPU while idle (~10% of a core on WSLg,
  where every frame goes through RDP compositing or software GL). The GUI
  previously repainted at ~60 fps around the clock to poll for socket
  messages; repaints are now event-driven — the socket listener wakes the
  GUI when a message arrives, so an idle viewer draws no frames and plots
  appear with lower latency than the old 16 ms poll.
- `T(:)` on a 3-D tensor no longer panics the interpreter — it flattens
  column-major (the `reshape`/`ijk2k` order); linear indexing `T(k)`,
  index vectors `T(I)`, and `T(end)` now work on tensors too.
- `rustlab run` exits with code 1 when the script fails to parse or
  dies on a runtime error (previously it printed the error but exited
  0, silently passing CI gates). `AudioEof`/`Interrupted` remain
  clean exits.
- `max(a, b)` / `min(a, b)` are now elementwise over any mix of scalars,
  vectors, and matrices, with the same implicit-expansion (broadcast) rules
  as `+`. Previously the two-argument form accepted only two scalars. Per
  element, a `NaN` loses to any non-`NaN` partner. `max(M1, M2)` is the
  canonical union of two 0/1 masks.
- Imaginary numeric literals: `2j`, `1.5i`, `3e8j` parse directly, so
  `z = 1 + 2j` works and printed complex values (`1+2j`) round-trip as
  input. The suffix binds to the literal, so it is immune to a variable
  named `i` or `j` shadowing the builtin constant.

### Changed
- The non-integer index error now suggests a fix: `index 2.5 is invalid
  (must be a positive integer; round a computed index explicitly with
  floor()/round(), or use integer arithmetic)`.

### Docs
- Corrected the physical rationale for the harmonic-mean face coefficients
  in the `laplacian_eps_2d` reference (series composition of half-cell
  fluxes / O(h) accuracy, not flux-conservation).
- Added warnings near the `spsolve` and `transpose` entries: reshaping a
  complex right-hand side with postfix `'` silently conjugates it — use
  `.'` for pure reshaping.

## 0.3.6

### Breaking / behavior changes
- **Non-integer indices are now a hard error.** `v(2.5)` raises
  `index 2.5 is invalid (must be a positive integer)`; earlier versions
  silently floored fractional indices. Scripts that relied on implicit
  flooring must round explicitly — `v(floor(n/2))` — or use integer
  arithmetic, e.g. `(n + 1) / 2` for the midpoint of an odd-length `n`.
  Landed in PR #24 alongside logical-mask indexing (`v(v > 2)`) and
  element-wise comparison fixes.
