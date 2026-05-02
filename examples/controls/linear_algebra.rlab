# Linear algebra for controls: lyap, gram, svd, logspace
#
# This example covers the four utility functions used throughout control theory:
#
#   lyap(A, Q)         — solve continuous Lyapunov equation A*X + X*A' + Q = 0
#   gram(A, B, "c")    — controllability Gramian (energy to reach states)
#   gram(A, C, "o")    — observability Gramian (energy to observe states)
#   svd(A)             — singular value decomposition [U, S, V] = svd(A)
#   logspace(a, b, n)  — n log-spaced points from 10^a to 10^b
#
# ─────────────────────────────────────────────────────────────────────────────

# ── lyap: Lyapunov equation ───────────────────────────────────────────────────
# Solves A*X + X*A' + Q = 0 for X.
# A must be stable (all eigenvalues have negative real parts) for a unique solution.
#
# Common uses:
#   • Lyapunov stability test: if Q>0 and X>0 exist, the system is stable
#   • Computing Gramians (see below)
#   • H2 norm: ||G||_2² = trace(B' * X * B) where X = lyap(A', C'*C)

A = [-2, 1; 0, -3];
Q = [1, 0; 0, 1];
X = lyap(A, Q);

disp("Lyapunov solution X:")
X
disp("Residual A*X + X*A' + Q (should be ≈ 0):")
A*X + X*A' + Q

# ── gram: Controllability Gramian ─────────────────────────────────────────────
# Wc = gram(A, B, "c") solves A*Wc + Wc*A' + B*B' = 0.
#
# Physical meaning:
#   • Wc eigenvalues measure how easy each state direction is to reach
#   • rank(Wc) = rank(ctrb(A,B)) — so full rank ⟺ controllable
#   • trace(Wc) ≈ total energy needed to excite the system from rest
#
# The condition number of Wc indicates how "balanced" the system is.

B = [1; 0];   # only state x1 is directly actuated
Wc = gram(A, B, "c");

disp("Controllability Gramian Wc:")
Wc
disp("Eigenvalues of Wc (all positive ⟺ controllable):")
eig(Wc)

# ── gram: Observability Gramian ───────────────────────────────────────────────
# Wo = gram(A, C, "o") solves A'*Wo + Wo*A + C'*C = 0.
#
# Physical meaning:
#   • Wo eigenvalues measure how easy each state direction is to observe
#   • rank(Wo) = rank(obsv(A,C)) — full rank ⟺ observable
#
# Together, Wc and Wo inform balanced truncation for model reduction.

C = [1, 0];   # observe state x1 only
Wo = gram(A, C, "o");

disp("Observability Gramian Wo:")
Wo
disp("Eigenvalues of Wo (all positive ⟺ observable):")
eig(Wo)

# ── svd: Singular Value Decomposition ─────────────────────────────────────────
# [U, S, V] = svd(A) returns orthogonal U, singular values S (as vector), orthogonal V
# such that A ≈ U * diag(S) * V'.
#
# Uses:
#   • rank: number of singular values above tolerance
#   • 2-norm (spectral norm): S(1)
#   • Condition number: S(1) / S(end)
#   • Pseudo-inverse: V * diag(1./S) * U'  (for full-rank A)
#   • Directions of maximum/minimum gain

M = [3, 1; 1, 3; 0, 1];    # a 3×2 matrix
[U, S, V] = svd(M);

disp("Singular values S:")
S
disp("Largest singular value (= 2-norm of M):")
S(1)
disp("Condition number S(1)/S(end):")
S(1) / S(length(S))

# Verify reconstruction: M ≈ U(:,1:2) * diag(S) * V'
disp("Reconstruction error U*diag(S)*V' - M (should be ≈ 0):")
Ur = [U(1,1), U(1,2); U(2,1), U(2,2); U(3,1), U(3,2)];   # first 2 cols of U
Vt = [V(1,1), V(2,1); V(1,2), V(2,2)];                     # V transposed
max(max(abs(Ur * diag(S) * Vt - M)))

# ── logspace: logarithmically-spaced frequency vectors ───────────────────────
# logspace(a, b, n) returns n points from 10^a to 10^b.
# This is the standard way to create frequency axes for Bode plots.
#
# Compare with linspace:
#   linspace(-1, 3, 5) → [−1, 0, 1, 2, 3]            (equal gaps)
#   logspace(-1, 3, 5) → [0.1, 1, 10, 100, 1000]      (equal ratio)

w_log  = logspace(-1, 3, 5);
disp("logspace(-1, 3, 5) — 0.1 to 1000 rad/s:")
w_log

w_fine = logspace(-2, 4, 500);
disp("Fine grid: 500 log-spaced points from 0.01 to 10000 rad/s")
disp("First 4 points:")
w_fine(1:4)
disp("Last point:")
w_fine(500)
