# Development Plan: Control Systems Toolbox

**Target example:** `examples/controls/classical_control.rlab`
**Current phase:** complete
**Status:** All phases complete (Phase 6 implemented 2026-04-04)

---

## Overview

Add a classical control systems toolbox to rustlab, covering transfer function
creation and arithmetic, step/Bode analysis, state-space representation,
controllability/observability, LQR design, and root locus.  The work is split
into six phases ordered by dependency.  Each phase must be fully implemented,
tested, and committed before the next begins.

---

## Phase 1 — Language Foundations
**Status: complete**

Self-contained additions with no dependency on new value types.  All items in
this phase are independent of each other and can be done in any order.

### 1a. `%` comment syntax
- **What:** treat `%` as a line comment (same as `#`)
- **File:** `crates/rustlab-script/src/lexer.rs`
- **Change:** add `'%'` arm in tokenizer, skip to end of line
- **Test:** `% comment\nx = 1` parses without error and sets x=1

### 1b. `if / else / end` control flow
- **What:** conditional branching
- **Syntax:** `if expr \n body [else \n body] end`
- **Files:**
  - `lexer.rs` — add `Token::If`, `Token::Else`
  - `ast.rs` — add `Stmt::If { cond: Expr, then_body: Vec<Stmt>, else_body: Vec<Stmt> }`
  - `parser.rs` — parse in `parse_stmts_until_end`; `else` handled as a secondary terminator
  - `eval/mod.rs` — eval `cond` as `Value::Bool`, run appropriate branch
- **Test:** `if 1 == 1 \n x=1 \n else \n x=2 \n end` → x=1

### 1c. Multiple-assignment from function calls
- **What:** `[a, b, c] = func()` unpacks a tuple return
- **Syntax:** `[name, name, ...] = call_expr`
- **Files:**
  - `ast.rs` — add `Stmt::MultiAssign { names: Vec<String>, expr: Expr, suppress: bool }`
  - `value.rs` — add `Value::Tuple(Vec<Value>)`
  - `parser.rs` — recognise `[IDENT, IDENT, ...] =` at statement level
  - `eval/mod.rs` — unpack `Value::Tuple` into named env entries
- **Test:** `function that returns tuple`; `[x, y] = f()` → correct values bound

### 1d. `disp()` and `fprintf()` builtins
- **What:** standard formatted output functions
- **`disp(x)`** — prints value followed by newline; alias for `print` with forced newline
- **`fprintf(fmt, arg, ...)`** — C-style formatted print; supports `%d %f %g %s %%`, `\n \t`
- **File:** `eval/builtins.rs`
- **Test:** `fprintf("%.2f\n", 3.14159)` prints `3.14`

### 1e. `all()` and `any()` builtins
- **What:** aggregate boolean tests over vectors
- **`all(v)`** — true if every real part of every element is nonzero
- **`any(v)`** — true if at least one element's real part is nonzero
- **File:** `eval/builtins.rs`
- **Test:** `all([1,2,3])` → true; `all([1,0,3])` → false

### 1f. `rank()` builtin
- **What:** matrix rank via SVD threshold
- **Signature:** `rank(M)` → scalar integer
- **Algorithm:** compute singular values; count those above `eps * max_sv * max(nrows, ncols)`
- **File:** `eval/builtins.rs`
- **Dependency:** `ndarray-linalg` (already available via `eig`)
- **Test:** `rank([1,2;2,4])` → 1; `rank(eye(3))` → 3

### 1g. `roots()` builtin
- **What:** roots of a polynomial given as coefficient vector (descending powers)
- **Signature:** `roots([1, 2, 10])` → complex vector (roots of s²+2s+10)
- **Algorithm:** build companion matrix, return eigenvalues via existing `eig` logic
- **File:** `eval/builtins.rs`
- **Test:** `roots([1, -3, 2])` → [2, 1] (roots of s²-3s+2)

---

## Phase 2 — Transfer Function Type
**Status: complete**
**Depends on:** Phase 1 (`roots`)

### 2a. `Value::TransferFn`
- **What:** new value variant `TransferFn { num: Vec<f64>, den: Vec<f64> }`
- Polynomials in descending-power order
- **Files:** `value.rs`
  - Add variant, `type_name()` → `"tf"`, `Display` renders `10 / (s² + 2s + 10)`
  - `binop()` — TF arithmetic:
    - `TF * scalar`, `scalar * TF`, `TF / scalar` — scale numerator
    - `scalar / TF` — numerator = `[scalar]`, denominator from TF
    - `TF + TF`, `TF - TF` — common denominator, polynomial add/sub
    - `TF * TF` — polynomial multiply num and den
    - `TF ^ usize` — repeated multiplication (integer exponents only)

### 2b. `tf()` builtin
- **Signatures:**
  - `tf("s")` → Laplace variable: num=`[1,0]`, den=`[1]`
  - `tf(num_vec, den_vec)` → explicit TF
- **File:** `eval/builtins.rs`
- **Test:** `s = tf("s"); G = 10 / (s^2 + 2*s + 10)` → correct num/den

