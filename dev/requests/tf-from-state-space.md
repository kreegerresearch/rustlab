# Feature Request: `tf(sys)` — convert state-space to transfer function

## Problem

Rustlab has `ss(G)` to convert a transfer function to state space (observable canonical form), but no inverse: given a state-space $(A, B, C, D)$ — typically built from physics — there is no way back to a polynomial transfer function. Concretely:

```rustlab
% have: A, B, C, D from linearised equations of motion
% want: G(s) = num(s)/den(s) so that we can use pole(G), zero(G), bode(G), step(G), margin(G)
```

Without this, students who build a plant from physics — i.e. anything multi-state and not naturally a TF — must derive the coefficient polynomials by hand to use the TF-domain functions (`pole`, `zero`, `bode`, `step`, `margin`).

## Encountered in

`rustlab_controls`:

- **Lesson 16 — Transfer Functions.** Cart-pole's $H_x(s) = (Ls^2 - g) / [s^2(MLs^2 - (M+m)g)]$ is derived **by hand** from the linearised equations because there is no `tf(sys)` to compute it directly from the existing $(A, B, C, D)$. Hand derivation works, but it's three pages of algebra and one sign mistake away from a wrong plot.
- **Lesson 17 — Bode Plots.** Cart-pole frequency response — used `freqresp(A, B, C, D, w)` directly. Fine, but the natural workflow is "build the TF, then `bode(G)`".
- **Lesson 18 — Sensitivity & Complementary Sensitivity.** The cart-pole's LQR loop $L(s) = K(sI-A)^{-1}B$ is a state-space-derived SISO TF; computed via `freqresp(A, B, K, 0, w)` and re-wrapped manually for `bode`-style plotting.
- **Phase 6 (planned)** — every cart-pole loop-shaping example needs `tf(sys)` to express the closed-loop with $S$, $T$, $L$ as TF objects.

## Prerequisites

Two small additions are required before `tf(sys)` is fully useful.

### P1. `ss(A, B, C, D)` constructor

`crates/rustlab-script/src/eval/builtins.rs` currently rejects this with an explicit error:

```text
ss: expected tf, got matrix (direct ss(A,B,C,D) construction not yet supported)
```

Without it there is no way to *get* a state-space value from raw matrices except by going TF → SS, which is exactly the path `tf(sys)` is meant to invert. Add the 4-matrix overload:

```rustlab
sys = ss(A, B, C, D)        % returns Value::StateSpace { a, b, c, d }
```

Validation: `A` square `n×n`; `B` is `n×m`; `C` is `p×n`; `D` is `p×m` (with `D = 0` accepted as scalar zero or `zeros(p,m)`).

### P2. `tfdata(G)` accessor

Not currently registered (no `r.register("tfdata", …)` in `builtins.rs`). The tests below need a way to extract numerator/denominator coefficient vectors from a `Value::TransferFn`:

```rustlab
[num, den] = tfdata(G)      % multi-return; num and den are real row-vectors
```

Trivial to implement — `Value::TransferFn { num, den }` already stores them as `Vec<f64>`.

## Proposed API

```rustlab
G = tf(sys)              % sys: Value::StateSpace from ss(...) or ss(A,B,C,D)
G = tf(A, B, C, D)       % four-matrix overload — equivalent to tf(ss(A,B,C,D))
```

This extends the existing `tf` dispatch in `builtins.rs:7028` (currently `tf("s")` and `tf(num, den)`) with a third arm matching on `Value::StateSpace`, and a fourth arm for the four-matrix form. Both produce `Value::TransferFn { num, den }`.

**SISO scope.** Initial implementation is SISO only: `B` is `n×1`, `C` is `1×n`, `D` is `1×1`. MIMO (a matrix of TFs) is a separate, larger feature and can be deferred until rustlab grows a `TransferFnMatrix` value type.

## Implementation — Faddeev–LeVerrier

For SISO with `n ≤ 10` (well beyond bootcamp use), the Faddeev–LeVerrier recursion is the right algorithm: it computes both polynomials from $(A, B, C, D)$ in $O(n^4)$ without any eigenvalue solve, root-finding, or symbolic machinery.

Given $(A, B, C, D)$ with $A \in \mathbb{R}^{n\times n}$, set $M_0 = I$, $c_n = 1$. For $k = 1, \dots, n$:

$$
c_{n-k} = -\frac{1}{k}\,\mathrm{tr}(A M_{k-1}), \qquad
M_k = A M_{k-1} + c_{n-k} I.
$$

Then:

