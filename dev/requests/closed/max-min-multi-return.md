# Feature Request: `[m, idx] = max(v)` / `[m, idx] = min(v)` multi-return forms

**Status:** Landed in commit `1dbc15c` ("Add multi-return [m,i] form for max/min; magnitude-key for complex"). `max` / `min` now register via `register_nargout` in `crates/rustlab-script/src/eval/builtins.rs:137-138`; `[m, i] = max(v)` / `[m, i] = min(v)` return value + 1-based index.

## Problem

`max(v)` and `min(v)` in rustlab return only the extremum value. The MATLAB / Octave convention is a two-output form:

```matlab
[m, i] = max(v)        % m = maximum value, i = index of first occurrence
[m, i] = min(v)
```

In rustlab today, getting the index requires a separate `argmax(v)` / `argmin(v)` call:

```rustlab
m   = max(real(mag_dB));
idx = argmax(real(mag_dB));
```

Functionally fine, but it scans the vector twice and disconnects two values that are conceptually a single result. Multi-return user functions already work in rustlab 0.3.0 (`function [a, b] = name(...)` and `[a, b] = bode(G)` etc.), so the language has the machinery — `max` and `min` just don't expose it.

## Encountered in

`rustlab_controls`:

- **Lesson 17 — Bode Plots.** `bode_basics.rlab` locates the resonance peak via `m_peak = max(real(mag_dB)); idx_peak = argmax(real(mag_dB))`. The two-line workaround was an early surprise — students reasonably expect `[m_peak, idx_peak] = max(...)`.
- **Lesson 18 / 19** — same pattern for sensitivity-peak frequency lookup, controller-margin tabulation, etc.

Beyond `rustlab_controls`, this is the most common MATLAB-style ergonomics gap students hit; covered in the second chapter of any signal-processing textbook.

## Proposed API

Add the multi-return form for both `max` and `min`. Single-return form unchanged:

```rustlab
m       = max(v)            % unchanged
[m, i]  = max(v)            % new: i is the index of the first occurrence of m
m       = max(M)            % unchanged: column-wise reduction (1xN row)
[m, i]  = max(M)            % new: i is a 1xN row of indices, one per column
[m, i]  = max(M, [], 1)     % new: column-wise; i is 1xN row of row indices
[m, i]  = max(M, [], 2)     % new: row-wise; i is Nx1 column of column indices
```

Same shape for `min`.

## Implementation approach

Use the existing `register_nargout` mechanism (see `crates/rustlab-script/src/eval/builtins.rs:144` for `sort`, `:211` for `eig`, `:291` for `find`). The builtin receives `nargout` and dispatches:

- `nargout == 1`: existing fast path, value-only fold.
- `nargout >= 2`: enumerate-and-fold tracking both extremum and 1-based index.

Note: the original request claimed "both already compute the index internally; they just discard it" — this is incorrect. Current `builtin_max` (`builtins.rs:1486`) is `v.iter().map(|c| c.re).fold(f64::NEG_INFINITY, f64::max)`, a pure value fold. The `nargout >= 2` path needs a real `enumerate().fold(...)` that tracks index alongside the running extremum.

## Comparison-key semantics (decided)

This change pulls in a semantic decision that also affects `argmax` / `argmin`. **Lock the rules below in for all four builtins** so single-return and multi-return forms stay consistent:

- **Purely real input** (every element has zero imaginary part): compare by real value. Same as today.
- **Complex input** (any element has a nonzero imaginary part): compare by **magnitude** `|z|`. **This diverges from MATLAB's tie-breaking on equal magnitudes** (MATLAB falls back to phase angle); rustlab uses first-occurrence on equal magnitudes — same rule as the real case. Document the divergence on every affected entry in `docs/functions.md`.
- **NaN handling**: same as MATLAB. NaN is treated as missing — skipped during the fold. If *every* element is NaN, error (do not silently return NaN at index 1). Applies identically to `max`, `min`, `argmax`, `argmin`.
- **Tie-breaking**: first occurrence wins. Matches MATLAB and the existing `argmax` behavior, and applies under both real and magnitude orderings.

The `argmax` / `argmin` comparison key today (`a.re.partial_cmp(&b.re)`, `builtins.rs:1979`) must be updated to match the rules above so `[~, i] = max(v)` and `argmax(v)` agree on every input.

## Error rules — never silent

Always error or warn explicitly; do not paper over edge cases:

- `[m, i] = max([])` → error: `"max: argument must be a non-empty vector"` (matches existing single-return).
- `[m, i] = max(a, b)` (two-arg elementwise form) → error: `"max: multi-return form is not defined for the elementwise two-vector form; use [m,i] = max(v) on a single vector"`.
- All-NaN input → error: `"max: input is all NaN"` (and the analogous message for `min`).
- Unrecognized `dim` in `max(M, [], dim)` → error as today.

## Tests to add

- `[m, i] = max([3, 1, 4, 1, 5, 9, 2, 6])` → `m = 9`, `i = 6`.
- `[m, i] = min([3, 1, 4, 1, 5])` → `m = 1`, `i = 2` (first occurrence).
- `[m, i] = max(M)` for a 3×4 matrix → `m` is a 1×4 row of column maxima; `i` is a 1×4 row of row indices.
- `[m, i] = max(M, [], 1)` → matches the no-dim default.
- `[m, i] = max(M, [], 2)` → `m` is `nrows × 1`, `i` is `nrows × 1` of column indices.
- Single-return form `m = max(v)` continues to return just the value (no API break).
- Tie-breaking: first occurrence wins, both `max` and `min`.
- **Cross-consistency:** for every test input, `[~, i] = max(v)` and `argmax(v)` return the same index. Same for min/argmin.
- **Complex magnitude:** `[m, i] = max([1+0i, 0+2i, 1.5+0i])` → `m = 0+2i`, `i = 2` (magnitude 2 wins over real 1.5). Document in the test that this diverges from MATLAB (MATLAB picks `0+2i` here too because of magnitude — but on equal magnitudes MATLAB uses phase angle while we use first-occurrence; add a test that pins the equal-magnitude case).
- **NaN skipping:** `[m, i] = max([NaN, 1, 2, NaN])` → `m = 2`, `i = 3`.
- **All-NaN errors:** `[m, i] = max([NaN, NaN])` → error.
- **Empty errors:** `[m, i] = max([])` → error.
- **Two-vector form errors:** `[m, i] = max([1,2,3], [3,2,1])` → error.

## Doc / follow-up checklist

- Update `docs/functions.md` `max` / `min` entries with the multi-return form **and** the magnitude-comparison rule for complex inputs (call out the divergence from MATLAB on equal magnitudes).
- Update `docs/functions.md` `argmax` / `argmin` entries — the comparison key changes there too, so the doc must say "by real value for real inputs; by magnitude for complex inputs."
- Update `AGENTS.md` capability table if it lists return shapes for these.
- Update `docs/quickref.md` — both the `max`/`min` lines and any complex-comparison commentary.
- Rewrite the workaround sites in `rustlab_controls` (Lesson 17/18/19) once landed; remove the `argmax`/`argmin` second call.
- Add REPL `HelpEntry` updates for `max` / `min` / `argmax` / `argmin` (per the workflow rule for builtin changes).

## Severity

Nice-to-have for the multi-return ergonomics. Medium-priority for the magnitude-comparison alignment, because it's a semantic correctness change that fixes a real divergence between `max` and `argmax` behavior on complex inputs. Worth doing both together so the cross-consistency test (`[~, i] = max(v) == argmax(v)`) holds on every commit.
