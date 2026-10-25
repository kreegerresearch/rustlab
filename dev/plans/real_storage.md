# Real-valued array storage

**Status:** first slice shipped in 0.3.17 (perf flywheel iteration 03,
`dev/plans/perf_flywheel_iter03.md` phase 3). As built, the storage lives in
`Shared<T>` itself (the name and call sites stay; `Store<D>` below is the
design name). Not yet converted: Tensor3 kernels, the less common builtins,
NPY `save` choosing `<f8` by storage (it still checks the values, with the
same result).
**Source request:** `../rustlab_llm/perf/iterations/02-requests.md` § R4.
**Builds on:** copy-on-write arrays (`eval/shared.rs`, iteration 02 item 2).

## Why

Every `Value::Vector` / `Matrix` / `Tensor3` element is a `Complex<f64>`, 16
bytes, even when the imaginary part is zero, which it is for almost all
data. After iteration 02 the training workload is bound by memory traffic,
not arithmetic. `par.grad_flat_B8` runs 8 windows on 8 cores at 1.78× the
serial time of one, and the gap does not move with allocator or scheduling
changes (iteration 02 item 3). Each window allocates, copies and returns
16-byte numbers. Halving the bytes is the lever left.

Measured in `profile_step.rlab` (rustlab 0.3.10):

| Path | Today | With real storage |
|---|---:|---:|
| `parmap` output, 8 × 811,009 numbers | 101,376 KB | 50,688 KB |
| `zeros` per 8 windows | 300 MB | 150 MB |
| `reshape` per 8 windows | 103 MB | 52 MB |

**Checkpoints are already real.** The NPY writer emits `<f8` when every
imaginary part is (near) zero: `ckpt_001000.npz` (`theta` only) is 6,488,804
bytes, 8 per parameter, and `ckpt_latest.npz` (`theta`, `m`, `v`) is 19.5 MB,
3 × 8 per parameter. Real storage does not shrink checkpoints. `load` does
get cheaper: today it widens `<f8` to 16 bytes in memory, and with real
storage it would not. The request's 9.7 MB figure would need `f32`.

## Representation

The storage choice goes **inside the copy-on-write handle**, not into new
`Value` variants. There are about 1,150 `Value::Vector` / `Matrix` / `Tensor3`
match sites (492 in `builtins.rs` alone). With new variants, every site would
need new arms, and a missed site would fall into a `_ =>` arm and be wrong or
an error. With the choice inside the handle, a site not yet converted stays
correct and is only slower.

```rust
// eval/array_store.rs (new), replacing Shared<CVector> etc. in Value
pub struct Store<D: Dimension>(Arc<Inner<D>>);

struct Inner<D> {
    /// Authoritative when Some: every element is real.
    real: Option<Array<f64, D>>,
    /// Authoritative when `real` is None; otherwise a cache of the real
    /// data widened to complex, filled on first complex read.
    complex: OnceLock<Array<C64, D>>,
}

pub type VStore = Store<Ix1>;  // Value::Vector(VStore)
pub type MStore = Store<Ix2>;  // Value::Matrix(MStore)
pub type TStore = Store<Ix3>;  // Value::Tensor3(TStore)
```

API:

- `Deref<Target = Array<C64, D>>`: the complex view. For real data it widens
  the buffer once into the `OnceLock` cache (`x → C64(x, 0.0)`). This is what
  keeps the existing call sites compiling and correct.
- `as_real(&self) -> Option<&Array<f64, D>>`: the fast path for converted
  code.
- `make_mut(&mut Self) -> &mut Array<C64, D>`: copies when shared (as today)
  and makes the buffer complex. Used by write sites that are not converted.
- `make_mut_real(&mut Self) -> Option<&mut Array<f64, D>>`: copies when
  shared, `None` when the data is complex. A write of a real value tries this
  first.
- `From<Array<f64, D>>` and `From<Array<C64, D>>`, so builtins return either
  one with `.into()`.
- `bytes()`: storage size, for the profiler's KB columns (`value_bytes`).

A write clears the cache. `get_mut`, `is_unique`, `ptr_eq` and `into_inner`
keep their copy-on-write meaning. There is still no `DerefMut`.

Room for later: an `f32` mode becomes a third authoritative field, with the
same widening cache, and no `Value` change.

## When a value is complex

**Storage is invisible.** Everything a script can observe (printing,
comparisons, `save` output, cache fingerprints, `parmap` results) depends on
the values only, never on which storage holds them. A complex buffer whose
imaginary parts are all zero behaves exactly like a real one. The few
exceptions are listed under "Behavior changes" below.

