# Controls Bootcamp Functions — Implementation Plan

**Status:** Complete (all phases implemented in commit 48dfa8c)

Source request: `prompts/rustlab_feature_request.md`

---

## Already implemented — cross off from feature request

The following functions from the request already exist as builtins:

| Function | Notes |
|---|---|
| `expm(A)` | Padé [6/6] scaling-and-squaring |
| `inv(A)` | LU-based via Gaussian elimination |
| `rank(A)` | SVD threshold on A'A eigenvalues |
| `det(A)` | LU decomposition |
| `trace(A)` | Sum of diagonal |
| `diag(v)` / `diag(A)` | Construct diagonal matrix / extract diagonal |
| `norm(v)` / `norm(A, 2)` | 2-norm and spectral norm |
| `ctrb(A, B)` | Controllability matrix |
| `obsv(A, C)` | Observability matrix |
| `horzcat` / `vertcat` | And `[A, B]` / `[A; B]` literal syntax |
| `lqr(A, B, Q, R)` | Already calls CARE internally via a Newton solve |

`logspace` exists internally (used by `bode`) but is not exposed as a builtin — trivial to fix.

---

## What needs to be built

### Phase A — Trivial (< 1 hour)

**`logspace(a, b, n)`**  
Internal function `logspace()` already exists in `builtins.rs`. Just register it.  
Signature: `w = logspace(-1, 3, 200)` → vector of n points from 10^a to 10^b.

---

### Phase B — Straightforward (each ~1–2 hours)

**`rk4(f, x0, t)` → matrix**

Fixed-step 4th-order Runge-Kutta. `f` is a lambda or function handle `f(x, t) → x_dot`.

Architecture: Like `arrayfun`, this cannot be a plain `BuiltinFn` because it must call back into the evaluator (to invoke `f`). Implement as a special form in `eval/mod.rs` alongside `eval_arrayfun`.

Algorithm:
```
for k = 1..len(t)-1:
    h  = t[k+1] - t[k]
    k1 = h * f(X[:,k],   t[k])
    k2 = h * f(X[:,k] + k1/2, t[k] + h/2)
    k3 = h * f(X[:,k] + k2/2, t[k] + h/2)
    k4 = h * f(X[:,k] + k3,   t[k+1])
    X[:,k+1] = X[:,k] + (k1 + 2*k2 + 2*k3 + k4) / 6
```

Output: `n × length(t)` matrix where column k is the state at `t(k)`.  
`x0` may be a scalar (1-state system) or a column vector ([n; 1] matrix).

---

**`lyap(A, Q)` → matrix X**

Solves the continuous Lyapunov equation: `A*X + X*A' + Q = 0`.

Implementation via Kronecker product vectorization:

```
vec(X) = -inv(A⊗I + I⊗A) * vec(Q)
```

We already have `kron`, `inv`, matrix multiply, and eye. Assembling this is ~20 lines:
1. n = size(A, 1)
2. K = kron(A, eye(n)) + kron(eye(n), A)   — n²×n² matrix
3. q_vec = column-major vectorize Q         — n² vector
4. x_vec = -inv(K) * q_vec
5. Reshape back to n×n matrix

**Complexity caveat:** O(n^4) storage for K. This is fine for controls problems (n ≤ 20 in any practical lesson). For n > 50, emit a runtime warning.

---

**`gram(A, B, type)` → matrix**

Thin wrapper around `lyap`. Two forms:
- `gram(A, B, "c")` — controllability Gramian: solves `A*W + W*A' + B*B' = 0` → `lyap(A, B*B')`
- `gram(A, C, "o")` — observability Gramian: solves `A'*W + W*A + C'*C = 0` → `lyap(A', C'*C)`

This is a regular builtin (no evaluator access needed — `lyap` takes matrices, not callables).

---

**`freqresp(A, B, C, D, w)` → matrix**

Evaluates the state-space frequency response H(jω) = C*(jωI - A)^{-1}*B + D at each frequency
in vector `w` (rad/s). Returns a matrix of size `p × length(w)` for SISO (p=1) systems, where
each column is the complex response at the corresponding frequency.