### 2c. `pole()` and `zero()` builtins
- **Signatures:** `pole(G)`, `zero(G)` → complex vector
- **Algorithm:** call `roots(G.den)` / `roots(G.num)`
- **File:** `eval/builtins.rs`
- **Test:** `pole(tf([10],[1,2,10]))` → approx `-1±3j`

---

## Phase 3 — State-Space Type
**Status: complete**
**Depends on:** Phase 2

### 3a. `Value::StateSpace`
- **What:** `StateSpace { A: CMatrix, B: CMatrix, C: CMatrix, D: CMatrix }`
- **Files:** `value.rs`
  - Add variant, `type_name()` → `"ss"`, `Display`
  - `eval/mod.rs` `Expr::Field` — match `Value::StateSpace` and return the named matrix

### 3b. `ss()` builtin
- **What:** convert `TransferFn` to `StateSpace` via observable canonical form
- **File:** `eval/builtins.rs`
- **Test:** `ss(tf([10],[1,2,10]))` → A, B, C, D round-trip eigenvalues match `pole(G)`

### 3c. `ctrb()` and `obsv()` builtins
- **`ctrb(A, B)`** → `[B, AB, A²B, …]` (controllability matrix, n×n²)
- **`obsv(A, C)`** → `[C; CA; CA²; …]` (observability matrix, n²×n)
- **File:** `eval/builtins.rs`
- **Test:** full-rank controllability for a controllable second-order system

---

## Phase 4 — Frequency and Time-Domain Analysis
**Status: complete**
**Depends on:** Phases 2–3

### 4a. `bode()` builtin
- **Signatures:**
  - `bode(G)` — plot magnitude (dB) and phase (deg) panels, auto frequency range
  - `bode(G, w)` — user-supplied frequency vector
  - `[mag, phase, w] = bode(G)` — return data without plotting
- **Algorithm:** evaluate `G(jω) = num(jω) / den(jω)` at each frequency
- **File:** `eval/builtins.rs` + plot integration

### 4b. `step()` builtin
- **Signatures:**
  - `step(G)` — plot only
  - `[y, t] = step(G)` — return time and output vectors
  - `[y, t] = step(G, t_end)` — user-supplied final time
- **Algorithm:** convert TF → SS, integrate state equations with RK4 (u=1 input)
- **File:** `eval/builtins.rs`
- **Test:** step response of `10/(s²+2s+10)` reaches steady-state = 1.0

### 4c. `margin()` builtin
- **Signatures:**
  - `margin(G)` — plot Bode with margins annotated
  - `[Gm, Pm, Wcg, Wcp] = margin(G)` — return values
- **Algorithm:**
  1. Compute Bode data over dense log-spaced grid
  2. Phase crossover Wcg: ω where ∠H(jω) = −180°
  3. Gain crossover Wcp: ω where |H(jω)| = 0 dB
  4. GM = 1/|H(jWcg)|, PM = 180° + ∠H(jWcp)
- Returns `Value::Tuple([Gm, Pm, Wcg, Wcp])`

---

## Phase 5 — Optimal Control
**Status: complete**
**Depends on:** Phase 3

### 5a. `lqr()` builtin
- **Signature:** `[K, S, e] = lqr(sys_ss, Q, R)`
- **What:** solve continuous-time algebraic Riccati equation (CARE)
  `A'P + PA − PBR⁻¹B'P + Q = 0`
  Return `K = R⁻¹B'P`, solution `S = P`, closed-loop eigenvalues `e`
- **Algorithm:** Hamiltonian matrix eigendecomposition via existing `eig` code
- Returns `Value::Tuple([K, S, e])`
- **Test:** LQR gain for double-integrator produces stable closed-loop eigenvalues

---

## Phase 6 — Root Locus *(advanced, optional)*
**Status: complete**
**Depends on:** Phase 2

### 6a. `rlocus()` builtin
- **Signature:** `rlocus(G)`
- **What:** plot closed-loop pole paths as K sweeps from 0 → ∞
- **Algorithm:** for K in log-spaced vector, find roots of `den + K*num = 0`
  using companion-matrix eigenvalues; scatter-plot each set on the complex plane
- **File:** `eval/builtins.rs` + scatter/dot plot style on complex plane

---

## Phase Completion Checklist

Before marking a phase complete:
- [ ] All items in the phase are implemented
- [ ] Each item has at least one unit test (see AGENTS.md testing rules)
- [ ] `cargo test --workspace` passes
- [ ] Help entries added for all new builtins
- [ ] `dev/plans/controls.md` status field updated

---

## Dependency Graph

```
Phase 1  ──────────────────────────────────────────►  all independent
   │
   ▼
Phase 2  (TransferFn type, tf/pole/zero)
   │
   ├──► Phase 6  (rlocus — needs TF only)
   │
   ▼
Phase 3  (StateSpace, ss/ctrb/obsv)
   │
   ├──► Phase 5  (lqr — needs SS only)
   │
   ▼
Phase 4  (bode/step/margin)
```