Real storage comes from producers that know their output is real:

- the constructors `zeros`, `ones`, `rand`, `randn`, `randi`, `linspace`,
  ranges `a:b:c`, numeric `[…]` literals with no complex element, and `load`
  of `<f8` / integer NPY;
- elementwise and matrix operations whose operands are both real-stored;
- `real`, `imag`, `abs`, `angle`, `sum`, `mean`, comparisons, and the rule-based
  sites that already zero the imaginary part today (real-in, real-out for
  `.* ./ .^`, and the real path of `matmul_conj`);
- the scalar-function builtins (`apply_scalar_fn_promoting`), when no element
  needs the complex branch: `sqrt([4, 9])` is real, `sqrt([-4, 9])` is complex.

A value becomes complex when:

- an operand is complex (`v * 1j`, `v + z`, `[v, 2j]`);
- a builtin's range is complex for that input (`sqrt`, `log`, `asin`, `acos`,
  `.^` with a negative base and a non-integer exponent);
- a builtin is complex by nature (`fft`, `ifft`, `eig`, `roots`, `freqz`,
  `stft`, …): these return complex storage even when the imaginary parts
  happen to be zero, as today;
- a complex element is written into it (`v(3) = 2j`, a region write with a
  complex right-hand side): the whole buffer is converted, copy-on-write as
  usual, so `b = a; b(1) = 1j` leaves `a` real.

**No automatic demotion.** Results are never scanned to check whether they
could go back to real storage, because that costs a pass over every result.
A complex result with zero imaginary parts stays complex-stored until a
rule-based site makes it real (`real(x)`, a real-in/real-out operation). It
still prints and compares as real, as it does today.

## Builtin migration

1. **Day one is correct without conversions.** An unconverted builtin calls
   `to_cvector()` / `to_cmatrix()` or derefs to `&CVector`, gets the widened
   cache, and returns complex storage. The cost is one widening per value
   (24 bytes per element held while the value lives) and a complex result.
2. **Measure, then convert.** `--profile` gets two pseudo-rows: `[widen]`
   (complex views built from real storage: count and KB) and `[promote]`
   (real buffers converted to complex by a write). These rows rank what to
   convert next, which beats guessing across 1,150 sites.
3. **Converting a builtin** means reading `as_real()` first, with a real
   kernel, and returning `Array<f64, _>.into()`, keeping the complex path
   unchanged. Elementwise kernels get `f64` versions of the non-generic
   drivers (`drive`, `drive_new`), one more rayon instantiation each, not one
   per operator.
4. **Coercion helpers.** Add `to_rvector()` / `to_rmatrix()` returning
   `Cow<Array<f64, _>>` (borrowed for real storage; for complex storage,
   built only when every imaginary part is zero). Builtins that need real
   input and error on complex input (window, filter design, sizes, indices)
   switch to these and stop building complex copies just to read `.re`.

`IntArray` keeps its own storage. It widens to whichever form is cheaper:
real, once real storage exists.

## The first slice (iteration 03)

The paths `profile_step.rlab` exercises, in order. After each step the
workspace tests and `perf-check` must pass.

| # | Step | Files |
|---|---|---|
| 0 | `Store<D>` with `Deref` widening, swapped into `Value`; every producer still complex. No behavior change; the compiler finds every `&**v` site that needs an explicit `&***v` / `.c()` | `eval/array_store.rs` (new), `value.rs`, mechanical edits elsewhere |
| 1 | `[widen]` / `[promote]` profile rows; `value_bytes` reports storage size | `profile.rs`, `mod.rs` |
| 2 | Real producers: `zeros`, `ones`, `rand`, `randn`, ranges, real literals, `load` of `<f8` | `builtins.rs`, `value.rs` |
| 3 | Real consumers: elementwise (`f64` drivers), `matmul_conj` (skips the `real_parts` copy for real-stored operands), indexing read and write (`par_block_copy`, `region_write_runs`), `reshape` / transpose (`par_transpose` over `T: Copy`), one-row `[…]` (`par_fill`), the `apply_scalar_fn_*` family, `sum` / `mean` / `max` | `elementwise.rs`, `rustlab-core/src/matmul.rs`, `builtins.rs`, `value.rs`, `mod.rs` |
| 4 | `parmap` packing: real per-task results pack into a real `Matrix` / `Tensor3` | `parmap.rs` |
| 5 | `save`: chooses `<f8` / `<c16` by storage, or exact zero imaginary parts, instead of the `1e-12` tolerance | `builtins.rs` |
| 6 | Whatever `[widen]` still shows in `profile_step.rlab` | by profile |

