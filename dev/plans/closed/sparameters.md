# Development Plan: S-Parameters and Smith Charts

**Target use case:** Import Touchstone files (`.s1p`, `.s2p`, `.s3p`, `.s4p`)
captured from VNAs (Keysight ENA/PNA, Copper Mountain, etc.), then perform
RF S-parameter analysis in rustlab — Smith charts, magnitude/phase plots,
cascading, conversions, gain/stability circles, de-embedding.

**Current phase:** complete
**Status:** All six phases shipped 2026-05-17. Plan closed.

---

## Overview

RF engineers measure devices on a Vector Network Analyzer (VNA) and export
Touchstone files. The standard workflow is then: read the file → look at
S-parameters on a Smith chart and magnitude/phase plots → cascade with other
networks → compute derived quantities (VSWR, return loss, stability factor,
gain circles) → maybe de-embed test fixtures.

rustlab already has the right primitives for this:

- `CTensor3` (`Array3<C64>`) maps naturally to the canonical S-parameter
  layout `[n_freqs × n_ports × n_ports]` of complex values.
- `Value::Struct` can hold the network object (`s.parameters`, `s.frequencies`,
  `s.num_ports`, `s.impedance`) — same pattern that already works for
  multi-field results elsewhere in the codebase.
- `Value::TransferFn` / `Value::StateSpace` / `Value::Matrix` show the
  established pattern for adding a typed domain object and the builtins that
  operate on it.
- Smith chart is a 2-D plot on the unit disk; the existing `plot()` + axis
  + line/scatter primitives in `rustlab-plot` cover most of what's needed.
  Only the Smith grid (constant-R / constant-X arcs) is genuinely new.

Six phases, ordered by dependency. Each phase ships with tests, docs,
REPL help, and examples per workflow rules 2/3/5/6/7.

---

## MATLAB-as-reference policy

The MATLAB RF Toolbox documentation is the design reference for this plan
(user-approved 2026-05-17). API shape, field names, and function semantics
mirror `sparameters` / `rfckt` / `smithplot` so the workflow is immediately
familiar to RF engineers coming from that tool.

