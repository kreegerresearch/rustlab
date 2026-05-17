# Function Reference

Complete reference for all built-in functions and constants available in the rustlab scripting language.

---

## Constants

| Name | Value | Description |
|------|-------|-------------|
| `i`  | `0 + 1i` | Imaginary unit. Use in expressions: `z = 3 + i*4` |
| `j`  | `0 + 1i` | Alias for `i`. Both are always available: `z = 3 + j*4` |
| `pi` | 3.14159… | π |
| `e`  | 2.71828… | Euler's number |
| `true` | `Bool(true)` | Boolean true — can be used directly in `if` / `while` conditions |
| `false` | `Bool(false)` | Boolean false — can be used directly in `if` / `while` conditions |

---

## Math

### Type-preservation note for element-wise operators

rustlab stores all vectors and matrices as `Complex<f64>`. The element-wise operators `.*`, `./`, and `.^` between two **essentially-real** operands (every imaginary part below `f64::EPSILON ≈ 2.2e-16`) return a result whose imaginary part is exactly zero — no `real(...)` wrapper required to drop floating-point noise.

```
u = [1.0, 2.0, 3.0]
v = [4.0, 5.0, 6.0]
w = u ./ v          % w has imag(w) ≡ 0 exactly
```

The guard fires only when **both** operands are essentially real. Pass a complex operand and the imag part is preserved, so phasor / lossy-material code is unaffected:

```
z = [1+j, 2+j]
w = z ./ [3, 4]     % w has nonzero imag — z's complex content is preserved
```

This applies to `.*`, `./`, and `.^` on `Vector` and `Matrix` types. Other operators (`*`, `/`, `inv`, `fft`, etc.) still produce complex outputs even on real inputs — extending the no-noise behaviour to those would require a typed-real value variant, which is tracked as a future plan.

### `abs(x)`
Absolute value or magnitude. Element-wise on all numeric types.
- Scalar: `abs(-3.0)` → `3.0`
- Complex: `abs(3 + j*4)` → `5.0` (L2 norm)
- Vector: element-wise magnitude, returns real vector
- Matrix: element-wise magnitude, returns real matrix of the same shape

### `angle(x)`
Phase angle in radians (`atan2(im, re)`).
- Complex: `angle(1 + j*1)` → `0.7854` (π/4)
- Vector: element-wise

### `real(x)`
Real part of a scalar, complex number, vector, or matrix. A 1×1 matrix returns a scalar.

### `imag(x)`
Imaginary part of a scalar, complex number, vector, or matrix. A 1×1 matrix returns a scalar.

### `conj(x)`
Complex conjugate — negates the imaginary part. Accepts scalar, complex, vector, or matrix.
```
conj(3 + j*4)          # → 3 - 4i
conj([1+j, 2-j*3])     # → [1-1i, 2+3i]
conj(5.0)              # → 5.0  (real input unchanged)
```
- Element-wise for vectors and matrices.
- Real scalars and real-valued inputs are returned unchanged.

### `cos(x)`
Cosine, element-wise. Accepts scalar, complex, vector, or matrix.

### `sin(x)`
Sine, element-wise. Accepts scalar, complex, vector, or matrix.

### `acos(x)`
Inverse cosine in radians, element-wise. Accepts scalar, complex, vector, or matrix.

### `asin(x)`
Inverse sine in radians, element-wise. Accepts scalar, complex, vector, or matrix.

### `atan(x)`
Inverse tangent in radians (single-argument), element-wise. Accepts scalar, complex, vector, or matrix.
For the two-argument form see `atan2(y, x)`.

### `tanh(x)`
Hyperbolic tangent, element-wise. Accepts scalar, complex, vector, or matrix.
```
tanh(0.0)          # → 0.0
tanh(1.0)          # → ~0.762
tanh([-1, 0, 1])   # → [~-0.762, 0.0, ~0.762]
```
- Saturates toward ±1 for large |x|; used as a classic neural network activation.

### `sqrt(x)`
Square root, element-wise. Accepts scalar, complex, vector, or matrix.

### `exp(x)`
Natural exponential `eˣ`, element-wise. Accepts scalar, complex, vector, or matrix.
```
exp(j * pi)   # → -1 + 0i  (Euler's identity)
```

### `log(x)`
Natural logarithm (base e), element-wise. Accepts scalar, complex, vector, or matrix.

### `log10(x)`
Base-10 logarithm, element-wise. Accepts scalar, complex, vector, or matrix.
```
log10(1000.0)   # → 3.0
```
Commonly used for dB calculations:
```
db = 20.0 * log10(abs(X) + 1e-12)
```

### `log2(x)`
Base-2 logarithm, element-wise. Accepts scalar, complex, vector, or matrix.
```
log2(8.0)    # → 3.0
log2(1024)   # → 10.0
```
Useful for computing bit depths and octave-spaced frequency grids.

### `sinh(x)`
Hyperbolic sine, element-wise. Accepts scalar, complex, vector, or matrix.
```
sinh(0.0)   # → 0.0
sinh(1.0)   # → ~1.175
```

### `cosh(x)`
Hyperbolic cosine, element-wise. Accepts scalar, complex, vector, or matrix.
```
cosh(0.0)   # → 1.0
cosh(1.0)   # → ~1.543
```
Identity: `cosh(x)^2 - sinh(x)^2 = 1`.

### `floor(x)`
Largest integer ≤ x, applied to real and imaginary parts independently.
```
floor(3.7)         # → 3.0
floor(-2.3)        # → -3.0
floor(2.9 + 1.4i)  # → 2.0 + 1.0i
floor([1.1, 2.9])  # → [1.0, 2.0]
```

### `ceil(x)`
Smallest integer ≥ x, applied to real and imaginary parts independently.
```
ceil(3.2)    # → 4.0
ceil(-2.7)   # → -2.0
```

### `round(x)`
Round to nearest integer (half away from zero), applied to real and imaginary parts independently.
```
round(2.5)    # → 3.0
round(2.4)    # → 2.0
round(-2.5)   # → -3.0
```

### `sign(x)`
Sign / unit direction, element-wise.
- Real scalar: returns −1, 0, or +1.
- Complex: returns `z / |z|` (unit vector in the direction of z), or 0+0i when z is zero.
```
sign(-5.0)    # → -1.0
sign(0.0)     # → 0.0
sign(3 + 4i)  # → 0.6 + 0.8i
```

### `mod(x, m)`
Modulo: `x − m·floor(x/m)`, element-wise. The result always has the same sign as `m` (Python-style, not C-style).
- `x`: scalar, complex, vector, or matrix.
- `m`: real scalar.
```
mod(7.0, 3.0)    # → 1.0
mod(-1.0, 3.0)   # → 2.0
mod(0:5, 3.0)    # → [0, 1, 2, 0, 1, 2]
```

### `atan2(y, x)`
Four-quadrant arctangent in radians. Returns the angle of the point (x, y), correctly handling all quadrants.
```
atan2(1.0, 1.0)    # → π/4
atan2(-1.0, -1.0)  # → -3π/4
```
- Both arguments can be scalar, vector, or matrix. Broadcasting is supported (scalar × vector).
- Always returns a real value.

### `meshgrid(x, y)`
Create 2D grid matrices from two vectors. Returns a tuple `[X, Y]` where X replicates `x` as rows and Y replicates `y` as columns.
```
[X, Y] = meshgrid(1:3, 1:2)
# X = [1,2,3; 1,2,3]   (2×3)
# Y = [1,1,1; 2,2,2]   (2×3)
```
- Useful for evaluating functions over a 2D grid: `R = sqrt(X .^ 2 + Y .^ 2)`.

### `rect_mask(X, Y, x0, y0, w, h)`
Axis-aligned rectangle mask. Returns an `ny×nx` real-valued matrix the same shape as the meshgrid inputs, with `1.0` inside `[x0, x0+w] × [y0, y0+h]` (inclusive on all four sides) and `0.0` outside. `w` and `h` must be finite and non-negative; zero-extent rectangles match only the boundary line / point.
```
[X, Y] = meshgrid(linspace(0, 1, 21), linspace(0, 1, 21))
M = rect_mask(X, Y, 0.25, 0.25, 0.5, 0.5)
```

### `disk_mask(X, Y, xc, yc, r)`
Closed-disk mask. Returns an `ny×nx` real-valued matrix with `1.0` where `(X-xc)² + (Y-yc)² ≤ r²` and `0.0` elsewhere. `r` must be finite and non-negative; `r = 0` matches only the cells closest to the centre.
```
[X, Y] = meshgrid(linspace(-1.5, 1.5, 200), linspace(-1.5, 1.5, 200))
D = disk_mask(X, Y, 0, 0, 1)
# sum(sum(D)) * (3/199)^2  ≈  π
```

### `polygon_mask(X, Y, verts)`
Polygon mask via even-odd ray casting (PNPOLY). `verts` is an `N×2` matrix where each row is `[x, y]`; the polygon is implicitly closed (an edge connects vertex `N-1` back to vertex `0`). Returns an `ny×nx` real-valued matrix with `1.0` inside and `0.0` outside.
```
[X, Y] = meshgrid(linspace(0, 1, 50), linspace(0, 1, 50))
T = polygon_mask(X, Y, [0,0; 1,0; 0.5,1])    # triangle
```
Degenerate inputs return an all-zero mask: fewer than 3 vertices, or all vertices collinear (zero interior area). Behaviour at points exactly on a polygon edge is implementation-defined — callers needing exact-edge semantics should perturb the polygon slightly. Compose masks with element-wise math: `.* M2` (intersection), `1 - M` (complement), `max(M1, M2)` (union), `M1 .* (1 - M2)` (set difference).

### `gradient(F)` / `gradient(F, dx, dy)`
2-D gradient of a scalar field on a uniform grid. `F` is an `ny×nx` matrix where rows index `y` and columns index `x`. Returns a tuple `[Fx, Fy]` of the same shape.
```
[X, Y] = meshgrid(linspace(-1, 1, 21), linspace(-1, 1, 21));
F = X .^ 2 + Y .^ 2;
[Fx, Fy] = gradient(F, 0.1, 0.1);    % Fx ≈ 2X, Fy ≈ 2Y
```
- 2nd-order central differences in the interior; 2nd-order one-sided differences at boundaries (NumPy convention).
- `dx`, `dy` default to 1.0 if omitted; both must be positive.
- Each axis must have length ≥ 3.
- Complex inputs are supported (frequency-domain EM fields, etc.).

### `divergence(Fx, Fy)` / `divergence(Fx, Fy, dx, dy)`
2-D divergence `∂Fx/∂x + ∂Fy/∂y`. `Fx` and `Fy` must share shape; output has the same shape.
```
D = divergence(Fx, Fy, 0.1, 0.1);
```
- Same stencils, defaults, and shape requirements as `gradient`.

### `curl(Fx, Fy)` / `curl(Fx, Fy, dx, dy)`
Z-component of `∇×F` for a 2-D vector field: `∂Fy/∂x − ∂Fx/∂y`. Returns a scalar field with the same shape as `Fx`.
```
Cz = curl(Fx, Fy, 0.1, 0.1);
```
- Same stencils, defaults, and shape requirements as `gradient`.

### `gradient3(F)` / `gradient3(F, dx, dy, dz)`
3-D gradient of a scalar field on a uniform grid. `F` is a Tensor3 of shape `(m, n, p)`. The grid convention extends the 2-D one: axis 0 = y (rows), axis 1 = x (columns), axis 2 = z (pages), so `F(i, j, k)` ↔ `(x = (j-1)*dx, y = (i-1)*dy, z = (k-1)*dz)`.

Returns a tuple `[Fx, Fy, Fz]` of three Tensor3s with the same shape as `F`.
```
T = reshape(1:60, 3, 4, 5);
[Fx, Fy, Fz] = gradient3(T, 0.1, 0.1, 0.1);
```
- Same stencils as `gradient` (2nd-order central interior + 2nd-order one-sided boundaries).
- Each axis must have length ≥ 3.
- `dx`, `dy`, `dz` default to 1.0 if omitted; all must be positive.
- Complex inputs are supported.

### `divergence3(Fx, Fy, Fz)` / `divergence3(Fx, Fy, Fz, dx, dy, dz)`
3-D divergence `∂Fx/∂x + ∂Fy/∂y + ∂Fz/∂z`. All three components must share shape; output is a Tensor3 of the same shape.
```
D = divergence3(Fx, Fy, Fz, 0.1, 0.1, 0.1);
```
- Same stencils, defaults, and shape requirements as `gradient3`.

### `curl3(Fx, Fy, Fz)` / `curl3(Fx, Fy, Fz, dx, dy, dz)`
3-D curl `∇×F`. Returns a tuple `[Cx, Cy, Cz]` of three Tensor3s with the same shape as `Fx`:

- `Cx = ∂Fz/∂y − ∂Fy/∂z`
- `Cy = ∂Fx/∂z − ∂Fz/∂x`
- `Cz = ∂Fy/∂x − ∂Fx/∂y`

```
[Cx, Cy, Cz] = curl3(Fx, Fy, Fz, 0.1, 0.1, 0.1);
```
- Same stencils, defaults, and shape requirements as `gradient3`.

---

## Statistics

### `min(v)` / `min(M)` / `min(a, b)` / `min(M, [], dim)` / `[m, i] = min(...)`
Smallest value, with an optional second output for the 1-based index of the first occurrence.
```
min([3.0, 1.0, 4.0, 1.5])         # → 1.0
[m, i] = min([3.0, 1.0, 4.0, 1.5])    # m = 1.0, i = 2 (first occurrence)
min(5, 3)                          # → 3.0  (elementwise two-scalar form)
min([1, 5, 3; 4, 2, 6])           # → [1, 2, 3]   (column mins, 1×N row)
[M, I] = min(A, [], 2)             # row mins; M and I are nrows×1
```
- **Multi-return** is valid for the 1-arg vector/matrix form and the 3-arg axis form. Calling `[m, i] = min(a, b)` on the elementwise two-argument form errors — the index has no defined meaning there.
- **Comparison key.** Purely-real input compares by real value (so `max([-3, 1, -5])` is `1`, not `-5`). If any element has a nonzero imaginary part the comparison switches to magnitude `|z|`. **This diverges from MATLAB on equal magnitudes**: rustlab returns the first occurrence, MATLAB falls back to phase angle.
- **NaN handling.** `NaN` entries are skipped during the fold (MATLAB-compatible). All-NaN input errors explicitly rather than silently returning `NaN` at index 1.
- **Tie-breaking.** First occurrence wins, both real and complex paths.

### `max(v)` / `max(M)` / `max(a, b)` / `max(M, [], dim)` / `[m, i] = max(...)`
Largest value, with an optional second output for the 1-based index of the first occurrence. Same rules as `min` (comparison key, NaN handling, tie-breaking, multi-return restrictions).
```
max([3.0, 1.0, 4.0, 1.5])             # → 4.0
[m, i] = max([3, 1, 4, 1, 5, 9, 2])  # m = 9, i = 6
max(0, -5)                             # → 0.0
[M, I] = max([1, 5, 3; 4, 2, 6])     # M = [4, 5, 6], I = [2, 1, 2]
```

### `mean(v)`
Arithmetic mean. Returns a complex scalar for complex vectors.
```
mean([1.0, 2.0, 3.0])   # → 2.0
mean(randn(1000))        # → ≈ 0.0
```

### `median(v)`
Median value, computed on real parts. For even-length vectors returns the average of the two middle elements.
```
median([3.0, 1.0, 2.0])         # → 2.0
median([4.0, 1.0, 3.0, 2.0])   # → 2.5
```
- Scalar input returns the scalar unchanged.
- Complex vectors: imaginary parts are ignored; result is always a real scalar.

### `std(v)`
Sample standard deviation (Bessel-corrected, N−1 denominator).
```
std(randn(10000))   # → ≈ 1.0
```

### `sum(v)`
Sum of all elements. Accepts scalar, complex, vector, or matrix. Returns complex if any
imaginary part is non-negligible, otherwise scalar.
```
sum([1.0, 2.0, 3.0])   # → 6.0
sum(ones(4) * j)        # → 0+4i
```

### `cumsum(v)`
Cumulative sum of a vector. Returns a vector of the same length where each element is the
running total up to that index.
```
cumsum([1.0, 2.0, 3.0, 4.0])   # → [1, 3, 6, 10]
```

### `argmin(v)` / `argmin(M)` / `argmin(M, dim)`
1-based index of the minimum element. Comparison key matches `min`: real value for purely-real input, magnitude `|z|` for complex input. NaN entries are skipped; all-NaN input errors. First-occurrence tie-break.
```
argmin([3.0, 1.0, 4.0, 1.5])   # → 2
argmin([1, 5, 3; 4, 2, 6])    # → [1, 2, 1]   (per-column, 1×N row)
argmin(A, 2)                    # column index of each row's min, nrows×1
```
- For complex inputs, **diverges from MATLAB on equal magnitudes** (rustlab uses first-occurrence; MATLAB uses phase-angle tie-break).
- Always agrees with the index returned by `[m, i] = min(...)` on the same input.

### `argmax(v)` / `argmax(M)` / `argmax(M, dim)`
1-based index of the maximum element. Same rules as `argmin`.
```
argmax([3.0, 1.0, 4.0, 1.5])   # → 3
argmax([1, 5, 3; 4, 2, 6])    # → [2, 1, 2]   (per-column, 1×N row)
argmax(A, 2)                    # column index of each row's max, nrows×1
```
- Always agrees with the index returned by `[m, i] = max(...)` on the same input.

### `sort(v)`
Sort a vector ascending by real part. Imaginary components are preserved.
```
sort([3.0, 1.0, 2.0])         # → [1.0, 2.0, 3.0]
sort([3.0, -1.0, 0.5])        # → [-1.0, 0.5, 3.0]
```
- Returns a scalar unchanged.
- Useful for top-K sampling: sort logits descending, slice, apply softmax.

### `trapz(v)` / `trapz(x, v)`
Trapezoidal numerical integration. With one argument, assumes unit spacing between samples.
With two arguments, uses `x` as the sample positions.
```
trapz([0.0, 1.0, 2.0, 1.0, 0.0])            # → 4.0  (unit spacing)
trapz(linspace(0,1,5), [0,1,2,1,0] * 1.0)   # area under triangle
```
- Returns a scalar (real or complex).
- Returns `0.0` for vectors with fewer than 2 elements.

### `prod(v)`
Product of all elements. Accepts scalar, complex, vector, or matrix.
```
prod([1.0, 2.0, 3.0, 4.0])   # → 24.0
prod([2, 3, 5])               # → 30.0
```

### `all(v)`
Returns `true` if all elements are nonzero.
```
all([1, 2, 3])     # → true
all([1, 0, 3])     # → false
```
- Scalar: nonzero → true. Vector: all elements nonzero (real or imaginary part).

### `any(v)`
Returns `true` if any element is nonzero.
```
any([0, 0, 3])     # → true
any([0, 0, 0])     # → false
```

---

## ML / Activation Functions

### `softmax(v)` / `softmax(M)` / `softmax(M, dim)`
Numerically-stable softmax over the real parts of the input. Each output slice sums to 1.0. Subtracts the per-slice maximum before exponentiating to prevent overflow.

**Vector form:**
```
p = softmax([1.0, 2.0, 3.0, 4.0])    # → [0.032, 0.087, 0.237, 0.644]
sum(p)                                 # → 1.0
```