For the bootcamp, restrict to SISO (p=1, m=1) to keep the output a vector. For MIMO, return a
3-axis structure or emit a not-implemented error — MIMO frequency response can be added later.

Algorithm (loop over w):
```
for each ω in w:
    H(:,k) = C * inv(j*ω*I - A) * B + D
```

Uses existing `inv` and matrix multiply. Each `inv` is O(n³). For typical controls problems
(n ≤ 10, length(w) ≤ 500), this is fast enough.

---

### Phase C — More Involved (~2–4 hours each)

**`place(A, B, poles)` → vector K**

Ackermann's formula for SISO systems (B is n×1 vector):

```
K = e_n' * inv(ctrb(A, B)) * p(A)
```

where `p(s)` is the characteristic polynomial with roots at the desired poles, and `e_n'` is the
last standard basis row vector `[0, 0, ..., 0, 1]`.

Steps:
1. Compute `C = ctrb(A, B)` using existing builtin.
2. Compute `p(A)`: build polynomial coefficients from roots via `poly_from_roots(poles)`,
   then evaluate `p(A) = c[n]*A^n + ... + c[1]*A + c[0]*I` by Horner's scheme.
3. Return `e_n' * inv(C) * p(A)` as a row vector.

Need a `poly_from_roots` helper (not to be confused with `roots`, which goes the other way).
`poly_from_roots(r)` = convolve `(s - r[0])*(s - r[1])*...` iteratively.

**MIMO:** For MIMO pole placement, Ackermann only works for SISO. MIMO requires the
Rosenbrock-place or Van Dooren algorithm, which is substantially more complex. Scope to SISO
for now; emit a clear error if `B` has more than one column.

---

**`care(A, B, Q, R)` → matrix P**

Solves the Continuous Algebraic Riccati Equation (CARE):
`A'P + PA - PBR^{-1}B'P + Q = 0`

Implementation via **Newton-Kleinman iteration**:

```
P0 = solve_lyap(A, Q)     # initial guess: set the BR^{-1}B'P term to zero
for k = 1..max_iter:
    Ak = A - B*inv(R)*B'*Pk
    Pk+1 = lyap(Ak', Pk*B*inv(R)*B'*Pk + Q)
    if ||Pk+1 - Pk||_F < tol: break
```

The inner Lyapunov equation is `Ak'*X + X*Ak + Rhs = 0`, which maps to `lyap(Ak', Rhs)` using
our `lyap` function. Convergence is typically quadratic (4–8 iterations for well-conditioned problems).

**Failure modes to handle:**
- `A` not stabilizable: initial `Ak` unstable → `lyap` system matrix `Ak⊗I + I⊗Ak` is singular.
  Detect via check that all eig(A-B*K_lqr) have negative real parts before iterating.
- Non-convergence after max_iter: emit a runtime error.

**Relationship to `lqr`:** `lqr` already exists and internally solves a CARE. Its current
implementation can be checked — if it already uses a CARE solve internally, we can refactor
`care` to call that same code path. Look at `builtin_lqr` before writing `care` from scratch.

---

**`dare(A, B, Q, R)` → matrix P**

Solves the Discrete Algebraic Riccati Equation (DARE):
`P = A'PA - A'PB*(R + B'PB)^{-1}*B'PA + Q`

Implementation via value iteration (Lyapunov recursion):

```
P0 = Q
for k = 1..max_iter:
    L = inv(R + B'*Pk*B)
    Pk+1 = A'*Pk*A - A'*Pk*B*L*B'*Pk*A + Q
    if ||Pk+1 - Pk||_F < tol: break
```

This is a pure matrix iteration — no Lyapunov solve required. Converges for stable+controllable
systems but slowly (linear convergence). Alternative: Newton method for DARE (uses a Lyapunov
solve per step, quadratic convergence). Use simple iteration first; upgrade if needed.

---

**`svd(A)` → [U, S, V]**

Singular Value Decomposition: `A = U * S * V'`.  
Returns a 3-element tuple: `[U, S, V] = svd(A)`.  
`S` is returned as a vector of singular values (not a diagonal matrix), consistent with `eig`.  
If only one output: `s = svd(A)` returns just the singular value vector.

