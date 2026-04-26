# rustlab Quick Reference

Concise cheat sheet for the rustlab scripting language. Full signatures and examples: `docs/functions.md`.

Run a script: `rustlab run script.r` — Interactive REPL: `rustlab`

> **For AI agents:** This file is the canonical capability index. Check it to know what functions exist before generating code. It is kept in sync with the actual builtins; if a function is not listed here, it is not implemented.

---

## Language

| Syntax | Description |
|---|---|
| `j`, `i` | Imaginary unit; complex literal: `z = 3.0 + j*4.0` |
| `pi`, `e` | Built-in constants |
| `Inf`, `NaN` | IEEE infinity and Not-a-Number |
| `true`, `false` | Boolean constants — usable in `if` and `while` conditions |
| `v(1)`, `v(end)`, `v(2:4)` | 1-based indexing; `end` = last element; slice returns Vector |
| `s(3)`, `s(1:5)`, `s(:)` | String indexing — 1-based; returns string |
| `v(i) = val`, `M(r,c) = val` | Indexed assignment; vectors auto-grow as needed |
| `f(args)(i)` | Chain call and index without a temporary variable |
| `[a; b; c]` | Column vector literal |
| `[a, b; c, d]` | Matrix literal — `,` same row, `;` new row |
| `[A, B]` / `[A; B]` | Horizontal / vertical concatenation |
| `[X, Y] = f(...)` | Destructuring assignment |
| `1:5`, `0:0.5:2`, `10:-1:1` | Range: `start:stop` or `start:step:stop` |
| `.*`, `./`, `.^` | Element-wise multiply, divide, power |
| `*` | Matrix multiply |
| `'` | Conjugate transpose |
| `.'` | Non-conjugate transpose |
| `+=`, `-=`, `*=`, `/=` | Compound assignment: `x += 1` is equivalent to `x = x + 1` |
| `1_000_000` | Underscore digit separators in numeric literals (ignored by parser) |
| `format commas` | Enable thousands-separator commas in all numeric output |
| `format default` | Restore normal numeric display |
| `;` | Suppress output on a statement |
| `#` / `%` | Comment |
| `...` | Line continuation — rest of line ignored, statement continues on next line |
| `run file.r` | Execute a script file; merges variables and functions into current scope |
| `error('msg')` | Halt execution with an error message |
| `clear` | Remove all user variables and functions; keeps built-in constants |
| `clf` | Clear current figure (reset subplots, series, labels) |
| `for i = 1:n` … `end` | For loop; also iterates over a vector |
| `while cond` … `end` | While loop; condition is Bool, Scalar (nonzero), or Complex |
| `if expr` … `elseif expr` … `else` … `end` | Conditional; `elseif` and `else` are optional; single-line: `if cond, body; end` |
| `switch expr` … `case val` … `otherwise` … `end` | Match value against cases; first match wins; `otherwise` is default |
| `function [out] = name(args)` … `end` | User-defined function |
| `return` | Early return from a function |
| `@(x, y) expr` | Anonymous function (lambda); captures current env by snapshot |
| `@name` | Function handle — reference to a builtin or user function |
| `arrayfun(f, v)` | Apply callable to each element; scalar results → Vector, vector results → Matrix |
| `feval("name", args...)` | Call function by string name |
| `profile(fn1, fn2)` | Enable call profiling for named functions; `profile()` tracks all |
| `profile_report()` | Print profiling table to stderr immediately |
| `logspace(a, b, n)` | n log-spaced points from 10^a to 10^b |
| `rk4(f, x0, t)` | Fixed-step 4th-order Runge-Kutta; f(x,t)→x_dot |
| `lyap(A, Q)` | Solve Lyapunov equation A*X + X*A' + Q = 0 |
| `gram(A, B, "c"/"o")` | Controllability or observability Gramian |
| `care(A, B, Q, R)` | Continuous Algebraic Riccati Equation → P |
| `dare(A, B, Q, R)` | Discrete Algebraic Riccati Equation → P |
| `place(A, B, poles)` | Ackermann pole placement (SISO) → K |
| `freqresp(A, B, C, D, w)` | H(jω) from state-space at each frequency ω |
| `svd(A)` | Jacobi SVD → Tuple [U, sigma_vector, V] |
| `{"a", "b", "c"}` | String array literal (all elements must be strings) |
| `sa(i)` | String array indexing (1-based); `end` supported |
| `s.field` | Struct field access |
| `s.field = val` | Struct field assignment (auto-creates struct) |

