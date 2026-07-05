# Changelog

Notable user-facing changes to rustlab, newest first. Versions are the
workspace version in `Cargo.toml`; entries under a version may land across
several PRs while that version is current. **Breaking / behavior changes**
get their own subsection with migration guidance — downstream script owners
should re-validate against those entries when upgrading.

## Unreleased

### Added
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
