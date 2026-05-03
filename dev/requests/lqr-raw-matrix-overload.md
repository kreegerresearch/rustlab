# Feature Request: `lqr(A, B, Q, R)` — accept raw matrices, not only `ss()` from a TF

## Problem

`lqr(sys, Q, R)` requires `sys` to be a state-space struct produced by `ss(G)` from a transfer function. Passing a manually-built struct with `.A`, `.B`, `.C`, `.D` errors:

```
type error: lqr: first argument must be a state-space system, got struct
```

This forces users with raw $(A, B)$ — typically anything multi-state, MIMO, or built directly from physics — to either (a) construct a transfer-function representation just to feed `ss()`, which is awkward for $4 \times 4$ MIMO plants, or (b) bypass `lqr` entirely and call `care(A, B, Q, R)` then form $K = R^{-1}B^T P$ themselves.

## Encountered in

`rustlab_controls`:
- **Lesson 11 — LQR on the cart-pole.** $A$ is $4 \times 4$, $B$ is $4 \times 1$, no obvious TF starting point. Workaround in use: `care` directly.
- Same workaround in **Lesson 15 — LQG**.

The workaround is fine — `care` is the underlying solver — but the lesson loses the chance to introduce the canonical `lqr(...)` call.

## Proposed API

Add an overload that takes raw matrices:

```rustlab
[K, S, e] = lqr(A, B, Q, R)             % new — raw matrices
[K, S, e] = lqr(sys, Q, R)              % existing — state-space struct
```

Or alternatively (or additionally), accept a hand-built struct with `.A, .B, .C, .D`:

```rustlab
sys = struct("A", A, "B", B, "C", C, "D", D);
[K, S, e] = lqr(sys, Q, R);             % currently errors; should accept
```

Both forms map to the same internal call: solve `care(A, B, Q, R)` for $P$, return `K = R^{-1} B^T P`, `S = P`, `e = eig(A - B*K)`.

## Tests to add

- `[K, S, e] = lqr([0,1;-1,-0.3], [0;1], eye(2), 1.0)` returns the same `K` as the existing `lqr(ss(tf([1],[1,0.3,1])), eye(2), 1.0)`.
- `[K, S, e] = lqr(struct("A", A, "B", B, "C", C, "D", 0), eye(2), 1.0)` works.
- Sanity: `eig(A - B*K)` are all in LHP for stabilisable `(A, B)` and PSD `Q`, PD `R`.

## Severity

Nice-to-have. The `care` workaround is clean and arguably more honest pedagogically (since LQR *is* "solve CARE, form $K$"), but the matlab-equivalent `lqr(A, B, Q, R)` form is what students will look for.

## Related

`dlqr(A, B, Q, R)` for discrete-time LQR has the same shape today (TF-derived only) — the same overload should land on both. Same again for `kalman(...)` if/when it lands as a builtin (currently composed from `care`/`dare`).