**Matrix form** — softmax each row (`dim=2`, default) or each column (`dim=1`) independently. Per-row default matches the ML/transformer convention where rows are tokens and columns are categories — intentionally diverges from `sum`/`mean`/`std` which default to `dim=1`, mirroring `layernorm`.
```
P = softmax([1, 2; 3, 4])             # per-row (default), each row sums to 1
P = softmax(S, 2)                     # explicit per-row
P = softmax(S, 1)                     # per-column, each column sums to 1
```
- Replaces the manual `for t = 1:T; A(t) = softmax(S(t, :)); end` attention idiom.
- 1-D-shaped matrices (1×N or N×1) are treated as vectors regardless of `dim`, matching `sum`/`mean`/`layernorm`.
- Single scalar input returns `1.0`.
- Monotone: larger input values produce larger output probabilities.

### `relu(x)`
Rectified linear unit: `max(0, x)`, element-wise.
```
relu(3.5)                              # → 3.5
relu(-2.0)                             # → 0.0
relu([-3.0, -1.0, 0.0, 2.0, 5.0])     # → [0, 0, 0, 2, 5]
relu(M)                                # element-wise over a matrix
```
- Accepts scalar, vector, or matrix.
- Clamps negative values to zero; positive values pass through unchanged.

### `gelu(x)`
Gaussian error linear unit, element-wise. Uses the standard tanh approximation:
`GELU(x) = 0.5 · x · (1 + tanh(√(2/π) · (x + 0.044715 · x³)))`
```
gelu(0.0)                              # → 0.0
gelu(1.0)                              # → ~0.841
gelu([-2.0, 0.0, 2.0])                # → [~-0.045, 0.0, ~1.955]
```
- Accepts scalar, vector, or matrix.
- Allows small negative outputs near `x ≈ -0.17` — unlike ReLU.
- Approaches identity for large positive `x`; approaches zero for large negative `x`.

### `layernorm(v)` / `layernorm(v, eps)` / `layernorm(M[, dim[, eps]])`
Layer normalisation: subtracts the mean and divides by the population standard deviation.
`y = (x − mean(x)) / sqrt(var(x) + eps)`
```
y = layernorm([1.0, 2.0, 3.0, 4.0, 5.0])   # zero mean, ~unit variance
layernorm(v, 1e-8)                           # custom epsilon
Y = layernorm(M)                             # per-row by default (ML convention)
Y = layernorm(M, 1)                          # per-column
Y = layernorm(M, 2, 1e-8)                    # per-row, custom eps
```
- `eps` defaults to `1e-5` and prevents division by zero for constant inputs.
- Uses **population variance** (divides by N, not N-1).
- Output has zero mean and variance ≈ 1.0 for each normalised slice.
- Matrix form normalises each row (`dim=2`, default — ML convention where rows are samples and columns are features) or each column (`dim=1`) independently.
- 1-D-shaped matrices (1×N or N×1) are treated as vectors regardless of `dim`.
- Single scalar input returns `0.0`.

> Note: `layernorm`'s per-row default deliberately diverges from `sum`/`mean`/`std` (which default to `dim=1`). The ML/transformer convention dominates here.

### `tanh(x)` (activation context)
Hyperbolic tangent used as a classic bounded activation function. See also `tanh` in the Math section.
```
tanh([-2.0, 0.0, 2.0])   # → [~-0.964, 0.0, ~0.964]
```
- Output range (−1, 1); zero-centered, unlike sigmoid.
- Used in RNNs, LSTMs, and side-by-side activation comparisons with ReLU/GELU.

---

## Array Construction

### `zeros(n)` / `zeros(m, n)` / `zeros([m, n])`
Returns a length-n complex zero vector, or an m×n zero matrix when two arguments (or a 2-element vector) are given. Accepts the output of `size()` directly.
```
zeros(4)         # → [0+0j, 0+0j, 0+0j, 0+0j]
zeros(2, 3)      # → 2×3 matrix of zeros
zeros(size(A))   # → zero matrix matching A's dimensions
```

### `ones(n)` / `ones(m, n)` / `ones([m, n])`
Returns a length-n complex ones vector, or an m×n matrix of ones when two arguments (or a 2-element vector) are given. Accepts the output of `size()` directly.
```
ones(3)          # → [1+0j, 1+0j, 1+0j]
ones(2, 3)       # → 2×3 matrix of ones
ones(size(A))    # → ones matrix matching A's dimensions
```

### `linspace(start, stop, n)`
`n` evenly spaced real values from `start` to `stop` (inclusive).
```
linspace(0.0, 1.0, 5)   # → [0.0, 0.25, 0.5, 0.75, 1.0]
```

For the degenerate single-element case, rustlab follows the Octave / MATLAB convention: `linspace(a, b, 1)` returns `[b]` (the endpoint), not `[a]`. The numpy convention is the opposite — beware when porting numpy code.

For `n = 0`, returns an empty vector.

### `len(v)` / `length(v)`
Number of elements in a vector, rows in a matrix, or characters in a string.

For scalars / complex / bool / `Tensor3`, `length` returns `1` / `1` / `1` / the longest axis — matching the MATLAB convention. This means generic helpers that handle "scalar OR vector" can call `length(x)` directly without boxing the input as `length([x])`. Use `numel(x)` for total element count and `size(x)` for full shape.

### `numel(x)`
Total number of elements: `rows × cols` for matrices, `1` for scalars.

### `size(x)`
Returns a 2-element vector `[rows, cols]`. Vectors return `[1, n]`.

### `ndims(x)`
Number of dimensions: `1` for scalars, `2` for vectors and matrices, `3` for `Tensor3` values.

### `logspace(a, b, n)`
`n` logarithmically spaced points from `10^a` to `10^b` (inclusive).
```
logspace(0, 3, 4)   # → [1, 10, 100, 1000]
logspace(-2, 2, 5)  # → [0.01, 0.1, 1, 10, 100]
```
Useful for frequency vectors in Bode plots and log-scale analysis.

---

## Random Numbers

### `rand()` / `rand(n)` / `rand(m, n)`
Samples drawn uniformly from `[0, 1)`.
- `rand()` — single scalar.
- `rand(n)` — length-n vector.
- `rand(m, n)` — m×n matrix.
```
u  = rand()         # one scalar in [0, 1)
v  = rand(512)      # length-512 noise vector
M  = rand(8, 8)
```

### `randn()` / `randn(n)` / `randn(m, n)`
Samples from the standard normal distribution (μ=0, σ=1).
- `randn()` — single scalar.
- `randn(n)` — length-n vector.
- `randn(m, n)` — m×n matrix.
```
z     = randn()                  # one scalar from N(0, 1)
noise = randn(1024) * 0.1        # length-1024 noise vector
W     = randn(64, 128)           # weight matrix for a linear layer
W     = randn(128, 64) * 0.02    # Xavier-style small-weight init
```
All values are real (zero imaginary part).

### `randi(imax)` / `randi(imax, n)` / `randi([lo, hi], n)`
Random integers.
```
randi(6)          # single integer in [1, 6]  — one die roll
randi(6, 100)     # 100 integers in [1, 6]
randi([0, 1], 8)  # 8 random bits
randi([-5, 5], 50)  # 50 integers in [-5, 5]
```

### `seed(N)` / `seed()`

Set the shared RNG seed for `rand`, `randn`, `randi`, `sprand`, etc.

`seed(N)` re-seeds with a deterministic 64-bit value, making subsequent random draws bit-stable across runs — useful for reproducible notebooks and tests. `seed()` (no argument) re-randomizes from system entropy, which is the default state at startup.

```
seed(42)              # all subsequent random draws are deterministic
x = randn(100)
seed()                # back to non-deterministic
```

The seed is process-global; calling it from one script affects every subsequent random call until the next `seed()`.

---

## FFT

### `fft(v)`
Forward FFT using the Cooley-Tukey radix-2 algorithm. Input is zero-padded to the next power of two if necessary.
```
X = fft(x)          # len(X) is next power of two >= len(x)
```

### `ifft(X)`
Inverse FFT. Input length must be a power of two (as returned by `fft`).
```
x_rec = real(ifft(X))   # round-trip reconstruction
```

### `fftshift(X)`
Rearranges FFT output so the DC component (bin 0) moves to the center. Negative frequencies appear on the left.
```
Xs = fftshift(X)   # [A B] → [B A]
```

### `fftfreq(n, sample_rate)`
Frequency bin values in Hz for an n-point FFT.
- Bins `0..n/2` → positive frequencies `0` to `sr/2 − sr/n`
- Bins `n/2..n` → negative frequencies `−sr/2` to `−sr/n`
```
freqs = fftfreq(256, 8000.0)   # 256-point FFT at 8 kHz
```

### `spectrum(X, sample_rate)`
The recommended way to display FFT results with a correct Hz axis.

Applies `fftshift` to the spectrum and pairs it with the DC-centered frequency axis, returning a **2×n matrix** that plugs directly into `plotdb`:
- Row 1: frequency axis in Hz (DC = 0, negative on left, positive on right)
- Row 2: complex spectrum (DC centered)

```
X = fft(x)
H = spectrum(X, sr)
plotdb(H, "Magnitude Spectrum")
savefig("spectrum.svg")
```

This is the standard workflow for viewing FFT output with a proper frequency axis. Internally it is equivalent to:
```
# What spectrum() does for you:
Xs    = fftshift(X)
freqs = fftshift(fftfreq(len(X), sr))
# (pairs them into a matrix for plotdb)
```

---

## DSP — FIR Filters (manual tap count)

All FIR design functions return a complex coefficient vector.

### `fir_lowpass(taps, cutoff_hz, sample_rate, window)`
Windowed-sinc lowpass filter.
```
h = fir_lowpass(64, 1000.0, 44100.0, "hann")
```

### `fir_highpass(taps, cutoff_hz, sample_rate, window)`
Windowed-sinc highpass filter (spectral inversion of lowpass).
```
h = fir_highpass(64, 3000.0, 44100.0, "hamming")
```

### `fir_bandpass(taps, low_hz, high_hz, sample_rate, window)`
Windowed-sinc bandpass filter (difference of two lowpass filters).
```
h = fir_bandpass(128, 500.0, 2000.0, 44100.0, "blackman")
```

**Window names:** `"rectangular"`, `"hann"`, `"hamming"`, `"blackman"`, `"kaiser"`

Approximate stopband attenuation by window:

| Window | Stopband attenuation |
|--------|----------------------|
| Rectangular | ~21 dB |
| Hann | ~44 dB |
| Hamming | ~41 dB |
| Blackman | ~74 dB |
| Kaiser (auto β) | user-specified |

### `convolve(x, h)`
Linear convolution. Output length = `len(x) + len(h) − 1`.
```
y = convolve(signal, h)
```

### `upfirdn(x, h, p, q)`
Upsample by `p`, apply FIR filter `h`, then downsample by `q` — all in one pass using
a polyphase decomposition. The filter is split into `p` subfilters so each output sample
costs only `⌈len(h)/p⌉` multiply-adds instead of `len(h)`.

**Signature:** `upfirdn(x, h, p, q)`

| Argument | Type | Description |
|---|---|---|
| `x` | vector | Input signal (complex or real) |
| `h` | vector | Real FIR filter coefficients |
| `p` | scalar | Upsample factor (≥ 1) |
| `q` | scalar | Downsample factor (≥ 1) |

**Output length:** `floor(((len(x) − 1)·p + len(h) − 1) / q) + 1`

| `p` | `q` | Use case | Filter cutoff |
|-----|-----|----------|---------------|
| 1   | 1   | FIR filtering (equivalent to `convolve`) | any |
| >1  | 1   | Interpolation — increase sample rate by `p` | `sr / (2·p)` |
| 1   | >1  | Decimation — reduce sample rate by `q` | `sr / (2·q)` |
| >1  | >1  | Rational rate conversion `p/q` | `sr / (2·max(p,q))` |

**Interpolation by 4:**
```
sr = 44100.0
h  = fir_lowpass(128, sr / 8.0, sr, "hann")   # cutoff at sr/2/4
y  = upfirdn(x, h, 4, 1)
# len(y) = (len(x)-1)*4 + 128
```

**Decimation by 3:**
```
sr = 48000.0
h  = fir_lowpass(128, sr / 6.0, sr, "hann")   # cutoff at sr/2/3
y  = upfirdn(x, h, 1, 3)
# len(y) ≈ len(x) / 3
```

**Rational sample-rate conversion 3/2:**
```
sr     = 44100.0
cutoff = sr / 2.0 / 3.0                        # governed by the larger factor
h      = fir_lowpass(128, cutoff, sr, "hann")
y      = upfirdn(x, h, 3, 2)
# len(y) ≈ len(x) * 3/2
```

See `examples/upfirdn.rlab` for a runnable demonstration of all three cases.

### `window(name, n)`
Generate a standalone window function vector of length `n`.
```
w = window("hann", 64)
```

---

## DSP — Kaiser FIR (automatic tap count)

Kaiser filters automatically compute the window shape parameter β and the required tap count from the desired stopband attenuation and transition bandwidth — no manual tap count needed.

### `fir_lowpass_kaiser(cutoff_hz, trans_bw_hz, stopband_attn_db, sample_rate)`
```
h = fir_lowpass_kaiser(1000.0, 200.0, 60.0, 8000.0)
```
For 60 dB attenuation and 200 Hz transition width at 8 kHz: β ≈ 5.65, ~185 taps.

### `fir_highpass_kaiser(cutoff_hz, trans_bw_hz, stopband_attn_db, sample_rate)`
```
h = fir_highpass_kaiser(3000.0, 200.0, 60.0, 8000.0)
```

### `fir_bandpass_kaiser(low_hz, high_hz, trans_bw_hz, stopband_attn_db, sample_rate)`
```
h = fir_bandpass_kaiser(1000.0, 2500.0, 200.0, 60.0, 8000.0)
```

### `fir_notch(center_hz, bandwidth_hz, sample_rate, num_taps, window)`
Notch filter via spectral inversion of a bandpass. Rejects a narrow band around `center_hz`.
```
h = fir_notch(1000.0, 200.0, 8000.0, 65, "hann")
```

**Kaiser design guidelines:**

| Attenuation | β | Typical use |
|-------------|---|-------------|
| 40 dB | 3.40 | General audio |
| 60 dB | 5.65 | Most signal processing |
| 80 dB | 7.86 | High-fidelity |
| 100 dB | 10.06 | Demanding applications |

### `freqz(h, n_points, sample_rate)`
Complex frequency response of a filter at `n_points` frequencies from 0 to Nyquist.
Returns a **2×n matrix**:
- Row 1: frequency axis in Hz
- Row 2: complex H(f)

```
Hz = freqz(h, 512, 44100.0)
plotdb(Hz, "Frequency Response")
savefig("response.svg")
```

---

## Fixed-Point Quantization

Fixed-point simulation for FPGA/ASIC bitwidth studies. Operations compute at full float precision internally, then quantize the output to the specified Q format — matching real hardware behaviour exactly.

### `qfmt(word_bits, frac_bits [, round_mode [, overflow_mode]])`

Creates a Q-format specification. All quantization and arithmetic functions accept a `qfmt` spec as their format argument.

| Parameter | Values | Default |
|-----------|--------|---------|
| `word_bits` | 2–32 | required |
| `frac_bits` | 0 to word_bits−1 | required |
| `round_mode` | `"floor"` `"ceil"` `"zero"` `"round"` `"round_even"` | `"floor"` |
| `overflow_mode` | `"saturate"` `"wrap"` | `"saturate"` |