---

## Math (all element-wise)

| Function | Description |
|---|---|
| `exp(v)` | $e^v$ |
| `sqrt(v)` | Square root |
| `abs(v)` | Absolute value / modulus |
| `log(v)` | Natural logarithm |
| `log10(v)`, `log2(v)` | Base-10 and base-2 logarithms |
| `sin(v)`, `cos(v)` | Trig (radians) |
| `asin(v)`, `acos(v)`, `atan(v)` | Inverse trig |
| `atan2(y, x)` | Four-quadrant arctangent |
| `tanh(v)`, `sinh(v)`, `cosh(v)` | Hyperbolic trig |
| `floor(v)`, `ceil(v)`, `round(v)` | Rounding (applied to real and imaginary parts independently) |
| `sign(v)` | −1/0/+1 for real; `z/\|z\|` for complex |
| `mod(v, m)` | Modulo: `v − m·floor(v/m)` (m must be a real scalar) |
| `real(v)`, `imag(v)` | Real and imaginary parts |
| `conj(v)` | Complex conjugate — negates imaginary part |
| `angle(v)` | Phase = atan2(Im, Re), element-wise |

---

## Array Construction & Inspection

| Function | Description |
|---|---|
| `linspace(a, b, n)` | n evenly-spaced points from a to b |
| `zeros(n)` / `zeros(m, n)` / `zeros([m, n])` | Zero vector or matrix; accepts `size()` output |
| `ones(n)` / `ones(m, n)` / `ones([m, n])` | Ones vector or matrix; accepts `size()` output |
| `eye(n)` | n×n identity matrix |
| `rand(n)` | n floats uniform [0, 1) |
| `randn(n)` / `randn(m, n)` | n floats (or m×n matrix) from N(0,1) |
| `randi(imax)` / `randi(imax, n)` / `randi([lo,hi], n)` | Random integers |
| `len(v)` / `length(v)` | Number of elements |
| `size(v)` | `[rows, cols]` as a Vector |
| `numel(v)` | Total element count |
| `diag(v)` | Diagonal matrix from vector; or extract diagonal |
| `reshape(M, r, c)` / `reshape(M, r, c, p)` | Reshape to r×c (Matrix) or r×c×p (Tensor3); column-major walk |
| `repmat(M, r, c)` | Tile M r×c times |
| `transpose(M)` | Non-conjugate transpose |
| `horzcat(A, B, ...)` | Horizontal concatenation (also `[A, B]`) |
| `vertcat(A, B, ...)` | Vertical concatenation (also `[A; B]`) |
| `meshgrid(x, y)` | Returns `[X, Y]` matrices for 2D grids |
| `[Fx,Fy] = gradient(F[, dx, dy])` | 2-D gradient (rows index y, cols index x); 2nd-order interior + boundary |
| `divergence(Fx, Fy[, dx, dy])` | 2-D divergence ∂Fx/∂x + ∂Fy/∂y |
| `curl(Fx, Fy[, dx, dy])` | 2-D scalar curl ∂Fy/∂x − ∂Fx/∂y (z-component of ∇×F) |

---

## Geometry / Masks

Returns a real-valued matrix the same shape as the meshgrid `X` / `Y` inputs, with `1.0` inside the shape and `0.0` outside. Compose with element-wise math: `M1 .* M2` (intersection), `1 - M` (complement), `max(M1, M2)` (union).

