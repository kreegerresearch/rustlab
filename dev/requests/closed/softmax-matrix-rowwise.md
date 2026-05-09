# Feature Request: `softmax(M)` / `softmax(M, dim)` — row- or column-wise overload

**Status:** ✓ landed. `softmax(M)` defaults to per-row (`dim=2`, ML convention); `softmax(M, 1)` is per-column. Mirrors the `layernorm(M[, dim[, eps]])` precedent. 9 unit tests in `crates/rustlab-script/src/tests.rs` `mod ml_tests`.


## Problem

`softmax(v)` accepts only a vector today. To apply softmax row-wise to a `T × T` matrix — the canonical attention operation — users have to wrap it in a per-row loop:

```rustlab
A = zeros(T, T);
for t = 1:T
  A(t, :) = softmax(S(t, :));
end
```

This loop is mechanical scaffolding around a built-in. Every transformer lesson re-emits it (lessons 08, 13, 14, 15 in `rustlab_llm`), and the same pattern shows up wherever a probability distribution is computed per row of a logits matrix (classification heads, mixture weights, attention weights).

The matrix `layernorm(M[, dim[, eps]])` overload that landed in 0.3.0 already establishes the precedent for adding a row/column-aware overload to a vector-only ML primitive. `softmax` is the obvious next candidate.

## Encountered in

`rustlab_llm`:
- **Lesson 08 — Scaled Dot-Product Attention.** The attention-weights example `A = softmax(S_masked)` row-wise is currently a manual loop. The lesson explicitly states the formula

  $$A_{t, i} = \frac{\exp(\tilde S_{t, i})}{\sum_{j=1}^{T} \exp(\tilde S_{t, j})}$$

  and a one-line `softmax(M, 2)` would mirror it exactly.
- **Lesson 13 — Transformer Block**, **Lesson 14 — Full GPT**, **Lesson 15 — Backpropagation through attention.** Same row-wise softmax loop, propagated four times.

The workaround is correct but verbose, and it implies "softmax is sequential per row," which is misleading — every row is independent.

## Proposed API

Mirror `layernorm(M[, dim[, eps]])`:

```rustlab
p = softmax(v)                          % existing — vector → vector
P = softmax(M)                          % new — per-row by default (ML convention)
P = softmax(M, 2)                       % per-row, explicit
P = softmax(M, 1)                       % per-column
```

Default `dim` should be **2** (per-row), matching `layernorm(M)`'s ML-convention default and the ML idiom that rows are samples / tokens and columns are categories / features. The note from the `layernorm` docs applies verbatim: this default deliberately diverges from `sum`/`mean`/`std` (which default to `dim=1`), but ML usage dominates.

Numerical stability rule from the vector form (subtract the per-slice max before exponentiating) carries over per-row / per-column.

## Tests to add

- `softmax([1.0, 2.0; 3.0, 4.0])` (default dim=2) returns a 2×2 matrix where each row sums to 1.0 and equals `softmax([1, 2])` / `softmax([3, 4])`.
- `softmax(M, 1)` returns column-sums of 1.0.
- Numerical stability: `softmax([1000.0, 1001.0; 1.0, 2.0])` produces no `NaN`/`Inf`.
- 1×N matrix is treated as a vector (consistent with `layernorm`).
- `softmax(M, 2)` agrees row-by-row with the manual `softmax(M(t, :))` loop.

## Severity

Nice-to-have. The per-row loop is a one-liner, but: (a) the lesson series writes it four separate times and the loop *misleads* readers about the parallel nature of softmax, and (b) the `layernorm(M, dim)` precedent makes this a small, consistent addition rather than a new pattern.

## Related

`softmax_rows`/`softmax_cols` would be acceptable alternatives, but the dim-arg overload matches the `layernorm`/`sum`/`mean` family and avoids adding new names.

If a `cross_entropy(P, y)` or `log_softmax(M, dim)` ever lands, the same dim-aware shape would compose with this naturally — backprop through attention computes `log_softmax` row-wise on the same matrices.
