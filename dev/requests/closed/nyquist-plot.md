# Feature Request: `nyquist(G)` builtin

**Status:** **Landed** in rustlab 0.3.1 (commit `d81644d`). `nyquist(G)`, `[re, im, w] = nyquist(G)`, `nyquist(G, w)`, and `nyquist(G, "pos-only")` all implemented with equal-aspect across backends. Verified in `rustlab_controls/lessons/21-nyquist/*.rlab`.

## Problem

The classical-control toolbox in rustlab 0.3.1 has `bode`, `step`, `margin`, `rlocus`, `pole`, `zero`, `tf`, `ss`, `tf(sys)`, `freqresp` — but **no `nyquist` plot**. Nyquist is the standard visual for closed-loop stability analysis: encirclements of $-1$, gain and phase margins read off geometrically, the closest-approach distance to $-1$ as the sensitivity peak $1/M_S$, and the visual statement of the Kalman frequency-domain inequality $|1 + L(j\omega)| \geq 1$ ("locus stays outside the unit disk around $-1$").

## Encountered in

`rustlab_controls`:

- **Lesson 18 — Benefits of Feedback** (already shipped). `lessons/18-feedback-benefits/cartpole_lqr_loop.rlab` builds the LQR loop $L(s) = K(sI - A)^{-1}B$ via the new `tf(A, B, K, 0)` and verifies the Kalman FDI by sampling `min |1 + L(jω)|` over a logspace grid (currently $\geq 1.0000$, with $Pm = 60.7485°$ from `margin(L)`). A `nyquist(L)` would show this graphically — the locus skirts the unit circle around $-1$ but never enters — which is the canonical Nyquist intuition the numerical check leaves implicit.
- **Lesson 21 — Nyquist Criterion & Stability Margins** (Phase 6) is the named lesson. **Hard requirement before that lesson can ship.**
- **Lesson 22 — Loop Shaping** (Phase 6, planned). Loop shapes are designed by reading $|S|$ peaks and crossover off Nyquist plots; cleaner than reading them off two Bode subplots.
- **Lesson 23 — Robustness Limits** (Phase 6, planned). The cart-pole's RHP zero (Lesson 16: $z = +3.13$) caps the achievable closed-loop bandwidth; this is most naturally seen as the locus's distance from $-1$ near $\omega \approx 3$.

## Workaround in use today

With `tf(A, B, C, D)` landed in 0.3.1 (`f6e736e`), the workaround is now a one-liner that *almost* gets there:

```rustlab
L  = tf(A, B, K, 0);
w  = logspace(-2, 3, 600);
H  = freqresp(A, B, K, 0, w);    % evaluates L(jw) on the grid

% Manual plotting:
figure()
hold("on")
plot(real(H), imag(H), "color", "blue", "label", "L(jw), w > 0")
plot(real(H), -imag(H), "color", "blue", "label", "mirror, w < 0")
scatter([-1.0], [0.0])
title("Nyquist plot")
xlabel("Re(L)"); ylabel("Im(L)")
grid("on"); legend(); hold("off")
```

This works but requires the user to:
1. Pick a frequency grid by hand (the auto-grid heuristic is exactly what `bode(G)` already implements internally — `nyquist` should reuse it).
2. Mirror to negative frequencies manually.
3. Plot the $-1$ marker manually.
4. Densify near the unit circle around $-1$ themselves if they want a clean closest-approach reading.

Each of these is a small thing; together they undermine "Nyquist is the canonical robustness visual." A builtin would handle them once.

## Proposed API

Mirror the existing `bode(G)` / `rlocus(G)` shape:

```rustlab
nyquist(G)                                  % plot only
[re, im, w] = nyquist(G)                    % capture the locus
[re, im, w] = nyquist(G, w)                 % user-supplied frequency grid
nyquist(G, "pos-only")                      % omit the negative-frequency mirror image
```

Where:

- `G` is a `Value::TransferFn` (from `tf(...)`) or a `Value::StateSpace` (from `ss(...)` or `ss(A, B, C, D)` — both available in 0.3.1).
- `re`, `im` are real-valued vectors of the real and imaginary parts of $G(j\omega)$ (positive-frequency branch).
- `w` is the frequency grid in rad/s. Default: same auto-range heuristic as `bode(G)` (decades around the dominant pole/zero), with extra densification where the locus is near $-1$ to give a clean closest-approach reading.

## Plot conventions