| Function | Description |
|---|---|
| `rect_mask(X, Y, x0, y0, w, h)` | Axis-aligned rectangle mask, inclusive on all four sides |
| `disk_mask(X, Y, xc, yc, r)` | Closed-disk mask `(X-xc)² + (Y-yc)² ≤ r²` |
| `polygon_mask(X, Y, verts)` | Polygon mask via even-odd ray casting; `verts` is N×2 |

---

## Tensor3 (rank-3)

A `Tensor3` is a complex `(m, n, p)` array — `m` rows, `n` columns, `p` pages. 1-based indexing on every axis. No broadcasting between Matrix and Tensor3, and no `*`/`/` between two Tensor3s — use `.*` / `./`.

| Function | Description |
|---|---|
| `zeros3(m, n, p)` / `zeros3([m,n,p])` | Rank-3 complex zero tensor; bracket form accepts `size()` output |
| `ones3(m, n, p)` | Rank-3 complex ones tensor |
| `rand3(m, n, p)` | Tensor3 of U[0, 1) samples |
| `randn3(m, n, p)` | Tensor3 of N(0, 1) samples |
| `reshape(A, m, n, p)` | Reshape Vector / Matrix / Tensor3 → Tensor3 (column-major) |
| `cat(3, A, B, ...)` | Concatenate matrices along the page axis (`cat(1,...)` rows, `cat(2,...)` cols) |
| `permute(A, [d1, d2, d3])` | Reorder axes; `[d1, d2, d3]` is a permutation of `[1, 2, 3]` |
| `squeeze(A)` | Drop singleton dims → Matrix / Vector / Scalar; non-Tensor3 inputs pass through |
| `size(A)` / `size(A, 3)` | `[m, n, p]`; `size(A, 3)` is valid only for Tensor3 |
| `ndims(A)` | `3` for Tensor3, `2` otherwise (Octave convention) |
| `A(:, :, k)` | Page slice — drops trailing singleton, returns Matrix(m, n) |
| `save("T.npy", A)` / `load(...)` | NPY preserves rank-3 shape natively |
| `[Fx,Fy,Fz] = gradient3(F[, dx, dy, dz])` | 3-D gradient (axis 0 = y, axis 1 = x, axis 2 = z); same stencils as `gradient` |
| `divergence3(Fx, Fy, Fz[, dx, dy, dz])` | 3-D divergence ∂Fx/∂x + ∂Fy/∂y + ∂Fz/∂z |
| `[Cx,Cy,Cz] = curl3(Fx, Fy, Fz[, dx, dy, dz])` | 3-D curl ∇×F (returns 3 Tensor3 components) |

**Page slice + write:**
```r
T = reshape(1:24, 2, 3, 4)
page = T(:, :, 2)              # Matrix(2, 3)
U = zeros3(2, 2, 3)
U(:, :, 2) = [1, 2; 3, 4]      # page assignment
```

**Stack matrices into pages:**
```r
stacked = cat(3, [1,2;3,4], [5,6;7,8])    # Tensor3(2, 2, 2)
```

---

## Statistics

| Function | Description |
|---|---|
| `sum(v)` | Sum all elements |
| `prod(v)` | Product of all elements |
| `cumsum(v)` | Cumulative sum |
| `min(v)`, `max(v)`, `min(a,b)`, `max(a,b)` | Min / max of vector or two scalars |
| `argmin(v)`, `argmax(v)` | 1-based index of min / max |
| `mean(v)` | Arithmetic mean |
| `median(v)` | Median (real parts; average of two middles for even length) |
| `std(v)` | Standard deviation (N-1 denominator) |
| `sort(v)` | Sort ascending by real part |
| `trapz(v)` / `trapz(x, v)` | Trapezoidal integration (unit or explicit spacing) |
| `hist(v)` / `hist(v, n)` | Histogram; returns 2×n matrix (bin centers, counts). Alias: `histogram()` |
| `histogram(v); savefig(file)` | Save histogram to PNG or SVG |
| `all(v)` | True if all elements nonzero |
| `any(v)` | True if any element nonzero |