Acceptance, from the request: the `parmap` row reports about 50,688 KB out;
`par.grad_flat_B8` improves by 10 % or more over iteration 02; gate PASS with
the trajectory bit-identical to 0.3.11 (the division change already landed).

Binary size: the `f64` drivers and the `T: Copy` transpose are a few new
monomorphizations. Measure them against the 0.3.11 binary; the estimate is
30–60 KB.

## Behavior changes (fixed ahead of the slice, 0.3.11)

Real storage would have changed three results. They were fixed on complex
storage first (2026-10-19, CHANGELOG 0.3.11 *Breaking / behavior changes*;
tests `real_value_semantics` in `rustlab-script/src/tests.rs`), so the slice
itself can be held to bit-identical results. The `rustlab_llm` trajectory
moved by 2.0e-16 relative.

1. **Array `./` matches scalar `/`.** Today a real-valued array division runs
   complex division, `(x·y + 0·0) / (y·y + 0·0)`. On 100,000 random pairs,
   34,998 results differ from `x / y` in the last digit, and
   `[1e200] ./ [1e200]` is `NaN` because `y·y` overflows. Real storage
   computes `x / y`. This is the "within rounding" part of the training
   trajectory.
2. **No negative-zero imaginary parts.** `-v` of a real array leaves `-0.0`
   in every imaginary part today, so `angle(-[1, 2])` is `-π`, not `π`.
   Anything that reads the sign of a zero imaginary part through `atan2` or a
   branch cut (`angle`, `sqrt` and `log` on the complex path) changes.
3. **NPY tolerance.** `save` drops imaginary parts below `1e-12` today, which
   loses data silently. It will write `<c16` unless every imaginary part is
   exactly zero.

How: `Value::div_c64` (a real divisor divides each part; used by every
array division site and the elementwise kernels), `Value::neg_c64` (`0.0 - im`)
in `negate`, and `Value::unsigned_zero_im` before `arg` in `angle`,
`pow_c64` and the complex branch of `apply_scalar_fn_promoting`. CSV cells use
the same exact rule as NPY. TOML has no complex numbers, so a value with any
nonzero imaginary part is saved as a `{ __complex_re, __complex_im }` table of
its parts instead of dropping those below `1e-15`.

## Risks

| Risk | Mitigation |
|---|---|
| A hot path that is not converted widens on every call and ends up slower than today (a 16-byte copy on top of the 8-byte buffer) | `[widen]` row; step 6 of the slice; `perf-check` after each step |
| Widened caches double memory for values that stay alive | The cache is filled only when an unconverted site reads it; `[widen]` shows the KB |
| An unconverted write site promotes a buffer that never needed it, and the speedup disappears silently | `[promote]` row; region writes and `v(i) = x` are converted in step 3 |
| The persistent function cache sees two storages of one value as different keys | Fingerprints hash values (real when every imaginary part is zero), not storage; test both |
| `OnceLock` widening races in `parmap` workers | `OnceLock` is `Sync`; the cache is filled once and only read afterwards |

## Test strategy

- **Differential run.** A test-only switch (thread-local) makes every
  producer build complex storage. The example scripts and the `tests.rs`
  snippets run in both modes, and the outputs must be identical apart from
  the three behavior changes above, which get their own tests.
- **Kernels.** For each converted kernel, the real result equals the real part
  of the complex kernel bit for bit (same pattern as the existing
  `matches_binop_bit_for_bit` tests), except division, which equals the
  scalar `/`.
- **Promotion and copy-on-write.** `v = zeros(1, 3); v(2) = 1j` is complex;
  `b = a; b(1) = 1j` leaves `a` real-stored; `sqrt([-4, 9])` is complex and
  `sqrt([4, 9])` is real.
- **Round trips.** `save` / `load` of real-stored and complex-stored values;
  `parmap` with real and complex task results; cache fingerprints equal
  across storage.
- **Gate.** `RUSTLAB=$PWD/target/release/rustlab make -C ../rustlab_llm
  perf-check`.

## Decisions (2026-10-19)

1. Division and negative-zero fixes: done ahead of the slice (above).
2. No automatic demotion of complex results with zero imaginary parts.
3. Exact NPY (and CSV / TOML) writers: done.