- Real axis horizontal, imaginary axis vertical, equal aspect ratio (a circle should look like a circle).
- Plot the positive-frequency locus and its complex-conjugate mirror (negative frequencies) by default — together they form the closed contour.
- Mark $-1 + 0j$ with a small `x` or `+`.
- A faint unit circle around $-1$ is the standard reference (sensitivity peak $M_S = 1/r$ where $r$ is the locus's closest distance to $-1$). Optional.
- Arrows along the locus showing the direction of increasing $\omega$ are conventional; nice-to-have, not required.

## Implementation note

Most of the plumbing already exists in 0.3.1:

- `bode(G)` already auto-picks a frequency grid — `nyquist(G)` can call the same internal grid generator for the initial pass.
- `freqresp(A, B, C, D, w)` already evaluates $G(j\omega)$ on a grid.
- For `Value::TransferFn`, evaluate via Horner on the polynomial coefficients (the rustlab implementation already does this for `bode`).
- Densification near $-1$ is a **two-pass** operation: evaluate on the initial coarse grid, find indices where $|1 + G(j\omega)|$ falls below a threshold (e.g. $\leq 2$), insert a refined sub-grid in those intervals, and re-evaluate. This avoids baking the densification into the initial grid generator (which would be brittle, since the location of the closest approach is plant-dependent and not known a priori).

### Cross-backend rendering (the load-bearing concern)

`nyquist` is the first builtin to need **equal aspect ratio for a non-heatmap plot**. Today, equal-aspect support exists in each backend *only for heatmap/imagesc*. The pattern needs to be lifted to a general figure-level setting (call it `axis_equal: bool` on the subplot, or a public `axis("equal")` builtin) and wired through all four rendering surfaces. Each backend has its own implementation:

| Surface | Crate / file | What's there now | What `nyquist` needs |
|---|---|---|---|
| **ratatui** (terminal braille) | `rustlab-plot/src/ascii.rs` | No aspect handling for line/scatter; braille cells are ~2:4 (non-square) | Compensate for cell aspect when computing `xlim`/`ylim` so a unit circle reads as round. Pad the shorter axis. |
| **viewer** (egui IPC) | `rustlab-plot/src/viewer_client.rs` + `rustlab-viewer` | No aspect lock | Set the egui plot's `data_aspect(1.0)` (or equivalent) when the figure flag is on. |
| **SVG** (notebooks rendered to GitHub) | `rustlab-plot/src/file.rs` | Aspect-shrink only for imagesc cells (`imagesc_svg_cells_are_square` test, l. 1827) | Same shrink logic, applied at the panel level for line/scatter when the flag is on. GitHub renders the static `.svg` directly via `render_markdown.rs:101` (`![plot N](…/plot-N.svg)`), so this path *must* be square or the lesson page will mislead readers. |
| **Plotly HTML** (interactive notebooks) | `rustlab-plot/src/html.rs` | `scaleanchor: "x"` already used for heatmaps (l. 149-170) | Emit the same `scaleanchor` pair on the y-axis layout when the flag is on. |

`nyquist(G)` should set the flag automatically on the panel it creates — users shouldn't have to add `axis("equal")` themselves.

**Verification across surfaces**: every test in the next section needs to pass on the SVG and Plotly paths (they're the deterministic ones) and be eyeballed in the ratatui and viewer paths. The first-order test is the cleanest cross-backend check: render `nyquist(tf([1], [1, 1]))` and confirm the locus traces a visibly round circle in all four. If it looks like an ellipse anywhere, the aspect plumbing is incomplete on that surface.

So the full addition is: (a) figure-level `axis_equal` flag plumbed through all four backends, (b) `nyquist` builtin that sets the flag and emits the locus + decorations, (c) two-pass densification near $-1$. (a) is the load-bearing piece — without it the feature ships broken on at least three of the four surfaces.

## Tests to add

1. **First-order**: `G(s) = 1/(s+1)` — locus is a circle of radius $0.5$ centered at $(0.5, 0)$, passing through $(1, 0)$ at $\omega = 0$ and tending to the origin as $\omega \to \infty$. Closest approach to $-1$ is exactly $1.0$ (at $\omega \to \infty$, where $|1 + G(j\omega)|^2 = (\omega^2+4)/(\omega^2+1) \to 1$) — no encirclement.
2. **Second-order, lightly damped**: `G(s) = 1/(s² + 0.3 s + 1)` (from rustlab_controls Lesson 17). Closest approach to $-1$ is small (≈ $1/M_S$ where $M_S \approx 3.37$ is the sensitivity peak height the lesson computes).
3. **Cart-pole LQR loop** (integration with `rustlab_controls`): for $L(s) = K(sI-A)^{-1}B$ with the cart-pole and the LQR weights $Q = \text{diag}(1, 0.1, 10, 0.1)$, $R = 0.1$ from Lesson 11, the locus stays outside the unit disk centered at $-1$ — verifies the Kalman FDI graphically. The closest approach to $-1$ should be $\geq 1$ on the entire grid (currently $\min |1 + L(j\omega)| = 1.0000$ to four decimals on the test grid in `cartpole_lqr_loop.rlab`).
4. **Marginal**: `G(s) = 1/s` — locus is the negative imaginary axis (positive-frequency) plus its mirror, with a "detour" around the origin for the standard Nyquist contour. (Plot decoration choice — skipping the indentation around the origin is acceptable; the open-ended locus reads correctly without it.)
5. **Captured arrays**: `[re, im, w] = nyquist(tf([1], [1, 0.1, 1]))` and verify lengths agree, `re` and `im` are real-valued, and `re(1) + 1j*im(1)` matches `freqresp` at `w(1)`.
6. **Aspect ratio across backends**: `nyquist(tf([1], [1, 1]))` (the first-order plant). Render to each surface and confirm the circle reads as round:
   - **SVG** — assert in a unit test that the panel's plotted-region width equals its height (within 1 pixel) when the figure's `axis_equal` flag is on. Same approach as the existing `imagesc_svg_cells_are_square` test in `rustlab-plot/src/file.rs:1827`.
   - **Plotly HTML** — assert the emitted layout contains `scaleanchor: "x"` on the y-axis for the panel.
   - **ratatui** — visual check during development; lock in via a snapshot test of the rendered character grid if practical.
   - **viewer** — visual check during development.

## Severity

**Hard requirement before Lesson 21 ships** (Phase 6). The rest of Phase 6 (Lessons 20, 22, 23) doesn't strictly need the builtin but reaches for it in supporting plots — the manual workaround already appears in Lesson 18 and would re-appear three more times before Phase 6 closes.

## Out of scope / follow-ups

- **`nichols(G)`** — natural sibling, same input, log-magnitude vs phase parametric plot. Not requested here; deferred.
- **`encirclements(G)`** — programmatic count of encirclements of $-1$ for the Nyquist criterion. Useful for a stability-by-Nyquist test in Lesson 21 but not strictly required (visual count works for the lesson's plant choices).
- **`pi_circles(G)`** — overlay constant-$|S|$ circles on the Nyquist plot. Loop-shaping convenience; deferred to Phase 6 if/when needed.