---

## Linear Algebra

| Function | Description |
|---|---|
| `dot(u, v)` | Inner (dot) product |
| `cross(u, v)` | 3-element cross product |
| `outer(u, v)` | Outer product → N×M matrix |
| `kron(A, B)` | Kronecker tensor product |
| `norm(v)`, `norm(v,p)` | Vector p-norm (1, 2, Inf); matrix Frobenius; works on sparse |
| `inv(M)` | Matrix inverse |
| `det(M)` | Determinant |
| `trace(M)` | Trace |
| `rank(M)` | Numerical rank |
| `eig(M)` | Eigenvalues (column vector) |
| `expm(M)` | Matrix exponential $e^M$ (Padé approximant) |
| `linsolve(A, b)` | Solve A·x = b (A may be dense or sparse); returns x |
| `roots(p)` | Roots of polynomial with coefficients p |

---

## Special Functions

| Function | Description |
|---|---|
| `laguerre(n, alpha, x)` | Associated Laguerre polynomial $L_n^\alpha(x)$, element-wise |
| `legendre(l, m, x)` | Associated Legendre polynomial $P_l^m(x)$, element-wise |
| `convolve(x, h)` | Linear convolution (output length = len(x)+len(h)-1) |
| `factor(n)` | Prime factorization |

---

## Fourier Transforms

| Function | Description |
|---|---|
| `fft(v)` | Discrete Fourier transform (zero-pads to next power of 2) |
| `ifft(V)` | Inverse DFT |
| `fftshift(V)` | Shift zero-frequency to center |
| `fftfreq(n, sr)` | Frequency axis for n-point DFT at sample rate sr |
| `spectrum(v, sr)` | Returns 2×n matrix: row 1 = Hz (DC-centered), row 2 = complex spectrum |

---

## DSP — Filters

| Function | Description |
|---|---|
| `fir_lowpass(taps, cutoff_hz, sr, window)` | FIR lowpass coefficients |
| `fir_highpass(taps, cutoff_hz, sr, window)` | FIR highpass coefficients |
| `fir_bandpass(taps, low_hz, high_hz, sr, window)` | FIR bandpass coefficients |
| `butterworth_lowpass(order, cutoff_hz, sr)` | Butterworth IIR lowpass — returns b (numerator) coefficients only |
| `butterworth_highpass(order, cutoff_hz, sr)` | Butterworth IIR highpass — returns b (numerator) coefficients only |
| `filtfilt(b, a, x)` | Zero-phase forward-backward IIR filter; use `a=[1]` for FIR |
| `fir_lowpass_kaiser(cutoff_hz, trans_bw_hz, atten_db, sr)` | Auto-designed Kaiser lowpass |
| `fir_highpass_kaiser(cutoff_hz, trans_bw_hz, atten_db, sr)` | Auto-designed Kaiser highpass |
| `fir_bandpass_kaiser(lo_hz, hi_hz, trans_bw_hz, atten_db, sr)` | Auto-designed Kaiser bandpass |
| `fir_notch(center_hz, bw_hz, sr, taps, window)` | FIR notch filter |
| `firpm(n_taps, bands, desired)` | Parks-McClellan optimal equiripple FIR |
| `firpm(n_taps, bands, desired, weights)` | Parks-McClellan with per-band weights |
| `firpmq(n_taps, bands, desired [, weights [, bits [, n_iter]]])` | Integer-coefficient Parks-McClellan (default bits=16, n_iter=8); returns integer taps. For unit-gain passband use `freqz(h / sum(h), ...)` to normalize. |
| `freqz(h, n_points, sr)` | Complex frequency response → 2×n matrix |
| `upfirdn(x, h, p, q)` | Upsample·filter·downsample via polyphase decomposition |
| `window(name, n)` | Window vector; names: `"hann"` `"hamming"` `"blackman"` `"rectangular"` `"kaiser"` |