`"floor"` (truncate toward −∞) is the hardware default — it is free in RTL (just drop the LSBs). `"round_even"` (convergent/banker's) minimises bias in long filter chains.

```
fmt = qfmt(16, 15)                            # Q0.15, floor, saturate
fmt = qfmt(16, 15, "round_even", "saturate")  # same with convergent rounding
fmt = qfmt(8,  7,  "floor",      "wrap")      # 8-bit, wrap on overflow
```

In the REPL, a `qfmt` value displays its full spec:
```
QFmt<16-bit Q0.15, round=round_even, overflow=saturate>
```

### `quantize(x, fmt)`

Snap every element to the nearest representable value in `fmt`. Works on scalars, complex, vectors, and matrices. Real and imaginary parts are quantized independently. Returns the same type as the input — compatible with all existing math, FFT, plot, and save functions.

```
fmt = qfmt(16, 15, "round_even", "saturate")
xq  = quantize(x, fmt)
hq  = quantize(h, fmt)
noise = x - real(xq)    # quantization noise vector
```

### `qadd(a, b, fmt)`

Element-wise add, result quantized to `fmt`. Both inputs must be real scalars or real vectors of equal length.

```
y = qadd(xq, dc_offset, fmt)
```

### `qmul(a, b, fmt)`

Element-wise multiply, result quantized to `fmt`. The full Q-product is computed internally (no intermediate truncation).

```
scaled = qmul(xq, gain, fmt)
```

### `qconv(x, h, fmt)`

Fixed-point FIR convolution. Accumulates products at full precision (equivalent to a wide hardware accumulator), then quantizes each output sample to `fmt`. Output length = `len(x) + len(h) − 1`.

```
y = qconv(xq, hq, fmt_out)
```

### `snr(x_ref, x_quantized)`

Signal-to-noise ratio in dB between a float reference and a quantized signal. Both must be real vectors of equal length.

```
SNR = 10 · log₁₀(signal_power / noise_power)
```

Returns `+Inf` when signals are identical, `-Inf` when the reference is all-zeros.

```
db = snr(y_ref, y_quantized)
```

### Bitwidth study example

```
h = firpm(63, [0.0, 0.20, 0.30, 1.0], [1.0, 1.0, 0.0, 0.0])
# Scale randn to stay inside the Q1.14 range (±2); unscaled N(0,1) saturates
# ~5 % of samples, which swamps the coefficient-quantization noise floor.
x = randn(1024) * 0.3
y_ref = real(convolve(x, real(h)))

fmt_data = qfmt(16, 14, "round_even", "saturate")
xq = quantize(x, fmt_data)

fmt8  = qfmt(8,  7,  "round_even", "saturate")
fmt16 = qfmt(16, 15, "round_even", "saturate")

y8  = qconv(xq, real(quantize(h, fmt8)),  fmt_data)
y16 = qconv(xq, real(quantize(h, fmt16)), fmt_data)

print(snr(y_ref, y8))   # ~30 dB  (8-bit coeff)
print(snr(y_ref, y16))  # ~74 dB  (16-bit coeff)
```

---

## DSP — Parks-McClellan optimal FIR

`firpm` designs optimal equiripple FIR filters using the Remez exchange algorithm (). It minimises the maximum weighted error across all specified bands simultaneously, producing the minimum-ripple design for a given tap count.

### `firpm(n_taps, bands, desired)`
### `firpm(n_taps, bands, desired, weights)`

| Parameter | Type | Description |
|-----------|------|-------------|
| `n_taps` | integer | Number of filter taps (forced odd — Type I symmetric) |
| `bands` | vector | Frequency band edges, normalized to [0, 1] where 1 = Nyquist |
| `desired` | vector | Target amplitude at each band edge (piecewise-linear, same length as `bands`) |
| `weights` | vector | Optional — one weight per band pair (default: all 1.0) |

Band edges come in pairs: `[f_low1, f_high1, f_low2, f_high2, ...]`. The gaps between pairs are transition bands (don't-care regions).

**Low-pass (0 to 0.20 Nyquist pass, 0.30 Nyquist+ stop):**
```
h = firpm(63, [0.0, 0.20, 0.30, 1.0], [1.0, 1.0, 0.0, 0.0])
```

**Band-pass (pass 0.30 to 0.50 Nyquist):**
```
h = firpm(79, [0.0, 0.25, 0.30, 0.50, 0.55, 1.0],
              [0.0, 0.0,  1.0,  1.0,  0.0,  0.0])
```

**Weighted — enforce 10x tighter stopband than passband:**
```
h = firpm(51, [0.0, 0.25, 0.35, 1.0],
              [1.0, 1.0,  0.0,  0.0],
              [1.0, 10.0])
```

**Compared to Kaiser:**
- Kaiser automatically determines tap count from attenuation and transition width.
- `firpm` gives the optimal (fewest-ripple) filter for a fixed tap count, often requiring fewer taps than Kaiser for the same spec.

### `firpmq(n_taps, bands, desired [, weights [, bits [, n_iter]]])`

Integer-coefficient Parks-McClellan. Designs an optimal equiripple FIR like `firpm`, then iteratively requantizes the coefficients to `bits`-bit integers (default 16) over `n_iter` rounds (default 8).

```
h = firpmq(63, [0.0, 0.20, 0.30, 1.0], [1.0, 1.0, 0.0, 0.0])
h = firpmq(63, [0.0, 0.20, 0.30, 1.0], [1.0, 1.0, 0.0, 0.0], [1, 10], 12, 16)
```

- Returns integer taps (stored as complex with zero imaginary part).
- For unit-gain passband in frequency response, normalize: `freqz(h / sum(h), ...)`.
- Useful for FPGA/ASIC implementations where coefficients must fit in fixed-width registers.

---

## DSP — IIR Filters

### `butterworth_lowpass(order, cutoff_hz, sample_rate)`
Butterworth IIR lowpass filter. Higher order gives a steeper rolloff.
```
h = butterworth_lowpass(4, 1000.0, 44100.0)
y = convolve(x, h)
```

### `butterworth_highpass(order, cutoff_hz, sample_rate)`
Butterworth IIR highpass filter.
```
h = butterworth_highpass(4, 3000.0, 44100.0)
```

> **Note:** `butterworth_lowpass` and `butterworth_highpass` return only the numerator (`b`) coefficients as a vector. For FIR-style filtering, use `convolve(x, h)`. For zero-phase IIR filtering with `filtfilt`, you need both `b` and `a` coefficient vectors.

### `filtfilt(b, a, x)`
Zero-phase forward-backward IIR filter. Applies the filter defined by `b` (numerator) and `a` (denominator) coefficients forward and then backward, eliminating phase distortion.
```
# FIR zero-phase filtering (a = [1])
h = fir_lowpass(64, 1000.0, 44100.0, "hann")
y = filtfilt(h, [1], x)

# IIR zero-phase filtering (requires both b and a)
y = filtfilt(b, a, x)
```
- `b` and `a` must be non-empty real vectors.
- `x` is the input signal (real parts used).
- Use `a = [1]` for FIR filters (equivalent to zero-phase convolution).
- The output has the same length as `x` with no group delay.

---

## Linear Algebra

### `trace(M)`
Sum of the main diagonal elements of a square (or rectangular) matrix `M`.
```
M = [1,2;3,4]
trace(M)    # → 5.0  (1 + 4)

A = [1+j,0;0,2]
trace(A)    # → complex 3+1i
```
- Works on non-square matrices: sums `min(rows, cols)` diagonal elements.
- Returns a scalar if the imaginary part is negligible, otherwise complex.
- `trace(scalar)` returns the scalar unchanged.

### `det(M)`
Determinant of a square matrix `M`, computed via LU decomposition with partial pivoting.
```
M = [1,2;3,4]
det(M)      # → -2.0

I = eye(3)
det(I)      # → 1.0
```
- `M` must be square; non-square input is a type error.
- `det([])` (0×0) returns `1.0` by convention.
- Returns a scalar if the imaginary part is negligible, otherwise complex.
- `det(scalar)` returns the scalar unchanged.

### `outer(a, b)`
Outer (tensor) product of two vectors, returning an N×M matrix where `result[i,j] = a[i] * b[j]`.
```
outer([1,2,3], [10,20])   # → 3×2 matrix [[10,20],[20,40],[30,60]]
```
- Both arguments are coerced to vectors (scalars and column matrices accepted).
- Supports complex values.

### `kron(A, B)`
Kronecker tensor product of two matrices. For A (m×n) and B (p×q) returns an mp×nq matrix
where block `(i,j)` equals `A[i,j] * B`.
```
kron(eye(2), [1,2;3,4])   # → block-diagonal 4×4 matrix
```
- Accepts matrix, vector, or scalar for both arguments.
- Essential for multi-qubit state space construction.

### `expm(M)`
Matrix exponential e^M via scaling-and-squaring with a [6/6] Padé approximant (Higham 2008).
```
H = [0, -j; j, 0]        # Pauli-Y (up to factor)
expm(-j * H * pi/2)       # time-evolution operator
expm(zeros(3,3))          # → eye(3)
```
- `M` must be square.
- For diagonal or real-symmetric matrices the result is exact to double precision.
- `expm(scalar)` returns `exp(scalar)`.

### `eig(A)` / `[V, D] = eig(A)` / `eig(A, B)` / output-form flag

Dense eigendecomposition (`A` square; `B` square, same size, invertible).
Hand-rolled Hessenberg reduction + shifted QR (Wilkinson) for the eigenvalues;
shifted inverse iteration on `A` (or `inv(B)·A` for the generalized form) for
each eigenvector.

```
v = eig([2,1;1,2])            # N×1 column of eigenvalues, ~[3; 1]
[V, D] = eig([2,1;1,2])       # V eigenvector matrix; D diagonal matrix (matlab default)
e = eig(A, B)                 # generalized: A·v = λ·B·v
[V, D] = eig(A, B)            # generalized eigenvectors and eigenvalues
```

**Output-form flag** (matlab convention) — overrides the default D shape:

```
eig(A, "vector")              # D as N×1 column vector
eig(A, "matrix")              # D as N×N diagonal matrix
[V, D] = eig(A, "vector")     # D vector even with two outputs
[V, D] = eig(A, B, "matrix")  # generalized + explicit diagonal
```

- 1-output default is the column vector; 2-output default is the diagonal matrix.
- The flag is parsed off the tail of the argument list, so it composes with
  both the standard and generalized forms.
- Eigenvalues are returned in convergence order, not sorted.
- Defective matrices may produce an ill-conditioned `V`; the eigenvalues
  remain accurate.
- Generalized form requires `B` invertible. SPD-aware Cholesky reduction is
  a future optimisation; QZ for non-invertible `B` is deferred.

### `eigs(A, n)` / `eigs(A, n, which)` / `eigs(A, B, n)` / `eigs(A, B, n, which)`

Sparse partial eigensolver. Returns a tuple `[V, D]` where `V` is a dense `n_rows × n` matrix of eigenvectors (column `k` is the eigenvector for `D(k)`) and `D` is a length-`n` vector of eigenvalues.

- `which` is `"sm"` (smallest magnitude, default) or `"lm"` (largest magnitude).
- `eigs(A, n[, which])` solves the standard problem `A x = λ x`.
- `eigs(A, B, n[, which])` solves the generalized problem `A x = λ B x` with `B` Hermitian positive-definite.
- `A` (and `B`) must be **sparse** — call `sparse(A)` first if you have a dense matrix. Use `eig` for dense problems.

Auto-routing:
- Hermitian / SPD `A` → hand-rolled symmetric Lanczos with full reorthogonalization.
- General `A` → hand-rolled Arnoldi.
- Generalized `eigs(A, B, n)` reduces to a standard problem via the existing `SparseChol` factor of `B` and routes through Arnoldi.

```
% Smallest 4 eigenpairs of a 100-grid Laplacian.
nx = 10; ny = 10;
L = -1 * laplacian_2d(nx, ny);
[V, D] = eigs(L, 4, "sm");

% Largest 2 eigenvalues.
[V, D] = eigs(L, 2, "lm");

% Generalized: A x = λ B x  (B SPD).
[V, D] = eigs(A, B, 6, "sm");
```

**Convergence and limits.** The default Krylov dimension is `min(n_rows, max(6n+10, 40))`. For matrices with closely-spaced eigenvalues (clusters), Lanczos may need more iterations to resolve them. **Implicit restart** and **shift-invert** are not yet implemented; if convergence is poor on a particular system, the next steps in `dev/plans/em_requests_queue.md` cover those enhancements.

**Hand-rolled algorithm references:**
- Symmetric Lanczos with full reorthogonalization — Saad, *Numerical Methods for Large Eigenvalue Problems*, ch. 6; Golub & Van Loan §10.1.
- Arnoldi for general matrices — Saad ch. 8; Golub & Van Loan §10.5.
- Small dense symmetric eigenproblem at the centre of Lanczos via cyclic Jacobi rotations.
- Small dense Hessenberg eigenproblem via shifted QR + inverse iteration (eigenvectors).

### `laguerre(n, alpha, x)`
Associated Laguerre polynomial L_n^α(x) computed via 3-term recurrence.
```
laguerre(0, 0, x)    # → 1  (for any x)
laguerre(1, 0, 0)    # → 1
laguerre(2, 1, 1.0)  # → L_2^1(1) = 0.5
```
- `n` must be a non-negative integer scalar.
- `alpha` is a real scalar (often an integer in physics, e.g. `2*l+1` for radial wavefunctions).
- `x` may be scalar, vector, or matrix (element-wise).
- For hydrogen radial wavefunctions use `laguerre(n-l-1, 2*l+1, rho)`.

### `legendre(l, m, x)`
Associated Legendre polynomial P_l^m(x), Condon-Shortley phase convention.
```
legendre(1, 0, 0.5)  # → P_1^0(0.5) = 0.5
legendre(2, 0, 0.0)  # → P_2^0(0) = -0.5
legendre(1, 1, 0.0)  # → P_1^1(0) = -1.0  (Condon-Shortley)
```
- `l`, `m` must be integer scalars with `0 <= m <= l` (use negative `m` for m < 0 via symmetry).
- `x` may be scalar, vector, or matrix (element-wise); typically `|x| <= 1` (cosine of colatitude).
- For spherical harmonics: Y_l^m(θ,φ) = N · P_l^m(cosθ) · e^{imφ}.

### `factor(n)`
Prime factorization of a positive integer `n`. Returns a real vector of prime factors
in ascending order, with repetition.
```
factor(12)    # → [2, 2, 3]
factor(17)    # → [17]
factor(1)     # → [] (empty vector)
factor(360)   # → [2, 2, 2, 3, 3, 5]
```
- `n` must be a positive integer scalar.
- `factor(0)` and `factor(-3)` produce a type error.

---

## Matrix

### `eye(n)`
Returns an n×n identity matrix.
```
eye(3)   # → 3×3 identity
```

### `reshape(A, m, n)` / `reshape(A, m, n, p)`
Reshape a vector, matrix, or rank-3 tensor using column-major order (standard for matrix languages).
```
reshape([1,2,3,4,5,6], 2, 3)   # → 2×3 matrix, columns filled first
reshape(M, 1, numel(M))         # flatten any matrix to a row vector
reshape(v, len(v), 1)           # column vector → n×1 matrix
reshape(1:24, 2, 3, 4)          # → 2×3×4 Tensor3 (column-major walk)
```
- Total elements must be preserved: `numel(A)` must equal `m * n` (or `m * n * p`).
- If `m == 1` or `n == 1` (and no `p`), returns a vector instead of a matrix.
- The 4-argument form returns a `Tensor3`. See [Rank-3 Tensors](#rank-3-tensors).

### `repmat(A, m, n)`
Tile matrix `A` m times vertically and n times horizontally.
```
repmat([1,2;3,4], 2, 3)   # → 4×6 tiled matrix
repmat(eye(2), 1, 4)       # → 2×8 block-identity
```

### `transpose(A)` / `A.'`
Non-conjugate transpose — swaps rows and columns without conjugating imaginary parts.
```
transpose([1+j, 2; 3, 4-j])   # same as writing A.'
```
- Use `conj(transpose(A))` or `A'` notation for Hermitian (conjugate) transpose.

### `diag(v)` / `diag(M)`
- `diag(v)` — creates an n×n diagonal matrix from vector `v`.
- `diag(M)` — extracts the main diagonal of matrix `M` as a vector.
```
diag([1, 2, 3])         # → 3×3 diagonal matrix
diag([1,2;3,4])         # → [1, 4]
```

### `horzcat(A, B, ...)` / `[A B]`
Concatenate matrices (or vectors) side by side. All inputs must have the same number of rows.
```
horzcat(eye(2), ones(2,3))   # → 2×5 matrix
```

### `vertcat(A, B, ...)` / `[A; B]`
Stack matrices vertically. All inputs must have the same number of columns.
```
vertcat(eye(2), zeros(3,2))  # → 5×2 matrix
```

### `rank(M)`
Numerical rank of a matrix (number of singular values above a tolerance threshold).
```
rank(eye(4))          # → 4
rank([1,2;2,4])       # → 1  (linearly dependent rows)
```

### `dot(u, v)`
Inner (dot) product of two vectors. Both must have the same length. Returns a scalar (or complex).
```
dot([1, 2, 3], [4, 5, 6])   # → 32.0
```

### `cross(u, v)`
3-element cross product. Both vectors must have length 3.
```
cross([1, 0, 0], [0, 1, 0])   # → [0, 0, 1]
```

### `norm(v)` / `norm(v, p)`
Vector p-norm (default p=2). For matrices, Frobenius norm.
```
norm([3, 4])         # → 5.0  (L2)
norm([3, 4], 1)      # → 7.0  (L1)
norm([3, 4], Inf)    # → 4.0  (max abs)
```

### `inv(M)`
Matrix inverse via LU decomposition with partial pivoting.
```
A = [1, 2; 3, 4]
B = inv(A)
A * B   # ≈ eye(2)
```
- `M` must be square and non-singular.
- `inv(scalar)` returns `1/scalar`.

### `linsolve(A, b)`
Solve the linear system `A·x = b` for `x`. `A` must be square and non-singular.
```
A = [2, 1; 1, 3]
b = [5, 10]
x = linsolve(A, b)   # → [1, 3]
```

### `roots(p)`
Roots of a polynomial with coefficient vector `p`. Coefficients are in descending order of power (highest degree first).
```
roots([1, -3, 2])     # → [2, 1]  (x² - 3x + 2 = 0)
roots([1, 0, -1])     # → [1, -1] (x² - 1 = 0)
```
- Returns a complex vector. Uses companion matrix eigendecomposition.

### `svd(A)`
Singular value decomposition via Jacobi eigendecomposition of A'A. Returns a tuple `[U, sigma, V]`.
```
[U, S, V] = svd(A)
# U: m×m unitary, S: min(m,n)-length singular value vector, V: n×n unitary
# A ≈ U * diag(S) * V'
```
- Currently operates on real parts only; a warning is printed if imaginary parts are discarded.

---

## Indexed Assignment

Indexed writes mirror the read forms — anywhere `M(rows, cols)` reads a region, `M(rows, cols) = ...` writes it. Indices may be scalars, `:` (all), or vector index sets (including colon ranges like `1:2:6`).

### Element write: `M(i, j) = scalar`
Single-element store into a Matrix (or 2-D SparseMatrix). 1-based indexing.
```
M = zeros(3, 3)
M(2, 3) = 7      % only element (2, 3) updated
```

### Row write: `M(i, :) = vec` (preferred) or `M(i) = vec` (legacy)
Assign a length-`ncols` Vector into row `i`. The two-argument form is symmetric with the row-read `M(i, :)`. The single-argument form is the legacy row-write that still works for back-compat.
```
A = zeros(3, 3)
A(2, :) = [10, 20, 30]   % row 2 becomes [10, 20, 30]
```

### Column write: `M(:, j) = vec`
Assign a length-`nrows` Vector into column `j`.
```
B = zeros(3, 3)
B(:, 1) = [7, 8, 9]
```

### Submatrix region write: `M(rows, cols) = matrix`
Assign a Matrix of shape `(len(rows), len(cols))` into the cross-product region.
```
C = zeros(3, 3)
C(1:2, 2:3) = [1, 2; 3, 4]
```

### Scalar broadcast: `M(rows, cols) = scalar`
A scalar (or complex) RHS broadcasts to every position in the target region. `M(:, :) = 5` fills the matrix; `M(2, :) = 0` zeroes a row.
```
D = zeros(2, 3)
D(:, :) = 5
```

### Vector strided / indexed write: `v(idx_set) = vec`
For a Vector LHS, any non-scalar index set — `:`, an explicit list, or any colon range — selects positions to write to. The RHS Vector length must match the index count. A scalar (or complex) RHS broadcasts.
```
v = zeros(6)
v(1:2:6) = [10, 20, 30]    % strided positions 1, 3, 5
v([2, 4, 6]) = [-1, -2, -3] % explicit positions
v(:) = 9                    % scalar broadcast across the whole vector
```

### Auto-create and grow
Single-index assignment auto-creates a Vector or grows the existing one to fit the index (filling new positions with zero). This applies only to the scalar-index forms; region writes require the target container to already exist.
```
v(3) = 7        % creates v = [0, 0, 7]
v(6) = 99       % grows v = [0, 0, 7, 0, 0, 99]
```

### Errors
- Shape mismatch (`A(2, :) = [10, 20]` when `A` has 3 columns) hard-errors with both shapes named.
- Index out of bounds errors with the index and container size.
- Sparse matrices: only the single-element form `S(i, j) = scalar` is supported for region writes.

---

## Rank-3 Tensors

A `Tensor3` is a complex 3-dimensional array of shape `(m, n, p)` — `m` rows, `n` columns, `p` pages. Elements use 1-based indexing `A(i, j, k)`. Slicing with `A(:, :, k)` extracts page `k` as a regular Matrix (the trailing singleton is dropped).

**Conventions and limitations:**
- All Tensor3 storage is complex (`C64`); real values are stored with imaginary part 0.
- No broadcasting between `Matrix` and `Tensor3` — operations between them error.
- `*` and `/` between two `Tensor3`s also error; use `.*` and `./` for element-wise.
- `reshape` walks data column-major (matches Octave), so `reshape(1:24, 2, 3, 4)` fills the first column of page 1 first.
- I/O via `save`/`load` to `.npy` preserves the rank-3 shape natively.

### `zeros3(m, n, p)` / `zeros3([m, n, p])`
Create an m×n×p tensor of complex zeros. The bracket form accepts the output of `size()`.
```
A = zeros3(2, 3, 4)
size(A)            # → [2, 3, 4]
ndims(A)           # → 3
numel(A)           # → 24
```

### `ones3(m, n, p)`
Create an m×n×p tensor of complex ones.

### `rand3(m, n, p)`
Create an m×n×p tensor with samples drawn uniformly from `[0, 1)`.

### `randn3(m, n, p)`
Create an m×n×p tensor with samples drawn from the standard normal `N(0, 1)`.

### Indexing and assignment
```
T = reshape(1:24, 2, 3, 4)

# Single element — 1-based on every axis
T(1, 1, 1)            # → 1
T(2, 3, 4)            # → 24

# Page slice — trailing singleton dropped, returns Matrix(2, 3)
page2 = T(:, :, 2)

# Slabs along the page axis
row1 = T(1, :, :)     # Matrix(3, 4)
col2 = T(:, 2, :)     # Matrix(2, 4)

# Range slice keeps rank-3 if the result has more than one non-singleton dim
chunk = T(:, :, 1:2)  # Tensor3(2, 3, 2)

# Assignment mirrors indexing
U = zeros3(2, 2, 3)
U(:, :, 2) = [1, 2; 3, 4]    # page write
U(1, 1, 1) = 99               # element write
```

### Arithmetic
```
E = T * 2                    # scalar broadcast
F = T + 10                   # scalar broadcast
G = T .^ 2                   # element-wise

H = ones3(size(T)) + T        # element-wise (same shape)
J = ones3(size(T)) .* T       # element-wise multiply
```
- `T1 * T2` errors — use `.*` for element-wise.
- `Matrix + Tensor3` errors — there is no broadcasting between the two ranks.

### `permute(A, [d1, d2, d3])`
Reorder the axes of a Tensor3. `[d1, d2, d3]` must be a permutation of `[1, 2, 3]`.
```
T = reshape(1:24, 2, 3, 4)
P = permute(T, [2, 1, 3])    # swap rows ↔ cols
size(P)                       # → [3, 2, 4]
```

### `squeeze(A)`
Drop singleton dimensions from a Tensor3. The result's rank depends on how many singletons were removed:
```
squeeze(reshape(1:6, 2, 3, 1))    # → Matrix(2, 3)
squeeze(reshape(1:6, 1, 2, 3))    # → Matrix(2, 3)
squeeze(reshape(1:5, 1, 1, 5))    # → Vector(5)
squeeze(reshape([1], 1, 1, 1))    # → Scalar
```
Non-Tensor3 inputs pass through unchanged.

### `cat(3, A, B, ...)`
Concatenate matrices along the page axis to build a Tensor3 (or grow an existing one). `cat(1, ...)` and `cat(2, ...)` exist for vertical/horizontal matrix concatenation; only `dim == 3` produces a Tensor3.
```
M1 = [1, 2; 3, 4]
M2 = [5, 6; 7, 8]
stacked = cat(3, M1, M2)          # Tensor3(2, 2, 2)
stacked(:, :, 1)                   # → [1, 2; 3, 4]
stacked(:, :, 2)                   # → [5, 6; 7, 8]

# Append another page to an existing Tensor3
more = cat(3, stacked, [9, 10; 11, 12])
size(more)                          # → [2, 2, 3]
```

### `size(A)` / `size(A, dim)` / `ndims(A)` / `numel(A)`
- `size(A)` returns a 3-element vector for Tensor3, `[rows, cols]` for Matrix.
- `size(A, 3)` is valid only for Tensor3.
- `ndims(A)` returns `3` for Tensor3, `2` otherwise (Octave convention — no `ndims == 1`).
- `numel(A)` returns `m * n * p` for Tensor3.

### I/O
```
save("/tmp/T.npy", T)              # NPY preserves the rank-3 shape
T2 = load("/tmp/T.npy")
ndims(T2)                           # → 3
```

See `examples/tensor3/tensor3.rlab` for a full runnable demo.

---

## Sparse Vectors & Matrices

Sparse storage keeps only non-zero entries, enabling O(nnz) operations on matrices that would be infeasible in dense form. All indices are 1-based. Sparse matrices participate transparently in arithmetic — mixed sparse+dense pairs auto-promote to dense.

### `sparse(I, J, V, m, n)` / `sparse(A)`
Build a sparse matrix. With four or five arguments, construct an m×n matrix from 1-based row indices `I`, column indices `J`, and values `V` (COO triples). With one matrix/vector argument, convert a dense input to sparse (near-zero entries are dropped).
```
S = sparse([1, 2, 3], [1, 2, 3], [10, 20, 30], 3, 3)   # diagonal sparse
S = sparse(A)                                            # dense → sparse
```

### `sparsevec(I, V, n)`
Build a sparse vector of length `n` from 1-based indices and corresponding values.
```
sv = sparsevec([1, 5, 10], [1.0, 2.0, 3.0], 10)
```

### `speye(n)`
n×n sparse identity matrix.
```
speye(4)    # → 4×4 sparse identity (4 stored non-zeros)
```

### `spzeros(m, n)`
m×n all-zero sparse matrix (zero stored entries).
```
Z = spzeros(1000, 1000)   # no memory for entries
```

### `spdiags(V, D, m, n)`
Build a sparse matrix from diagonal vectors. Each column of `V` is placed on diagonal `D[k]` of the result: `D=0` is the main diagonal, `D>0` is super-diagonal, `D<0` is sub-diagonal.
```
V = [1,1,1; 2,2,2; 3,3,3]
D = [-1, 0, 1]
S = spdiags(V, D, 3, 3)    # tridiagonal
```

### `sprand(m, n, density)`
Random sparse matrix with approximately `density × m × n` non-zero entries. Values are drawn from `[0, 1)`.
```
S = sprand(100, 100, 0.01)   # ~100 non-zeros in a 100×100 sparse matrix
```

### `spsolve(A, b)` / `spsolve(A, b, mode)` / `spsolve(A, b, mode, ordering)`

Solve the linear system `A·x = b` where `A` is sparse. The optional `mode` is `"auto"` (default), `"cholesky"`, or `"lu"`. The optional `ordering` is `"auto"` (default), `"identity"` (alias `"natural"`), or `"amd"`.

- **`"auto"`** — detect Hermitian-positive-definite structure (`SparseMat::is_spd_estimate`). If SPD, factor with the hand-rolled sparse Cholesky. Otherwise factor with the hand-rolled sparse LU with partial pivoting. Either path stays sparse end-to-end.
- **`"cholesky"`** — force the sparse Cholesky path. Returns an error if `A` is not Hermitian positive definite.
- **`"lu"`** — force the sparse LU path. Useful when you know `A` is not Hermitian and want to skip the SPD pre-check.

**Ordering.** The optional fourth argument selects the fill-reducing column permutation:
- **`"auto"`** (default) — reads the matrix's `ordering_hint`. The `laplacian_1d`, `laplacian_2d`, `laplacian_3d`, and `laplacian_eps_2d` builders set the hint to `Identity` because natural ordering matches the banded fill pattern of those stencils. Auto falls back to AMD when no hint is set.
- **`"identity"`** (alias `"natural"`) — natural / identity ordering. Roughly 5× faster than AMD on grid-natural Laplacians. Wrong choice for matrices with irregular sparsity — the lack of fill-reducing reordering will blow up the factor's nnz.
- **`"amd"`** — basic approximate minimum degree on the symmetric pattern of `A + A^T`. Safe default for unknown patterns.

Dense `Value::Matrix` input dispatches through the legacy dense-Gaussian-elimination fallback; users who want the sparse paths on a dense matrix should call `sparse(A)` first.

The sparse paths are the scaling fix for grid-style assemblies. A 100×100 Lesson-05 grid produces a $10^4 \times 10^4$ sparse matrix; the old dense fallback densified it (~800 MB) and ran Gaussian elimination at $O(N^3)$. The Cholesky and LU paths stay sparse, factor in roughly $O(N^{1.5})$ on banded patterns, and scale to grids an order of magnitude larger.

**Real-vs-complex auto-routing.** When every entry of `A` and `b` has imaginary part below $10^{-12}$, the solve uses the real-only (`f64`) factorization, which is roughly 4× faster than the complex (`Complex<f64>`) path. Otherwise the complex path runs.

```
x = spsolve(A, b)                       # auto-detect
x = spsolve(A, b, "cholesky")           # force SPD path
x = spsolve(A, b, "lu")                 # force sparse LU

# Canonical Poisson solve. -L is SPD, so auto picks Cholesky.
nx = 50; ny = 50;
L = laplacian_2d(nx, ny);
rhs = ones(nx*ny, 1);
v = spsolve(-L, rhs);

# Indefinite assembly. Auto routes through sparse LU.
A = [1, 2; 2, 1];                       # eigenvalues 3, -1
x = spsolve(sparse(A), [1; 1]);         # → [1/3, 1/3]
```

**Implementation notes.**
- Cholesky: Davis up-looking algorithm (Davis, *Direct Methods for Sparse Linear Systems*, ch. 4).
- LU: Davis Gilbert-Peierls algorithm with partial pivoting (ch. 6), default tolerance 0.1.
- Ordering: `AmdOrdering` is a basic minimum-degree heuristic on the symmetric pattern of $A + A^T$. Davis-style external-degree refinement (ch. 7) is deferred.

The pivot tolerance, real-vs-complex thresholds, and orderings are all sized for the curriculum's typical inputs; users who need different defaults can build the factorizations directly via `rustlab_core::sparse_solve` from Rust. For a full design walkthrough — dispatch chain, ordering hints, factor reuse, the underlying Davis algorithms — see `docs/sparse_solve.md`.

### `chol(A)` / `chol(A, ordering)`, `lu(A)` / `lu(A, ordering)`, `solve(F, b)`

Reusable factor handles for the factor-once-solve-many pattern. `chol(A)` factors a Hermitian-positive-definite sparse matrix as `L·L^H` and returns an opaque handle. `lu(A)` factors a general sparse matrix as `P·L·U` with partial pivoting (threshold 0.1). `solve(F, b)` runs the cached triangular solves on a right-hand side.

This is the canonical fast path for parameter sweeps and animations: factor once (the dominant cost), then solve per-frame at a small fraction of the factor cost. The same real-vs-complex auto-routing as `spsolve` applies — real-only `A` produces a real factor that takes ~4× less time and memory than the complex equivalent.

The optional `ordering` argument matches `spsolve`'s: `"auto"` (default; consult the matrix's hint, fall back to AMD), `"identity"` / `"natural"` (force natural ordering — best for grid Laplacians, where it's ~5× faster than AMD), `"amd"` (force AMD).

```
% Animation / sweep: factor once, solve per frame
L = -1 * laplacian_2d(100, 100);          % SPD
F = chol(L);                              % factor cost (~0.03 s)
for k = 1:50
  rho = randn(10000, 1);
  v = solve(F, rho);                      % per-frame solve (~0.005 s)
  imagesc(reshape(v, 100, 100));
  frame
end
saveanim("sweep.html");

% LU for non-Hermitian / complex
A = sparse([1+j, 2; 3, 4-j]);
F = lu(A);
x1 = solve(F, [1; j]);
x2 = solve(F, [j; 1]);
```

`chol()` errors when `A` is not SPD — there is no auto fallback to LU, on the assumption that the user explicitly chose Cholesky. If you want auto-dispatch with a single call, use `spsolve(A, b)`. A real factor refuses a complex `b`; refactor with `chol`/`lu` on the complex matrix in that case.

### `laplacian_2d(nx, ny [, dx, dy] [, bc])`

Sparse 5-point Laplacian stencil on a uniform `nx × ny` grid. Returns an `(nx·ny) × (nx·ny)` sparse matrix `L` approximating $+\nabla^2$. Sign convention: Poisson $\nabla^2 V = -\rho/\varepsilon_0$ solves as `V = spsolve(L, -rho/eps0)`.

**Node ordering — column-major.** `V(i, j) → k = (j-1)*ny + i` (1-based). This matches rustlab's `reshape(V_flat, ny, nx)` and `V_grid(:)'` convention so state vectors and grids round-trip without transposes. The third argument of `ij2k` / `k2ij` is `ny`, not `nx`.

**Boundary conditions.** The optional trailing string argument selects:
- `"dirichlet"` (default) — homogeneous Dirichlet (`V = 0` outside the grid). Cells at the grid edge have the same diagonal (`-2/dx² - 2/dy²`) as interior cells but skip the cross-boundary off-diagonal entries. For non-zero Dirichlet values, encode them in the right-hand side.
- `"neumann"` — homogeneous Neumann (zero normal flux). Boundary cells absorb the missing direction's coefficient back into the diagonal — for a four-side Neumann grid the corner diagonal becomes `-(1/dx² + 1/dy²)`. Constants are in the null space, so the resulting linear system is singular: pin one cell (zero a row, set its diagonal to 1, and pin the corresponding RHS) before calling `spsolve`.
- `"periodic"` — wrap. Edge cells point to their wrap-around neighbours. Constants are in the null space; the same row-pinning idiom applies.

```
# Default unit spacing, Dirichlet
L = laplacian_2d(nx, ny)

# Anisotropic grid
L = laplacian_2d(nx, ny, dx, dy)

# Neumann boundaries on the same grid
L = laplacian_2d(nx, ny, dx, dy, "neumann")

# Periodic boundaries (Brillouin-zone-style wrap)
L = laplacian_2d(nx, ny, "periodic")

# Canonical Poisson solve for a point source at the grid centre
nx = 32; ny = 24;
L = laplacian_2d(nx, ny);
rho = zeros(ny, nx);
rho(ny/2, nx/2) = 1.0;
V = spsolve(L, -rho(:)');
V_grid = reshape(V, ny, nx);
```

### `laplacian_1d(n [, dx] [, bc])`

Sparse tridiagonal Laplacian on a length-`n` 1-D grid. Returns an `n × n` sparse matrix approximating `+d²/dx²`. The `bc` argument is the same string-form selector as `laplacian_2d`.

```
L = laplacian_1d(100)                          # Dirichlet, dx=1
L = laplacian_1d(100, 0.01, "periodic")        # periodic with explicit spacing
```

### `laplacian_3d(nx, ny, nz [, dx, dy, dz] [, bc])`

Sparse 7-point Laplacian on an `nx × ny × nz` uniform grid. Returns an `(nx·ny·nz) × (nx·ny·nz)` sparse matrix.

**Node ordering — column-major-of-pages.** `V(i, j, kk) → k = ((kk-1)*nx + (j-1))*ny + i` (1-based). Axis 0 = y (rows), axis 1 = x (cols), axis 2 = z (pages) — the `Tensor3` convention. The `ijk2k` / `k2ijk` helpers handle the round-trip; their last two arguments are `ny` and `nx`.

```
L = laplacian_3d(8, 8, 8)                              # Dirichlet, unit spacing
L = laplacian_3d(8, 8, 8, "neumann")                   # Neumann, unit spacing
L = laplacian_3d(8, 8, 8, 0.1, 0.1, 0.05)              # anisotropic
L = laplacian_3d(8, 8, 8, 0.1, 0.1, 0.05, "periodic")  # periodic anisotropic
```

### `laplacian_eps_2d(eps_map [, dx, dy] [, bc])`

Variable-coefficient Laplacian `∇·(ε∇V)` on a 2-D uniform grid via flux-conservative discretization with harmonic-mean half-cell coefficients:

$$\varepsilon_{i,j+1/2} = \frac{2\,\varepsilon(i,j)\,\varepsilon(i,j+1)}{\varepsilon(i,j) + \varepsilon(i,j+1)}$$

The harmonic mean is the physically correct face-coefficient choice for piecewise-uniform media — it preserves flux continuity across material interfaces (where arithmetic-mean discretizations introduce artificial sources).

`eps_map` is shape `(ny, nx)` matching `meshgrid` / `imagesc`. Real or complex entries (lossy materials are common in FDFD-style problems). Setting `eps_map ≡ 1` reduces this to the constant-coefficient `laplacian_2d`. The same `bc` selector is supported.

```
# Dielectric slab in vacuum: half the grid has eps=4
eps = ones(ny, nx);
eps(:, 1:nx/2) = 4.0;
L = laplacian_eps_2d(eps, dx, dy);

# Magnetostatic 1/mu form: pass 1./mu_map
A = laplacian_eps_2d(1.0 ./ mu_map, dx, dy);

# Lossy material with imaginary eps
eps_lossy = 4.0 - 0.1 * j * ones(ny, nx);
L = laplacian_eps_2d(eps_lossy, dx, dy);
```

### `ij2k(i, j, ny)`

Column-major grid-to-flat index conversion (1-based). Returns `(j-1)*ny + i`. The third argument is `ny` (row count), not `nx` — this matches the `laplacian_2d` ordering.

```
k = ij2k(3, 4, 6)     # → (4-1)*6 + 3 = 21
```

### `k2ij(k, ny)`

Inverse of `ij2k`. Returns a tuple `[i, j]` destructurable via `[i, j] = k2ij(k, ny)`.

```
[i, j] = k2ij(21, 6)  # → i = 3, j = 4
```

### `ijk2k(i, j, kk, ny, nx)`

3-D version of `ij2k`. Converts 1-based grid indices `(i, j, kk)` to the column-major-of-pages flat index `k = ((kk-1)*nx + (j-1))*ny + i`. The fourth and fifth arguments are `ny` (rows) and `nx` (cols) — same `Tensor3` convention used by `laplacian_3d`.

```
k = ijk2k(2, 3, 4, 5, 6)    # → ((4-1)*6 + (3-1))*5 + 2 = 102
```

### `k2ijk(k, ny, nx)`

Inverse of `ijk2k`. Returns a tuple `[i, j, kk]` destructurable via `[i, j, kk] = k2ijk(k, ny, nx)`.

```
[i, j, kk] = k2ijk(102, 5, 6)   # → i = 2, j = 3, kk = 4
```

### `full(S)`
Convert a sparse value to dense. Acts as the identity for already-dense inputs.
```
M = full(S)       # sparse → matrix
v = full(sv)      # sparse vector → vector
```

### `nnz(S)`
Number of stored non-zero entries. For dense inputs, returns `numel`.
```
nnz(speye(4))          # → 4
nnz(sparse(zeros(5)))  # → 0
```

### `issparse(x)`
Returns `1` if `x` is a sparse matrix or sparse vector, `0` otherwise.
```
issparse(speye(3))   # → 1
issparse(eye(3))     # → 0
```

### `nonzeros(S)`
Vector of stored non-zero values in storage order.
```
nonzeros(sparse([1,2], [1,2], [7.0, 9.0], 2, 2))   # → [7.0, 9.0]
```

### `find(S)`
Locate non-zero entries. For a sparse matrix, returns a tuple `[I, J, V]` of 1-based row indices, column indices, and values. For a sparse vector, returns `[I, V]`.
```
[I, J, V] = find(S)      # sparse matrix
[I, V] = find(sv)        # sparse vector
```

Native O(nnz) operations: `S+S`, `S-S`, `S*scalar`, `S/scalar`, `S*M` (SpMM), `S*v'` (SpMV via SpMM), `dot(sv, sv)`, `dot(sv, v)`, `transpose(S)`, `S'`. Indexing `S(i,j)` reads (returning 0 for absent entries) and `S(i,j) = val` writes (setting to 0 removes the entry).

---

## Plotting

All plot functions accumulate series into a shared **figure state** and render immediately. Use `figure()`, `hold()`, `subplot()` etc. to control layout before calling plot functions.

### Figure State

#### `figure()` / `figure("file.html")` / `figure(N)`
Create a new figure or switch between existing figures. Returns a numeric handle.

Multiple figures can coexist — each has its own plot data, labels, and output mode (TUI, HTML, or viewer). With no arguments, creates a new TUI figure. With an HTML path, creates a new HTML-mode figure. With a numeric argument, switches to that figure (creating it if it doesn't exist).
```
fig1 = figure()              % new figure (TUI mode), handle = 1
plot(sin(t))
fig2 = figure("temp.html")  % new figure (HTML mode), handle = 2
plot(cos(t))                 % writes to temp.html
figure(fig1)                 % switch back to fig1
hold on
plot(cos(t))                 % adds to fig1 (TUI)
figure(5)                    % create/switch to figure 5
```

#### `hold on` / `hold off`
When hold is on, new `plot()`/`stem()` calls add series to the current subplot instead of replacing them. Also accepts function-call form: `hold("on")`, `hold(1)`.
```
hold on
plot(signal1, "label", "first")
plot(signal2, "label", "second")
hold off
```

#### `grid on` / `grid off`
Show or hide grid lines on the current subplot. Also accepts function-call form: `grid("on")`, `grid(1)`. Default is on.

#### `viewer` / `viewer on` / `viewer on <name>` / `viewer off`
Connect to a running `rustlab-viewer` process. When connected, all plot commands (`plot`, `stem`, `bar`, `bode`, `surf`, etc.) render in the external egui viewer with zoom/pan/crosshairs instead of the terminal. `viewer off` disconnects and returns to terminal plotting. Bare `viewer` (no argument) reports the current connection state and where the active figure will be rendered (rustlab-viewer, HTML file, or the TUI).

Requires the `viewer` feature (included in `make install`). Start `rustlab-viewer` before typing `viewer on`.
```
viewer on          % connect to default viewer
plot(x, sin(x))   % renders in viewer window
viewer             % status: "connected, current figure → rustlab-viewer (figure id N)"
viewer off         % back to terminal
```

**Automatic fallback.** If the viewer is closed or crashes while still connected, the next plot command detects the broken connection, prints `viewer: connection lost (...) — falling back to terminal rendering`, clears the viewer session, and renders the current figure in the TUI. Subsequent plots continue to render in the terminal until you run `viewer on` again.

**Named sessions** allow multiple viewers to run simultaneously, each receiving plots from different rustlab instances:
```
% Terminal 1:                    % Terminal 2:
rustlab-viewer --name filters    rustlab-viewer --name analysis

% REPL 1:                        % REPL 2:
viewer on filters                viewer on analysis
plot(h)                          plot(spectrum)
```

Multiple rustlab instances can also send plots to the same viewer — each process gets unique figure IDs so plots don't interfere.

#### `subplot(rows, cols, idx)`
Switch to subplot panel. `rows` and `cols` define the grid; `idx` is 1-based (row-major order).
```
subplot(2, 1, 1)
plot(x)
subplot(2, 1, 2)
stem(h)
```

#### `grid("on")` / `grid("off")`
Enable or disable grid lines on the current subplot.
```
grid("on")
```

#### `xlabel("text")`
Set the x-axis label on the current subplot.
```
xlabel("Time (s)")
```

#### `ylabel("text")`
Set the y-axis label on the current subplot.
```
ylabel("Amplitude")
```

#### `title("text")`
Set the title on the current subplot.
```
title("Frequency Response")
```

#### `xlim([lo, hi])`
Set x-axis bounds on the current subplot.
```
xlim([0.0, 1000.0])
```

#### `ylim([lo, hi])`
Set y-axis bounds on the current subplot.
```
ylim([-1.0, 1.0])
```

#### `legend("s1", "s2", ...)`
Retroactively set labels on series in the current subplot (in order).
```
hold("on")
plot(a)
plot(b)
legend("signal a", "signal b")
```

---

## Visualization — Interactive (terminal)

These functions open a full-screen terminal chart and wait for a keypress before returning.

### `plot(v)`
Line chart of a real or complex vector (sample index on x). For complex vectors, shows magnitude (blue) and real part (green) overlaid.
```
plot(signal, "440 Hz Sinusoid")
```

### `plot(x, v)`
Line chart with explicit x-axis vector.
```
t = linspace(0.0, 1.0, 1000)
plot(t, signal, "label", "sine wave")
```

### `plot(v, "color", c, "label", lbl, "style", s)`
Plot with options. Options are trailing key-value string pairs:
- `"color"` — color name: `"red"`, `"green"`, `"blue"`, `"cyan"`, `"magenta"`, `"yellow"`, `"black"`, `"white"`, or single-letter shortcuts (`"r"`, `"g"`, `"b"`, ...)
- `"label"` — legend label string
- `"style"` — `"solid"` (default) or `"dashed"`
```
plot(signal, "color", "red", "label", "filtered")
plot(t, noise, "color", "g", "style", "dashed", "label", "noise")
```

### `plot(M)` / `plot(x, M)`
Plot a matrix: one line series per column.
```
plot(M)           # sample index x, each column a series
plot(t, M)        # explicit x axis
```

### `stem(v)` / `stem(x, v)`
Stem (lollipop) chart — one vertical bar per sample. Supports the same color/label/style options as `plot()`.
```
stem(real(h), "Impulse Response")
stem(n, h, "color", "red", "label", "h[n]")
```

### `plotdb(Hz [, title])`
Frequency response in dB. `Hz` is the 2×n matrix returned by `freqz()` or `spectrum()`.
- x-axis: frequency in Hz
- y-axis: 20·log₁₀|H(f)|
```
plotdb(freqz(h, 512, sr), "Lowpass Response")
plotdb(spectrum(fft(x), sr), "Signal Spectrum")
```

### `hist(v [, n_bins])`
Bar chart histogram of `v`. Default bin count is 10. Displays interactively and returns a **2×n matrix**:
- Row 1: bin centers
- Row 2: counts
```
hist(randn(2000), 30)
H = hist(data, 20)   # capture bin data
```
Alias: `histogram()`

### `bar(y)` / `bar(x, y)` / `bar(y, title)` / `bar(x, y, title)`
Bar chart. Each element of `y` is rendered as a filled vertical bar. `x` specifies the bar centre positions (defaults to 0, 1, 2, …).
```
bar([3, 1, 4, 1, 5, 9, 2, 6])
bar([1,2,3], [10,20,30], "Counts")
```

#### Categorical bar charts: `bar(labels, y)` / `bar(labels, y, title)`
When the first argument is a string array, it provides categorical x-axis labels:
```
bar({"Jan", "Feb", "Mar"}, [10, 20, 30])
bar({"A", "B", "C"}, [5, 8, 3], "Results")
```

#### Grouped bar charts: `bar(M)` / `bar(x, M)` / `bar(x, M, title)`
When `y` is a matrix, each column becomes a separate bar group rendered side-by-side. This is the grouped bar chart style.
```
A = [10, 20; 15, 25; 12, 18]
bar(A)                        % 3 positions, 2 groups
bar([1, 2, 3], A, "Sales")   % explicit x positions
```
- Negative bar heights are supported (bars extend downward from zero).
- Press any key to close the terminal display.

### `hline(y)` / `hline(y, color)` / `hline(y, color, label)`
Draw a horizontal reference line at the specified y value. `yline()` is an alias. Best used with `hold("on")` to overlay on an existing plot.
```
plot(x, data)
hold on
hline(threshold, "r", "limit")     % red dashed line at y=threshold
hline([lo, hi], "g")                % two green lines
```
- Lines are rendered as dashed by default.
- Accepts a scalar (one line) or a vector (multiple lines).

### `scatter(x, y)` / `scatter(x, y, title)`
Scatter plot — renders each (x, y) pair as a dot. No lines are drawn between points.
```
scatter(x, y)
scatter(t, noise, "Noise vs Time")
```
- Press any key to close.

### `imagesc(M)` / `imagesc(M, colormap)`
Display a matrix as a false-color heatmap in the terminal. Each cell is colored according to its magnitude using the specified colormap. Supported colormaps: `"viridis"` (default), `"jet"`, `"hot"`, `"gray"`.
```
imagesc(spectrogram_matrix)
imagesc(M, "jet")
```

### `surf(Z)` / `surf(X, Y, Z)` / `surf(X, Y, Z, colormap)`
Plot a Z-grid as a 3D surface. `Z` is a matrix (rows = Y samples, cols = X samples). `X` and `Y` may be 1-D vectors or 2-D `meshgrid` matrices. Optional colormap: `"viridis"` (default), `"jet"`, `"hot"`, `"gray"`.

Per-backend behaviour:

- **Terminal** — heatmap of Z (no 3D interaction in a terminal).
- **Viewer** (`viewer on`) — interactive 3D: left-drag rotate, scroll zoom, shift+scroll scale Z, right-drag pan, `R` to reset.
- **HTML** (`savefig("...html")`) — Plotly 3D surface (draggable in browser).
- **SVG / PNG** — static isometric wireframe.
- **Notebook** (`rustlab-notebook render`) — captured as a figure snapshot; HTML output embeds a Plotly 3D surface (rotate/zoom in browser), PDF output embeds the SVG wireframe.

```
[X, Y] = meshgrid(linspace(-3, 3, 40), linspace(-3, 3, 40));
Z = sin(X.^2 + Y.^2);
surf(X, Y, Z);            % X, Y from meshgrid
surf(X, Y, Z, "jet");     % with colormap
surf(Z);                  % x = 1..cols, y = 1..rows
```

### `contour(Z)` / `contour(X, Y, Z)` / `contour(X, Y, Z, ...)`

Line contours of a 2-D scalar field. `Z` is an `ny × nx` matrix; `X` and `Y` may be 1-D vectors of length `nx` and `ny`, or 2-D `meshgrid` matrices. The 1-arg form `contour(Z)` defaults `X = 1..ncols`, `Y = 1..nrows`.

Optional trailing arguments (any order, each at most once):

- **Scalar** — `nlevels`, the number of auto-spaced round-number contour levels (default 10).
- **Vector** — explicit list of level values (sorted internally).
- **String** — single-letter colour code (`"k"`, `"r"`, `"g"`, `"b"`, `"c"`, `"m"`, `"y"`, `"w"`) for line colour; otherwise interpreted as the subplot title.

Algorithm: marching squares (NaN cells skipped; saddle points resolved by the cell-centre value). Auto-level placement picks step size from `{1, 2, 2.5, 5} × 10^k` so labels read cleanly. Each axis must have length ≥ 2.

```
[X, Y] = meshgrid(linspace(-2, 2, 41), linspace(-2, 2, 41));
Z = X .^ 2 + Y .^ 2;
contour(X, Y, Z);                  % 10 auto levels, black lines
contour(X, Y, Z, 20, "k");         % 20 levels, explicit colour
contour(X, Y, Z, [0.5, 1, 2]);     % explicit levels
contour(X, Y, Z, "Equipotentials"); % set the subplot title
```

`hold on` lets you overlay contours on `imagesc` heatmaps and on each other:

```
hold on;
imagesc(Z);
contour(X, Y, Z, 8, "k");          % black contours over the heatmap
hold off;
```

Per-backend behaviour:

- **Terminal** — not rendered (a one-time warning is printed). Use `savefig("...svg")` or `savefig("...html")` to view.
- **HTML** (`savefig("...html")`) — Plotly contour trace per `ContourData`. Exact level lines.
- **SVG / PNG** — marching-squares line segments via plotters' `PathElement`.
- **Notebook** — captured as a figure snapshot; output follows the renderer's HTML / SVG path.

### `contourf(Z)` / `contourf(X, Y, Z)` / `contourf(X, Y, Z, ...)`

Filled contours of a 2-D scalar field. Same argument forms as `contour` (the colour-string slot is unused for filled contours; the colormap is `viridis` in v1).

```
contourf(X, Y, Z);
contourf(X, Y, Z, 12);             % 12 colour bands
contourf(X, Y, Z, [0, 1, 2, 4]);   % explicit bands
```

Per-backend behaviour:

- **HTML** — Plotly contour with `coloring="fill"` — exact polygon fill between adjacent levels.
- **SVG / PNG** — per-cell discrete-band approximation (each cell painted with the colour for its centre-value's band). Looks like a coarse colourmap; v1 limitation. Exact polygon fill is HTML-only.
- **Terminal** — not rendered.

### `quiver(X, Y, U, V)` / `quiver(X, Y, U, V, ...)` / `quiver(U, V)`

Arrow plot of a 2-D vector field. `U` and `V` are same-shape matrices giving the field's x- and y-components on the grid `X × Y`. `X` and `Y` may be 1-D vectors (length = `ncols`, `nrows`) or `meshgrid` matrices. `quiver(U, V)` is a shortcut that defaults `X` and `Y` to `1..ncols`, `1..nrows`.

Arrows auto-scale so the longest one equals the nearest-neighbour cell distance — dense fields never overlap. Trailing modifier arguments (in any order):

- **Scalar** — a positive multiplier applied on top of the auto-scale.
- **String** — a single-letter colour code (`"k"/"r"/"g"/"b"/"c"/"m"/"y"/"w"`) sets arrow colour; any other string is the subplot title.

```
quiver(X, Y, U, V);
quiver(X, Y, U, V, 0.5);            % half-length arrows
quiver(X, Y, U, V, "Vortex field");
quiver(X, Y, U, V, "k");            % black arrows
quiver(U, V);                       % shortcut — indexed axes
```

Under `hold on`, quiver overlays stack on top of `imagesc` heatmaps and `contour` traces. NaN entries in `U` or `V` skip that cell.

Per-backend behaviour:

- **HTML** (`savefig("...html")`) — single Plotly scatter line trace with `null`-separated polylines per arrow (shaft + triangular head).
- **SVG / PNG** — plotters line + head polyline per cell.
- **Terminal** — not rendered (one-time warning).

### `streamplot(X, Y, U, V)` / `streamplot(X, Y, U, V, ...)`

Streamlines of a 2-D vector field. Traces are integrated by RK4 forward and backward from each seed, clipping at the domain boundary and terminating on NaN samples, near-zero field magnitude, or closed-orbit return. Each streamline carries a midpoint arrowhead showing direction.

Trailing modifier arguments (in any order):

- **Scalar** — `density`, a positive knob; default `1.0` places a 10×10 seed grid (≈ 100 seeds).
- **Matrix (N×2)** — explicit `(x, y)` seed points; overrides the default grid.
- **String** — single-letter colour code as in `quiver`; any other string is the subplot title.

```
streamplot(X, Y, U, V);
streamplot(X, Y, U, V, 2.0);        % 20×20 seed grid
streamplot(X, Y, U, V, [[-1.5, 0.5]; [0.0, 1.8]; [1.5, -1.2]]);  % 3 seeds
streamplot(X, Y, U, V, "Field lines");
streamplot(X, Y, U, V, "r");        % red streamlines
```

Same `hold on` / backend behaviour as `quiver`.

### `loglog(x, y [, opts])` / `semilogx(x, y [, opts])` / `semilogy(x, y [, opts])`

Log-axis line plots. Implemented as **pre-transform shims** over `plot()`: the data is mapped through `log10` and the resulting axes are labeled `log10(x)` / `log10(y)` to indicate the transform. Power-law data renders as a straight line on `loglog`, exponential decay as a straight line on `semilogy`, etc.

```
% Power law y = x^2 looks like a straight line of slope 2.
x = logspace(0, 3, 50);
y = x .^ 2;
loglog(x, y);

% Bode-style frequency response.
f = logspace(0, 4, 200);
H = freqz_eval(f, ...);
semilogx(f, 20 * log10(abs(H)))
```

`loglog` requires both `x` and `y` to be strictly positive. `semilogx` requires positive `x`; `semilogy` requires positive `y`. Negative or zero values produce a clear error.

Tick labels are the log10 values themselves (0, 1, 2, 3 instead of 1, 10, 100, 1000). Proper LogCoord-style axes with decade labels are tracked as a future enhancement; the pre-transform shim correctly captures the curve shapes that the curriculum needs.

### `polar(theta, r [, opts])`

Polar plot via Cartesian pre-transform: plots `(r·cos(θ), r·sin(θ))` and labels the axes accordingly. `theta` is in radians; both arguments must be real-valued.

```
% Three-petal rose curve.
theta = linspace(0, 2*pi, 360);
r = 1 + 0.3 * cos(3 * theta);
polar(theta, r);

% Antenna pattern (Hertzian dipole).
theta = linspace(-pi, pi, 360);
r = abs(sin(theta));
polar(theta, r)
```

Radial gridlines and angular tick labels are tracked as a future enhancement; the pre-transform plot still produces the correct closed shape and is sufficient for visual verification of antenna lobes, polar response curves, etc.

---

## Visualization — File Output (PNG / SVG / HTML)

File format is detected from the extension (`.svg`, `.png`, or `.html`).

### `savefig(filename)`
Save the current figure state to file. Any interactive plot (`plot`, `stem`, `bar`, `scatter`, `plotdb`, `histogram`, `imagesc`, `surf`) pushes data into the figure, then `savefig(path)` renders it.

```
plot(real(signal), "440 Hz Sinusoid")
savefig("signal.svg")

stem(real(h), "Impulse Response")
savefig("impulse.png")

plotdb(freqz(h, 512, sr), "Lowpass Response")
savefig("response.svg")

imagesc(M, "jet")
savefig("heatmap.svg")

savefig("report.html")    % interactive Plotly HTML with zoom/pan/hover
```

### `frame()`

Snapshot the current figure into the animation frame buffer, then clear the trace data on the active figure so the next loop iteration starts with a clean canvas. Subplot layout, axis labels, titles, limits, hold state, and grid setting are preserved across the call — only `series`, `heatmap`, `surface`, `contours`, `quivers`, and `streamlines` are wiped.

```
figure()
for k = 1:60
  Ez = step(k);
  imagesc(Ez, "viridis")
  title(sprintf("t = %d", k))     % set title AFTER imagesc — imagesc clears it
  frame()
end
saveanim("wave.html", 30)
```

Calling `figure()` or `figure(N)` clears the frame buffer in addition to its existing reset behaviour, so "start a new animation" is the natural pattern.

### `saveanim(path)` / `saveanim(path, fps)`

Flush the animation frame buffer to disk. The path extension picks the output format. `fps` defaults to 10 and controls per-frame display duration.

| Extension | Output | When to use |
|---|---|---|
| `.html` / `.htm` | Self-contained Plotly animation with play/pause buttons and a per-frame slider (`1000/fps` ms per frame). | Interactive viewing in a browser; embeds inside the rendered HTML notebook. |
| `.gif` | Animated GIF, per-frame NeuQuant palette quantization. GIF stores delays in centiseconds — fps above ~100 rounds to the same 1 cs floor. | Portable, GitHub-renders inline in Markdown, embeds in PDFs (via the LaTeX `animate` package), shareable. |

- **Path extension**: `.html`, `.htm`, or `.gif` only. Other extensions (`.svg`, `.png`, `.mp4`) return a clear error.
- **Empty buffer**: errors with `saveanim: no frames captured (call frame() at least once)`.
- **On success**: the buffer is drained, so a subsequent `frame()` loop starts clean without an explicit `figure()`.

**Memory + size budget.** `frame()` clones the full `FigureState` (every plotted vector, every heatmap matrix). For a 200×200 heatmap × 500 frames the buffer holds ~160 MB before flush. Output sizes vary by format:

| Demo | Plotly HTML | Animated GIF |
|---|---|---|
| 60 frames, 100×100 heatmap | ~13 MB | ~5 MB |
| 120 frames, 100×100 heatmap | ~26 MB | ~10 MB |

Plotly bundles are typically 2–3× larger than the equivalent GIF for heatmap-style data, but render at full resolution and have hover/zoom. GIF is fixed-resolution but trivially portable.

```
% Travelling Gaussian pulse — same loop, two output formats
[X, Y] = meshgrid(linspace(-3, 3, 100), linspace(-3, 3, 100));
figure()
for k = 1:60
  c = (k - 30) * 0.1;
  Z = exp(-((X - c).^2 + Y.^2));
  imagesc(Z, "viridis"); title(sprintf("k = %d", k))
  frame()
end
saveanim("pulse.html", 30)        % interactive Plotly
% saveanim("pulse.gif", 30)       % or portable GIF
```

## Figure & Plot Controls

These functions configure the figure that the next chart-creation call (`plot`, `stem`, `imagesc`, `contour`, `quiver`, `streamplot`, etc.) will populate. They have no effect on the math — only on the rendered output.

### `figure()`
Open a new figure. Subsequent chart calls draw into it. With `hold off` (the default), each new chart-creating call clears the current figure first.

### `clf()`
Clear the current figure's series, titles, and axes — convenient at the top of a notebook cell or example script.

### `close` / `close all` / `close(N)`
Dismiss figures.

- `close` — remove the current figure from the figure store.
- `close all` (or `close("all")`) — remove every open figure in one go.
- `close(N)` — remove the figure with handle `N` (the value `figure()` returned).

When `rustlab-viewer` is connected, `close` also closes the corresponding viewer window; `close all` clears every viewer window via a single `Reset` message and keeps the IPC connection itself open so subsequent plots route to fresh viewer figures. Closing the active figure switches focus to the most recently used remaining figure; closing the last one resets to a fresh anonymous figure on the terminal.

`close` is for the regular figures returned by `figure()`. To release an animation-style `LiveFigure` handle (see `figure_live`), use `figure_close(fig)` instead.

### `hold("on")` / `hold("off")` / `hold(1)` / `hold(0)`
Toggle whether the next chart call clears the figure first. With `hold on`, multiple `plot` / `quiver` / `contour` calls overlay on the same axes — the canonical pattern for combining heatmaps with field arrows.

```
figure(); hold on;
imagesc(V);
quiver(X, Y, Ex, Ey);
contour(X, Y, V, 8, "k");
```

### `grid("on")` / `grid("off")` / `grid(1)` / `grid(0)`
Show or hide gridlines.

### `title(s)` / `xlabel(s)` / `ylabel(s)`
Set the figure title or axis labels. Strings only.

```
title("Frequency response")
xlabel("Frequency (Hz)")
ylabel("Magnitude (dB)")
```

### `xlim(lo, hi)` / `ylim(lo, hi)`
Set explicit axis ranges. `lo` and `hi` are real scalars.

```
xlim(0, 1000)
ylim(-60, 5)
```

### `axis("equal")` / `axis("auto")` / `axis("xy")` / `axis("ij")` / `axis([xmin, xmax, ymin, ymax])`
Control aspect ratio, y-axis orientation, or set both axis limits at once.

- `axis("equal")` — lock the visual aspect so one data unit on x equals one data unit on y. Honored across all four rendering backends (terminal, viewer, SVG, Plotly HTML). Use it for parametric plots and any chart where geometric shape matters (Nyquist plots, complex-plane scatters, unit circles).
- `axis("auto")` — release the aspect lock; the chart fills the available area independently on each axis (default).
- `axis("xy")` — physics / meshgrid y-axis on the current panel: matrix row 0 sits at the **bottom**, y-axis labels go bottom-up (0 at bottom, `nrows` at top). Opt-in for `imagesc` / `image` / `heatmap` panels.
- `axis("ij")` — image-pixel y-axis on the current panel: matrix row 0 sits at the **top**, labels reversed (0 at top, `nrows` at bottom). Default for `imagesc` / `image` / `heatmap`. Matches MATLAB/Octave `imagesc`.
- `axis([xmin, xmax, ymin, ymax])` — set both axis limits at once. Equivalent to `xlim([xmin, xmax]); ylim([ymin, ymax])`.

```
theta = linspace(0, 2*pi, 200);
plot(cos(theta), sin(theta));
axis("equal")            % a circle should look like a circle
```

```
% Heatmap of a 2-D physics field with y pointing up the page.
M = some_field();
imagesc(M, "viridis");
axis("xy")               % flip this panel: row 1 at bottom, y grows upward
```

For a process-wide default (e.g. an EM / heat-transfer notebook preamble that wants every heatmap in physics y without typing `axis("xy")` after each `imagesc`), see `set_default_axis(...)` below.

### `set_default_axis("xy" | "ij")`
Set the per-process **default** y-axis orientation for newly-created subplot panels.

- `set_default_axis("xy")` — every subplot built after this call starts in physics y (row 0 at the bottom, y grows upward).
- `set_default_axis("ij")` — restore the image-pixel default (row 0 at the top, labels reversed). Matches MATLAB/Octave `imagesc`.

Best used **once in a notebook preamble** — e.g. a curriculum that uses `imagesc` heavily on `meshgrid`-style fields drops one line at the top:

```
set_default_axis("xy");      % whole-notebook physics convention
% ... every imagesc/image/heatmap below renders with row 0 at the bottom
```

Per-panel `axis("xy")` / `axis("ij")` still overrides the default for individual plots. The call also retro-applies to every existing panel in the current figure, so a one-line preamble works even though the default canvas was already created before the call.

| Use case | Right choice |
|---|---|
| Physics / EM / heat-transfer fields where y is a real spatial coordinate | `axis("xy")` per panel, or `set_default_axis("xy")` in a preamble |
| Image pixel data (image row 0 is the top scanline) | default (`ij`) — no action needed |
| Sparse-matrix sparsity patterns (`spy(A)`-style) | default (`ij`) — row 1 at top matches `spy()` semantics |
| ML attention heatmaps (query index 0 at top) | default (`ij`) |
| Confusion matrices (true-label row 0 at top) | default (`ij`) |
| GIS / map data with latitude growing northward | `axis("xy")` |

### `legend("loc1", "loc2", ...)` / `legend("off")`
Set legend labels for the series in the current figure (in plot order). `legend("off")` hides the legend.

```
plot(t, x, "label", "input");
plot(t, y, "label", "output");
legend("input", "output")
```

### `subplot(rows, cols, idx)`
Switch the active subplot in a grid layout. `idx` is 1-based, row-major.

```
subplot(2, 1, 1); plot(t, x); title("input");
subplot(2, 1, 2); plot(t, y); title("output");
```

### `yline(y)` / `yline(y, color)` / `yline(y, color, label)`
Draw a horizontal reference line across the current axes — useful for thresholds, target values, or zero lines on bode-style plots.

```
yline(-60)                  % default colour, no label
yline(0, "k")               % black zero line
yline(-3, "r", "-3 dB")     % labelled threshold
```

## Import / Export

### `save(filename, x)`
Save a single variable to a file. Format is determined by the file extension.

| Extension | Format | Notes |
|-----------|--------|-------|
| `.npy` | NumPy binary | Real arrays stored as `float64`, complex as `complex128`. Compatible with `numpy.load()` in Python. |
| `.csv` | CSV text | Complex values written as `a+bi`. Real arrays produce plain numbers. |
| `.toml` | TOML text | Top-level value must be a struct. Nested structs, vectors, booleans, and strings are supported. |

```
save("signal.npy", x)
save("coeffs.csv", h)
save("config.toml", cfg)
```

### `save(filename, "name1", x1, "name2", x2, ...)`
Save multiple named variables into a single `.npz` archive (a zip file containing one `.npy` entry per variable). The `.npz` extension is required.

```
save("session.npz", "signal", x, "filter", h, "freqs", f)
```

The resulting file is directly readable by `numpy.load("session.npz")` in Python.

### `load(filename)`
Load a single array from a `.npy` or `.csv` file, or a struct from a `.toml` file. Returns a scalar, vector, matrix, or struct depending on the file format and content.

```
x = load("signal.npy")
h = load("coeffs.csv")
cfg = load("config.toml")       % returns a struct
sr = cfg.audio.sample_rate       % access nested fields
```

### `load(filename, varname)`
Load one named array from a `.npz` archive.

```
x = load("session.npz", "signal")
h = load("session.npz", "filter")
```

### `whos(filename)`
List the contents of a `.npz` archive — name, type (`real` or `complex`), and size of each stored array. Returns `None`; output is printed.

```
whos("session.npz")
```

Example output:
```
  Name                 Type       Size
  ────────────────────────────────────────────
  signal               complex    1024
  filter               real       65
  freqs                real       512
```

---

## Controls Toolbox

Classical control systems — transfer functions, state-space, frequency analysis, and optimal control.

### `tf(arg)` / `tf(num, den)` / `tf(sys)` / `tf(A, B, C, D)`

Create a transfer function.

```
s = tf("s")              % Laplace variable: num=[1,0], den=[1]
G = tf([10], [1, 2, 10]) % 10 / (s² + 2s + 10)
```

Build TFs from `s` using arithmetic — the preferred idiom:

```
s   = tf("s")
G   = 10 / (s^2 + 2*s + 10)
C   = 5 * (s + 2) / s       % PI controller
T   = G * C / (1 + G * C)   % closed-loop
```

Supported arithmetic: `+`, `-`, `*`, `/`, `^` (integer exponent), and scalar operands.

**Convert from state-space (SISO).** Either pass a `sys` value built by `ss(...)` or pass the four matrices directly:

```
% From a state-space value:
sys = ss([0,1; -4,-0.5], [0;1], [1,0], 0)
G   = tf(sys)                  % G(s) = 1 / (s² + 0.5s + 4)

% Equivalent — four-matrix sugar:
G = tf([0,1; -4,-0.5], [0;1], [1,0], 0)
```

Uses the Faddeev–LeVerrier recursion to compute `det(sI − A)` and `C·adj(sI − A)·B + D·det(sI − A)` directly, in O(n⁴), without eigenvalue solves or root-finding. SISO only — `B` is n×1, `C` is 1×n, `D` is 1×1. No automatic pole-zero cancellation: redundant factors stay in both numerator and denominator (a future `minreal(G)` would handle that).

### `tfdata(G)`

Extract numerator and denominator coefficient vectors from a transfer function. Always multi-return.

```
G = tf([1, 2], [1, 3, 5])
[num, den] = tfdata(G)   % num = [1, 2], den = [1, 3, 5]
```

Coefficients are in descending-power order (index 0 = highest power), matching the convention used everywhere else.

### `pole(G)`

Roots of the denominator (open-loop poles).

```
G = tf([10], [1, 2, 10])
p = pole(G)   % ≈ [-1+3j, -1-3j]
```

### `zero(G)`

Roots of the numerator (transmission zeros).

```
G = tf([1, 1], [1, 2, 10])
z = zero(G)   % ≈ -1
```

### `ss(G)` / `ss(A, B, C, D)`

Two forms:

- `ss(G)` — convert a transfer function to state-space in observable canonical form.
- `ss(A, B, C, D)` — build a state-space directly from matrices (any input/output dimensions).

```
% TF → SS (observable canonical form):
sys = ss(G)
A = sys.A   B = sys.B   C = sys.C   D = sys.D

% Build SS from physics-derived matrices:
sys = ss([0,1; -4,-0.5], [0;1], [1,0], 0)
```

Each field is a `CMatrix`. Eigenvalues of `A` match `pole(G)` for the converted form.

**Shape rules for `ss(A, B, C, D)`:** `A` is n×n, `B` is n×m, `C` is p×n, `D` is p×m. A scalar `0` for `D` is accepted and broadcast to p×m.

### `ctrb(A, B)`

Controllability matrix `[B, AB, A²B, …]` — size n × (n·m).

Full column rank ↔ system is controllable.

```
sys = ss(G)
Wc  = ctrb(sys.A, sys.B)
rank(Wc)   % should equal n for controllable system
```

### `obsv(A, C)`

Observability matrix `[C; CA; CA²; …]` — size (n·p) × n.

Full row rank ↔ system is observable.

```
Wo = obsv(sys.A, sys.C)
rank(Wo)
```

### `bode(G)` / `bode(G, w)` / `[mag, phase, w] = bode(G)`

Bode magnitude and phase plot (log10(ω) x-axis). Always plots; returns data as a tuple.

- `mag` — magnitude in dB
- `phase` — phase in degrees (unwrapped)
- `w` — frequency vector in rad/s

```
G = tf([10], [1, 2, 10])
bode(G)                      % interactive plot
[m, p, w] = bode(G)          % capture data
[m, p, w] = bode(G, w_vec)  % user-supplied frequencies
```

### `nyquist(G)` / `nyquist(G, w)` / `nyquist(G, "pos-only")` / `[re, im, w] = nyquist(G)`

Nyquist plot of $L(j\omega)$ in the complex plane — the canonical visual for closed-loop stability analysis. `G` is a `tf` (from `tf(...)`) or a `ss` (from `ss(...)` / `ss(A, B, C, D)`). Always plots; returns data as a tuple.

The plot shows:
- The positive-frequency locus and its conjugate mirror (closed contour by default; `"pos-only"` omits the mirror).
- A scatter marker at $-1 + 0j$ — encirclements of $-1$ count to the Nyquist criterion; the closest-approach distance is the sensitivity peak $1/M_S$.
- Equal aspect ratio so a unit circle around $-1$ reads as round (Kalman frequency-domain inequality $|1 + L(j\omega)| \geq 1$).

The default frequency grid uses the same auto-range heuristic as `bode(G)` (decades around the dominant pole), then refines near $-1$ in a two-pass densification so the closest-approach reading is clean. Pass an explicit `w` to override.

Returned arrays are the **positive-frequency** branch only.

```
G = tf([1], [1, 0.3, 1])     % lightly-damped second order
nyquist(G)                    % plot
[re, im, w] = nyquist(G)      % capture positive-frequency locus

% Loop transfer for an LQR design — verify the Kalman FDI graphically:
sys = ss(tf([10], [1, 2, 10]))
L   = tf(sys.A, sys.B, K, 0)
nyquist(L)                    % locus skirts the unit circle around -1
```

### `step(G)` / `step(G, t_end)` / `[y, t] = step(G)`

Unit step response. Always plots; returns data as a tuple.

```
G = tf([10], [1, 2, 10])
step(G)
[y, t] = step(G)       % capture
[y, t] = step(G, 5)    % specify final time (seconds)
```

Auto `t_end = 10 / min(|Re(poles)|)` capped at 100 s.

### `margin(G)` / `[Gm, Pm, Wcg, Wcp] = margin(G)`

Stability margins from the Bode plot.

- `Gm` — gain margin (linear ratio; `Inf` if no phase crossover)
- `Pm` — phase margin (degrees; `Inf` if no gain crossover)
- `Wcg` — phase crossover frequency, rad/s
- `Wcp` — gain crossover frequency, rad/s

```
G = tf([1], [1, 0.5, 1, 0])
[Gm, Pm, Wcg, Wcp] = margin(G)
fprintf("GM=%.1f dB  PM=%.1f deg\n", 20*log10(Gm), Pm)
```

### `[K, S, e] = lqr(sys, Q, R)`

Linear-Quadratic Regulator — solves the continuous-time algebraic Riccati equation (CARE).

- `sys` — StateSpace value from `ss()`
- `Q` — n×n state weighting matrix (positive semi-definite)
- `R` — m×m input weighting matrix (positive definite)
- `K` — m×n optimal gain matrix: u = −K·x
- `S` — n×n Riccati solution
- `e` — closed-loop eigenvalues of (A − B·K)

```
sys = ss(tf([1], [1, 0, 0]))   % double integrator
[K, S, e] = lqr(sys, eye(2), 1)
% all Re(e) < 0 → closed-loop stable
```

Algorithm: Hamiltonian matrix eigendecomposition.

### `rlocus(G)`

Root locus — plot closed-loop pole trajectories as loop gain K sweeps 0 → ∞.

Each coloured path shows where one open-loop pole migrates as K increases. Trajectories start at the open-loop poles (K = 0) and end at the finite zeros or at infinity (K → ∞).

```
s = tf("s")
G = 1 / (s * (s + 1))
rlocus(G)
```

The plot x-axis is the real part of the poles, y-axis is the imaginary part.

---

## S-Parameters (RF)

Read, build, and inspect RF S-parameter networks captured from VNAs as Touchstone files (`.s1p`, `.s2p`, `.s3p`, `.s4p`). Phase 1 covers the data type, file I/O, and basic accessors. Conversions, Smith-chart plotting, and analysis (VSWR, stability, gain circles) ship in later phases.

### `sparameters(filename)` / `sparameters(S, freqs)` / `sparameters(S, freqs, Z0)`

Construct an N-port S-parameter network.

```
% Read a 2-port Touchstone file written by a VNA:
s = sparameters("amp.s2p")

% Build from raw arrays — Tensor3 of shape [n_freqs, n_ports, n_ports]
% plus a real frequency vector in Hz.
S = zeros3(101, 2, 2)
f = linspace(1e9, 6e9, 101)
s = sparameters(S, f)             % default reference impedance Z0 = 50 Ω
s = sparameters(S, f, 75)         % explicit Z0 (Ω)
```

Returns a struct with these fields, indexable through normal `s.field` access:

| Field | Type | Meaning |
|---|---|---|
| `parameters` | Tensor3 | Complex S-parameters, shape `[n_freqs, n_ports, n_ports]`. `parameters(k, i, j)` is `S_{ij}` at the k-th frequency. |
| `frequencies` | real Vector | Length `n_freqs`, Hz, strictly increasing. |
| `num_ports` | Scalar | Port count. |
| `impedance` | Scalar | Reference impedance Z0 (Ω). |

Validation: the S array must be square in the port dimensions, the frequency vector must match `n_freqs` and be strictly increasing, and the reference impedance must be positive. The Touchstone v1.1 reader handles `.s1p` through `.s4p` with formats `RI` / `MA` / `DB` and frequency units Hz / kHz / MHz / GHz. The writer (via the standard `save(s, path)` interface in a later phase) emits the lossless `RI` form in Hz.

A printed sparameters value renders as a single-line summary:

```
sparameters: 2-port, 201 frequencies [1 GHz .. 6 GHz], Z0 = 50 Ω
```

### `nports(s)`

Port count of an S-parameter network.

```
s = sparameters("amp.s2p")
n = nports(s)                     % 2
```

### `freqs(s)`

The (real) frequency vector in Hz.

```
s = sparameters("amp.s2p")
f = freqs(s)
```

### `sij(s, i, j)`

Generic S-parameter slice — complex Vector of length `n_freqs` containing `S_{ij}(f)` at every frequency. Port indices are 1-based and must satisfy `1 ≤ i, j ≤ nports(s)`.

```
s   = sparameters("3port.s3p")
s31 = sij(s, 3, 1)                % port-3 reflection from port-1 drive
```

### `s11(s)` / `s12(s)` / `s21(s)` / `s22(s)`

Convenience accessors for the four 2-port S-parameter traces. Each returns a complex Vector of length `n_freqs`. `s11` works on any network with at least one port; `s12`, `s21`, `s22` require at least two.

```
s    = sparameters("amp.s2p")
db21 = mag2db(abs(s21(s)))        % insertion-loss / gain in dB vs frequency
```

### Parameter conversions

Convert between the common network-parameter representations. Each `xx2yy` builtin takes an `xx`-tagged sparameters network and returns a `yy`-tagged one with the same frequency vector and reference impedance. Calling a conversion on a wrongly-tagged input is a hard error (no silent guessing).

```
s   = sparameters("amp.s2p")      % S-tagged
z   = s2z(s)                      % Z-tagged; per-frequency Z = Z0·(I+S)(I−S)⁻¹
s2  = z2s(z)                      % round-trips back to S
y   = s2y(s)
abcd = s2abcd(s)                  % 2-port only
t   = s2t(s)                      % 2-port only (cascade-form)
```

| Function | Direction | Ports | Formula |
|---|---|---|---|
| `s2z(s)` / `z2s(z)` | S ↔ Z | N | `Z = Z0·(I+S)·(I−S)⁻¹`; `S = (Z−Z0·I)·(Z+Z0·I)⁻¹` |
| `s2y(s)` / `y2s(y)` | S ↔ Y | N | `Y = (1/Z0)·(I−S)·(I+S)⁻¹`; `S = (I−Z0·Y)·(I+Z0·Y)⁻¹` |
| `s2t(s)` / `t2s(t)` | S ↔ T | 2 | Pozar §4.4 (T multiplies under cascade) |
| `s2abcd(s)` / `abcd2s(a)` | S ↔ ABCD | 2 | Pozar Table 4.2 (voltage/current chain) |
| `parameter_type(s)` | inspection | any | Returns "S" / "Z" / "Y" / "T" / "ABCD" |

The Display label tracks the parameter type:

```
> z
sparameters: 2-port Z, 201 frequencies [10 MHz .. 6 GHz], Z0 = 50 Ω
```

The accessor names `s11`/`s12`/`s21`/`s22`/`sij` still slice the parameter tensor regardless of type — what they extract is whatever the current parameter set is (`Z11` if Z-tagged, `Y21` if Y-tagged, etc.). The Display tag tells you which.

### `cascade(s1, s2, ...)`

Cascade two or more 2-port S-parameter networks. Computed via T-parameter multiplication: convert each to T, multiply, convert back. All inputs must share the same frequency grid (strict — no auto-interpolation) and reference impedance.

```
att = sparameters("pad_10dB.s2p")
pair = cascade(att, att)              % 20 dB total insertion loss
chain = cascade(input_match, lna, output_filter)
```

### `deembed(meas, left, right)`

Remove known fixture networks on either side of a device under test. Computed via `T_DUT = T_left⁻¹ · T_meas · T_right⁻¹`. All three networks must be 2-port S-parameters on the same frequency grid and reference impedance.

```
meas = sparameters("dut_with_fixtures.s2p")
L    = sparameters("fixture_left.s2p")
R    = sparameters("fixture_right.s2p")
dut  = deembed(meas, L, R)
```

### `newref(s, Z_new)`

Renormalise an S-parameter network to a different reference impedance. The math goes through the Z-domain detour; the network's original Z0 is read from `s.impedance`. The returned network carries `Z_new` in its `impedance` field. Scalar `Z_new` only in Phase 2 — per-port renormalisation lands in Phase 6.

```
s50 = sparameters("amp.s2p")       % typically 50 Ω
s75 = newref(s50, 75)              % renormalise for a 75 Ω system
```

### Saving to Touchstone

`save(s, "out.s2p")` writes the network as Touchstone v1.1 (RI format, Hz frequency unit, 15 sig-fig precision so the round-trip is effectively lossless against f64). The path's `.sNp` extension is inspected to dispatch; the port-count digit doesn't have to match `nports(s)` literally (the writer always emits the actual port count), but the convention is to use it. The input must be S-typed — convert with `z2s`/`y2s`/etc. first if you started in another domain.

```
s   = sparameters("amp.s2p")
s75 = newref(s, 75)
save("amp_75ohm.s2p", s75)
```

### `smith(...)` and `marker(...)` — Smith chart plotting

Plot reflection coefficients on a Smith chart. The chart grid (constant-resistance circles and constant-reactance arcs, clipped to the unit disk) is generated automatically; data traces overlay it. The panel is locked to `axis("equal")` and to the unit-disk bounds so geometry stays round across every rendering backend.

```
s = sparameters("amp.s2p")
smith(s)                              % default: plot S11 and S22 on a Z-grid
smith(s, 2, 1)                        % a specific Sij port pair
smith(gamma_vec)                      % raw complex Vector of reflection coefficients
smith(0.5 + 0.3*j)                    % single-point trace (matching-network endpoint)
smith("amp.s2p")                      % convenience: smith() + sparameters() in one call
smith(s, "grid", "Y")                 % admittance grid (constant-G circles + constant-B arcs)
smith(s, "grid", "ZY")                % immittance overlay (both Z and Y grids)
```

Grid modes:

| Mode | Layout |
|---|---|
| `"Z"` (default) | Impedance grid — constant-R circles + constant-X arcs |
| `"Y"` | Admittance grid (mirror image of Z) |
| `"ZY"` | Both overlaid — useful for matching-network design |

`marker(gamma, label)` drops a labelled scatter point on the active Smith axes. The label appears in the legend; chart cardinal points are good things to mark:

```
marker(0,  "matched")    % chart centre — Γ = 0, perfect match
marker(-1, "short")      % left edge of real axis
marker(1,  "open")       % right edge of real axis
marker(gamma_mid, "λ/8 transmission line")
```

**Cross-backend behaviour.** The grid arcs are synthesised as ordinary dashed line series with empty labels — every rustlab plot backend (terminal, SVG/PNG via plotters, HTML via Plotly, LaTeX/PDF via the SVG path, animation GIF/HTML, live `rustlab-viewer`) renders them through its existing line-series path. Empty labels are suppressed from the legend (Plotly emits `showlegend: false`; SVG already keys legend inclusion on a non-empty label). There is no per-backend Smith-specific code, by design.

### `rfplot(...)` — magnitude / phase / group-delay vs frequency

Standard RF-engineering plots of S-parameter traces against frequency on a log-x axis. The default form takes a 2-port network and lays out the canonical 2×2 review panel; single-trace forms pull out one transformation of one port pair.

```
s = sparameters("amp.s2p")

% Standard 2x2 review panel: |S11| dB, |S21| dB, |S12| dB, |S22| dB
% (top-left, top-right, bottom-left, bottom-right). Log frequency axis.
rfplot(s)

% Single-trace forms — kind is one of magnitude, db, phase, unwrap, groupdelay.
rfplot(s, "db",          2, 1)      % S21 in dB
rfplot(s, "magnitude",   2, 1)      % |S21|, linear
rfplot(s, "phase",       2, 1)      % wrapped phase, degrees
rfplot(s, "unwrap",      2, 1)      % unwrapped phase, degrees
rfplot(s, "groupdelay",  2, 1)      % group delay τ_g = -dφ/dω, seconds
```

| Kind | Y-axis quantity |
|---|---|
| `"magnitude"` | `|Sij|` (linear) |
| `"db"` | `20·log10|Sij|`, floored at −200 dB |
| `"phase"` | `arg(Sij)` in degrees, wrapped to (−180, 180] |
| `"unwrap"` | Unwrapped `arg(Sij)` in degrees |
| `"groupdelay"` | `−dφ/dω` in seconds via central difference on the unwrapped phase |

Group delay uses a forward/backward difference at the endpoints and central differences in the interior. The unwrap rule is the standard ±2π jump removal applied to the running cumulative correction.

For non-2-port networks the default form falls back to a single `|S11|` dB trace; the single-trace form works for any port pair within range. The frequency axis always goes through `semilogx` so the x-coordinate is `log10(f)` — the canonical RF convention. Each panel gets the appropriate Y-axis label (`|Sij| (dB)`, `arg(Sij) (deg)`, etc.) automatically.

### Analysis: VSWR, return loss, stability, gain

The standard RF-analysis suite. All operate on an `sparameters` network and return per-frequency vectors; the circles helpers return tagged structs consumable by `smith_circle()` for Smith-chart overlay.

**Port-level metrics**

```
s   = sparameters("amp.s2p")
v1  = vswr(s, 1)                        % real Vector: VSWR at port 1
rl1 = return_loss(s, 1)                 % real Vector: -20·log10|S11|, dB
il  = insertion_loss(s, 2, 1)           % real Vector: -20·log10|S21|, dB
```

| Function | Returns | Definition |
|---|---|---|
| `vswr(s, port)` | real Vector | `(1+|Sii|)/(1−|Sii|)`; capped at 1e6 to keep plots finite |
| `return_loss(s, port)` | real Vector, dB | `−20·log10|Sii|`; floored at 200 dB |
| `insertion_loss(s, i, j)` | real Vector, dB | `−20·log10|Sij|`; floored at 200 dB |

**Reflection with termination** (2-port only)

```
gin  = gammain(s, 0.3 + 0.4*j)          % broadcast scalar across frequencies
gout = gammaout(s, gamma_source_vec)    % per-frequency vector matching n_freqs
```

| Function | Returns | Definition |
|---|---|---|
| `gammain(s, gamma_load)` | complex Vector | `S11 + S12·S21·ΓL / (1 − S22·ΓL)` |
| `gammaout(s, gamma_source)` | complex Vector | `S22 + S12·S21·ΓS / (1 − S11·ΓS)` |

**Stability** (2-port only)

```
K   = stabilityk(s)                     % Rollett K vs frequency
[m1, m2] = stabilitymu(s)               % single-number tests
```

Unconditionally stable iff `K > 1` AND `|Δ| < 1`, or equivalently iff `µ1 > 1` (equivalently `µ2 > 1`).

| Function | Returns | Definition |
|---|---|---|
| `stabilityk(s)` | real Vector | `(1 − |S11|² − |S22|² + |Δ|²) / (2·|S12·S21|)`; `Δ = S11·S22 − S12·S21` |
| `stabilitymu(s)` | tuple `(µ1, µ2)` of real Vectors | `µ1 = (1−|S11|²)/(|S22 − Δ·conj(S11)| + |S12·S21|)`; symmetric for µ2 |

**Simultaneous conjugate match** (2-port only — useful where K > 1)

```
gms = gammams(s)                        % source termination for max gain
gml = gammaml(s)                        % matched load termination
mag = gainmax(s)                        % maximum available / stable gain, dB
```

Defining property: `Γin(s, gml) = conj(Γms)` and `Γout(s, gms) = conj(Γml)`.

| Function | Returns | Definition |
|---|---|---|
| `gammams(s)` | complex Vector | Source Γ for simultaneous conjugate match |
| `gammaml(s)` | complex Vector | Load Γ for simultaneous conjugate match |
| `gainmax(s)` | real Vector, dB | `MAG = |S21/S12|·(K − √(K²−1))` when K > 1; `MSG = |S21/S12|` otherwise |

**Stability and gain circles** (2-port only — return tagged structs for Smith overlay)

```
in_circles = stability_circles(s, "input")
out_circles = stability_circles(s, "output")
g15_circles = gain_circles(s, 15)       % loci of ΓL giving 15 dB operating gain
```

Both return a struct with these fields:

| Field | Type | Meaning |
|---|---|---|
| `centres` | complex Vector | Circle centres, length n_freqs |
| `radii` | real Vector | Circle radii |
| `frequencies` | real Vector | Hz |
| `domain` | string | `"source"` (Γs plane) or `"load"` (ΓL plane) |

Overlay the circles on a Smith chart by iterating:

```
smith(s)
cs = in_circles.centres
rs = in_circles.radii
for k = 1:len(freqs(s))
  smith_circle(cs(k), real(rs(k)))
end
```

Note the Phase-2 parser gotcha: `in_circles.centres(k)` is parsed as a call to a function named `centres`, not as field access + index. Stash `in_circles.centres` into an intermediate variable first.

### `smith_circle(centre, radius [, label])`

Overlay one parametric circle on the active Smith axes. `centre` is a complex scalar; `radius` is a non-negative real. The optional label appears in the legend (empty/missing label keeps it out). Use this to render stability or gain circles, or any other matching-design construct that has a circular locus in the reflection plane.

### Phase 6 — polish

**`interp_freq(s, freqs_new)`** — linearly interpolate an S-parameter network onto a new frequency grid. Required before cascading two networks measured on different VNA sweeps, and before any builtin that needs uniform spacing (notably `s2td`). The new grid must be monotonically increasing and entirely within the source range; extrapolation is rejected because RF measurements are bandlimited and extrapolated values give worse answers than failing.

```
a = sparameters("amp.s2p")        % e.g. 1, 1.5, 2 GHz
b = sparameters("filter.s2p")     % e.g. 1, 2 GHz
b_unified = interp_freq(b, freqs(a))   % onto amp's grid
chain     = cascade(a, b_unified)
```

**Touchstone noise-parameter access** — many `.s2p` files include an optional noise block after the S-block. The reader picks it up automatically when present. Access the per-frequency noise data:

| Builtin | Returns | Meaning |
|---|---|---|
| `has_noise(s)` | Bool | True iff the network carries a noise block |
| `noise_freqs(s)` | real Vector, Hz | Noise-block frequency grid (need not match the S grid) |
| `nfmin(s)` | real Vector, dB | Minimum noise figure NFmin |
| `gamma_opt(s)` | complex Vector | Optimum source reflection for minimum noise |
| `rn(s)` | real Vector | Normalised equivalent noise resistance Rn/Z0 |

Guard noise-accessor calls with `has_noise(s)` when working with a mix of files.

**`s2td(s, i, j [, "impulse" | "step"])`** — time-domain conversion via IFFT. The frequency grid must be uniformly spaced (call `interp_freq` onto a uniform grid first if not). Default mode is `"step"` (TDR convention). Returns `[t, y]` where `t` is in seconds (length 2N) and `y` is real.

```
s = sparameters("cable.s2p")
[t, step] = s2td(s, 2, 1)              % step response of S21
[t, imp]  = s2td(s, 2, 1, "impulse")
```

The result is the **baseband-equivalent response** — no DC extrapolation is performed. For a spectrum starting at f₀ > 0 (the usual VNA case), the time-domain signal carries the band-limited-pulse oscillation that's standard for VNA-derived TDR.

**Mixed-mode 4-port conversion** — for high-speed differential designs where ports 1+3 and 2+4 form differential pairs.

```
s_se = sparameters("diffpair_4port.s4p")    % single-ended
s_mm = s2smm(s_se)                          % mixed-mode (d1, d2, c1, c2)
s_back = smm2s(s_mm)                        % round-trips to single-ended
```

| Builtin | Direction | Result tag |
|---|---|---|
| `s2smm(s)` | single-ended → mixed-mode | `"Smm"` |
| `smm2s(smm)` | mixed-mode → single-ended | `"S"` |

The mixed-mode network is organised as the block matrix `[Sdd Sdc; Scd Scc]` with ports `[d1, d2, c1, c2]`. Port pairing convention: port 1 (positive) and port 3 (negative) form differential pair 1; ports 2/4 form differential pair 2 — the universal convention every commercial mixed-mode-capable VNA uses.

**Touchstone v2 tolerance** — `.s2p` files with `[Version] 2.0` and v1-compatible layouts now parse cleanly. Recognised keyword lines (consumed and ignored unless they affect the data):

- `[Version] 2.0` — informational, accepted
- `[Number of Ports]`, `[Number of Frequencies]`, `[Number of Noise Frequencies]` — informational
- `[Two-Port Data Order]`, `[Matrix Format]` — accepted when their value matches the v1-style default
- `[Network Data]`, `[Noise Data]`, `[End]`, `[Begin Information]`, `[End Information]` — accepted
- `[Reference] <z0>` — single-scalar form overrides the `# R <z0>` header default

Still rejected (with a clear error): per-port `[Reference]` lists, `[Mixed-Mode-Order]` tables. For files that use those features, use a single-ended export from the VNA and apply `s2smm` after loading.

---

## Language

### `print(a [, b, ...])`
Print one or more values to stdout, space-separated.
```
print(x)
print("mean:", mean(v), "std:", std(v))
```

### `disp(x)`
Display a value followed by a newline. Similar to `print` but always appends a newline and takes exactly one argument.
```
disp("Hello, world!")
disp(A)
```

### `fprintf(fmt, args...)`
Formatted print. Supports C-style format specifiers: `%d`, `%f`, `%g`, `%e`, `%s`, `%%`. Flags: `-`, `+`, `0`, `#`, `,` (comma inserts thousands separators). Escape sequences: `\n`, `\t`.
```
fprintf("x = %f, n = %d\n", 3.14, 42)
fprintf("GM=%.1f dB  PM=%.1f deg\n", 20*log10(Gm), Pm)
fprintf("population: %,d\n", 1234567)       % → population: 1,234,567
fprintf("price: $%,.2f\n", 1234567.89)      % → price: $1,234,567.89
```
- Does not append a trailing newline unless `\n` is included in the format string.

### `sprintf(fmt, args...)`
Same format specifiers and flags as `fprintf`, but returns the formatted string instead of printing it.
```
s = sprintf("%,.2f", 1234567.89)    % → "1,234,567.89"
s = sprintf("%d items", 42)         % → "42 items"
```

### `commas(x)` / `commas(x, precision)`
Format a number with thousands-separator commas. Returns a string.
```
commas(1234567)         % → "1,234,567"
commas(1234567.89)      % → "1,234,567.89"
commas(1234567.89, 2)   % → "1,234,567.89"
commas(-9876543)        % → "-9,876,543"
```
- With one argument: integers display without decimals, floats use default precision.
- With two arguments: the second specifies the number of decimal places.

### `format` command
Set the global display format mode. Affects how numeric values are auto-printed.
```
format commas       % enable thousands separators in all output
format default      % restore normal display
format              % show current mode
```
Example:
```
format commas
x = 1234567         % → x = 1,234,567
format default
x                   % → 1234567
```

### Underscore digit separators
Underscores can be used inside numeric literals for readability. They are stripped during parsing and have no effect on the value. Works like Rust, Python, and C++14.
```
x = 1_000_000           % → 1000000
fs = 48_000              % → 48000
y = 3.141_592_653        % → 3.141592653
z = 1_234.567_89         % → 1234.56789
v = [1_000, 2_000]       % works in vectors
```

### Range operator: `start:stop` / `start:step:stop`
```
1:5          # [1, 2, 3, 4, 5]
0:0.5:2      # [0.0, 0.5, 1.0, 1.5, 2.0]
10:-1:1      # [10, 9, 8, ..., 1]
```

### Indexing (1-based): `v(i)` / `v(start:stop)`
```
v(1)       # first element
v(end)     # last element
v(2:4)     # elements 2, 3, 4
```

### Indexed assignment: `v(i) = val` / `M(r,c) = val`
Assign to a specific position. Vectors are auto-created and grown as needed.
```
v(3) = 99         # create or update element 3
M(2, 1) = 0.5    # update matrix element (row 2, col 1)
```
Inside a loop:
```
for i = 1:5
  x(i) = i ^ 2
end
# x is now [1, 4, 9, 16, 25]
```

### Chained call-and-index: `f(args)(i)`
Index the return value of a function call without a temporary variable.
```
v = linspace(0, 1, 10)(3)   # third element of the range
loss = gd_step(w, b, x, y)(3)
```

### `for` loop
Iterate over a range or vector.
```
for VAR = start:stop
  body
end

for VAR = start:step:stop
  body
end

for VAR = some_vector
  body
end
```
Example:
```
s = 0
for i = 1:10
  s = s + i
end
# s = 55

for i = 1:n
  result(i) = my_fn(data(i))
end
```
- The loop variable remains in scope after `end`.
- Use `for i = n:-1:1` to iterate in reverse.

### `while` loop
Repeat a block while a condition is truthy. The condition is re-evaluated at the top of each iteration.
```
while cond
  body
end
```
The condition can be a `Bool`, a `Scalar` (non-zero = true), or a `Complex` (non-zero real or imaginary part = true).

**Examples:**
```
# count down to zero
n = 5
while n > 0
  print(n)
  n = n - 1
end

# infinite loop (typical for streaming pipelines — exits via audio EOF)
while true
  frame = audio_read(src)
  audio_write(dst, frame)
end
```
- Use `while true` for event loops or streaming pipelines. The loop exits when `audio_read` encounters EOF, which is propagated as a clean exit (exit code 0) by `rustlab run`.
- `true` and `false` are pre-defined Boolean constants.

#### No `break` or `continue` — by design

Rustlab deliberately omits `break` and `continue`. Both keywords are rejected as unstructured control flow; the structured form is to lift the exit condition into the `while` header.

For "find the first index where some condition holds":
```
i = 2;
N = length(w);
i_cross = 0;
while i <= N && i_cross == 0
    if real(mag(i-1)) >= 0 && real(mag(i)) < 0
        i_cross = i;
    end
    i = i + 1;
end
```
This stops at the crossing (same early-exit benefit `break` would give) and keeps the exit condition in the loop header where loop invariants belong, instead of buried mid-body.

For "skip iterations matching some predicate," invert the condition:
```
for i = 1:N
    if mod(i,2) ~= 0
        do_thing(i);
    end
end
```
`continue` would save one indent level and nothing else.

See `dev/requests/break-continue.md` for the full rejection rationale.

### `if` / `elseif` / `else`
Conditional branching with optional chained conditions.
```
if cond
  body
elseif cond2
  body2
else
  default_body
end
```
Single-line form using comma as separator:
```
if x > 5, x = 99; end
if income > b2, tax = tax + (income - b2) * r3; income = b2; end
```
The condition can be a `Bool` or `Scalar` (0 = false, nonzero = true).

### `switch` / `case` / `otherwise`
Match a value against one or more cases.
```
switch quarter
    case 1
        multiplier = 4.0
    case 2
        multiplier = 2.4
    otherwise
        error('Invalid quarter')
end
```
Executes the first matching case. Falls through to `otherwise` if no case matches.

### `error(msg)`
Halt script execution with an error message.
```
error('Invalid input')
```

### `sleep(seconds)`
Pause execution for the given duration in seconds. Accepts a non-negative scalar; fractional seconds are supported. Useful for pacing real-time control loops and animations.
```
sleep(0.01)    # pause for 10 ms
sleep(1.5)     # pause for 1.5 seconds
```

### `clear`
Remove all user-defined variables and functions from the workspace. Built-in constants (`j`, `pi`, `e`, etc.) are preserved. No parentheses needed.
```
clear
```

### `clf`
Clear the current figure — resets all subplot series, titles, and labels. No parentheses needed.
```
clf
```

Typical usage at the top of a script:
```
clear; clf;
```

### `run` (script include)
Execute another `.rlab` file, merging its variables and function definitions into the current scope. Works in both the REPL and inside scripts.
```
run helper_functions.rlab
result = my_helper(42)
```

### Line continuation: `...`
Continue a long expression on the next line.
```
y = a + b + ...
    c + d
```
Everything after `...` on the line is ignored (treated as a comment).

### Element-wise operators: `.* ./ .^`
```
a .* b     # element-wise multiply
a ./ b     # element-wise divide
a .^ 2     # element-wise square
```

### Concatenation: `[a, b, c]`
```
c = [1:4, 5:8]   # [1, 2, 3, 4, 5, 6, 7, 8]
```

### Conjugate transpose: `v'`
```
col = row'
```

### Comments: `#`
```
# This is a comment
x = 1.0   # inline comment
```

### Compound assignment: `+=`, `-=`, `*=`, `/=`
Shorthand for updating a variable in place.
```
x = 10
x += 5    # x is now 15
x -= 3    # x is now 12
x *= 2    # x is now 24
x /= 4   # x is now 6
```

### Suppress output: `;`
```
h = fir_lowpass(64, 1000.0, 44100.0, "hann");   # no output printed
```

---

## Structs

Structs are key-value containers with named fields, useful for grouping related data.

### `struct("field1", val1, "field2", val2, ...)`
Create a struct from field-value pairs. Requires an even number of arguments.
```
s = struct("x", 1, "y", 2, "name", "origin")
s.x      # → 1
s.name   # → "origin"
```

### Field access and assignment: `s.field` / `s.field = val`
Access or set a field. Setting a field on an undefined variable auto-creates a struct.
```
s.z = 3           # add new field
s.x = 10          # update existing field
pt.x = 1; pt.y = 2   # auto-creates struct pt
```

### `isstruct(x)`
Returns `true` if `x` is a struct, `false` otherwise.
```
isstruct(s)     # → true
isstruct(42)    # → false
```

### `fieldnames(s)`
Prints all field names of a struct (sorted alphabetically). Returns `None`.
```
fieldnames(s)   # prints: name, x, y, z
```

### `isfield(s, "name")`
Returns `true` if the struct has the named field.
```
isfield(s, "x")     # → true
isfield(s, "w")     # → false
```

### `rmfield(s, "name")`
Returns a new struct with the named field removed. Errors if the field does not exist.
```
s2 = rmfield(s, "z")
```

---

## Cell Arrays (String Arrays)

String arrays hold ordered collections of strings and are created with brace syntax.

### `{"a", "b", "c"}`
String array literal. All elements must be strings (single- or double-quoted). Creates a `StringArray` value.
```
labels = {"Jan", "Feb", "Mar"}
colors = {'red', 'green', 'blue'}
```

### Indexing: `sa(i)` / `sa(2:4)` / `sa(:)`
1-based indexing into string arrays. Scalar index returns a string; slice or `:` returns a new string array. `end` is supported.
```
labels = {"a", "b", "c", "d"}
labels(2)         # → "b"
labels(end)       # → "d"
labels(1:3)       # → {"a", "b", "c"}
```

### `iscell(x)`
Returns `true` if `x` is a string array, `false` otherwise.
```
labels = {"a", "b"}
iscell(labels)    # → true
iscell([1, 2])    # → false
```

### `length(sa)` / `numel(sa)` / `size(sa)`
Standard size functions work on string arrays:
- `length(sa)` — number of elements
- `numel(sa)` — same as `length`
- `size(sa)` — returns `[1, n]`

### Categorical bar charts: `bar(labels, y)` / `bar(labels, y, title)`
When the first argument to `bar` is a string array, it becomes the x-axis category labels:
```
bar({"Jan", "Feb", "Mar"}, [10, 20, 30])
bar({"A", "B", "C"}, [5, 8, 3], "Results")
```
Categorical labels appear on the x-axis in terminal, HTML (Plotly), and file (PNG/SVG) output.

---

## Higher-Order Functions

### `arrayfun(f, v)`
Apply a callable (lambda, function handle, or user function) to each element of a vector. Returns a vector if all results are scalar, or a matrix if all results are vectors of equal length.
```
arrayfun(@(x) x^2, 1:5)          # → [1, 4, 9, 16, 25]
arrayfun(@abs, [-3, 4, -5])      # → [3, 4, 5]
```

### `feval("name", args...)`
Call a function by string name. Useful for dynamic dispatch.
```
feval("sin", pi/2)    # → 1.0
feval("fir_lowpass", 32, 1000.0, 44100.0, "hann")
```

### `rk4(f, x0, t)`
Fixed-step 4th-order Runge-Kutta ODE integrator. Solves dx/dt = f(x, t) from `t(1)` to `t(end)`.

| Argument | Type | Description |
|---|---|---|
| `f` | callable | Dynamics function `f(x, t)` → x_dot; accepts lambda, handle, or user function |
| `x0` | scalar or vector | Initial state |
| `t` | vector | Time points (at least 2); step size = `t(i+1) - t(i)` |

Returns an `nx × nt` matrix where each column is the state at the corresponding time point.
```
# Scalar ODE: dx/dt = -x
X = rk4(@(x, t) -x, 1.0, linspace(0, 5, 100))

# 2D system: harmonic oscillator
f = @(x, t) [x(2); -x(1)]
X = rk4(f, [1; 0], linspace(0, 10, 200))
```

---

## Profiling

### `profile(fn1, fn2, ...)`
Enable call profiling for the named functions. Call with no arguments to track all functions. Function names can be bare identifiers or strings.
```
profile(fft, convolve)    # track only fft and convolve
profile()                 # track all function calls
```

### `profile_report()`
Print a profiling summary table to stderr showing call counts, total time, and data throughput for each tracked function.
```
profile(fft, convolve)
# ... run workload ...
profile_report()
```

---

## Controls Toolbox — Advanced

These functions complement the core control systems toolbox (tf, ss, bode, step, etc.) with advanced analysis and design tools.

### `lyap(A, Q)`
Solve the continuous Lyapunov equation `A*X + X*A' + Q = 0` for X.
```
A = [0, 1; -2, -3]
Q = eye(2)
X = lyap(A, Q)
```
- `A` and `Q` must be square matrices of the same size.
- Uses the Kronecker product / vectorization approach.

### `gram(A, B, type)`
Controllability or observability Gramian.
```
Wc = gram(A, B, "c")   # controllability Gramian
Wo = gram(A, C, "o")    # observability Gramian
```
- `type` must be `"c"` (controllability) or `"o"` (observability).
- Solves the corresponding Lyapunov equation internally.

### `care(A, B, Q, R)`
Solve the Continuous Algebraic Riccati Equation: `A'P + PA - PBR⁻¹B'P + Q = 0`.
```
P = care(A, B, Q, R)
```
- Returns the stabilizing solution P.
- Used internally by `lqr`.

### `dare(A, B, Q, R)`
Solve the Discrete Algebraic Riccati Equation: `P = A'PA - A'PB(R + B'PB)⁻¹B'PA + Q`.
```
P = dare(A, B, Q, R)
```
- Uses value iteration (up to 1000 iterations, tolerance 1e-12).

### `place(A, B, poles)`
Pole placement via Ackermann's formula (SISO systems only). Returns the state feedback gain vector K such that `eig(A - B*K)` matches the desired poles.
```
A = [0, 1; 0, 0]
B = [0; 1]
K = place(A, B, [-1, -2])
```
- `B` must be n×1 (single input).
- `poles` must have length n (one per state).

### `freqresp(A, B, C, D, w)`
Frequency response from state-space matrices at each frequency in `w` (rad/s). Computes `H(jω) = C(jωI − A)⁻¹B + D`.
```
w = logspace(-1, 2, 200)
H = freqresp(A, B, C, D, w)
```
- Returns a `p × length(w)` matrix of complex frequency response values (p = number of outputs).

---

## Streaming DSP

Real-time, frame-by-frame FIR filtering via stdin/stdout raw PCM. Rustlab acts as a pure stream processor — any byte source (microphone bridge, network socket, file) can feed it.

**Architecture:** `producer | rustlab run filter.rlab | consumer`

The streaming pipeline is stateless from the script's perspective: `state_init` allocates a history buffer, `filter_stream` advances it by one frame and returns the updated state, and `audio_read`/`audio_write` handle stdin/stdout I/O as raw f32 little-endian PCM.

### `state_init(n)`
Allocate a zero-filled overlap-save history buffer of length `n` (typically `length(h) - 1`).
```
h     = firpm(64, [0.0, 0.2, 0.3, 1.0], [1.0, 1.0, 0.0, 0.0])
state = state_init(length(h) - 1)
```
Returns a `FirState` handle (an `Arc<Mutex<Vec<C64>>>` internally). The same handle is returned by `filter_stream` after each frame — no heap allocation per frame.

### `filter_stream(frame, h, state)`
Filter one frame through FIR coefficients `h` using the overlap-save algorithm, using and updating the history in `state`.

| Argument | Type | Description |
|---|---|---|
| `frame` | Vector | Input frame (any length; typically the FRAME from `audio_read`) |
| `h` | Vector | FIR coefficients (M taps) |
| `state` | FirState | History buffer of length M−1 (from `state_init` or previous call) |

Returns a **Tuple** `[y, new_state]` where `y` is the filtered output frame and `new_state` is the updated `FirState` handle (same Arc pointer — no copy).

**How overlap-save works:**
1. Prepend M−1 history samples to the frame → extended buffer of length `len(x) + M - 1`
2. Direct-form convolution with `h` to produce `len(x)` output samples
3. Update history to the last M−1 samples of the input frame

Output at position `i` exactly equals `sum(h[k] * extended[i + M - 1 - k])` — identical to a full offline `convolve` on the concatenated input, frame boundaries are invisible.

```
h     = firpm(64, [0.0, 0.2, 0.3, 1.0], [1.0, 1.0, 0.0, 0.0])
state = state_init(length(h) - 1)
src   = audio_in(44100.0, 256)
dst   = audio_out(44100.0, 256)
while true
  frame = audio_read(src)
  [y, state] = filter_stream(frame, h, state)
  audio_write(dst, y)
end
```

**Correctness guarantee:** If you concatenate all input frames and run `convolve(full_input, h)`, the result matches the concatenation of all output frames to within numerical precision (tested to < 1e-9 in the test suite).

---

## Audio I/O

Raw PCM streaming over stdin / stdout. Each sample is a **32-bit IEEE 754 float, little-endian** (`f32 LE`). Audio is **mono** — use bridge programs (sox, ffmpeg, arecord/aplay) to convert from hardware to this format.

### `audio_in(sample_rate, frame_size)`
Create an `AudioIn` metadata descriptor. Does not open any file or device.

```
src = audio_in(44100.0, 256)
```

- `sample_rate` — Hz (informational; used for documentation and future extensions)
- `frame_size` — number of f32 samples per `audio_read` call

### `audio_out(sample_rate, frame_size)`
Create an `AudioOut` metadata descriptor. Does not open any file or device.

```
dst = audio_out(44100.0, 256)
```

### `audio_read(src)`
Read exactly one frame of `frame_size` f32 LE samples from stdin. Blocks until the full frame is available.

```
frame = audio_read(src)   # returns a complex Vector of length FRAME
```

- Returns a complex `Vector` (imaginary parts are 0.0).
- When stdin closes cleanly mid-frame (source finished), raises `AudioEof` which `rustlab run` maps to exit code 0, no error message. This is the normal "pipeline finished" signal.

### `audio_write(dst, frame)`
Write one frame of complex samples to stdout as f32 LE. Flushes after every frame.

```
audio_write(dst, y)   # y is a complex Vector; real parts are written
```

- Only the real parts of `frame` are written. Imaginary parts are discarded (FIR output is always real for a real-coefficient filter and real input).
- Flushes stdout after each frame to minimize pipeline latency.

---

### Bridge programs

Rustlab has no audio hardware support by design. Use an external bridge to connect hardware:

| Platform | Capture | Playback |
|---|---|---|
| macOS | `sox -d -r 44100 -c 1 -b 32 -e float -t raw -` | `sox -r 44100 -c 1 -b 32 -e float -t raw - -d` |
| Linux | `arecord -r 44100 -c 1 -f FLOAT_LE -t raw` | `aplay -r 44100 -c 1 -f FLOAT_LE -t raw` |
| Any | `ffmpeg -f avfoundation -i :0 -f f32le -ar 44100 -ac 1 pipe:1` | `ffmpeg -f f32le -ar 44100 -ac 1 -i pipe:0 -f alsa default` |

**Full macOS pipeline:**
```sh
sox -d -r 44100 -c 1 -b 32 -e float -t raw - \
  | rustlab run filter.rlab \
  | sox -r 44100 -c 1 -b 32 -e float -t raw - -d
```

**TCP network DSP node (any platform):**
```sh
# Terminal 1: start rustlab as a server on two ports
nc -l 9999 | rustlab run filter.rlab | nc -l 9998

# Terminal 2: send audio in
cat /tmp/audio.raw | nc localhost 9999

# Terminal 3: receive filtered audio
nc localhost 9998 > /tmp/filtered.raw
```

See `examples/audio/` for ready-to-run scripts for macOS, Linux, WSL2, and TCP streaming, plus a hardware-free integration test (`test_filter.sh`).

---

## Live Plotting

`figure_live`, `plot_update`, `plot_labels`, `plot_limits`, `figure_draw`, `figure_close`, and `mag2db` provide real-time visualization that stays open across multiple draw calls — suitable for oscilloscopes, spectrum monitors, and animated simulations. When the `viewer` feature is enabled and `rustlab-viewer` is running, `figure_live()` automatically connects to the viewer for egui rendering with zoom/pan.

### `figure_live(rows, cols)`

```
fig = figure_live(rows, cols)
```

Opens the ratatui alternate screen in raw mode and initialises a `rows × cols` grid of subplot panels. Returns a `live_figure` handle. Errors with a runtime message if stdout is not a real terminal (e.g. in CI or when piped).

### `plot_update(fig, panel, y)` / `plot_update(fig, panel, x, y)`

```
plot_update(fig, panel, y)       # x-axis auto-generated (1, 2, ..., N)
plot_update(fig, panel, x, y)    # explicit x-axis
```

Replaces the data in the given 1-based panel without redrawing. Call `figure_draw` after updating all panels for a single atomic screen refresh per loop iteration — this avoids partial-state flicker.

### `plot_labels(fig, panel, title, xlabel, ylabel)`

```
plot_labels(fig, panel, title, xlabel, ylabel)
```

Set the title and axis labels for a live figure panel (1-based). Labels persist across redraws — typically set once after `figure_live()`.

### `plot_limits(fig, panel, xlim, ylim)`

```
plot_limits(fig, panel, [x0, x1], [y0, y1])
```

Set fixed axis limits for a live figure panel (1-based). Pass `[lo, hi]` vectors.

### `figure_draw(fig)`

```
figure_draw(fig)
```

Flushes all panel data to the terminal in one draw call. Returns immediately (no keypress wait).

### `figure_close(fig)`

```
figure_close(fig)
```

Drops the `LiveFigure`, restoring raw mode and leaving the alternate screen. This fires automatically when the script ends or the process is interrupted (Ctrl-C) — `figure_close` is only needed when the script wants to return to the normal terminal mid-execution.

### `mag2db(X)`

```
db = mag2db(X)
```

Converts magnitude to dB: `20 · log10(|X|)`, element-wise. Applies a 1e-10 floor so silence maps to −200 dB rather than −∞.

**Example — real-time spectrum monitor:**

```r
sr       = 44100.0;
fft_size = 1024;
half     = fft_size / 2;

h   = window(fft_size, "hann");
adc = audio_in(sr, fft_size);
fig = figure_live(2, 1);

while true
    frame = audio_read(adc);
    X     = fft(frame .* h);
    freqs = fftfreq(fft_size, sr);

    plot_update(fig, 1, frame);
    plot_update(fig, 2, freqs(1:half), mag2db(X(1:half)));
    figure_draw(fig);
end
```

See `examples/audio/spectrum_monitor.rlab` for the full annotated script.

---

## REPL Commands

These are interactive commands available in the `rustlab` REPL only (not in script files).

| Command | Description |
|---------|-------------|
| `whos` | List all variables with type, size, and value preview |
| `clear` | Remove all user-defined variables (keeps `j`, `pi`, `e`) |
| `run <file>` | Execute a `.rlab` script; its variables persist in the session. File-relative paths (`savefig`, `save`, `load`) resolve relative to the script's own directory. |
| `ls [path]` | List directory contents |
| `cd [path]` | Change working directory |
| `pwd` | Print current working directory |
| `help` or `?` | Show help. `? <name>` for detail on a specific function |
