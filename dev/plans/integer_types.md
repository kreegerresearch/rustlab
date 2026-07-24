# Integer types — tagged-width integers with efficient storage

## Agent handoff — read this first

**Where we are:** Design complete, scope signed off with the user (2026-07-23).
**No code has landed.** The locked-in decisions below are agreed scope and are
*not* up for renegotiation without explicit user approval.

**What this adds:** a real integer type so scripts can define, store, and
operate on integers efficiently, instead of every number being a
`Complex<f64>` (16 bytes/element, imaginary half wasted). Tagged-width
(`int8…int64`, `uint8…uint64`) with a selectable overflow policy.

**Compatibility stance: hybrid MATLAB** (locked 2026-07-23). MATLAB-faithful
except two deliberate, labeled deviations — **A** `int + double → double`
(MATLAB keeps it integer) and **B** an opt-in `Wrap` overflow mode (MATLAB only
saturates). See "Locked-in design decisions".

**Phase progress at a glance:**

| Phase | Milestone | State | Headline deliverable |
|-------|-----------|-------|----------------------|
| 0 — Design & scoping | M0 | **complete** (2026-07-23) | this document; type model + hybrid-MATLAB semantics locked with the user |
| 1 — Scalar integer + widening + literals | M1 | **in progress** | ✅ `IntClass` (core) + `Value::Int` + widening at all 5 chokepoints + Display/`whos`; ⬜ literals, casts, arithmetic, `class()`/`intmax`/… builtins |
| 2 — Packed integer arrays + indexing | M2 | not started | `IntArray` packed storage, array constructors, integer index vectors |
| 3 — Cross-class width semantics | M3 | not started | cross-class-mix errors, lossy-narrowing saturation, full `uint64` range |
| 4 — I/O & interop | M4 | not started | NPY int dtypes (also fixes today's "can't load numpy int arrays" gap), CSV, TOML, `whos` class reporting |

Full milestone acceptance criteria are in the **Milestones** section below.

**Next concrete action:** start Phase 1. Land the `Value` enum + coercion
changes first and get a review before fanning out into constructors and
arithmetic (per the user's request).

## Motivation

Every numeric value today is stored as `Complex<f64>`:
`Scalar(f64)`, `Vector(Array1<C64>)`, `Matrix(Array2<C64>)`. A real integer
vector costs 16 bytes/element with the imaginary half always zero, and there
is no way to express integer semantics (fixed width, wraparound, saturation)
that the project's fixed-point / DSP surface (`qfmt`, `quantize`, `bitand`
family once added) naturally wants. Users asked to "efficiently define, store,
and operate on integers." This plan delivers that as an additive type, not a
rewrite.

## Locked-in design decisions (signed off 2026-07-23, hybrid MATLAB compat)

**Overarching goal (locked 2026-07-23): hybrid MATLAB compatibility.**
MATLAB-faithful wherever it's low-cost, with **two deliberate, clearly-labeled
deviations** that favor least-surprise for new code:

- **Deviation A — `int + double → double`** (decision 4). MATLAB makes the
  integer class dominate (`int8(1)+2.7 → int8(4)`); we promote to `double`
  instead. Documented loudly for users porting MATLAB code.
- **Deviation B — opt-in `Wrap` overflow** (decision 3). MATLAB only
  saturates; `Wrap` is an extension MATLAB-ported code never selects.

Everything else follows MATLAB.

1. **Tagged-width integer, not N enum variants and not a single width.**
   One packed integer value carries metadata `{ class: IntClass, overflow:
   OverflowMode }` where `IntClass ∈ {int8, int16, int32, int64, uint8,
   uint16, uint32, uint64}`. This gives per-width semantics (`int32(x)`,
   `class(x)`, casts, saturation) without eight `Value` variants.
2. **Storage: i128-backed (MATLAB-compat).** Stored in `i128` (scalar) /
   `Vec<i128>` (array) so the **full `uint64` range** (2⁶⁴−1, which overflows
   `i64`) is representable — MATLAB supports full `uint64`. The class tag
   enforces range on construction/arithmetic. True-narrow packing (i8 = 1 byte)
   is a later optimization (Phase 2 stretch) that does **not** change the
   script-visible surface.
3. **Overflow default Saturate; `Wrap` is an opt-in extension (Deviation B).**
   Reuse `rustlab_core::OverflowMode { Saturate, Wrap }` (already used by
   `qfmt`). **Saturate is the default and the MATLAB-faithful behavior.**
   `Wrap` rides on the value (`int8(x, "wrap")`) and propagates through ops.
4. **Mixed `int + double → double` (Deviation A).** NumPy-style, least-surprise
   for a numeric compute environment; no hidden saturation of a float result.
   `int ⊗ int` of the **same** class stays that class under the value's overflow
   mode.
5. **Mixing different integer classes is an error (MATLAB rule).**
   `int8(x) + int16(y)` raises `operands have different integer classes (int8
   vs int16); cast one explicitly`. No silent widening. (Replaces the earlier
   "resolve to wider class" idea.) The only legal integer combinations are
   therefore same-class `int ⊗ int` (decision 4) and `int ⊗ double` (which
   promotes to `double` per Deviation A). MATLAB has the same cross-class
   prohibition but keeps `int + scalar double` integer — that difference *is*
   Deviation A.
6. **Double→integer conversion rounds half away from zero (MATLAB rule).**
   `int8(2.5) → 3`, `int8(-2.5) → -3`. Applied at every cast site (constructors,
   `cast`, `intN(...)`). One shared `IntClass::from_f64(x, mode)` helper. (Not
   an arithmetic concern — Deviation A means `int + double` never rounds; the
   double result is exact.)
7. **Bare `0x`/`0b`/`0o` literals take the smallest fitting unsigned class
   (MATLAB rule).** `0xFF → uint8`, `0xFFFF → uint16`, `0x1_0000 → uint32`,
   `0x1_0000_0000 → uint64`. Not `double`. (Typed suffixes like `0xFFu8` remain
   deferred — see Non-goals; the cast form covers them.)
8. **Widening at the coercion chokepoints is the compatibility mechanism.**
   Integers widen to `f64`/`C64` inside the five `Value` coercion methods, so
   all 286 existing builtins keep working untouched. Only integer-*producing*
   and integer-*aware* functions get new code.

## Architectural facts the plan rests on (verified — do not re-derive)

- Storage aliases (`crates/rustlab-core/src/types.rs`): `C64 =
  Complex<f64>`, `CVector = Array1<C64>`, `CMatrix = Array2<C64>`, `CTensor3 =
  Array3<C64>`. There is **no** integer storage type today.
- The `Value` enum lives in `crates/rustlab-script/src/eval/value.rs`
  (~2749 lines). ~1422 `Value::{Scalar,Vector,Matrix,Complex}` references
  exist but are concentrated in **7 files** under
  `crates/rustlab-script/src/`.
- **The five coercion chokepoints** (all methods on `Value` in `value.rs`),
  with builtin call counts — these are where widening goes:
  `to_scalar` (105 calls), `to_usize` (100), `to_cvector` (57),
  `to_cmatrix_arg` (35), `to_real_vector` (13).
- Arithmetic funnels through one path: `Value::elementwise_with_broadcast(a:
  &CMatrix, b: &CMatrix, op: BinOp)` in `value.rs` (~line 425), with implicit
  expansion via `broadcast_pair`.
- `OverflowMode { Saturate, Wrap }` already exists in
  `crates/rustlab-core/src/types.rs:547` with `from_str` (`"saturate"|"sat"`,
  `"wrap"`); `parse_overflow_mode` in `builtins.rs:3719` already maps a script
  string to it (used by `qfmt`, default `Saturate`).
- Display already special-cases integer-valued floats
  (`value.rs:360`: `if e.fract() == 0.0 && e.abs() <= i32::MAX as f64`), so an
  integer type displays naturally with minimal new formatting.
- The number lexer (`crates/rustlab-script/src/lexer.rs:498`) handles decimal
  digits, `_` separators, `e/E` exponents, and imaginary `i/j` suffix — **no
  radix literals**. `0xff` currently lexes as `Number(0)` + `Ident("xff")` and
  errors at parse time. (Aside: `1i` imaginary literals *do* work; the old
  bug-hunt note claiming otherwise is stale.)
- NPY I/O only supports `<f8` / `<c16` (`build_npy_bytes` at `builtins.rs:5889`,
  `parse_npy_bytes` at `:6000`); integer numpy dtypes (`<i4`, `<u1`, …) are
  rejected today. Phase 4 fixes this as a bonus.
- Builtin registration is `r.register("name", builtin_fn)` in `builtins.rs`
  (286 registered). Every builtin needs a `HelpEntry` in
  `crates/rustlab-cli/src/commands/repl.rs` `HELP` and exactly one
  `CategoryRow` slot — **two tests enforce** this coverage.

## Non-goals (out of scope for this plan)

- A user-facing 128-bit integer *class* (`int128`), bignums / arbitrary
  precision. (`i128` is used only as the internal backing store so `uint64`'s
  full range fits — decision 2. MATLAB has no `int128` either.)
- Complex integers.
- Integer matrices in the linear-algebra core (`eig`, `svd`, `lu`, sparse):
  they continue to consume `f64`/`C64` via widening. No packed-integer BLAS.
  (MATLAB likewise rejects most linear-algebra on integer types.)
- Typed integer *literals* with a class suffix (`0xdeadbeefu32`). MATLAB
  supports these, but they're deferred — the cast form (`uint32(0xdeadbeef)`)
  covers the need, and bare literals already get a sensible class (decision 7).
- Auto-narrowing of `double` results back to integers — that's Deviation A:
  `int + double → double`, full stop.

## Value-type surface

New in the `Value` enum (`value.rs`):

```
Int   { data: i128,       class: IntClass, overflow: OverflowMode }
IntArray { data: Vec<i128>, shape: (usize, usize), class: IntClass, overflow: OverflowMode }
```

- `i128` backing (not `i64`) so the full `uint64` range fits (decision 2).
- `IntArray` covers both vector (`shape.0 == 1` or `.1 == 1`) and matrix, the
  same way `Matrix`/`Vector` relate; a 1×N/N×1 `IntArray` is a vector for the
  shape helpers (mirrors the recent `flatten_column_matrix_args` convention).
- `IntClass` is a new enum in `rustlab-core` beside `OverflowMode`:
  `{ Int8, Int16, Int32, Int64, Uint8, Uint16, Uint32, Uint64 }` with
  `min()`/`max()` (as `i128`, so `uint64::MAX` is exact), `from_str`, `name()`,
  `is_signed()`, and `smallest_unsigned_for(value)` (literal typing, decision 7).
- Two shared helpers reused by every construction/cast/arithmetic site:
  `IntClass::coerce(value: i128, mode) -> i128` (range enforce — `clamp` for
  Saturate, `wrapping` reduce for Wrap) and `IntClass::from_f64(x: f64, mode)
  -> i128` (round-half-away-from-zero per decision 6, then `coerce`).

## Milestones

Each milestone is a demoable, independently-shippable checkpoint gating the
next phase. A phase is not "done" until its milestone's acceptance criteria
pass **and** the full workspace suite (`cargo test --workspace`) plus the
example sweep are green.

| Milestone | Gates | Demoable outcome | Acceptance criteria |
|-----------|-------|------------------|---------------------|
| **M0 — Design signed off** | Phase 0 | this plan | Type model + hybrid-MATLAB semantics locked with the user; plan merged. **(done 2026-07-23)** |
| **M1 — Integers are real** | Phase 1 | `x = int32(5); y = x + 2.7` → `double 7.7`; `int8(2.5)` → `3`; `class(x)` → `"int32"`; `0xFF` → `uint8` | `Value::Int` exists; widening keeps 100% of the pre-existing suite green; casts + `class`/`cast`/`intmax`/`intmin`/`isinteger`/`isa`/`double` land; smallest-fitting-unsigned literals; **test-pinned**: `int+double→double` (Deviation A), round-half-away casts, same-class `int⊗int`, saturate default + wrap opt-in |
| **M2 — Efficient integer arrays** | Phase 2 | `A = int8(zeros(1000,1000))` stores compactly and does elementwise math; integer index vectors work | `IntArray` packed storage + arithmetic + constructors (`zeros/ones/eye/randi/range`) + indexing; class + overflow mode preserved through reshape/transpose |
| **M3 — Width semantics complete** | Phase 3 | `int8(200)` → 127; `int8(x) + int16(y)` **errors** (MATLAB rule); full `uint64` works | cross-class `int⊗int` errors (only same-class `int⊗int` and `int⊗double→double` are legal); lossy narrowing saturates; full `uint64` range (i128 backing), each test-pinned |
| **M4 — Interop** | Phase 4 | `load("ints.npy")` of a numpy `int32` array round-trips as `int32` | NPY int dtypes (all widths + endianness) read/write; CSV/TOML integer save/load; `whos`/`class` report across shapes |

**Critical path:** M1 is the linchpin — the widening at the five coercion
chokepoints is what keeps M1–M4 additive rather than a rewrite. Land and
review that slice before anything else (see "What lands first"). M2 depends on
M1; M3 depends on M2; M4 depends on M2 (I/O needs `IntArray`) but not M3.

## Phases

### Phase 1 — Scalar integer + widening + literals  **Status:** in progress
**Milestone:** M1 — Integers are real.

The MVP that makes integers *real and usable*; all builtins work via widening.

1. ✅ **Done.** `IntClass` (+ `coerce`, `from_f64`, `smallest_unsigned_for`,
   `min`/`max`/`bits`/`name`/`is_signed`, `from_str`) in `rustlab-core` with 6
   unit tests; `Value::Int { data: i128, class, overflow }` in `value.rs` with
   Display (`5  (int32)`) and `type_name` → the class name.
2. ✅ **Done — linchpin verified.** Widening `Int → f64`/`C64` in all five
   chokepoints (`to_scalar`, `to_usize`, `to_cvector` in `value.rs`;
   `to_real_vector`, `to_cmatrix_arg` in `builtins.rs`). `whos` reports the
   class. Full workspace suite stays green (2819 tests, 0 fail). `Int` marked
   uncacheable in `cache_value.rs` for now (integer-arg calls bypass the cache;
   correct, not yet memoized). **← review checkpoint reached here.**
3. **Lexer:** `0x` / `0b` / `0o` integer literals (reuse `_` separators),
   parsed via `u128::from_str_radix`, emitted as a `Value::Int` of the
   **smallest fitting unsigned class** (decision 7: `0xFF → uint8`, …). Reject
   a trailing class suffix with a clear "typed literals not supported" error.
   Cap at `uint64::MAX` → lex error above.
4. **Casts / introspection builtins:** `int8 … int64`, `uint8 … uint64`
   (each `intN(x [, "wrap"|"saturate"])`, rounding half away from zero per
   decision 6), `class(x)` (class-name string; `"double"`/`"complex"`/… for
   existing types), `cast(x, "int32")`, plus the MATLAB introspection set
   `intmax('int8')` / `intmin`, `isinteger(x)`, `isa(x, "int32")`, and
   `double(x)` (integer → `double`).
5. **Arithmetic** in the binop dispatch: `Int ⊗ Int` **same class** → that class
   under the value's overflow mode via `IntClass::coerce`; **different class →
   error** (decision 5, no widening); `Int ⊗ double → double` (Deviation A);
   comparisons return `Bool`.
6. ✅ **Done.** `whos` reports the class; Display prints e.g. `5  (int32)`.
7. Tests (`int_types_tests` in `tests.rs`): construction + range, saturate vs
   wrap round-trips, round-half-away casts, `int + double → double` (Deviation
   A), same-class `int⊗int`, **cross-class error**, smallest-fitting-unsigned
   literals + `uint64::MAX` cap, `class`/`cast`/`intmax`/`intmin`/`isinteger`/
   `isa`/`double`, and widening keeping a representative sample of existing
   builtins working on integer inputs. HELP + CategoryRow entries (new "Integer
   types" subcategory) + `docs/functions.md` / `quickref.md` section.

### Phase 2 — Packed integer arrays + indexing  **Status:** not started
**Milestone:** M2 — Efficient integer arrays.

Delivers "efficient store & operate" for arrays.

1. `Value::IntArray` packed storage + Display (aligned, class-tagged) +
   shape helpers (`size`, `length`, `numel`, `ndims`, `reshape`, transpose).
2. Widening `IntArray` → `CVector`/`CMatrix` in the coercion chokepoints
   (extends Phase 1's scalar widening to arrays).
3. Elementwise arithmetic for `IntArray` with implicit expansion, reusing the
   `IntClass::coerce` policy per element; `IntArray ⊗ double → CMatrix`
   (double).
4. **Constructors gaining an integer form:** `zeros/ones(…, "int32")`,
   `eye(…, "int32")`, `randi` returning an integer class, `colon`/range
   producing integers when bounds+step are integer, `int32([...])` over a
   vector/matrix literal.
5. **Integer index vectors:** an `IntArray` used as an index coerces cleanly
   (natural fit — indices are conceptually integers). Verify against the
   existing `.re as usize` index path.
6. Stretch: true-narrow packing behind the `IntClass` tag (i8 = 1 byte) if the
   memory win is worth the added storage-enum complexity — otherwise defer.
7. Tests: array round-trips, element overflow, constructor classes, indexing,
   reshape/transpose preserve class + mode.

### Phase 3 — Cross-class width semantics  **Status:** not started
**Milestone:** M3 — Width semantics complete.

1. **Cross-class `int ⊗ int` errors** (decision 5, MATLAB rule): finalize the
   error message. Note the interaction with Deviation A — because `int + double
   → double` for *any* double (scalar or not), there is **no** "integer stays
   integer" case here; the only non-error integer arithmetic is same-class
   `int ⊗ int`. (This is where the hybrid parts ways with MATLAB, which would
   keep `int + scalarDouble` integer.) Confirm same-class ops from Phase 1 are
   unaffected.
2. Lossy-narrowing on cast-down (`int32 → int8`) saturates silently under the
   value's overflow mode (consistent with Saturate default; `Wrap` wraps).
3. **Full `uint64` range** works end-to-end on the i128 backing (decision 2):
   values in `(i64::MAX, uint64::MAX]` construct, display, cast, and round-trip
   correctly. No ceiling — the i128 store removes the old i64 limit.
4. Tests: cross-class error for every disallowed pair, `int + double → double`
   for scalar *and* array doubles (Deviation A — confirming no integer-dominant
   case sneaks back in), narrowing saturation, and `uint64` values above 2⁶³.

### Phase 4 — I/O & interop  **Status:** not started
**Milestone:** M4 — Interop.

1. **NPY:** read/write integer dtypes (`<i1/<i2/<i4/<i8`, `<u1/<u2/<u4/<u8`,
   plus big-endian `>` per the earlier IO2 fix) → `IntArray` with the matching
   class. **This also fixes today's gap where numpy integer arrays cannot be
   loaded at all.**
2. **CSV / TOML:** integers already round-trip as whole numbers; preserve the
   integer *class* on save/load where the format allows (TOML integers map to
   the default `int64`; document the CSV limitation).
3. `whos` / `class` reporting across all shapes; `mat2str`/`num2str`
   (from the conversion-builtins plan) emit class-aware output.
4. Tests: numpy int-array load round-trip (all dtypes + endianness), CSV/TOML
   integer save/load.

## Relationship to the conversion-builtins plan

The conversion builtins (`dec2bin`/`bin2dec`/`dec2hex`/`bit*`/`num2str`/`char`
…, separately scoped) are **independent** and can land before or after this.
They operate on `double`/`Str` today and will simply gain `IntArray`
overloads once this type exists. The `0x/0b/0o` lexer literals (Phase 1, item
3) were split out of that discussion and live here.

## Risks

- **Deviation A surprises MATLAB porters.** `int + double → double` differs
  from MATLAB (integer class dominates). This is a *deliberate* hybrid choice;
  mitigate by documenting it loudly in help text + docs (and it's the only
  arithmetic deviation — casts, cross-class errors, saturation, `uint64` range,
  and literal typing all match MATLAB).
- **Widening must be total.** If any numeric builtin reads a `Value` variant
  directly instead of via the five coercion methods, it will reject integers.
  Phase 1 audits for direct `Value::Scalar/Vector/Matrix` matches in numeric
  builtins and routes stragglers through the coercion methods.
- **i128 scalar cost.** 16-byte scalars/elements (vs 8 for i64) are the price
  of full `uint64` range. Acceptable for correctness; the Phase 2 narrow-packing
  store recovers per-element memory for large arrays if measured to matter.
- **Display / test churn.** Existing tests that assert a `Value::Scalar`
  result from something that now returns `Value::Int` (e.g. `size`, `randi`)
  will need updating; Phase 1/2 sweeps these deliberately.

## What lands first

Phase 1, and within it the `Value::Int` variant plus widening at the five
coercion chokepoints — reviewed with the user before the constructors and
arithmetic fan out. That slice alone makes integers real and keeps the entire
existing builtin surface green through widening.

## Open questions

- **Typed literal suffixes** (`0xFFu8`, `0b1010s16`). MATLAB supports them;
  currently deferred (Non-goals) since the cast form covers the need and bare
  literals get a sensible class. Revisit if demand appears — the lexer work is
  modest once the type exists.
- **`num2str` of a matrix** (from the conversion plan): aligned multi-line
  `Str` vs `StringArray`-per-row. Not blocking this plan.
- **Narrow packing** (Phase 2 stretch): worth the storage-enum complexity, or
  is i128-backed "efficient enough"? Decide with a memory measurement, not up
  front.

## Status log

- **2026-07-23** — Design complete; initial three decisions signed off:
  tagged-width type; overflow selectable (Saturate default, Wrap); `int +
  double → double`. Plan written. No code yet.
- **2026-07-23** — **Compatibility revised to hybrid MATLAB** (user picked
  "Hybrid" after reverting an earlier "Full MATLAB" choice). MATLAB-faithful
  except two labeled deviations: **A** `int + double → double`, **B** opt-in
  `Wrap` mode. Adopted the MATLAB rules for cross-class errors (decision 5),
  round-half-away casts (6), smallest-fitting-unsigned literals (7), full
  `uint64` via i128 backing (2), and the `intmax`/`intmin`/`isinteger`/`isa`/
  `double` builtin set. No code yet.
- **2026-07-23** — **Phase 1 first slice landed** (branch
  `feature/integer-types`): `IntClass` in core (6 tests), `Value::Int` variant,
  widening at all five coercion chokepoints, Display + `whos` class reporting,
  `Int` uncacheable. Full workspace suite green (2819 tests). This is the
  planned review checkpoint before casts/literals/arithmetic fan out.