---

## Control Systems

| Function | Description |
|---|---|
| `tf(num, den)` | Create transfer function from numerator/denominator coefficient vectors |
| `pole(sys)` | Poles of a transfer function |
| `zero(sys)` | Zeros of a transfer function |
| `ss(A, B, C, D)` | Create state-space system |
| `ctrb(A, B)` | Controllability matrix |
| `obsv(A, C)` | Observability matrix |
| `bode(sys)` | Bode plot in terminal |
| `step(sys)` | Step response plot in terminal |
| `margin(sys)` | Gain and phase margins |
| `lqr(A, B, Q, R)` | LQR optimal gain matrix K |
| `rlocus(sys)` | Root locus plot in terminal |

---

## Fixed-Point Quantization

| Function | Description |
|---|---|
| `qfmt(word_bits, frac_bits)` | Create Q-format spec (default: floor rounding, saturate overflow) |
| `qfmt(w, f, round_mode, overflow_mode)` | Full spec; round: `"floor"` `"ceil"` `"zero"` `"round"` `"round_even"`; overflow: `"saturate"` `"wrap"` |
| `quantize(x, fmt)` | Quantize scalar/vector/matrix to Q-format grid |
| `qadd(a, b, fmt)` | Fixed-point element-wise add, result quantized to fmt |
| `qmul(a, b, fmt)` | Fixed-point element-wise multiply, result quantized to fmt |
| `qconv(x, h, fmt)` | Fixed-point FIR convolution, output quantized to fmt |
| `snr(x_ref, x_q)` | Signal-to-noise ratio in dB between reference and quantized signal |

---

## ML / Activation Functions

| Function | Description |
|---|---|
| `softmax(v)` | Softmax probability distribution (numerically stable) |
| `relu(v)` | Rectified linear unit: max(0, x), element-wise |
| `gelu(v)` | Gaussian error linear unit, element-wise |
| `layernorm(v)` / `layernorm(v, eps)` | Layer normalization: (v − mean) / std |

---

## Structs

| Syntax / Function | Description |
|---|---|
| `s = struct("x", 1, "y", 2)` | Create struct from field-value pairs |
| `s.field` | Access a field |
| `s.field = val` | Set a field (auto-creates struct if s is undefined) |
| `isstruct(x)` | True if x is a struct |
| `fieldnames(s)` | Print all field names |
| `isfield(s, "name")` | True if struct has the named field |
| `rmfield(s, "name")` | Return new struct with named field removed |

---

## Sparse Vectors & Matrices

| Function | Description |
|---|---|
| `sparse(I, J, V, m, n)` | Build m×n sparse matrix from 1-based index/value vectors |
| `sparse(A)` | Convert dense matrix/vector to sparse (drops near-zeros) |
| `sparsevec(I, V, n)` | Build sparse vector of length n from 1-based indices |
| `speye(n)` | n×n sparse identity matrix |
| `spzeros(m, n)` | m×n all-zero sparse matrix |
| `spdiags(V, D, m, n)` | Build sparse matrix from diagonals (D=0 main, >0 super, <0 sub) |
| `sprand(m, n, density)` | Random sparse matrix with ~density×m×n non-zeros, values in [0,1) |
| `spsolve(A, b [, mode])` | Solve A×x = b. `mode` is `"auto"` (default), `"cholesky"` (force SPD path), or `"lu"` (dense fallback). Auto detects SPD via Hermitian + real-positive diagonal. |
| `laplacian_2d(nx, ny [, dx, dy])` | 5-point sparse Laplacian with Dirichlet BC; column-major ordering `k = (j-1)*ny + i` |
| `ij2k(i, j, ny)` | Column-major grid → flat index (1-based); third arg is `ny`, not `nx` |
| `k2ij(k, ny)` | `[i, j] = k2ij(k, ny)` — inverse of `ij2k` |
| `full(S)` | Convert sparse to dense (identity for dense inputs) |
| `nnz(S)` | Number of stored non-zero entries |
| `issparse(x)` | 1 if sparse, 0 otherwise |
| `nonzeros(S)` | Vector of non-zero values in storage order |
| `find(S)` | `[I,J,V]` for sparse matrix, `[I,V]` for sparse vector (1-based) |
| `S(i,j)` | Index read (returns 0 for absent entries) |
| `S(i,j) = val` | Index write (setting to 0 removes the entry) |
| `transpose(S)` | Non-conjugate transpose (stays sparse) |
| `S'` | Conjugate transpose (stays sparse) |

