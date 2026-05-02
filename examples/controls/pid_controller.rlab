# PID Controller — discrete-time simulation
#
# Simulates a PID controller regulating a first-order plant with delay.
#
# Plant:  G(s) = K_p / (tau*s + 1)   (first-order lag)
# Controller: u(k) = Kp*e(k) + Ki*sum(e)*dt + Kd*(e(k)-e(k-1))/dt
#
# The loop runs at a fixed sample rate dt. A pure transport delay of
# N samples is modelled with a circular buffer.
#
# NOTE: This is an offline simulation.  For a real-time control loop,
#       uncomment the  sleep(dt)  call inside the loop to pace each
#       iteration to wall-clock time.
# ─────────────────────────────────────────────────────────────────────────

# ── Plant parameters ─────────────────────────────────────────────────────
K_plant = 2.0;       # DC gain
tau     = 0.5;       # time constant (s)

# ── Transport delay ──────────────────────────────────────────────────────
delay_s  = 0.1;      # pure delay (s)

# ── Simulation timing ────────────────────────────────────────────────────
dt    = 0.01;        # sample period (s)
T_end = 5.0;         # total simulation time (s)
N     = round(T_end / dt);        # number of steps
delay_samples = round(delay_s / dt);   # delay in samples

# ── PID gains (hand-tuned for this plant) ────────────────────────────────
Kp = 1.2;
Ki = 2.0;
Kd = 0.05;

# ── Actuator saturation ─────────────────────────────────────────────────
u_max =  10.0;
u_min = -10.0;

# ── Setpoint profile: step from 0 → 1 at t = 0.5 s, then 1 → 0.5 at t = 3 s
function r = setpoint(t)
    if t < 0.5
        r = 0.0;
    elseif t < 3.0
        r = 1.0;
    else
        r = 0.5;
    end
end

# ── Preallocate output vectors ───────────────────────────────────────────
t_vec = linspace(0, T_end, N);
y_vec = zeros(1, N);     # plant output
u_vec = zeros(1, N);     # control signal
r_vec = zeros(1, N);     # setpoint
e_vec = zeros(1, N);     # error

# ── Delay buffer (ring buffer of control signals) ────────────────────────
delay_buf = zeros(1, delay_samples + 1);

# ── State variables ──────────────────────────────────────────────────────
x       = 0.0;     # plant state
e_prev  = 0.0;     # previous error (for derivative)
e_sum   = 0.0;     # integral of error

# ── Main control loop ────────────────────────────────────────────────────
for k = 1:N
    t_now = (k - 1) * dt;

    # --- Setpoint & error ---
    r_k = setpoint(t_now);
    e_k = r_k - x;

    # --- PID law ---
    e_sum = e_sum + e_k * dt;
    if dt > 0
        de = (e_k - e_prev) / dt;
    else
        de = 0.0;
    end

    u_raw = Kp * e_k + Ki * e_sum + Kd * de;

    # --- Anti-windup: clamp output & freeze integrator on saturation ---
    if u_raw > u_max
        u_k = u_max;
        e_sum = e_sum - e_k * dt;
    elseif u_raw < u_min
        u_k = u_min;
        e_sum = e_sum - e_k * dt;
    else
        u_k = u_raw;
    end

    # --- Apply transport delay via ring buffer ---
    # Write current u into the buffer; read the delayed value
    buf_idx = mod(k - 1, delay_samples + 1) + 1;
    delay_buf(buf_idx) = u_k;
    read_idx = mod(k - 1 - delay_samples, delay_samples + 1) + 1;
    if k <= delay_samples
        u_delayed = 0.0;
    else
        u_delayed = delay_buf(read_idx);
    end

    # --- Plant dynamics: first-order lag  x_dot = (-x + K*u) / tau ---
    x_dot = (-x + K_plant * u_delayed) / tau;
    x     = x + x_dot * dt;    # forward Euler integration

    # --- Store results ---
    t_vec(k) = t_now;
    y_vec(k) = x;
    u_vec(k) = u_k;
    r_vec(k) = r_k;
    e_vec(k) = e_k;

    e_prev = e_k;

    # Uncomment for real-time pacing:  sleep(dt)
end

# ── Results ──────────────────────────────────────────────────────────────
disp("PID simulation complete.")
fprintf("  Sample period dt  = %.3f s\n", dt)
fprintf("  Transport delay   = %.3f s  (%d samples)\n", delay_s, delay_samples)
fprintf("  Gains: Kp=%.2f  Ki=%.2f  Kd=%.3f\n", Kp, Ki, Kd)

disp("")
disp("Steady-state check (t = 4.5 s, setpoint = 0.5):")
idx_check = round(4.5 / dt);
fprintf("  y(4.5) = %.4f   (target 0.5)\n", y_vec(idx_check))
fprintf("  e(4.5) = %.4f\n", e_vec(idx_check))

# ── Plot step-response ───────────────────────────────────────────────────
plot(real(y_vec), "PID Response: y(t) vs setpoint")
