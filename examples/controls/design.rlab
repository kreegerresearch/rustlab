# Optimal control design: care, dare, place, freqresp
#
# Functions covered:
#
#   care(A, B, Q, R)       — Continuous Algebraic Riccati Equation (CARE)
#                            Solves A'P + PA - PBR⁻¹B'P + Q = 0
#                            Used for continuous-time LQR gain: K = inv(R)*B'*P
#
#   dare(A, B, Q, R)       — Discrete Algebraic Riccati Equation (DARE)
#                            Used for discrete-time LQR and Kalman filter design
#
#   place(A, B, poles)     — Ackermann pole placement (SISO only)
#                            Returns K such that eig(A - B*K) = poles
#
#   freqresp(A,B,C,D,w)    — Frequency response H(jω) = C*(jω*I-A)⁻¹*B + D
#                            Returns complex vector (SISO) or matrix (MIMO)
#
# ─────────────────────────────────────────────────────────────────────────────

# ── System: double integrator ─────────────────────────────────────────────────
# x_dot = A*x + B*u,  y = C*x
# A = [0,1;0,0], B = [0;1] (position + velocity, force input)
# This system is marginally stable (eigenvalues at 0).

A = [0, 1; 0, 0];
B = [0; 1];
C = [1, 0];    # observe position only
D = [0];

disp("Open-loop eigenvalues (both at 0 — marginally stable):")
eig(A)

# ── care: Continuous LQR via CARE ─────────────────────────────────────────────
# lqr(sys, Q, R) already solves CARE internally. care(A,B,Q,R) gives just P.
# Use care when you only need P (e.g. to compute H2 norm or check Lyapunov).
#
# Q penalises position error (10×) and velocity error (1×).
# R = 1 (unit control cost).

Q_lqr = [10, 0; 0, 1];
R_lqr = 1;

P = care(A, B, Q_lqr, R_lqr);
disp("CARE solution P:")
P

# Optimal LQR gain (same as what lqr() returns)
K_care = inv(R_lqr) * B' * P;
disp("LQR gain K from CARE:")
K_care

disp("Closed-loop eigenvalues with K_care (should be stable):")
eig(A - B * K_care)

# Verify CARE residual: A'P + PA - PBR⁻¹B'P + Q ≈ 0
residual = A' * P + P * A - P * B * inv(R_lqr) * B' * P + Q_lqr;
disp("CARE residual max |element| (should be ≈ 0):")
max(max(abs(residual)))

# ── dare: Discrete LQR via DARE ───────────────────────────────────────────────
# Discretize the double integrator with dt = 0.1 s using expm.
dt = 0.1;
Ad = expm(A * dt);
Bd = [dt^2/2; dt];   # approximate: integral of expm(A*s)*B ds

disp("Discrete-time A:")
Ad
disp("Discrete-time B:")
Bd

P_d = dare(Ad, Bd, Q_lqr, R_lqr);
disp("DARE solution P_d:")
P_d

K_dare = inv(R_lqr + Bd' * P_d * Bd) * Bd' * P_d * Ad;
disp("Discrete LQR gain K_dare:")
K_dare

disp("Discrete closed-loop eigenvalues (should be inside unit circle):")
eig(Ad - Bd * K_dare)

# ── place: Pole placement ──────────────────────────────────────────────────────
# place(A, B, poles) computes K such that eig(A - B*K) = poles.
# Only works for SISO systems (B must be n×1).
# Choose poles with negative real parts for stability; faster poles = faster response.

desired_poles = [-3, -4];
K_place = place(A, B, desired_poles);

disp("Pole placement gain K:")
K_place

disp("Closed-loop eigenvalues (should match desired_poles):")
eig(A - B * K_place)

# Compare step responses conceptually: LQR minimises a cost, place just hits the poles.
# LQR-placed poles tend to be better balanced for energy vs performance.

# ── freqresp: Frequency response ─────────────────────────────────────────────
# freqresp(A, B, C, D, w) evaluates H(jω) = C*(jω*I - A)^{-1}*B + D at each ω.
# Returns a complex vector for SISO systems.
#
# The closed-loop system with K_lqr: A_cl = A - B*K, B_cl = B, C_cl = C, D_cl = D
# Here we compute the open-loop response (unstable plant) and closed-loop (stable).

w = logspace(-1, 2, 200);   # 0.1 to 100 rad/s

# Open-loop: A, B, C, D (double integrator — will have ω^{-2} rolloff)
H_ol = freqresp(A, B, C, D, w);
disp("Open-loop |H(jω)| at ω=1 rad/s (should be 1.0 for double integrator):")
abs(H_ol(100))   # ω=1 is roughly the middle of logspace(-1,2,200)

# Closed-loop with LQR gain
A_cl = A - B * K_care;
H_cl = freqresp(A_cl, B, C, D, w);
disp("Closed-loop |H(jω)| at ω=0.1 rad/s (DC-like, should be ≈ 1):")
abs(H_cl(1))

disp("Closed-loop magnitude at several frequencies (dB):")
20 * log10(abs(H_cl(1:5:50)))

# ── Summary of design comparison ─────────────────────────────────────────────
disp("")
disp("=== Design Summary ===")
disp("Pole placement K:")
K_place
disp("LQR K (from CARE):")
K_care
disp("Open-loop poles:")
eig(A)
disp("Placed poles:")
eig(A - B * K_place)
disp("LQR poles:")
eig(A - B * K_care)