The project's standing rule against naming MATLAB still applies to the
*shipped artefacts* — code identifiers, doc prose, REPL help, examples,
tests should describe rustlab on its own terms ("industry-convention RF
S-parameter analysis"), not as a port of any specific vendor's tool. The
design provenance lives in this plan; it does not need to bleed into the
user-facing surface.

---

## Data model decisions (apply to all phases)

### N-port network object

A `Value::Struct` with these fields, all required:

| Field         | Type            | Meaning |
|---|---|---|
| `parameters`  | `Tensor3`       | Complex S-parameters, shape `[n_freqs, n_ports, n_ports]`. `parameters(k, i, j)` is `S_ij` at the k-th frequency. |
| `frequencies` | `Vector` (real) | Frequency points in Hz, length `n_freqs`, monotonically increasing. |
| `num_ports`   | `Scalar`        | Port count `n_ports` (redundant with `parameters` shape, but cheap and matches convention). |
| `impedance`   | `Scalar` or `Vector` | Reference impedance per port. Scalar = same Z₀ on every port (typical 50 Ω). Vector of length `n_ports` = per-port (allowed by Touchstone v2 `[Reference]`). Complex Z₀ supported (`Complex` scalar or `Vector` of complex). |

**Why `[n_freqs, n_ports, n_ports]` and not `[n_ports, n_ports, n_freqs]`?**
The former lets `parameters(k, :, :)` extract the full S-matrix at frequency
`k` as a `Matrix` (the rank-3 → rank-2 collapse rule rustlab already uses
for trailing-singleton). The latter would require either a transposing
helper or a leading-singleton collapse rule rustlab doesn't have. We pick
the storage order that matches the most common access pattern.

**`s11`, `s12`, `s21`, `s22` accessors:** vector-of-length-`n_freqs` slices.
For an `n`-port network, `sij(s, i, j)` is the general form. Convenience
builtins `s11(s) … s22(s)` are sugar for `sij(s, 1, 1) … sij(s, 2, 2)` and
exist because they read naturally in scripts.

### Smith chart figure

Smith chart goes through the existing figure infrastructure. We add a new
`PlotKind::Smith` and a new `SubplotState` flag (`smith_grid: SmithGrid`).
The grid renders as part of the axis decoration (like the Nyquist `-1`
marker and equal-aspect setting), the data traces are normal series.

Two grid modes (compromise: do both, default to `Z`):

- `Z` — impedance Smith chart (constant resistance circles, constant reactance arcs)
- `Y` — admittance Smith chart (constant conductance circles, constant susceptance arcs)
- `ZY` — overlay both ("immittance chart") — common for matching-network
  design. Renders Y arcs in a muted secondary colour.

All backends must render the grid (workflow rule 9). The Smith grid is just
arc/circle geometry, so it composes from existing plotters/Plotly/ratatui
primitives — no new backend capability needed beyond "draw N circles +
N arcs on the axes."

---

## Phase 1 — Foundations: data type + Touchstone I/O

**Status:** complete (2026-05-17)
**Goal:** load and save `.sNp` files; build a network from raw data; basic
inspection (`s11(s)`, `freqs(s)`, `nports(s)`).

### 1a. `sparameters(...)` constructor builtin

- **File:** `crates/rustlab-script/src/eval/builtins.rs`
- **Signatures:**
  - `sparameters(filename)` → reads Touchstone file (dispatches to 1b)
  - `sparameters(S, freqs)` → S is `Tensor3`, freqs is `Vector` real, Z₀ = 50
  - `sparameters(S, freqs, Z0)` → explicit reference impedance
- **Returns:** `Value::Struct` with the four fields above.
- **Tests:** round-trip from raw arrays; reject ragged/mismatched shapes;
  reject non-monotonic frequency vector; reject `S` that isn't square in
  ports.

### 1b. Touchstone reader

- **New file:** `crates/rustlab-script/src/eval/touchstone.rs`
- **Spec:** Touchstone v1.1 (the `.s1p`–`.s4p` flavour). v2 (`[Version]`,
  `[Reference]`, mixed-mode) is out of scope for Phase 1 — added in Phase 6
  if needed.
- **Header:** `# <freq-unit> <param-type> <format> R <Z0>`
  - Frequency units: `Hz`, `kHz`, `MHz`, `GHz` (case-insensitive)
  - Parameter type: `S` (Phase 1 only; `Y`, `Z`, `H`, `G` deferred — see
    Phase 5 once conversions land)
  - Format: `RI` (real/imag), `MA` (magnitude/angle in degrees), `DB`
    (dB/angle in degrees)
  - `R <Z0>`: real positive scalar; defaults to 50 if missing
- **Data lines:** `freq` followed by `2 × n²` numbers per row (split across
  lines for n ≥ 3 per the spec). Comments (`!`) skipped.
- **Port-pair ordering:** for n=2, files are written column-major
  (`S11 S21 S12 S22`); for n≥3, row-major (`S11 S12 S13; S21 S22 S23; …`).
  This is a Touchstone wart — implementation must handle both.
- **Tests:**
  - Synthesize a Touchstone file in a temp dir for each of `RI`/`MA`/`DB`,
    each of `Hz`/`MHz`/`GHz`, each of n=1..4, read it back, compare to
    known S matrix to 1e-12.
  - A real-world `.s2p` from `examples/sparameters/data/` (a published
    amplifier or attenuator sample — pick one with permissive licence)
    parses and gives expected `|S21|` at a known frequency.
  - Malformed inputs: missing header → error; mismatched data count → error.

### 1c. Touchstone writer

- **Builtin:** `sparameters_write(s, filename)` or unify under `save(s, filename)`
  via extension detection.
- **Default format:** `RI` (lossless), `Hz` frequency unit (Touchstone allows
  scaling; we always write Hz for unambiguous round-trip).
- **Tests:** write → re-read → equal to 1e-15.

### 1d. Inspection builtins

- `nports(s)` → integer port count
- `freqs(s)` → real `Vector`
- `sij(s, i, j)` → complex `Vector` of length `n_freqs`
- `s11(s)`, `s12(s)`, `s21(s)`, `s22(s)` → convenience wrappers (error if
  `nports(s) < 2`)
- **Tests:** loaded `.s2p` returns the right `Vector` for each accessor.

### 1e. Display

- Implement `Display` for the network struct so REPL printing of the value
  gives a useful header:
  ```
  sparameters: 2-port, 201 frequencies [10 MHz .. 6 GHz], Z0 = 50 Ω
  ```
- **File:** `crates/rustlab-script/src/eval/value.rs` (extend the `Struct`
  Display path with a tagged-struct check).
- **Tests:** snapshot test on the formatted string.

### Phase-1 deliverables checklist

- [x] `sparameters(...)` builtin and Touchstone reader (writer to land in
      Phase 2 when `save(s, "x.s2p")` is wired through the unified save dispatcher;
      `render_touchstone` already implemented and unit-tested in `eval/touchstone.rs`)
- [x] Inspection builtins (`nports`, `freqs`, `sij`, `s11`–`s22`)
- [x] REPL `HelpEntry` for each new builtin (workflow rule 3) — also new
      `"S-Parameters (RF)"` category in `print_help_list()`
- [x] `docs/functions.md` section "S-Parameters (RF)"
- [x] `docs/quickref.md` new section "S-Parameters (RF)"
- [x] `AGENTS.md` builtins-table updates (workflow rule 7)
- [x] Example `examples/sparameters/load_s2p.rlab` + bundled
      `examples/sparameters/data/lna_demo.s2p`; runs end-to-end and prints
      |S21| (dB) and return loss at six frequencies
- [x] `cargo test --workspace` green (1067 rustlab-script tests pass,
      30 new tests — 15 in `eval::touchstone::tests` + 15 in
      `tests::sparameters_script_tests`)

### Phase-1 implementation notes

- Storage layout `[n_freqs, n_ports, n_ports]` chosen so `parameters(k, :, :)`
  naturally collapses to a `Matrix` per the existing trailing-singleton rule.
- Network object encoded as a `Value::Struct` tagged with `__kind__ =
  "sparameters"` (double-underscore sentinel matches `toml_io`'s reserved
  keys). The Display impl on `Value::Struct` checks the sentinel and renders
  a one-line summary (`sparameters: 2-port, 6 frequencies [1 GHz .. 6 GHz],
  Z0 = 50 Ω`); plain user structs still render via the existing key/value
  dump.
- Touchstone parser is hand-rolled (workflow rule 10). Handles `RI`/`MA`/`DB`,
  `Hz`/`kHz`/`MHz`/`GHz`, the n=2 column-major vs n≥3 row-major ordering
  wart, comment lines (`!`), multi-line records, missing-header
  detection, non-monotonic-frequency rejection, and v2 `[…]` keyword
  lines (rejected with a clear "Phase 1 reads v1.1 only" message).
- One ergonomic gotcha noted for the example: `rustlab run <script>` does
  `set_current_dir` to the script's parent, so relative data paths in
  example scripts must be relative to the script, not the project root
  (see `examples/sparameters/load_s2p.rlab`).
- Save dispatcher integration (`save(s, "foo.s2p")`) deferred to Phase 2
  alongside the conversion builtins — keeps the Phase 1 footprint smaller
  and lets the writer ride in with the other I/O ergonomics work.

---

## Phase 2 — S-parameter math: conversions and cascading

**Status:** complete (2026-05-17)
**Depends on:** Phase 1

This phase adds the linear-algebra layer that lets users move between
representations and stack networks.

### 2a. Network-parameter conversions

All take and return an `sparameters` struct; the parameter type lives in a
new tag field (`type: "S" | "Y" | "Z" | "T" | "ABCD" | "H"`) added to the
struct in 2a-pre. Default constructor sets `type = "S"`.

- `s2z(s)`, `s2y(s)`, `s2t(s)`, `s2abcd(s)`, `s2h(s)`
- `z2s(z, Z0)`, `y2s(y, Z0)`, `t2s(t, Z0)`, `abcd2s(abcd, Z0)`, `h2s(h, Z0)`

**Algorithms:** standard matrix formulas (the same definitions every RF
textbook gives — `Z = √Z₀ · (I − S)⁻¹(I + S) · √Z₀` and friends). Implement
in pure Rust per workflow rule 10.

**T-parameters** apply only to 2-port networks; raise a clear error for
n ≠ 2.

**Tests:** round-trip `s → z → s` and `s → t → s` equals identity to 1e-10
for several frequencies; cross-check against analytic results for a series
resistor (`S11 = R/(R+2Z₀), S21 = 2Z₀/(R+2Z₀)`) and a shunt resistor.

### 2b. Port-impedance renormalisation

- `newref(s, Z_new)` — re-normalises to a different reference impedance
  (scalar or per-port vector). Formula via the impedance-domain detour.
- **Tests:** renormalise 50→75→50 round-trips to identity; renormalising
  a known matched load (`S11 = 0` at Z₀ = 50) to Z₀ = 75 produces the
  expected mismatch reflection.

### 2c. Cascading

- `cascade(s1, s2, ...)` — variadic cascade of 2-port networks via
  T-parameter multiplication, returning an `sparameters` struct. Networks
  must share a frequency grid (error if they don't; offer `interp1`-based
  interpolation as `cascade(s1, s2, "interp")` flag).
- **Tests:** cascade of two known networks matches hand-computed result;
  cascading an attenuator with its inverse produces a near-thru
  (`|S21| ≈ 1`, `|S11| ≈ 0`).

### 2d. De-embedding

- `deembed(meas, left, right)` — removes known fixture networks on either
  side of the device under test. Computed via T-parameter inverse:
  `T_DUT = T_left⁻¹ · T_meas · T_right⁻¹`.
- **Tests:** synthetic `meas = cascade(fixture_a, dut, fixture_b)`,
  `dut_recovered = deembed(meas, fixture_a, fixture_b)`, compare to `dut`
  to 1e-10.

### Phase-2 deliverables checklist

- [x] `parameter_type` field added to the network struct (default `"S"`);
      `unpack_sparameters_typed` reads it, with default for legacy structs.
- [x] Conversion module `crates/rustlab-script/src/eval/sparam_conv.rs`
      implementing `s_to_z`/`z_to_s`/`s_to_y`/`y_to_s` (N-port) and
      `s_to_t`/`t_to_s`/`s_to_abcd`/`abcd_to_s` (2-port). Pure-Rust per-
      frequency Gauss-Jordan via reused `matrix_inv`.
- [x] Cascade chain `cascade_s_chain` and de-embed `deembed_s` via
      T-parameter multiplication; `renormalise_s` via Z-domain detour.
- [x] Builtins: `s2z`, `z2s`, `s2y`, `y2s`, `s2t`, `t2s`, `s2abcd`,
      `abcd2s`, `cascade` (variadic), `deembed`, `newref`,
      `parameter_type`. All registered.
- [x] `save("foo.s2p", s)` wired into the unified save dispatcher via
      `is_touchstone_extension` + `save_touchstone_value`. Writer bumped to
      15 sig-figs so disk round-trip is lossless against f64.
- [x] REPL `HelpEntry` for every new builtin; category list extended.
- [x] `docs/functions.md` § "S-Parameters (RF)" extended with the
      conversion table, cascade/deembed/newref subsections, and the Touchstone
      save behaviour.
- [x] `docs/quickref.md` § "S-Parameters (RF)" rewritten with three
      sub-tables (construction/inspection, conversions, composition).
- [x] `AGENTS.md` builtin table extended for all new entries.
- [x] Example `examples/sparameters/cascade_attenuator.rlab` exercises
      construction, cascade (10 dB pad × 2 → 20 dB), every conversion
      with `parameter_type` introspection, deembed, and 50→75→50 newref
      round-trip. Verified output.
- [x] `cargo test --workspace` green. 32 new rustlab-script tests
      (1067 → 1099); 16 in `eval::sparam_conv::tests` and 16 in
      `tests::sparameters_phase2_tests`.

### Phase-2 implementation notes

- **Tag-based dispatch over multiple structs.** Every conversion builtin
  goes through one shared `convert_builtin` shell that unpacks the struct,
  optionally checks an expected source tag (`Some("S")` for `s2z`, etc.),
  runs the pure math closure, and repacks with a new tag. This kept ~10
  builtins under 100 lines instead of writing each one fresh.
- **N-port conversion path.** `s2z` / `z2s` / `s2y` / `y2s` work for any N
  via per-frequency `matrix_inv` (reused from the existing `inv()`
  builtin — turned into `pub(super)` for this). T and ABCD are
  2-port-only by definition and error otherwise. T-parameters
  internally power `cascade` and `deembed`; the user-facing builtins
  also exist for completeness.
- **`series_resistor` is *not* a usable Z-domain anchor.** First version of
  the conv tests used a series-R 2-port as the round-trip anchor — `(I − S)`
  is *exactly* singular for that network, because pure series-R has no
  finite Z representation (open-port voltage is undefined without a
  reference). Replaced with a matched 10 dB attenuator (well-conditioned
  in every domain) plus a synthetic non-degenerate "generic S" helper.
  Worth remembering for Phase 5 stability/gain testing: any pure
  series-impedance or pure shunt-admittance anchor is degenerate somewhere.
- **Display includes the type letter** so a converted network reads as
  `sparameters: 2-port Z, …` and the user immediately sees they're no
  longer looking at S. The s11/s12/s21/s22 accessors still slice the
  parameter tensor regardless — by design, they're "the (1,1) element of
  whatever your parameter set is."
- **Scripting-language gotcha for tests.** Initial round of phase-2 tests
  used `s.parameters(1, 1, 2)` to peek into the parameter tensor — the
  parser treated `parameters(...)` as a builtin call, not a field-access
  followed by indexing. Switched to `s21(s)(1)` (accessor builtin returns
  a vector; index it directly via chained-index syntax) and intermediate
  `P = s.parameters; v = P(1, 2, 1)` patterns, both of which the parser
  handles cleanly. Worth fixing in the parser eventually, but unrelated to
  Phase 2 scope.
- **Touchstone writer precision bump.** First round-trip-via-disk test
  failed at 3e-12 because the writer was using `.10e`. Bumped to `.15e`
  (full f64 mantissa) since RI claims to be lossless. Lossless against
  f64 now confirmed.

---

## Phase 3 — Smith chart plotting

**Status:** complete (2026-05-17)
**Depends on:** Phase 1

### 3a. `PlotKind::Smith` and grid type

- **Files:**
  - `crates/rustlab-plot/src/figure.rs` — add `PlotKind::Smith` variant,
    `SubplotState::smith_grid: Option<SmithGrid>` (enum `{ Z, Y, ZY }`).
  - All backends (`ascii.rs`, `file.rs`, `html.rs`, `viewer_live.rs`) —
    when `smith_grid` is set on a subplot, draw the grid before the series.

### 3b. Grid geometry

Pure-geometry helper in `crates/rustlab-plot/src/smith.rs`:

- Constant-R circles: centre `(R/(R+1), 0)`, radius `1/(R+1)`, drawn for
  `R ∈ {0, 0.2, 0.5, 1, 2, 5}` (the conventional set; configurable).
- Constant-X arcs: centre `(1, 1/X)`, radius `1/|X|`, clipped to the unit
  disk, for `X ∈ {±0.2, ±0.5, ±1, ±2, ±5}`.
- Outer unit circle.
- Real axis from −1 to +1.
- For `Y` mode, mirror the same arcs around the imaginary axis (constant
  G/B circles are the impedance circles negated in real part).
- For `ZY` mode, draw the Z grid in primary line colour and Y grid in a
  muted secondary colour.

**Tests** (each backend):
- SVG: assert the right number of `<circle>` / `<path>` elements is emitted
  with the expected radii (pin the convention).
- HTML: assert the Plotly shapes array has the same arcs.
- Terminal: smoke test that `smith()` runs and renders enough characters
  to fill the unit disk on an 80×24 grid.

### 3c. `smith()` builtin (unified)

One builtin that subsumes both calling conventions seen in the reference
toolbox (legacy `smithchart(gamma)` and modern `smithplot(...)`). rustlab
doesn't expose stateful figure-handle mutation post-construction, so the
two variants collapse cleanly into one.

- `smith(s)` — plot `s11` (and `s22` if `nports(s) ≥ 2`) of an `sparameters` struct.
- `smith(s, i, j)` — plot `sij` only.
- `smith(s, "ports", [1 1; 2 1])` — multi-pair selection, one trace per row.
- `smith(filename)` — convenience: load Touchstone, plot `s11` / `s22`. Equivalent to `smith(sparameters(filename))`.
- `smith(gamma)` — plot a complex `Vector` directly as reflection
  coefficients. (Load-pull contours, matching-network paths, anything
  that isn't a network object.)
- `smith(..., "grid", "Y")` — admittance grid (`"Z"` default, `"ZY"` for the immittance overlay).
- Each call adds traces to the current axes (same convention as `plot()`);
  multiple `smith(...)` calls before `figure_close` overlay. Auto-legend.
- **Tests:**
  - Load known `.s2p`, plot `s11`, assert first/last points match the
    file's first/last `S11` to 1e-12.
  - `smith(complex_vec)` form draws exactly one trace with the expected
    coordinates.
  - `"grid", "Y"` swaps the rendered grid arcs to admittance circles
    (assertion against `smith.rs` geometry output).

### 3d. Marker / annotation helpers

- `marker(gamma_value, label)` — drop a labelled point at an arbitrary
  reflection coefficient (matched-load designs, intermediate impedances).
- **Tests:** marker at `0` lands at chart centre; marker at `1` lands at
  the right edge of the real axis.

### Phase-3 deliverables checklist

- [x] Geometry module `crates/rustlab-plot/src/smith.rs` with
      `SmithGrid` enum (Z / Y / ZY), `SmithFamily` (Frame / Impedance / Admittance),
      and `build_grid(mode, resolution) -> Vec<SmithArc>`. Polylines clipped
      to the unit disk; NaN-broken segments where arcs leave/re-enter.
- [x] HTML backend patched to emit `showlegend: false` for empty-label
      series (so 17 grid arcs don't pollute the legend). SVG/PNG already
      had the `!label.is_empty()` guard. Terminal and viewer have no legend
      so they're unaffected.
- [x] `smith()` and `marker()` builtins registered, with all calling
      conventions: `smith(s)`, `smith(s, i, j)`, `smith(gamma)`,
      `smith(complex_scalar)`, `smith(file_path)`, `smith(..., "grid", "Z"|"Y"|"ZY")`.
- [x] 13 phase-3 script tests in `tests::sparameters_phase3_tests` covering:
      grid composition (17 arcs + 1 trace pattern), axis-equal + unit-disk
      bounds + axis labels, sparameters multi-trace dispatch (S11+S22),
      explicit port-pair form, grid-mode parsing (Y/ZY by detecting family
      colors), option-key validation, SVG polyline emission, HTML
      `showlegend: false` count + scaleanchor presence, PNG magic + size,
      marker scatter dispatch, marker-at-origin coordinates, unsupported
      arg-count rejection, Touchstone-path dispatch.
- [x] 8 geometry tests in `eval::smith::tests` (frame+arc counts,
      admittance mirror, immittance both-families, R=0 = unit circle,
      X-arc clipping with NaN breaks, parse-string-to-mode).
- [x] Example `examples/sparameters/smith_chart.rlab` loads the bundled
      `data/lna_demo.s2p`, plots S11+S22 with cardinal-point markers
      (matched/short/open), saves SVG+PNG+HTML, then makes a second
      figure with the ZY immittance grid. Runs cleanly.
- [x] REPL `HelpEntry` for `smith` and `marker`, category list extended.
- [x] `docs/functions.md` § "S-Parameters (RF)" extended with the Smith
      chart subsection (grid modes table, marker examples, cross-backend
      note); `docs/quickref.md` mirror entry.
- [x] `AGENTS.md` builtin table extended for `smith` and `marker` with
      the implementation rationale.
- [x] `cargo test --workspace` green (default features). Also `cargo
      test --workspace --features viewer` green — the viewer build inherits
      Smith support without code changes because the chart is just line series
      flowing through the existing `WireSeries`/`WirePlotKind::Line` path.

### Phase-3 implementation notes

- **The big design decision: don't touch the backends at all.** The plan
  originally called for `PlotKind::Smith`, a `SubplotState::smith_grid` flag,
  and per-backend dispatch in `ascii.rs`/`file.rs`/`html.rs`/`viewer_live.rs`.
  Once I saw how nyquist works (regular line series + `axis_equal`), I
  collapsed the design: the Smith grid is **just polylines**. The script
  builtin pushes them as ordinary `Series::Line` with empty labels and a
  muted color. Every backend (terminal, SVG, PNG, HTML/Plotly, LaTeX/PDF
  via SVG, animation GIF/HTML, live viewer) already renders that exact
  shape correctly — including the rustlab-viewer, which inherits Smith
  support without protocol or rendering changes. Workflow rule 9 (cross-
  backend consistency) is satisfied by construction rather than by N×N
  per-backend test pinning.
- **Empty-label suppression.** Only one backend change was needed: HTML
  was emitting every series into the Plotly legend regardless of label.
  Added `showlegend: false` when `series.label.is_empty()` (Line + Scatter
  variants). SVG/PNG already gate legend inclusion on `!s.label.is_empty()`.
  Terminal and viewer have no legend so they're unaffected. The "empty
  label = no legend" convention is now uniform across backends and is
  available for any future use beyond Smith charts.
- **NaN as polyline break.** Constant-X arcs are circles centred *outside*
  the unit disk — most of each circle sits outside and only a small arc
  lies inside. The `clip_to_unit_disk` helper drops outside points and
  inserts a `(NaN, NaN)` marker at each in→out transition. plotters,
  Plotly, and egui_plot all interpret NaN as a polyline break, so the
  visible arcs render correctly with no extra rendering logic.
- **Complex-scalar input form.** Initial `smith()` rejected
  `Value::Complex` and `Value::Scalar` because the plan implied "Vector
  or sparameters." Added both to make `smith(0.5 + 0.1*j)` work as a
  single-point trace — useful for marking matching-network endpoints
  without first wrapping in a vector. Bare scalars are promoted to
  `(scalar + 0j)`.
- **Scripting-language note (carried over from Phase 2).** Complex
  literals use the `j` constant, not a `j`-suffix syntax: write
  `0.5 + 0.1*j`, not `0.5+0.1j`. Tests that used the suffix form
  surfaced this and were rewritten.
- **Canvas geometry under axis_equal.** With the default 900×500 canvas
  and the unit-disk x/y range, axis_equal compresses the chart to a
  ~500-pixel square on the left side of the canvas, leaving the right
  side blank. Same behaviour as nyquist; users who want a tighter frame
  call `savefig` with a square canvas. Not a Smith-specific issue;
  declined to special-case it.

---

## Phase 4 — Network plots (mag/phase/dB vs freq)

**Status:** complete (2026-05-17)
**Depends on:** Phase 1; nice-to-have synergy with Phase 3 (gallery)

These are conventional 2-D plots, no new plot kind needed — just sugar over
existing `plot()` / `semilogx()`.

- `rfplot(s)` — default 2×2 panel for a 2-port: `|S11| dB`, `|S22| dB`,
  `|S21| dB`, `|S12| dB` vs frequency on log-x axis.
- `rfplot(s, "magnitude", i, j)` — single trace, linear magnitude
- `rfplot(s, "db", i, j)` — single trace in dB
- `rfplot(s, "phase", i, j)` — wrapped phase in degrees
- `rfplot(s, "unwrap", i, j)` — unwrapped phase
- `rfplot(s, "groupdelay", i, j)` — `-dφ/dω` via central difference on
  unwrapped phase

**Tests:** assert each variant emits the correct number of series and
that a `|S21| = 1` flat network plots to a flat dB-zero line.

### Phase-4 deliverables checklist

- [x] `rfplot(s)` default form: 2×2 review panel for a 2-port with
      `|S11|`, `|S21|`, `|S12|`, `|S22|` in dB, log-x frequency axis.
      Non-2-port fallback: single `|S11|` dB trace.
- [x] Single-trace forms: `rfplot(s, "db"|"magnitude"|"phase"|"unwrap"|"groupdelay", i, j)`.
- [x] Phase unwrap: standard ±2π jump rule on the running cumulative
      correction (`unwrap_phase_rad` in builtins.rs).
- [x] Group delay τ_g = −dφ/dω: central differences on the unwrapped
      phase, forward/backward at the endpoints (`group_delay_seconds`).
- [x] dB floor at −200 dB (matches the existing `mag2db` clamp behaviour)
      so matched-port |S11|=0 still plots.
- [x] REPL `HelpEntry` for `rfplot`; category list extended.
- [x] `docs/functions.md` § "rfplot(...) — magnitude / phase / group-delay
      vs frequency" with the kind table.
- [x] `docs/quickref.md` § "Network plots vs frequency" sub-table.
- [x] `AGENTS.md` builtin-table entry for `rfplot` with implementation
      notes.
- [x] Example `examples/sparameters/measurement_review.rlab` exercises
      the full "pulled-from-the-VNA" workflow: inspect, 2×2 review,
      single-trace dB/unwrap/group-delay, Smith cross-reference. Runs
      cleanly producing 6 artefacts.
- [x] 12 phase-4 script tests in `tests::sparameters_phase4_tests`:
      default 2×2 layout, panel→port mapping (S11/S21/S12/S22 in the
      canonical positions), dB values for matched 10 dB attenuator,
      magnitude/phase/group-delay for the same anchor, unwrap past π
      (cos/sin construction with 3π/4 step per sample), log-x axis
      values via `log10(f)`, 1-port fallback to single trace, HTML
      `xaxis4`/`yaxis4` 2×2 grid presence, kind/index validation.
- [x] `cargo test --workspace` green for both default and `--features
      viewer` builds. 1124 rustlab-script tests (+12 from Phase 3).

### Phase-4 implementation notes

- **Sugar over `semilogx`.** rfplot is intentionally thin — each variant
  computes the y-vector (magnitude, dB, phase, etc.) and pushes it
  through `builtin_semilogx`, which routes through `builtin_plot`. No
  new plot kind, no backend touches. Inherits every backend's existing
  rendering (terminal, SVG/PNG, HTML/Plotly, viewer, LaTeX/PDF,
  animation) the same way Phase 3 did.
- **Panel mapping bug caught in tests.** First version had the
  `(i, j, panel_idx)` tuples swapped: `(1, 2, 2, "|S21| (dB)")` produced
  a label "S12" (from `S{i}{j}`) inside the "|S21|" panel. Fixed to use
  the canonical RF convention where i is the *output* port and j is the
  *input*, so S21 = `sij(s, 2, 1)`. The first-round test failure
  surfaced the inconsistency immediately — exactly what the
  matched-attenuator round-trip test exists to catch.
- **dB floor for matched ports.** `mag2db` clamps at -200 dB for
  |S| ≤ 1e-10, so an ideal matched return loss plots as a flat line at
  -200 dB rather than -∞ (which would break the plotter). Tests assert
  the floor value directly.
- **Unwrap test construction.** Created a synthetic 8-sample sweep
  where the phase advances by 3π/4 per sample (more than ±π →
  wrapping). Unwrapped phase at the last sample should be
  7 × (3π/4) rad = 945°. Verified to 1e-6° (the tiny error is sin/cos
  rounding). Note the script-language complex literal is `cos(phi) + j*sin(phi)`,
  not `cos(phi)+j*sin(phi)` — the parser-gotcha carried over from
  Phase 3.
- **2×2 layout in HTML.** Plotly subplot grids show up as
  `xaxis`/`yaxis`/`xaxis2`/`yaxis2`/... in the layout block. The test
  pins this naming directly (rather than parsing JSON) since it's the
  contract every Plotly-compatible viewer follows.

---

## Phase 5 — Analysis: VSWR, return loss, stability, gain

**Status:** complete (2026-05-17)
**Depends on:** Phases 1–3 (Smith for gain/stability circles)

Each is a small, standalone builtin. All accept `sparameters` and return
real vectors (or, for circles, structures consumable by `smith()`).

| Builtin | Returns | Definition |
|---|---|---|
| `vswr(s, port)` | real `Vector` | `(1 + |Sii|) / (1 − |Sii|)` |
| `return_loss(s, port)` | real `Vector`, dB | `−20·log10(|Sii|)` |
| `insertion_loss(s, i, j)` | real `Vector`, dB | `−20·log10(|Sij|)` |
| `gammain(s, gamma_load)` | complex `Vector` | `S11 + S12·S21·ΓL/(1 − S22·ΓL)` |
| `gammaout(s, gamma_source)` | complex `Vector` | `S22 + S12·S21·ΓS/(1 − S11·ΓS)` |
| `stabilityk(s)` | real `Vector` | Rollett's K: `(1−|S11|²−|S22|²+|Δ|²)/(2·|S12·S21|)` |
| `stabilitymu(s)` | real `Vector` × 2 | µ-parameters (single tuple return) |
| `gammams(s)`, `gammaml(s)` | complex `Vector` | Simultaneous-conjugate-match source/load Γ |
| `gainmax(s)` | real `Vector`, dB | `MAG`/`MSG` per Rollett K |
| `stability_circles(s, type)` | struct | centre + radius vs freq for input/output stability; renders as scatter on the Smith chart |
| `gain_circles(s, gain_db)` | struct | constant-gain circles |

**Tests:** for each, verify against an analytic case
(e.g., `K → ∞` for an isolator; `VSWR = 1` for `S11 = 0`; `MAG = 20 dB`
matches hand-computed value on a known amplifier `.s2p`).

### Phase-5 deliverables checklist

- [x] New math module `crates/rustlab-script/src/eval/sparam_analysis.rs`
      with all per-frequency formulas: `vswr`, `return_loss_db`,
      `insertion_loss_db`, `gamma_in`, `gamma_out`, `stability_k`,
      `stability_mu`, `gamma_ms`, `gamma_ml`, `gain_max_db`,
      `input_stability_circles`, `output_stability_circles`,
      `gain_circles`. Internal `TwoPortAtF` slice carries Δ and
      `|S12·S21|` so the formulas don't re-derive intermediates.
- [x] Quadratic-root selector (`pick_quadratic_root`) for Γms/Γml
      picks the root with magnitude < 1.
- [x] 13 script-level builtins registered: `vswr`, `return_loss`,
      `insertion_loss`, `gammain`, `gammaout`, `stabilityk`,
      `stabilitymu` (multi-return Tuple), `gammams`, `gammaml`,
      `gainmax`, `stability_circles`, `gain_circles`, `smith_circle`.
- [x] Tagged circles struct (`__kind__ = "stability_circles"` /
      `"gain_circles"`) with fields `centres` / `radii` /
      `frequencies` / `domain`. `domain` is `"source"` (for input
      stability) or `"load"` (output stability / gain circles).
- [x] `smith_circle(centre, radius [, label])` overlay helper renders
      one parametric circle as a 96-vertex solid line series; pairs
      with the circles structs via user-iteration over the `centres`/`radii`
      fields. Cross-backend by construction (just a line series).
- [x] 17 math tests in `eval::sparam_analysis::tests` against analytic
      anchors: matched-attenuator K = 5.05, µ1 = µ2 = 10, MAG = -10 dB,
      Γms/Γml = 0, isolator K → ∞, simultaneous-conjugate-match
      defining identity `Γin(Γml) = conj(Γms)` for the toy amp, plus
      VSWR / return-loss / insertion-loss anchors and gammain
      broadcast/length-mismatch behaviour.
- [x] 19 script tests in `tests::sparameters_phase5_tests` covering
      every builtin: matched-attenuator anchor values (VSWR=1, RL=200,
      IL=10), gammain/gammaout thru-network passthrough, per-frequency
      vector broadcast, stabilitymu multi-return destructuring, tagged
      circles struct schema, 2-port-only rejection for every analysis
      builtin handed an N-port network, smith_circle adds correct
      series with min/max x-extent at radius bounds.
- [x] Example `examples/sparameters/amplifier_stability.rlab` loads
      `data/lna_demo.s2p` and produces a full per-frequency
      stability/gain report (K, µ1, µ2, VSWR×2, RL×2, MAG, Γms/Γml,
      forward IL), then overlays input stability circles on a Smith
      chart and saves SVG+HTML. Verified output:
      ```
      f/GHz     K     mu1    mu2   VSWR1   VSWR2   RL1/dB   RL2/dB   MAG/dB
      1.00   2.57    1.72   1.52    3.00    2.33     6.02     7.96    10.06
      ...
      6.00   1.75    1.60   1.54    2.03    1.86     9.37    10.46     8.78
      ```
      Network is unconditionally stable across the whole 1–6 GHz band
      (K > 1, µ1 > 1).
- [x] REPL `HelpEntry` for all 13 builtins; category list extended.
- [x] `docs/functions.md` § "Analysis: VSWR, return loss, stability,
      gain" + `smith_circle` subsection.
- [x] `docs/quickref.md` Analysis sub-table added.
- [x] `AGENTS.md` builtin-table entries for all 13 functions with
      implementation notes.
- [x] `cargo test --workspace` green for both default and `--features
      viewer` builds. 1160 rustlab-script tests (+36 from Phase 4).

### Phase-5 implementation notes

- **Math module split.** Phase 5 math goes in `eval/sparam_analysis.rs`
  next to the existing `eval/sparam_conv.rs`. Conversions are about
  changing representation; analysis is about extracting derived
  quantities. Keeping them separate matches the natural concern
  boundary and makes either module easy to extend without churning
  the other.
- **The `TwoPortAtF` helper.** Most analysis formulas need
  `Δ = S11·S22 − S12·S21` and `|S12·S21|`. Per-frequency loops were
  duplicating these. Extracting a tiny struct that computes them once
  per slice halved the per-formula visual noise and put the
  definitions in one place where the reader can audit them.
- **Quadratic-root selection.** Γms/Γml come from solving a quadratic
  whose two roots differ in which side of the unit circle they fall.
  `pick_quadratic_root(b, c)` returns `(b − sign(b)·√(b² − 4|c|²)) / (2c)`,
  which is the form that gives the |Γ| < 1 root for unconditionally
  stable networks. For K ≤ 1 the discriminant goes negative and the
  formula still returns a complex value, but callers should be
  filtering on K first.
- **Gain-circle numerical wobble at MAG.** The constant-gain-circle
  discriminant is `1 − 2·K·gp·|S12·S21| + (gp·|S12·S21|)²`. At
  `gain = MAG` this is exactly zero in real arithmetic; f64 rounds
  it slightly negative. A test failed initially because the radius
  came back NaN. Fixed by clamping `inside.max(0.0).sqrt()` when
  `inside > -1e-9` — preserves the geometric reality that the limiting
  case is a single point with radius zero. Only gains genuinely beyond
  MAG (well past the tolerance) still produce NaN radii.
- **The script-language parser gotcha (third time).** Phase 5's
  `amplifier_stability.rlab` initially called
  `in_circles.centres(k)` to extract per-frequency centre values.
  The parser reads that as a call to a function named `centres`, not as
  field access + index. Fix: stash `in_circles.centres` and
  `in_circles.radii` into intermediate variables first, then index. This
  has now bitten Phases 2, 3, and 5 — worth a small parser change
  someday but explicitly out of scope for Phase 5.
- **`stability_circles` and `gain_circles` return tagged structs**
  rather than raw vectors so the user can keep the per-frequency
  metadata together and so a future helper could overlay them with
  a single call (e.g. `smith_circles(struct)`). For now overlay is
  a 4-line loop; if usage shows it's repetitive, that helper lands
  in Phase 6 polish.
- **VSWR cap, return/insertion-loss floor.** VSWR is mathematically
  infinite at |S| = 1 (full reflect); return loss is infinite at
  |S| = 0 (perfect match). Both directions produce un-plottable
  values without a cap. Convention: VSWR capped at 1e6, dB values
  floored at 200 dB (matching the existing `mag2db` clamp). Both
  caps are documented in the help text.

---

## Phase 6 — Polish

**Status:** complete (2026-05-17)

Implemented:
- ✅ `interp_freq(s, new_freqs)` — linear S-parameter interpolation onto a
  monotonic frequency grid; rejects extrapolation.
- ✅ Touchstone noise-parameter parsing — reader picks up the optional
  5-column block when present; accessors `nfmin`, `gamma_opt`, `rn`,
  `noise_freqs`, and `has_noise` guard expose the data.
- ✅ `s2td(s, i, j [, "impulse"|"step"])` — time-domain (step/impulse)
  response via IFFT with a 2N-point conjugate-symmetric spectrum.
- ✅ Mixed-mode `s2smm` / `smm2s` — 4-port single-ended ↔ differential
  via the standard Bockelman/Eisenstadt orthogonal transformation.
- ✅ Touchstone v2 keyword tolerance — `[Version] 2.0` files with
  v1-compatible layouts parse cleanly; `[Reference]` scalar overrides
  the header default.

Deferred with reason:
- **Octave-comparison harness entries** — stock Octave has no
  `sparameters` equivalent, so adding harness entries means hand-writing
  every formula in `.m` which buys no independence. The 17+ analytic
  anchor tests in `eval::sparam_analysis::tests` and
  `eval::sparam_conv::tests` (matched attenuator, isolator, thru
  network, simultaneous-conjugate-match identity, lumped-element ABCD,
  series-resistor cascade) provide stronger independent verification
  than re-implementing the same formulas in `.m` would.
- **Full Touchstone v2** with per-port `[Reference]` Z0 lists and
  `[Mixed-Mode-Order]` tables — significant new parser surface for
  diminishing returns; rejected with a clear error in v6 so users know
  the workaround (single-ended `.s4p` export + `s2smm` post-load).

---

## Risk register

| Risk | Mitigation |
|---|---|
| **Touchstone parser corner cases** (column-major vs row-major ordering, embedded comments, line continuation, mixed whitespace) | Build a corpus of real-world `.s2p`/`.s3p` files from public Keysight/MiniCircuits/Modelithics distributions; round-trip every one in a test. |
| **Smith grid renders differently across backends** | Centralise the geometry in `rustlab-plot/src/smith.rs` returning `Vec<(centre, radius, arc_span)>`; each backend draws from the same list. Per workflow rule 9, pin each backend's emitted output in a test. |
| **API drift between "industry-convention" field names and our docs** | Document the field set once in `docs/functions.md` § S-Parameters and re-export the same list in REPL help. Any future field rename is a deliberate breaking change reviewed up front. |
| **Numerical conditioning of S↔Z conversion near singular S matrices** | Formula uses `(I − S)⁻¹`; ill-conditioned when `S` has an eigenvalue near 1 (open circuit on one port). Document the limitation and emit a warning when the condition number exceeds a threshold; do not silently return garbage. |
| **MATLAB-naming rule conflict** | Resolved: MATLAB docs are the design reference (user-approved), but no MATLAB references in shipped artefacts. See "MATLAB-as-reference policy" above. |
| **Scope creep into RF circuit synthesis** | Out of scope. This plan is *analysis* of measured/simulated S-parameters, not synthesis (matching networks, filter design from prototypes, etc.). If those land later they're a separate plan. |

---

## File / crate impact summary

| Crate | New files | Modified files |
|---|---|---|
| `rustlab-script` | `src/eval/touchstone.rs` | `eval/builtins.rs` (new builtins), `eval/value.rs` (Display for sparameters struct), `tests.rs` |
| `rustlab-plot` | `src/smith.rs` | `figure.rs` (PlotKind::Smith, SubplotState), `ascii.rs`, `file.rs`, `html.rs`, `viewer_live.rs` |
| `rustlab-cli` | — | `commands/repl.rs` (HelpEntry + category) |
| docs | `examples/sparameters/*.rlab`, `examples/sparameters/data/*.s2p` | `docs/functions.md`, `docs/quickref.md`, `docs/examples.md`, `AGENTS.md`, `README.md` (one-line capability mention) |

No new workspace dependencies expected. Touchstone is plain-text ASCII;
parser is hand-rolled per workflow rule 10. Smith geometry is plain
arithmetic.

---

## Test strategy summary

- **Unit tests** in each crate's `tests.rs` for every builtin and every
  conversion formula.
- **Integration tests** in `crates/rustlab-cli/tests/examples.rs` that run
  each `.rlab` example end-to-end and assert it produces the expected
  artefacts.
- **Backend pinning** per workflow rule 9: SVG, HTML, terminal each pin
  the Smith grid rendering.
- **Round-trip tests** for Touchstone read/write, all parameter
  conversions, and renormalisation.
- **Reference cases**: hand-computed S matrices for series resistor, shunt
  resistor, ideal attenuator, ideal isolator. These are the analytic
  anchors that catch sign / factor-of-2 / row-vs-column-major errors.
- **Octave-comparison** entries deferred to Phase 6 (Octave doesn't ship
  Touchstone reading by default; comparison would need a custom .m
  preamble per case).

---

## Next step

Plan is ready for final user review. On approval, start Phase 1 (Touchstone
I/O + `sparameters` constructor + inspection builtins).

---

## Phase 6 implementation notes

- **`interp_freq` design choice: rejected extrapolation.** RF measurements
  are bandlimited; extrapolating S-parameters past the swept range gives
  worse answers than failing. The error message tells the user the source
  range so they know how far they're off.
- **Touchstone reader restructure.** The v1.1 reader assumed `tokens.len()
  % per_record == 0` and read all records in one pass. Adding noise-block
  detection (which is signalled implicitly by a strictly-decreasing
  frequency transition for 2-port files) required restructuring to a
  one-record-at-a-time consumption with `cursor` advance, plus a
  branch to switch into noise-mode after exhausting the S-block.
  Two prior tests (`mismatched_data_count_errors`, `non_monotonic_freqs_error`)
  updated to match the more nuanced new error messages.
- **`handle_v2_keyword` is allow-list-shaped.** Recognised keywords are
  consumed and ignored when their value matches the v1 default; unknown
  ones surface a clear error. Per-port `[Reference]` lists explicitly
  rejected with a workaround suggestion. `[Mixed-Mode-Order]` rejected
  with a suggestion to use single-ended export + `s2smm`.
- **`s2td` made deliberate choices.** No DC extrapolation: for a spectrum
  that starts at f₀ > 0 (the usual VNA case), the IFFT gives the baseband-
  equivalent (band-limited-pulse) response — standard VNA TDR behaviour.
  Could add DC extrapolation later if it turns out users want a different
  default. Uniform-grid check (rejects with the `interp_freq` workaround)
  keeps the IFFT math simple.
- **Mixed-mode `M` is orthogonal**, so the inverse is just `Mᵀ` — no
  matrix-inverse needed. The Bockelman/Eisenstadt convention is the one
  every commercial mixed-mode VNA uses; documented in the function help
  and AGENTS.md.
- **Touchstone writer is still S-only.** A network with noise data
  loaded via `sparameters("x.s2p")` and re-saved via `save("y.s2p", s)`
  loses the noise block. Documented in the AGENTS entry; would be a small
  follow-up if users hit it.
- **Test count:** 1185 in rustlab-script (+25 from Phase 5: 7 conv-math,
  6 noise/v2 in touchstone module, 12 phase-6 script tests).