- **Denominator:** $\det(sI - A) = \sum_{k=0}^{n} c_k\, s^k$, coefficients $[c_n, c_{n-1}, \dots, c_0]$ (highest-power-first, matching rustlab's poly convention).
- **Numerator:** $C\,\mathrm{adj}(sI - A)\,B + D\,\det(sI - A) = \sum_{k=0}^{n-1} (C\,M_{n-1-k}\,B)\,s^k + D \cdot \det(sI - A)$.

For SISO this collapses to `Vec<f64>` for both num and den. The recurrence is numerically robust for the matrix sizes the bootcamp produces and avoids the conditioning trap of root-then-cancel approaches.

**Pole-zero cancellation is intentionally *not* part of this feature.** Faddeev–LeVerrier returns the uncancelled rational form (e.g. an unobservable mode at $-2$ shows up in *both* num and den, with a matched factor). Cancelling them requires factoring polynomials and matching roots within tolerance — a separate "minimal realization / `minreal(G)`" feature. Filed separately if needed.

## Worked examples

### Example 1 — first-order plant from $(A, B, C, D)$

```rustlab
A = [-2];  B = [1];  C = [1];  D = 0;
G = tf(A, B, C, D)
% expected: G(s) = 1 / (s + 2)
[num, den] = tfdata(G)        % num = [1], den = [1, 2]
pole(G)                        % [-2]
```

### Example 2 — double integrator

```rustlab
A = [0, 1; 0, 0];  B = [0; 1];  C = [1, 0];  D = 0;
G = tf(A, B, C, D)
% expected: G(s) = 1 / s^2
[num, den] = tfdata(G)        % num = [1], den = [1, 0, 0]
```

### Example 3 — physics-derived plant, then frequency response

```rustlab
% Mass-spring-damper: m x'' + b x' + k x = u
m = 1;  b = 0.5;  k = 4;
A = [0, 1; -k/m, -b/m];
B = [0; 1/m];
C = [1, 0];
D = 0;
G = tf(A, B, C, D)            % G(s) = 1 / (s^2 + 0.5 s + 4)
bode(G)                       % no manual freqresp wrapping needed
```

### Example 4 — round-trip sanity check

```rustlab
G  = tf([1, 2], [1, 3, 5]);     % build a TF directly
H  = tf(ss(G));                  % SS → TF round-trip
[gn, gd] = tfdata(G);
[hn, hd] = tfdata(H);
% gn/gd and hn/hd agree up to a common scale factor and floating-point noise
```

## Tests to add

In `crates/rustlab-script/src/tests.rs` (or a new `tests/state_space.rs` integration file):

1. **First-order, double-integrator, MSD.** For each of the worked examples above, assert the returned `num`/`den` match the analytic coefficients to `1e-10`.
2. **Round-trip.** For SISO `G` of degrees 1–4 with random real coefficients, `tf(ss(G))` produces a TF whose poles match `pole(G)` (sorted, within `1e-8`) and whose step response at sampled times matches `step(G)` within `1e-6`.
3. **Cart-pole from `rustlab_controls`** (integration smoke). Using the parameter set from `rustlab_controls`'s `lessons/09-inverted-pendulum/cartpole_model.rlab`, `tf(A, B, C, D)` produces a TF with the expected pole/zero structure: two poles at the origin and a real pair $\pm\sqrt{(M+m)g/(ML)}$, plus a numerator zero at $\pm\sqrt{g/L}$. Assert pole/zero locations rather than literal coefficients (parameter values live in another repo and may change).
4. **`D ≠ 0` passthrough.** `A=[-1]`, `B=[1]`, `C=[1]`, `D=2` → `G(s) = (2s + 3)/(s + 1)` (verify the `D · det(sI-A)` contribution to the numerator).
5. **Shape errors.** `tf(A, B, C, D)` with mismatched dimensions returns a clear `ScriptError::type_err` naming the offending matrix and its expected shape.

## Severity

Nice-to-have for Phases 5 / 6 of `rustlab_controls`; the hand-derivation workaround is documented in Lesson 16 and runs cleanly, but it is fragile for new plants. Phase 6 plans loop-shaping designs on the cart-pole that will reach for `tf(sys)` repeatedly.

## Out of scope / follow-ups

- **`minreal(G)`** — pole-zero cancellation within tolerance. Needed only when an SS realisation has unobservable/uncontrollable modes that the user expects to vanish in the TF. Separate request.
- **MIMO `tf(sys)`** — returns a $p \times m$ matrix of TFs. Requires a new `Value::TransferFnMatrix` (or similar). Deferred.
- **`zpk(sys)`** — zero-pole-gain parameterisation. Natural sibling; same input, different output factoring. Out of scope here.
