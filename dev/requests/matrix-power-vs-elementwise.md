# Bug / Compatibility Issue: `A^k` is element-wise on matrices

## Symptom

For a square matrix `A` and an integer `k`, `A^k` in rustlab returns **element-wise** powers — each entry of `A` raised to `k` independently — not the matrix product `A * A * ... * A`. The element-wise operator `A.^k` does the same thing. There is no rustlab spelling for true matrix power.

## Reproduction

```rustlab
A = [0, 1, 0; 0, 0, 1; -6, -11, -6];

A^2
% Matrix(3x3)
%   [0.000000, 1.000000, 0.000000]
%   [0.000000, 0.000000, 1.000000]
%   [36.000000, 121.000000, 36.000000]      ← squared element-wise

A * A
% Matrix(3x3)
%   [0.000000, 0.000000, 1.000000]
%   [-6.000000, -11.000000, -6.000000]
%   [36.000000, 60.000000, 25.000000]       ← actual matrix product
```

The two results are different. MATLAB and Octave both define `A^k` as the matrix product (and `A.^k` as element-wise). Rustlab's behaviour silently returns the wrong answer for any code that expects MATLAB convention.

## Why this matters

- **Cayley–Hamilton verification** (`rustlab_controls` Lesson 08) computes `A^n + c_{n-1}*A^{n-1} + ... + c_0*I`. With element-wise `^`, this evaluates to garbage. The lesson works around it by writing `A2 = A * A; A3 = A2 * A;` explicitly.
- **Matrix exponential series** demos and any controls/DSP work that needs powers of $A$ (e.g. discrete-state propagation $\mathbf{x}_k = A^k \mathbf{x}_0$) hit the same trap.
- **Silent miscompute** is the dangerous failure mode here: no error is raised, the script runs, the numbers look plausible, and the conclusion is wrong.

## Encountered in

`rustlab_controls`:
- Lesson 08 (Cayley–Hamilton) — explicitly documented in the lesson prose and in `AGENTS.md` Indexing notes as a gotcha.
- Almost every script that touches eigenvalue-of-A power identities has had to use repeated `*` instead of `^`.

## Proposed fix

Adopt MATLAB / Octave convention:

| Expression | Meaning |
|---|---|
| `A^k`  | matrix power: repeated `*` for positive integer `k`; `inv(A)^abs(k)` for negative; `expm(k * logm(A))` for non-integer (lower priority). |
| `A.^k` | element-wise power (current behaviour). |

The element-wise behaviour stays, accessed via the dot. The `A^k` slot adopts the standard matrix interpretation.

## Tests to add

- `A = [0, 1; -1, 0]; A^2` returns `-I` (since this is a 90° rotation; squaring gives 180°).
- `eye(n) ^ k == eye(n)` for any `k`.
- `A.^2` still returns element-wise squares.
- Cayley–Hamilton round-trip for `A = [0, 1, 0; 0, 0, 1; -6, -11, -6]`: `A^3 + 6*A^2 + 11*A + 6*I` is the zero matrix (currently it isn't).

## Severity

Pedagogically this is the highest-priority correctness issue I've hit in rustlab. Element-wise `A^k` violates a 40-year MATLAB/Octave convention that essentially every linear-algebra reference assumes. It is also the kind of error that is invisible until the result is plugged into a downstream identity that fails — at which point the user has no clue why.

## Migration concern

Any existing rustlab code that uses `A^k` on matrices and relies on the element-wise behaviour would break. Suggested mitigation: a deprecation warning for one release ("`A^k` on a matrix will mean matrix power in the next version; use `A.^k` for element-wise"), then flip the behaviour. Or land the change with a clear release-notes entry — the breakage is mechanical (`^` → `.^`) and grep-able.
