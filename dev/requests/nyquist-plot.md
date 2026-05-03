# Feature Request: `nyquist(G)` builtin

## Problem

The classical-control toolbox has `bode`, `step`, `margin`, `rlocus`, `pole`, `zero`, but **no `nyquist` plot**. Nyquist is the standard visual for closed-loop stability analysis — encirclements of $-1$, gain and phase margins read off geometrically, and the foundation of robust-control intuition (sensitivity peaks at the closest approach to $-1$).

## Encountered in

`rustlab_controls`:
- **Lesson 21 — Nyquist Criterion & Stability Margins** (Phase 6) is the canonical use case. Cannot be authored cleanly without this builtin.
- Workaround for the lesson would be to call `freqresp(A, B, C, D, w)` over a closed contour and plot real vs. imaginary parts manually — workable but undermines the lesson's "this is a fundamental tool" framing.

## Proposed API

Mirror the existing `bode(G)` / `rlocus(G)` shape:

```rustlab
nyquist(G)                                  % plot only, returns nothing or the figure handle
[re, im, w] = nyquist(G)                    % capture the locus
[re, im, w] = nyquist(G, w)                 % user-supplied frequency grid
nyquist(G, "neg-only")                      % omit the negative-frequency mirror image (matlab option)
```

Where:
- `G` is a transfer function (from `tf(...)`) or state-space struct (from `ss(...)`).
- `re`, `im` are vectors of the real and imaginary parts of $G(j\omega)$.
- `w` is the frequency grid in rad/s. By default cover several decades around the dominant pole/zero, mirrored to negative frequencies; auto-densify near the unit circle and near $-1$.

## Plot conventions

- Real axis horizontal, imaginary axis vertical.
- Plot the positive-frequency locus and its complex-conjugate mirror (negative frequencies) by default — together they form the closed contour.
- Mark $-1$ with a small `x` (or a unit circle for visual reference).
- Arrows along the locus showing the direction of increasing $\omega$ are matlab-standard; nice-to-have, not required.

## Tests to add

- Stable first-order $G(s) = 1/(s+1)$: locus is a half-circle in the right half-plane crossing through 1 at $\omega = 0$ and tending to 0 at $\omega = \infty$. No encirclement of $-1$.
- Marginally stable $G(s) = 1/s^2$: locus passes through the origin from above.
- `[re, im, w] = nyquist(tf([1], [1, 0.1, 1]))` and verify the captured arrays are the right length and have the expected magnitude.

## Severity

Hard requirement before Lesson 21 ships. The rest of Phase 6 (Lessons 20, 22, 23) doesn't strictly need it but uses it in supporting plots.