Implementation: **Golub-Reinsch algorithm** for real matrices (restrict to real; emit a warning
and take `real(A)` if complex is passed — this covers all controls bootcamp uses).

Steps:
1. **Bidiagonalization** via Householder reflections: find orthogonal U1, V1 such that
   `U1' * A * V1 = B` where B is upper bidiagonal.
2. **Implicit QR** with Wilkinson shift on B'B until superdiagonal elements vanish.
3. Accumulate U = U1 * (all left QR transformations), V = V1 * (all right QR transformations).

Reference: Golub & Van Loan "Matrix Computations" §8.6. The implementation is ~150–200 lines.
A simpler alternative is **one-sided Jacobi SVD** (~100 lines, slightly slower but easier to
implement correctly).

**Complexity vs. alternatives:**  
If full [U, S, V] is not needed urgently, a shortcut is: singular values of A = sqrt(real eigenvalues
of A'A). This gives just the values via existing `eig`. We could ship this first, then add U and V.
Decision: ship eigenvalue-based `svd(A)` (values only, no U/V) in phase C, add full [U, S, V] in a
follow-up if the bootcamp needs it. The feature request only shows `diag(S)` in the example.

---

## Implementation order

```
Phase A  logspace builtin exposure                    trivial
Phase B1 rk4                                          eval/mod.rs special form (~60 lines)
Phase B2 lyap                                         builtins.rs (~30 lines)
Phase B3 gram                                         builtins.rs (~20 lines)
Phase B4 freqresp                                     builtins.rs (~40 lines)
Phase C1 place                                        builtins.rs (~50 lines) + poly_from_roots helper
Phase C2 care                                         builtins.rs (~40 lines) — check if lqr already has this
Phase C3 dare                                         builtins.rs (~40 lines)
Phase C4 svd (values only first)                      builtins.rs (~30 lines via eigenvalue path)
```

Total estimated new code: ~350 lines across `eval/mod.rs` and `builtins.rs`.

---

## Tests

Each function needs at least one test in `tests.rs`. Proposed cases:

| Function | Test case |
|---|---|
| `logspace` | `logspace(-1, 2, 4)` → `[0.1, 1, 10, 100]` |
| `rk4` | `x_dot = -x` with `x0=1` → `x(end) ≈ exp(-t_end)` |
| `lyap` | `A=[-1,0;0,-2], Q=eye(2)` → verify `A*X+X*A'+Q ≈ 0` |
| `gram` | `eig(gram(A,B,"c"))` all non-negative for controllable system |
| `place` | `eig(A - B*place(A,B,poles))` matches desired poles |
| `care` | `P = care(A,B,Q,R)`, verify residual `A'P+PA-PBR⁻¹B'P+Q ≈ 0` |
| `dare` | `P = dare(A,B,Q,R)`, verify discrete residual |
| `freqresp` | 1st-order system `A=[-1], B=[1], C=[1], D=[0]`: `\|H(j*1)\| = 1/√2` |
| `svd` | `svd([3,0;0,2])` → `[3,2]`; `svd([1,1;0,1])` → values ≥ 0 |

---

## Docs updates (same commit as each function)

- `AGENTS.md` — add each new function to the builtin table
- `docs/quickref.md` — add each new function to the Language section
- `docs/functions.md` (if it exists) — add signature + example

---

## Decision points to resolve before implementing

1. **`lqr` internal CARE**: Read `builtin_lqr` before writing `care`. If it already does Newton-
   Kleinman or a Hamiltonian solve, extract that into a shared helper and `care` becomes a thin wrapper.

2. **`svd` scope**: Implement eigenvalue-based (singular values only, no U/V) first. Upgrade to
   full Golub-Reinsch only if the bootcamp examples need U and V explicitly.

3. **`rk4` step size**: The `t` vector fully specifies the time grid — `h = t[k+1]-t[k]`. The
   user controls accuracy by choosing `t`. No adaptive stepping.

4. **`place` MIMO**: Out of scope for now. MIMO pole placement is a multi-day algorithm.
   Emit `"place: only SISO systems supported (B must be n×1)"` for multi-column B.
