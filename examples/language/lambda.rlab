# Lambda / anonymous function examples

# ── Basic lambda ─────────────────────────────────────────────────────────────
sq  = @(x) x^2
inc = @(x) x + 1

sq(5)
inc(10)

# Compose: sq(inc(4)) = 5^2 = 25
composed = sq(inc(4))

# ── Multi-argument lambda ─────────────────────────────────────────────────────
hyp = @(a, b) sqrt(a^2 + b^2)
hyp(3, 4)
hyp(5, 12)

# ── Lambda over a vector (element-wise) ──────────────────────────────────────
double  = @(v) v .* 2
rectify = @(v) v .* (v > 0)

v = -3:3;
double(v)
rectify(v)

# ── Lexical capture: env snapshot taken at creation time ─────────────────────
gain = 0.5
attenuate = @(x) x * gain

attenuate(10)    % 5.0 — uses gain=0.5

gain = 99        % changing gain does not affect the lambda
attenuate(10)    % still 5.0

# ── Function handles with @ ───────────────────────────────────────────────────
h_sin  = @sin
h_abs  = @abs
h_sqrt = @sqrt

h_sin(pi / 2)
h_abs(-7)
h_sqrt(16)

# ── Passing a lambda to a named function ─────────────────────────────────────
function y = apply(f, x)
  y = f(x)
end

function y = apply_twice(f, x)
  y = f(f(x))
end

apply(@sq, 4)          % 16
apply_twice(@inc, 0)   % 2
apply_twice(sq, 3)     % (3^2)^2 = 81

# ── Building a simple function table ─────────────────────────────────────────
function y = tabulate(f, xs)
  y = f(xs)
end

xs = linspace(0, pi/2, 5);
tabulate(@sin, xs)
tabulate(@cos, xs)
tabulate(sq, xs)

# ── arrayfun: apply a function element-wise ──────────────────────────────────
xs = 1:5;

# Scalar output → vector
arrayfun(@(x) x^2, xs)

# Vector output → matrix (each row is f(xs(i)))
# @(x) [x, x^2, x^3] maps each scalar to a 3-element row
arrayfun(@(x) [x, x^2, x^3], xs)

# Use a named builtin handle
arrayfun(@sin, linspace(0, pi/2, 4))

# ── feval: call by string name ────────────────────────────────────────────────
feval("sqrt", 144)
feval("sin", pi/6)

function y = cube(x)
  y = x^3
end
feval("cube", 4)

# ── Inline DSP: window a signal without a named helper ───────────────────────
N  = 64;
t  = linspace(0, 1, N);
x  = sin(2 * pi * 5 .* t);     % 5 Hz tone

apply_hann = @(sig) sig .* (0.5 - 0.5 .* cos(2 .* pi .* linspace(0, 1, N)));
windowed = apply_hann(x);

disp("peak of windowed signal:")
max(abs(windowed))
