# Matrix/Vector Coercion

**Status:** Shipped — implicit broadcasting between 1×N matrices and
N-element vectors works for arithmetic and element-wise operators
(verified via Octave-compat divergence #7, see
`dev/plans/octave_compat_divergences.md`). The plot-side rendering
(treating a 1×N or N×1 matrix as a single line series) is not yet
wired up; track separately if a user hits it.

`randn(1, 500)` returns a 1×500 `Matrix`. Since 0.3.26, `randn(500)` is a 500×500 matrix, not a length-500 vector; a length-500 draw is `randn(1, 500)`. Downstream functions like `plot` still treat a 1×N matrix and a vector differently — `plot` on a matrix plots columns, so a 1×N matrix shows only the last column (1 data point).

## Problem

A 1×N matrix and an N-element vector should behave identically for element-wise ops, `plot`, arithmetic with vectors, etc. Currently they don't:

- `randn(1, N)` + vector → type error ("Add not defined for vector and matrix")
- `plot(randn(1, N))` → plots 1 point instead of N
- `x .* randn(1, N)` → type error

## Fix

Auto-coerce 1×N and N×1 matrices to vectors where needed:

- When a builtin receives a 1×N or N×1 matrix where a vector is expected, flatten it automatically
- Or: make `randn(1, N)` return a vector when one dimension is 1
- Or: make `plot` detect single-row/single-column matrices and treat them as vectors

Decide which approach is cleanest. The first (coerce at builtin boundary) is the most general.
