# Feature Request: `c2d(sys, dt)` and `d2c(sys, dt)` — continuous/discrete conversion

## Problem

Converting a continuous state-space model to discrete (zero-order hold or Tustin) is currently spelled out by hand at every use site:

```rustlab
% Continuous (A, B), sample period dt
Ad = expm(A * dt);
Bd = inv(A) * (Ad - eye(n)) * B;       % ZOH for invertible A; otherwise integrate
```

This is correct but verbose, and the inverse-of-A formula breaks when $A$ is singular (e.g. integrators in the cart-pole). The general formula via the augmented matrix $\exp([A, B; 0, 0]\,dt)$ is *less* obvious than `Ad = expm(A*dt)` so many users don't know about it. A builtin would hide the math and handle the singular-$A$ case once.

## Encountered in

`rustlab_controls`:
- **Lesson 06 — Discrete-Time Controllability.** First place the formula appears; copied to several scripts thereafter.
- **Lesson 13 — Luenberger Observer.** When discretising the observer.
- **Lesson 14 — Kalman Filter.** Discrete Kalman is the standard implementation in real systems; current lesson uses Euler stepping rather than exact discretisation to avoid the `inv(A)` issue.
- **Phase 6 lessons** (planned) will need `c2d` for digital-controller analysis.

## Proposed API

Mirror MATLAB's signatures:

```rustlab
sys_d = c2d(sys, dt)               % zero-order hold (default)
sys_d = c2d(sys, dt, "zoh")        % explicit
sys_d = c2d(sys, dt, "tustin")     % bilinear / Tustin
sys_d = c2d(sys, dt, "foh")        % first-order hold (lower priority)

sys_c = d2c(sys_d, dt)             % inverse — exact log-based
sys_c = d2c(sys_d, dt, "tustin")
```

Where `sys` is a state-space struct (from `ss(...)`) and the returned `sys_d` is the same shape with `.A`, `.B`, `.C`, `.D` updated. `D` and `C` pass through for ZOH; `C, D` get a small correction for Tustin (`Cd = C(I - A*dt/2)^-1`, etc.).

## Implementation notes

ZOH using the augmented-matrix trick handles singular $A$ correctly:

```
M = [A, B; zeros(m, n+m)] * dt
expM = expm(M)
Ad = expM[1:n, 1:n]
Bd = expM[1:n, n+1:end]
```

This works for all $A$ (integrators included) without special-casing.

Tustin / bilinear:
```
Ad = (I + A*dt/2) * inv(I - A*dt/2)
Bd = inv(I - A*dt/2) * B * dt
Cd = C * inv(I - A*dt/2)
Dd = D + Cd * B * dt/2
```

## Tests to add

- ZOH round-trip: `d2c(c2d(sys, 0.1), 0.1)` returns the original within tolerance.
- Stable continuous → stable discrete (eigenvalues inside unit disk).
- Cart-pole specifically: `c2d(ss_cartpole, 0.05)` should give a discrete model with eigenvalues at $e^{\lambda \cdot 0.05}$ for each continuous $\lambda$, including the integrator modes correctly handled.
- Tustin preserves stability of stable plants for any $dt > 0$.

## Severity

Nice-to-have. The manual ZOH formula works for invertible-$A$ cases; we Euler-step the Kalman lessons to dodge the singular-$A$ issue. A builtin would let the lessons present the discretisation as a one-line tool rather than a several-line incantation that changes form depending on the plant.
