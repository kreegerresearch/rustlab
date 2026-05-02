# ODE simulation with rk4
#
# rk4(f, x0, t) integrates x_dot = f(x, t) using fixed-step 4th-order Runge-Kutta.
#
# Arguments:
#   f   — a lambda or function handle: f(x, t) returns x_dot as a scalar or column vector
#   x0  — initial state: scalar for 1-state systems, column vector [n; 0; ...] for n-state
#   t   — time vector (linspace is typical); step size h = t(k+1) - t(k)
#
# Returns:
#   1-state: a vector of length(t) — x at each time step
#   n-state: an n × length(t) matrix — X(:,k) is the state at t(k)
#
# Tip: accuracy improves with more points. A good rule of thumb is to use at least
#      10–50 points per cycle of the fastest oscillation in the system.
#
# ─────────────────────────────────────────────────────────────────────────────

# ── Example 1: Scalar exponential decay — x_dot = -a*x, x(0) = 1 ─────────────
# Exact solution: x(t) = exp(-a*t)
a = 2.0;
f_decay = @(x, t) -a * x;

t = linspace(0, 3, 300);
x_sim  = rk4(f_decay, 1.0, t);
x_exact = exp(-a .* t);

disp("Exponential decay — max error vs exact:")
max(abs(x_sim - x_exact))

# ── Example 2: Undamped harmonic oscillator (2-state) ────────────────────────
# x1_dot =  x2        (position → velocity)
# x2_dot = -ω²*x1    (velocity → acceleration)
# Exact: x1(t) = cos(ω*t), x2(t) = -ω*sin(ω*t)

omega = 3.0;
f_sho = @(x, t) [x(2); -(omega^2) * x(1)];

t2  = linspace(0, 2*pi/omega, 2000);
X2  = rk4(f_sho, [1; 0], t2);   # 2 × 2000 matrix

disp("SHO: x1 at t=2π/ω (should be ≈ 1.0):")
X2(1, length(t2))

disp("SHO: x2 at t=2π/ω (should be ≈ 0.0):")
X2(2, length(t2))

# ── Example 3: Damped harmonic oscillator ─────────────────────────────────────
# x1_dot =  x2
# x2_dot = -(2ζω)*x2 - ω²*x1   (ζ = damping ratio)
# ζ < 1 → underdamped; ζ = 1 → critically damped; ζ > 1 → overdamped

zeta  = 0.3;
omega = 5.0;
f_dho = @(x, t) [x(2); -(2*zeta*omega)*x(2) - (omega^2)*x(1)];

t3 = linspace(0, 3, 1500);
X3 = rk4(f_dho, [1; 0], t3);

disp("Damped SHO: final x1 (should decay toward 0):")
X3(1, length(t3))

# ── Example 4: Linear system via state-space matrices ────────────────────────
# x_dot = A*x + B*u  (here u=0 for free response)
A_ss = [-1, 2; -2, -1];
B_ss = [1; 0];
x0   = [1; 0];

f_ss = @(x, t) A_ss * x;   # free response (no input)

t4 = linspace(0, 4, 800);
X4 = rk4(f_ss, x0, t4);

disp("Free response final state:")
X4(1, length(t4))
X4(2, length(t4))

# ── Example 5: Nonlinear pendulum ─────────────────────────────────────────────
# theta_dot  = omega_dot
# omega_dot  = -(g/L)*sin(theta) - b*omega   (b = damping)

g = 9.81;
L = 1.0;
b = 0.5;
f_pend = @(x, t) [x(2); -(g/L)*sin(x(1)) - b*x(2)];

t5 = linspace(0, 6, 3000);
X5 = rk4(f_pend, [pi/4; 0], t5);   # 45° initial angle

disp("Pendulum: final angle (rad) — should approach 0:")
X5(1, length(t5))