Native O(nnz) operations: `S+S`, `S-S`, `S*scalar`, `S/scalar`, `S*M` (SpMM), `S*v'` (SpMV via SpMM), `dot(sv,sv)`, `dot(sv,v)`, `transpose(S)`, `S'`.
Mixed sparse+dense pairs auto-promote to dense.

---

## Cell Arrays (String Arrays)

| Syntax / Function | Description |
|---|---|
| `{"a", "b", "c"}` | String array literal — all elements must be strings |
| `sa(i)` | 1-based indexing — returns a string |
| `sa(2:4)` | Slice indexing — returns a new string array |
| `length(sa)` / `numel(sa)` | Number of elements |
| `size(sa)` | `[1, n]` — row vector shape |
| `iscell(x)` | `true` if x is a string array, `false` otherwise |
| `bar(labels, y)` | Categorical bar chart with string array x-axis labels |

---

## Output & I/O

| Function | Description |
|---|---|
| `print(x, ...)` | Print to stdout, space-separated |
| `disp(x)` | Display a value (always appends newline) |
| `fprintf(fmt, args...)` | Formatted print; specifiers: `%d %f %g %e %s %%`; flags: `- + 0 # ,`; escapes: `\n \t` |
| `sprintf(fmt, args...)` | Same as `fprintf` but returns a string instead of printing |
| `commas(x)` / `commas(x, prec)` | Format number with thousands separators; returns string |
| `save("file.npy", x)` | Save array to NumPy .npy format |
| `save("file.npz", "a", a, "b", b, ...)` | Save multiple named arrays to .npz |
| `save("file.csv", x)` | Save array to CSV |
| `load("file.npy")` | Load .npy → value |
| `load("file.npz")` | Load all arrays from .npz into workspace |
| `load("file.npz", "name")` | Load one named array from .npz |
| `load("file.csv")` | Load CSV → scalar / vector / matrix |
| `save("file.toml", s)` | Save struct to TOML |
| `load("file.toml")` | Load TOML → struct |
| `whos` | List workspace variables with type and size |
| `whos("file.npz")` | Inspect arrays stored in an NPZ file |
| `sleep(seconds)` | Pause execution for a non-negative duration in seconds (fractional OK) |

---

## Plotting — Terminal (interactive, blocks until keypress)

