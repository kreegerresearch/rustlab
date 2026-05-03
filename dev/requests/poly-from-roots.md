# Feature Request: `poly(r)` — polynomial coefficients from roots

## Problem

`roots(p)` exists (find roots from coefficient vector). The inverse `poly(r)` (build coefficient vector from a list of roots) does not. This is the standard MATLAB / Octave pair; current rustlab has only the forward direction.

## Encountered in

`rustlab_controls`:
- **Lesson 08 — Cayley–Hamilton verification.** Want to compute the characteristic polynomial coefficients from `eig(A)` and substitute the matrix into the polynomial.
- Workaround uses Vieta's formulas inline:

```rustlab
ev = eig(A);                              % length-3 example
c2 = -real(ev(1) + ev(2) + ev(3));
c1 =  real(ev(1)*ev(2) + ev(1)*ev(3) + ev(2)*ev(3));
c0 = -real(ev(1) * ev(2) * ev(3));
```

The Vieta workaround is fine for $n = 2$ or $3$ but doesn't scale. For larger systems the explicit formula gets unwieldy; a builtin `poly` would be one line.

## Proposed API

Inverse of `roots` — same coefficient-vector convention (highest power first):

```rustlab
poly([2; 1])           % → [1, -3, 2]    i.e. (x - 2)(x - 1) = x² - 3x + 2
poly([1; 1])           % → [1, -2, 1]    i.e. (x - 1)²
poly([1+1j; 1-1j])     % → [1, -2, 2]    real coefficients despite complex roots
poly(eig(A))           % → characteristic polynomial of A
```

## Implementation

Standard convolution from a leading `1`:

```
p = [1]
for each root r:
    p = convolve(p, [1, -r])
return real(p) if all imaginary parts are negligible
```

Roughly $n$ vector-vector convolutions of growing size; trivial.

## Tests to add

- `poly([2; 1])` returns `[1, -3, 2]`.
- `poly(eig(A))` returns coefficients matching `det(s*I - A)` symbolically.
- Round-trip: `roots(poly(r))` returns `r` (modulo ordering).
- `poly(complex_conjugate_pair)` returns a real coefficient vector.

## Severity

Nice-to-have. Vieta workarounds are usable for lessons up through Phase 6 (no system is larger than 4 states).