| Function | Description |
|---|---|
| `plot(v)` / `plot(x, v)` | Line plot (sample-indexed or explicit x-axis) |
| `plot(v, "color", "r", "label", "name", "style", "dashed")` | Key-value options; colors: `r g b c m y k w` or full names |
| `stem(v)` / `stem(v, "title")` | Stem plot |
| `bar(y)` / `bar(x, y)` / `bar(y, "title")` | Bar chart |
| `bar(labels, y)` / `bar(labels, y, "title")` | Categorical bar chart (labels is a string array) |
| `bar(M)` / `bar(x, M)` / `bar(x, M, "title")` | Grouped bar chart (each column = group) |
| `scatter(x, y)` / `scatter(x, y, "title")` | Scatter plot |
| `hline(y)` / `hline(y, color, label)` | Horizontal reference line (dashed); `yline()` alias |
| `plotdb(Hz)` / `plotdb(Hz, "title")` | dB frequency response (Hz from `freqz` or `spectrum`) |
| `imagesc(M)` / `imagesc(M, cmap)` | Matrix heatmap; colormaps: `"viridis"` `"jet"` `"hot"` `"gray"` |
| `surf(Z)` / `surf(X, Y, Z)` / `surf(X, Y, Z, cmap)` | 3D surface; interactive rotate/zoom in viewer, Plotly 3D in HTML |
| `contour(Z)` / `contour(X, Y, Z [, n|levels [, "color"]])` | Line contours; honours `hold on` for overlay on `imagesc`. Terminal: not rendered. |
| `contourf(Z)` / `contourf(X, Y, Z [, n|levels])` | Filled contours; HTML uses Plotly polygon fill, SVG uses per-cell band approximation |
| `quiver(X, Y, U, V [, scale | "title" | "c"])` / `quiver(U, V)` | 2-D vector-field arrows; auto-scaled. NaN cells skipped. Overlays on `imagesc` / `contour` under `hold on`. Terminal: not rendered. |
| `streamplot(X, Y, U, V [, density | seeds | "title" | "c"])` | RK4 streamlines from a default 10×10 seed grid or an explicit Nx2 seeds matrix; midpoint arrowhead per line. Same overlay/backend behaviour as `quiver`. |

---

## Plotting — File Output (PNG, SVG, or HTML by extension)

Any interactive plot can be saved to a file by calling `savefig(path)` immediately after:

| Pattern | Description |
|---|---|
| `plot(v, "title"); savefig("file.svg")` | Line plot → file |
| `stem(v, "title"); savefig("file.svg")` | Stem plot → file |
| `bar(y, "title"); savefig("file.svg")` | Bar chart → file |
| `scatter(x, y, "title"); savefig("file.svg")` | Scatter plot → file |
| `plotdb(Hz, "title"); savefig("file.svg")` | dB response → file |
| `histogram(v); savefig("file.svg")` | Histogram → file |
| `imagesc(M); savefig("file.svg")` | Heatmap → file |
| `surf(X, Y, Z); savefig("file.html")` | 3D surface → interactive Plotly (or SVG/PNG wireframe) |
| `savefig("file.html")` | Export current figure to interactive HTML (Plotly) |

Supported extensions: `.svg`, `.png`, `.html`.

---

## Figure Controls (apply to the next `plot`/`stem`/… call)

| Function | Description |
|---|---|
| `fig = figure()` | Create new figure, return numeric handle |
| `fig = figure("file.html")` | Create new figure in HTML output mode |
| `figure(N)` | Switch to figure N (creates if it doesn't exist) |
| `hold on` / `hold off` | Overlay series on current subplot (also `hold("on")`) |
| `grid on` / `grid off` | Show / hide grid lines (also `grid("on")`) |
| `viewer` / `viewer on` / `viewer on <name>` / `viewer off` | Bare `viewer` = status (connection state + current figure routing); `on`/`off` route plots to/from external rustlab-viewer. Auto-falls-back to TUI if the viewer dies. Requires `viewer` feature. |
| `title("text")` | Set subplot title |
| `xlabel("text")` | Set x-axis label |
| `ylabel("text")` | Set y-axis label |
| `xlim([lo, hi])` | Fix x-axis range |
| `ylim([lo, hi])` | Fix y-axis range |
| `subplot(rows, cols, idx)` | Switch to panel idx (1-based, left-to-right then top-to-bottom) |
| `legend("s1", "s2", ...)` | Label series in order added |

---

## Streaming DSP

| Function | Description |
|---|---|
| `state_init(n)` | Allocate overlap-save history buffer of length n (use `length(h)-1`) |
| `filter_stream(frame, h, state)` | Filter frame through FIR h; returns Tuple `[y, state]` |

## Audio I/O

Raw f32 LE stdin/stdout PCM. Use bridge programs (sox, arecord/aplay) to connect hardware.

| Function | Description |
|---|---|
| `audio_in(sr, frame)` | Create AudioIn descriptor (sample_rate, frame_size) |
| `audio_out(sr, frame)` | Create AudioOut descriptor (sample_rate, frame_size) |
| `audio_read(src)` | Read one frame from stdin; exits cleanly on EOF |
| `audio_write(dst, y)` | Write one frame (real parts) to stdout; flushes after each frame |

---

## Live Plotting

Persistent terminal plots for real-time data (oscilloscopes, spectrum monitors, animations).

| Function | Description |
|---|---|
| `figure_live(rows, cols)` | Open persistent live terminal figure; errors if not a tty |
| `plot_update(fig, panel, y)` | Replace panel data (1-based); no immediate redraw |
| `plot_update(fig, panel, x, y)` | Same with explicit x-axis |
| `plot_labels(fig, panel, title, xlabel, ylabel)` | Set title and axis labels on a live panel |
| `plot_limits(fig, panel, xlim, ylim)` | Set fixed axis limits on a live panel |
| `figure_draw(fig)` | Flush all panels in one atomic refresh |
| `figure_close(fig)` | Restore terminal; also fires automatically on exit |
| `mag2db(X)` | 20·log10(|X|), floored at −200 dB; for spectrum dB display |

---

## Common Patterns

**2D grid:**
```r
x = linspace(-10.0, 10.0, N)
[X, Z] = meshgrid(x, x)
r_mat = sqrt(X .^ 2 + Z .^ 2)
```

**Build a vector in a loop:**
```r
for i = 1:n
  v(i) = some_fn(i)
end
```

**Trapezoidal integral with custom spacing:**
```r
norm = trapz(x, prob)
```

**FIR filter (windowed sinc):**
```r
h = fir_lowpass(63, 1000.0, 44100.0, "hann")
y = convolve(x, h)
```

**Auto-designed Kaiser lowpass:**
```r
h = fir_lowpass_kaiser(1000.0, 200.0, 60.0, 44100.0)
```

**Parks-McClellan equiripple lowpass:**
```r
h = firpm(63, [0, 0.2, 0.3, 1.0], [1, 1, 0, 0])
```

**Frequency response plot:**
```r
H = freqz(h, 512, 44100.0)
plotdb(H, "Lowpass response")
```

**Fixed-point quantization:**
```r
fmt = qfmt(16, 15, "round_even", "saturate")
xq  = quantize(x, fmt)
hq  = quantize(h, fmt)
yq  = qconv(xq, hq, fmt)
db  = snr(y_ref, yq)
```

**Transfer function and step response:**
```r
sys = tf([1], [1, 2, 1])
step(sys)
```

**State-space LQR design:**
```r
A = [0, 1; -1, -0.5]
B = [0; 1]
Q = eye(2)
R = [1]
K = lqr(A, B, Q, R)
```

**Multi-panel figure:**
```r
figure()
subplot(2, 1, 1)
  title("Signal")
  plot(x, "Signal")
subplot(2, 1, 2)
  title("Spectrum")
  plotdb(freqz(h, 512, sr), "Response")
```

**Save and reload workspace:**
```r
save("data.npz", "x", x, "y", y)
load("data.npz")
```

**Real-time FIR streaming (stdin → stdout):**
```r
sr    = 44100.0
FRAME = 256
h     = firpm(64, [0.0, 0.2, 0.3, 1.0], [1.0, 1.0, 0.0, 0.0])
state = state_init(length(h) - 1)
src   = audio_in(sr, FRAME)
dst   = audio_out(sr, FRAME)
while true
  frame = audio_read(src)
  [y, state] = filter_stream(frame, h, state)
  audio_write(dst, y)
end
```
Run as: `sox -d ... | rustlab run filter.r | sox ... -d` (see `examples/audio/`)
