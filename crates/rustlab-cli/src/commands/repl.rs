use anyhow::Result;
use rustlab_script::{lexer, parser, Evaluator};
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::HistoryHinter;
use rustyline::{error::ReadlineError, Helper, Hinter, Validator};
use rustyline::{CompletionType, Config, Context, Editor};

use crate::color;

// ─── Help text ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, serde::Serialize)]
pub struct HelpEntry {
    pub name: &'static str,
    pub brief: &'static str,
    pub detail: &'static str,
}

pub const HELP: &[HelpEntry] = &[
    // Math
    HelpEntry { name: "abs",    brief: "Absolute value / magnitude",
        detail: "abs(x)  — scalar, complex, vector, or matrix\n  Returns element-wise magnitude; complex inputs give their L2 norm per element.\n  abs([-1, 2; -3, 4])  →  [1, 2; 3, 4]" },
    HelpEntry { name: "angle",  brief: "Phase angle in radians",
        detail: "angle(x)  — scalar, complex, or vector\n  Returns the argument of a complex number." },
    HelpEntry { name: "real",   brief: "Real part",
        detail: "real(x)  — scalar, complex, vector, or matrix\n  1×1 matrix returns a scalar." },
    HelpEntry { name: "imag",   brief: "Imaginary part",
        detail: "imag(x)  — scalar, complex, vector, or matrix\n  1×1 matrix returns a scalar." },
    HelpEntry { name: "conj",   brief: "Complex conjugate",
        detail: "conj(x)  — scalar, complex, vector, or matrix\n  Negates the imaginary part. Real inputs are returned unchanged." },
    HelpEntry { name: "cos",    brief: "Cosine",        detail: "cos(x)  — element-wise, accepts scalar, complex, vector, or matrix" },
    HelpEntry { name: "sin",    brief: "Sine",          detail: "sin(x)  — element-wise, accepts scalar, complex, vector, or matrix" },
    HelpEntry { name: "acos",   brief: "Inverse cosine",  detail: "acos(x)  — element-wise arccos in radians, accepts scalar, complex, vector, or matrix" },
    HelpEntry { name: "asin",   brief: "Inverse sine",    detail: "asin(x)  — element-wise arcsin in radians, accepts scalar, complex, vector, or matrix" },
    HelpEntry { name: "atan",   brief: "Inverse tangent", detail: "atan(x)  — element-wise arctan in radians, accepts scalar, complex, vector, or matrix\n  For the 2-argument form use atan2(y, x)." },
    HelpEntry { name: "tanh",   brief: "Hyperbolic tangent", detail: "tanh(x)  — element-wise hyperbolic tangent, accepts scalar, complex, vector, or matrix\n  tanh(0.0)  → 0.0\n  tanh(1.0)  → ~0.762\n  tanh([-1,0,1])  → [~-0.762, 0.0, ~0.762]" },
    HelpEntry { name: "sinh",   brief: "Hyperbolic sine",     detail: "sinh(x)  — element-wise, accepts scalar, complex, vector, or matrix" },
    HelpEntry { name: "cosh",   brief: "Hyperbolic cosine",   detail: "cosh(x)  — element-wise, accepts scalar, complex, vector, or matrix" },
    HelpEntry { name: "floor",  brief: "Round toward −∞ (element-wise)",
        detail: "floor(x)  — largest integer ≤ x; applied to real and imaginary parts independently\n  floor(3.7)         → 3.0\n  floor(-2.3)        → -3.0\n  floor(2.9 + 1.4i)  → 2.0 + 1.0i" },
    HelpEntry { name: "ceil",   brief: "Round toward +∞ (element-wise)",
        detail: "ceil(x)  — smallest integer ≥ x; applied to real and imaginary parts independently\n  ceil(3.2)          → 4.0\n  ceil(-2.7)         → -2.0" },
    HelpEntry { name: "round",  brief: "Round to nearest integer (element-wise)",
        detail: "round(x)  — rounds half away from zero; applied to real and imaginary parts independently\n  round(2.5)         → 3.0\n  round(2.4)         → 2.0\n  round(-2.5)        → -3.0" },
    HelpEntry { name: "sign",   brief: "Sign / unit direction (element-wise)",
        detail: "sign(x)  — for real: -1, 0, or +1\n           for complex: z/|z| (unit direction), or 0 if z==0\n  sign(-5.0)         → -1.0\n  sign(0.0)          → 0.0\n  sign(3 + 4i)       → 0.6 + 0.8i" },
    HelpEntry { name: "mod",    brief: "Modulo  a − m·floor(a/m)  (element-wise)",
        detail: "mod(x, m)  — x: scalar/vector/matrix; m: real scalar\n  Always returns a result with the same sign as m (like Python %).\n  mod(7, 3)          → 1.0\n  mod(-1, 3)         → 2.0\n  mod([0:5], 3)      → [0, 1, 2, 0, 1, 2]" },
    HelpEntry { name: "sqrt",   brief: "Square root",   detail: "sqrt(x)  — element-wise, accepts scalar, complex, vector, or matrix" },
    HelpEntry { name: "exp",    brief: "Exponential",   detail: "exp(x)  — element-wise, accepts scalar, complex, vector, or matrix" },
    HelpEntry { name: "log",    brief: "Natural log",   detail: "log(x)  — element-wise (natural log), accepts scalar, complex, vector, or matrix" },
    HelpEntry { name: "log10",  brief: "Base-10 log",   detail: "log10(x)  — element-wise base-10 logarithm, accepts scalar, complex, vector, or matrix" },
    HelpEntry { name: "log2",   brief: "Base-2 log",    detail: "log2(x)  — element-wise base-2 logarithm, accepts scalar, complex, vector, or matrix" },
    // Array / stats
    HelpEntry { name: "zeros",    brief: "Zero vector or matrix",
        detail: "zeros(n)        — length-n complex zero vector\nzeros(m, n)     — m×n complex zero matrix\nzeros([m, n])   — same (accepts size() output)\nzeros(size(A))  — zero matrix matching A's dimensions" },
    HelpEntry { name: "ones",     brief: "Ones vector or matrix",
        detail: "ones(n)        — length-n complex ones vector\nones(m, n)     — m×n complex ones matrix\nones([m, n])   — same (accepts size() output)\nones(size(A))  — ones matrix matching A's dimensions" },
    HelpEntry { name: "linspace", brief: "Linearly spaced vector",
        detail: "linspace(start, stop, n)  — n evenly spaced real values from start to stop" },
    HelpEntry { name: "rand",  brief: "Uniform random vector  [0, 1)",
        detail: "rand(n)  — n samples drawn uniformly from [0, 1)" },
    HelpEntry { name: "randn", brief: "Normal random vector or matrix  (mean 0, std 1)",
        detail: "randn(n)     — length-n vector from N(0,1)\nrandn(m, n)  — m×n matrix from N(0,1)\n  All values are real (zero imaginary part)." },
    HelpEntry { name: "randi", brief: "Random integer(s) in a range",
        detail: "randi(imax)        — single integer in [1, imax]\nrandi(imax, n)     — n integers in [1, imax]\nrandi([lo,hi], n)  — n integers in [lo, hi] (inclusive)" },
    HelpEntry { name: "seed",  brief: "Seed the RNG used by rand/randn/randi/rand3/randn3/sprand",
        detail: "seed(N)  — re-seed the shared RNG with a non-negative integer for a reproducible random stream\nseed()   — re-seed from OS entropy (restores the default non-deterministic behaviour)\n  Useful in notebooks that commit rendered SVG/MD: a `seed(N)` line near the top makes re-renders bit-stable." },
    HelpEntry { name: "min",  brief: "Minimum value (single- or multi-return)",
        detail: "min(v)              — smallest value in a vector or 1-D matrix → scalar\nmin(M)              — column mins → 1×N row matrix\nmin(a, b)           — smaller of two scalars (elementwise form)\nmin(M, [], 1)       — explicit dim-1 (column-wise) min\nmin(M, [], 2)       — dim-2 (row-wise) min → N×1 column\n[m, i] = min(v)     — value + 1-based index of first occurrence\n[M, I] = min(A, [], 2) — multi-return on the axis form\n\n  Comparison key: real value for purely-real input; magnitude |z| for\n  complex input. Diverges from MATLAB on equal magnitudes (we pick the\n  first occurrence; MATLAB uses phase-angle tie-break).\n  NaN entries are skipped; all-NaN input errors. Multi-return is not\n  defined for the elementwise two-argument form — [m,i] = min(a,b) errors.\n\n  min([3, 1, 4])              → 1.0\n  [m, i] = min([3, 1, 4, 1])  → m = 1, i = 2\n  min(5, 3)                   → 3.0\n  min([1, 5; 4, 2])           → [1, 2]\n  min([1, 5; 4, 2], [], 2)    → [1; 2]" },
    HelpEntry { name: "max",  brief: "Maximum value (single- or multi-return)",
        detail: "max(v)              — largest value in a vector or 1-D matrix → scalar\nmax(M)              — column maxes → 1×N row matrix\nmax(a, b)           — larger of two scalars (elementwise form)\nmax(M, [], 1)       — explicit dim-1 (column-wise) max\nmax(M, [], 2)       — dim-2 (row-wise) max → N×1 column\n[m, i] = max(v)     — value + 1-based index of first occurrence\n[M, I] = max(A, [], 2) — multi-return on the axis form\n\n  Comparison key, NaN handling, and multi-return restrictions match `min`.\n\n  max([3, 1, 4, 1, 5, 9])     → 9\n  [m, i] = max([3, 1, 4, 5])  → m = 5, i = 4\n  max(0, -5)                  → 0.0\n  max([1, 5; 4, 2], [], 2)    → [5; 4]" },
    HelpEntry { name: "mean",   brief: "Mean (average) of a vector or matrix",
        detail: "mean(v)        — average of a vector or 1-D matrix → scalar\nmean(M)        — column means → 1×N row matrix\nmean(M, 1)     — explicit dim-1 (column-wise) mean\nmean(M, 2)     — dim-2 (row-wise) mean → N×1 column" },
    HelpEntry { name: "median", brief: "Median of a vector or matrix (real parts)",
        detail: "median(v)      — middle value of a vector or 1-D matrix → scalar\nmedian(M)      — column medians → 1×N row matrix\nmedian(M, 1)   — explicit dim-1\nmedian(M, 2)   — dim-2 → N×1 column\n\n  Odd length: middle element; even length: average of two middle elements." },
    HelpEntry { name: "std",    brief: "Standard deviation (N-1 denominator)",
        detail: "std(v)         — sample stddev of a vector or 1-D matrix → scalar\nstd(M)         — per-column stddev → 1×N row matrix\nstd(M, 1)      — explicit dim-1\nstd(M, 2)      — dim-2 → N×1 column" },
    HelpEntry { name: "sum",    brief: "Sum of elements",
        detail: "sum(v)         — sum of a vector or 1-D matrix → scalar\nsum(M)         — column sums → 1×N row matrix (octave default)\nsum(M, 1)      — explicit dim-1 (column-wise)\nsum(M, 2)      — dim-2 (row-wise) → N×1 column\nsum(sum(M))    — total of all elements (matlab idiom)\n\n  Returns complex if any imaginary part is non-negligible." },
    HelpEntry { name: "cumsum", brief: "Cumulative sum",
        detail: "cumsum(v)      — running total of a vector or 1-D matrix → same shape\ncumsum(M)      — per-column running totals → same shape as M\ncumsum(M, 1)   — explicit dim-1\ncumsum(M, 2)   — dim-2 (row-wise running totals)" },
    HelpEntry { name: "argmin", brief: "1-based position of the minimum",
        detail: "argmin(v)      — 1-based index of the min in a vector or 1-D matrix → scalar\nargmin(M)      — per-column argmin → 1×N row matrix\nargmin(M, 2)   — per-row argmin → N×1 column\n\n  Comparison key matches `min`: real value for purely-real input,\n  magnitude |z| for complex input (diverges from MATLAB on equal\n  magnitudes — first-occurrence wins). NaN entries are skipped;\n  all-NaN input errors. Always agrees with the index from\n  [m, i] = min(...)." },
    HelpEntry { name: "argmax", brief: "1-based position of the maximum",
        detail: "argmax(v)      — 1-based index of the max in a vector or 1-D matrix → scalar\nargmax(M)      — per-column argmax → 1×N row matrix\nargmax(M, 2)   — per-row argmax → N×1 column\n\n  Same comparison-key and NaN rules as `argmin`. Always agrees with\n  the index from [m, i] = max(...)." },
    HelpEntry { name: "sort",   brief: "Sort by real part",
        detail: "sort(v)               — ascending order (default)\nsort(v, \"ascend\")     — explicit ascending\nsort(v, \"descend\")    — descending order\n[s, idx] = sort(v)    — sorted values + 1-based permutation indices\n[s, idx] = sort(v, \"descend\")  — same with reversed order\n\n  Returns a vector or column-vector matrix matching the input shape;\n  imaginary components are preserved. Comparison uses the real part only.\n  sort([3,1,2])              → [1, 2, 3]\n  sort([3,1,2], \"descend\")   → [3, 2, 1]\n  v(idx) reproduces the sorted output." },
    HelpEntry { name: "trapz",  brief: "Trapezoidal numerical integration",
        detail: "trapz(v)      — integrate with unit spacing\ntrapz(x, v)   — integrate using x as sample positions\n  Returns a scalar (real or complex)." },
    HelpEntry { name: "hist", brief: "Histogram — plot and return bin counts",
        detail: "hist(v)        — 10 bins (default)\nhist(v, n)     — n bins\nReturns 2×n matrix: row 1 = bin centers, row 2 = counts\n\nAlias: histogram()" },
    HelpEntry { name: "len",      brief: "Length of vector/string  (alias: length)",
        detail: "len(x)  — number of elements in a vector, rows in a matrix, or chars in a string" },
    HelpEntry { name: "length",   brief: "Alias for len",
        detail: "length(x)  — see len" },
    HelpEntry { name: "numel",    brief: "Total number of elements",
        detail: "numel(x)  — total elements (rows*cols for matrices, m*n*p for tensor3, 1 for scalars)" },
    HelpEntry { name: "size",     brief: "Dimensions as a 2- or 3-element vector",
        detail: "size(x)        — [rows, cols] for matrices/vectors, [m, n, p] for tensor3\nsize(x, dim)   — size along dimension 1, 2, or 3 (3 requires tensor3)" },
    HelpEntry { name: "ndims",    brief: "Number of dimensions (2 or 3)",
        detail: "ndims(x)  — returns 3 for tensor3, 2 for everything else (Octave convention)" },
    // Matrix
    HelpEntry { name: "eye",       brief: "Identity matrix",
        detail: "eye(n)  — returns an n×n identity matrix" },
    HelpEntry { name: "transpose", brief: "Non-conjugate transpose  (also: A.')",
        detail: "transpose(A)  — transposes rows and cols without conjugating\n  Equivalent to the postfix operator A.'" },
    HelpEntry { name: "diag",      brief: "Create diagonal matrix or extract diagonal",
        detail: "diag(v)  — creates an n×n diagonal matrix from vector v\ndiag(M)  — extracts the main diagonal of matrix M as a vector" },
    HelpEntry { name: "trace",     brief: "Sum of the main diagonal",
        detail: "trace(M)  — returns the sum of diagonal elements" },
    HelpEntry { name: "reshape",   brief: "Reshape a vector, matrix, or tensor3",
        detail: "reshape(A, m, n)     — returns an m×n matrix (or length-n vector when m=1 or n=1)\nreshape(A, m, n, p)  — returns an m×n×p tensor3\n  Total elements must be preserved. Walk order is column-major (Octave convention)." },
    HelpEntry { name: "repmat",    brief: "Tile a matrix",
        detail: "repmat(A, m, n)  — tiles matrix A m times vertically, n times horizontally" },
    HelpEntry { name: "horzcat",   brief: "Horizontal concatenation  (also: [A B])",
        detail: "horzcat(A, B, ...)  — concatenates matrices side by side (same row count required)" },
    HelpEntry { name: "vertcat",   brief: "Vertical concatenation  (also: [A; B])",
        detail: "vertcat(A, B, ...)  — stacks matrices vertically (same column count required)" },
    HelpEntry { name: "cat",       brief: "Concatenate along a given dimension",
        detail: "cat(dim, A, B, ...)  — dim=1 (rows, like vertcat), dim=2 (cols, like horzcat),\n  dim=3 (pages, stacks matrices/tensor3s into a tensor3).\n  Example: cat(3, M1, M2)  → 2-page tensor3 from two equal-size matrices." },
    // Tensor3 (rank-3)
    HelpEntry { name: "zeros3",    brief: "Rank-3 zero tensor",
        detail: "zeros3(m, n, p)     — m×n×p complex zero tensor\nzeros3([m, n, p])   — same (accepts size() output)\n  Use A(:, :, k) to extract the k-th page as a matrix." },
    HelpEntry { name: "ones3",     brief: "Rank-3 ones tensor",
        detail: "ones3(m, n, p)     — m×n×p complex ones tensor\nones3([m, n, p])   — same (accepts size() output)" },
    HelpEntry { name: "rand3",     brief: "Uniform random rank-3 tensor  [0, 1)",
        detail: "rand3(m, n, p)  — m×n×p tensor of samples from U[0, 1)" },
    HelpEntry { name: "randn3",    brief: "Normal random rank-3 tensor  (mean 0, std 1)",
        detail: "randn3(m, n, p)  — m×n×p tensor of samples from N(0, 1)" },
    HelpEntry { name: "permute",   brief: "Reorder the axes of a tensor3",
        detail: "permute(A, [d1, d2, d3])  — reorders axes according to the permutation\n  permute(A, [2, 1, 3])  swaps rows and columns, leaves pages alone" },
    HelpEntry { name: "squeeze",   brief: "Drop singleton dimensions",
        detail: "squeeze(A)  — removes any dimensions of length 1 from a tensor3.\n  (m, n, 1) → matrix(m, n);  (m, 1, p) → matrix(m, p)\n  (m, 1, 1) → vector(m);     (1, 1, 1) → scalar\n  Non-tensor3 inputs pass through unchanged." },
    // Linear algebra
    HelpEntry { name: "dot",      brief: "Inner (dot) product of two vectors",
        detail: "dot(u, v)  — sum of element-wise products; conjugates u for complex vectors\n  Accepts dense, sparse, or mixed dense/sparse vector operands.\n  sparse·sparse uses O(nnz) merge; sparse·dense uses O(nnz) gather." },
    HelpEntry { name: "cross",    brief: "3-element cross product",
        detail: "cross(u, v)  — both vectors must have exactly 3 elements" },
    HelpEntry { name: "outer",    brief: "Outer (tensor) product of two vectors → N×M matrix",
        detail: "outer(a, b)  — result[i,j] = a[i] * b[j]\n  Accepts vectors or scalars." },
    HelpEntry { name: "kron",     brief: "Kronecker tensor product of two matrices",
        detail: "kron(A, B)  — for A (m×n) and B (p×q) returns an mp×nq matrix\n  Block (i,j) equals A[i,j]*B. Accepts matrices, vectors, or scalars." },
    HelpEntry { name: "norm",     brief: "Euclidean norm of a vector or Frobenius norm of a matrix",
        detail: "norm(v)       — L2 norm of a vector\nnorm(v, p)    — p-norm (1, 2, Inf supported)\nnorm(M)       — Frobenius norm of a matrix\n  Also works on sparse vectors and matrices.\n  For sparse matrices: norm(S,1) = max column sum, norm(S,Inf) = max row sum." },
    HelpEntry { name: "det",      brief: "Determinant of a square matrix",
        detail: "det(M)  — computed via LU decomposition with partial pivoting" },
    HelpEntry { name: "inv",      brief: "Inverse of a square matrix",
        detail: "inv(M)  — computed via Gauss-Jordan elimination; errors on singular matrices" },
    HelpEntry { name: "expm",     brief: "Matrix exponential  e^M",
        detail: "expm(M)  — scaling-and-squaring with a [6/6] Padé approximant\n  Used for time evolution: expm(-j*H*t)" },
    HelpEntry { name: "linsolve", brief: "Solve the linear system  A*x = b",
        detail: "linsolve(A, b)  — A is n×n (dense or sparse), b is a length-n vector\n  Sparse A is converted to dense internally.\n  Returns x as a vector." },
    HelpEntry { name: "eig",      brief: "Eigenvalues / eigendecomposition (nargout-aware)",
        detail: "e = eig(A)                    — N×1 column vector of eigenvalues\n[V, D] = eig(A)               — V eigenvector matrix (column k ↔ D(k,k))\n                                D diagonal matrix of eigenvalues (matlab default)\ne = eig(A, B)                 — generalized: A·v = λ·B·v\n[V, D] = eig(A, B)            — generalized eigenvectors and eigenvalues\n\nOutput-form flag (matlab convention) — overrides the default D shape:\n  eig(A, \"vector\")              — D as N×1 column vector\n  eig(A, \"matrix\")              — D as N×N diagonal matrix\n  [V, D] = eig(A, \"vector\")     — D vector even with two outputs\n  [V, D] = eig(A, B, \"matrix\")  — generalized form, explicit diagonal\n\n  Standard form algorithm: hand-rolled Hessenberg reduction +\n  shifted QR for the eigenvalues, then shifted inverse iteration\n  on A (or inv(B)·A for the generalized form) for each eigenvector.\n  Defective matrices may produce an ill-conditioned V; the eigenvalues\n  remain accurate.\n  Generalized form requires B invertible (Cholesky-route for SPD B is\n  a future optimization; QZ for non-invertible B is deferred).\n\nExample:\n  A = [3, 1; 1, 3]; B = [2, 0; 0, 1];\n  [V, D] = eig(A, B);\n  norm(A*V - B*V*D)         % ~ 1e-15" },
    HelpEntry { name: "eigs",     brief: "Sparse partial eigensolver — Lanczos / Arnoldi",
        detail: "[V, D] = eigs(A, n)\n[V, D] = eigs(A, n, which)         — \"sm\" (default) | \"lm\"\n[V, D] = eigs(A, B, n)            — generalized A x = λ B x; B SPD\n[V, D] = eigs(A, B, n, which)\n\n  A (and B) must be sparse — call sparse(A) first if dense.\n  Returns:\n    V — n_rows × n dense matrix of eigenvectors (column k ↔ D(k))\n    D — length-n vector of eigenvalues\n\nDispatch:\n  Hermitian / SPD A → hand-rolled Lanczos with full reorthogonalization.\n  General A         → hand-rolled Arnoldi with modified Gram-Schmidt.\n  Generalized form  → reduce via SparseChol(B), route through Arnoldi.\n\nDefault Krylov dimension is min(n_rows, max(6n+10, 40)). Implicit restart\nand shift-invert are deferred; if convergence stalls on a closely-spaced\nspectrum, increase the matrix size or wait for the next phase.\n\nExample:\n  L = -1 * laplacian_2d(20, 20);   % SPD form: -∇²\n  [V, D] = eigs(L, 4, \"sm\");        % four lowest eigenmodes" },
    HelpEntry { name: "laguerre", brief: "Associated Laguerre polynomial  L_n^α(x)",
        detail: "laguerre(n, alpha, x)  — 3-term recurrence; x may be scalar/vector/matrix\n  Used for hydrogen radial wavefunctions." },
    HelpEntry { name: "legendre", brief: "Associated Legendre polynomial  P_l^m(x)",
        detail: "legendre(l, m, x)  — Condon-Shortley convention; x may be scalar/vector/matrix\n  0 <= m <= l required. Used for spherical harmonics." },
    HelpEntry { name: "factor",   brief: "Prime factorization of a positive integer",
        detail: "factor(n)  — returns a real vector of prime factors in ascending order\n  factor(12) → [2, 2, 3]\n  factor(17) → [17]" },
    // DSP
    HelpEntry { name: "fir_lowpass",  brief: "FIR low-pass filter coefficients",
        detail: "fir_lowpass(taps, cutoff_hz, sample_rate, window)\n  window: \"hann\", \"hamming\", \"blackman\", \"rectangular\", \"kaiser\"" },
    HelpEntry { name: "fir_highpass", brief: "FIR high-pass filter coefficients",
        detail: "fir_highpass(taps, cutoff_hz, sample_rate, window)" },
    HelpEntry { name: "fir_bandpass", brief: "FIR band-pass filter coefficients",
        detail: "fir_bandpass(taps, low_hz, high_hz, sample_rate, window)" },
    HelpEntry { name: "butterworth_lowpass",  brief: "Butterworth IIR low-pass (returns b coefficients)",
        detail: "butterworth_lowpass(order, cutoff_hz, sample_rate)" },
    HelpEntry { name: "butterworth_highpass", brief: "Butterworth IIR high-pass (returns b coefficients)",
        detail: "butterworth_highpass(order, cutoff_hz, sample_rate)" },
    HelpEntry { name: "upfirdn",  brief: "Upsample·filter·downsample via polyphase decomposition",
        detail: "upfirdn(x, h, p, q)\n  x — input signal (complex vector)\n  h — real FIR filter coefficients\n  p — upsample factor (>= 1)\n  q — downsample factor (>= 1)\n\nSplits h into p polyphase subfilters; each output sample costs ceil(len(h)/p)\nmultiply-adds instead of len(h) — optimal polyphase complexity.\n\nOutput length: floor(((len(x)-1)*p + len(h) - 1) / q) + 1\n\nExamples:\n  y = upfirdn(x, h, 4, 1)   # 4x interpolation\n  y = upfirdn(x, h, 1, 3)   # 3x decimation\n  y = upfirdn(x, h, 3, 2)   # 3/2 rate conversion" },
    HelpEntry { name: "convolve", brief: "Linear convolution of two vectors",
        detail: "convolve(x, h)  — returns x convolved with h" },
    HelpEntry { name: "window",   brief: "Generate a window function vector",
        detail: "window(name, n)  — name: \"hann\", \"hamming\", \"blackman\", \"rectangular\", \"kaiser\"" },
    // FFT
    HelpEntry { name: "fft",      brief: "Forward FFT (zero-pads to next power of two)",
        detail: "fft(v)  — returns complex spectrum; length is next power of two >= len(v)" },
    HelpEntry { name: "ifft",     brief: "Inverse FFT",
        detail: "ifft(V)  — input length must be a power of two (as returned by fft)" },
    HelpEntry { name: "fftshift", brief: "Shift zero-frequency component to center",
        detail: "fftshift(V)  — rearranges FFT output so DC is at the center" },
    HelpEntry { name: "fftfreq",  brief: "FFT frequency axis",
        detail: "fftfreq(n, sample_rate)  — frequency bin values for an n-point FFT" },
    HelpEntry { name: "spectrum", brief: "DC-centered spectrum matrix ready for plotdb",
        detail: "spectrum(X, sample_rate)  — applies fftshift and pairs with Hz frequency axis\n  Returns 2×n matrix: row 1 = Hz (DC centered), row 2 = complex spectrum\n  Pass directly to plotdb()" },
    // Kaiser FIR
    HelpEntry { name: "fir_lowpass_kaiser",  brief: "Auto-designed Kaiser lowpass FIR",
        detail: "fir_lowpass_kaiser(cutoff_hz, trans_bw_hz, stopband_attn_db, sample_rate)\n  Beta and tap count computed automatically from attenuation and transition bandwidth." },
    HelpEntry { name: "fir_highpass_kaiser", brief: "Auto-designed Kaiser highpass FIR",
        detail: "fir_highpass_kaiser(cutoff_hz, trans_bw_hz, stopband_attn_db, sample_rate)" },
    HelpEntry { name: "fir_bandpass_kaiser", brief: "Auto-designed Kaiser bandpass FIR",
        detail: "fir_bandpass_kaiser(low_hz, high_hz, trans_bw_hz, stopband_attn_db, sample_rate)" },
    HelpEntry { name: "fir_notch", brief: "FIR notch filter (spectral inversion of bandpass)",
        detail: "fir_notch(center_hz, bandwidth_hz, sample_rate, num_taps, window)\n  Rejects a narrow band around center_hz." },
    // Fixed-point quantization
    HelpEntry { name: "qfmt", brief: "Create a Q-format spec (word bits, frac bits, rounding, overflow)",
        detail: "qfmt(word_bits, frac_bits)\nqfmt(word_bits, frac_bits, round_mode)\nqfmt(word_bits, frac_bits, round_mode, overflow_mode)\n\n  round_mode:    floor (default/hardware), ceil, zero, round, round_even\n  overflow_mode: saturate (default), wrap\n\nExample:\n  fmt = qfmt(16, 15, \"round_even\", \"saturate\")  # Q0.15, 16-bit" },
    HelpEntry { name: "quantize", brief: "Quantize a scalar / vector / matrix to a Q-format grid",
        detail: "quantize(x, fmt)\n  x   — scalar, complex, vector, or matrix\n  fmt — QFmt spec from qfmt()\n\nReal and imaginary parts are quantized independently.\nReturns same type as input — compatible with all math and plot functions.\n\nExample:\n  fmt = qfmt(16, 15)\n  xq  = quantize(x, fmt)" },
    HelpEntry { name: "qadd", brief: "Fixed-point element-wise add, quantized to fmt",
        detail: "qadd(a, b, fmt)\n  a, b — real scalars or vectors (same length)\n  fmt  — output QFmt spec\n\nComputes a+b at full precision, then quantizes to fmt.\n\nExample:\n  y = qadd(xq, offset, fmt)" },
    HelpEntry { name: "qmul", brief: "Fixed-point element-wise multiply, quantized to fmt",
        detail: "qmul(a, b, fmt)\n  a, b — real scalars or vectors (same length)\n  fmt  — output QFmt spec\n\nFull Q-product computed internally; result rounded to fmt.\n\nExample:\n  y = qmul(xq, gain, fmt)" },
    HelpEntry { name: "qconv", brief: "Fixed-point FIR convolution, output quantized to fmt",
        detail: "qconv(x, h, fmt)\n  x   — input signal (real vector)\n  h   — filter coefficients (real vector)\n  fmt — output QFmt spec\n\nAccumulates products at full precision, then quantizes each output.\nOutput length = len(x) + len(h) - 1.\n\nExample:\n  y = qconv(xq, hq, fmt)" },
    HelpEntry { name: "snr", brief: "Signal-to-noise ratio in dB between reference and quantized signal",
        detail: "snr(x_ref, x_quantized)\n  Both must be real vectors of equal length.\n  Returns 10*log10(signal_power / noise_power) in dB.\n  Returns Inf when signals are identical.\n\nExample:\n  db = snr(y_ref, y_q)\n  print(db)" },
    HelpEntry { name: "firpm",    brief: "Parks-McClellan optimal equiripple FIR filter",
        detail: "firpm(n_taps, bands, desired)\nfirpm(n_taps, bands, desired, weights)\n  bands   — normalized frequency edges [0,1], 1 = Nyquist; pairs define each band\n  desired — target amplitude at each band edge (piecewise-linear)\n  weights — optional, one value per band pair (default: all 1.0)\n  Example (lowpass): firpm(63, [0,0.2,0.3,1], [1,1,0,0])" },
    HelpEntry { name: "freqz",    brief: "Complex frequency response of a filter",
        detail: "freqz(h, n_points, sample_rate)  — returns 2×n matrix: row 1 = freq axis, row 2 = H(f)" },
    // Plotting
    // ML / activation functions
    HelpEntry { name: "softmax",   brief: "Softmax probability distribution",
        detail: "softmax(v)        — numerically-stable softmax over a vector\n  softmax(M)        — per-row softmax of a matrix (ML default, dim=2)\n  softmax(M, dim)   — per-row (2) or per-column (1) softmax\n  Subtracts the per-slice max before exp() to prevent overflow.\n  Each output slice sums to 1.0.\n  softmax([1,2,3,4])         → [0.032, 0.087, 0.237, 0.644]\n  softmax([1,2; 3,4])        → 2x2 with each row summing to 1\n  softmax([1,2; 3,4], 1)     → 2x2 with each column summing to 1" },
    HelpEntry { name: "relu",      brief: "Rectified linear unit  max(0, x)",
        detail: "relu(x)  — element-wise max(0, x)\n  Accepts scalar, vector, or matrix.\n  relu([-3, 0, 2, 5])  → [0, 0, 2, 5]" },
    HelpEntry { name: "gelu",      brief: "Gaussian error linear unit",
        detail: "gelu(x)  — 0.5·x·(1 + tanh(√(2/π)·(x + 0.044715·x³)))\n  Accepts scalar, vector, or matrix.\n  Allows small negative outputs near x ≈ -0.17." },
    HelpEntry { name: "layernorm", brief: "Layer normalisation  (x − mean) / std",
        detail: "layernorm(v)               — vector: zero mean, unit variance\nlayernorm(v, eps)          — vector with custom epsilon (default 1e-5)\nlayernorm(M)               — matrix: per-row by default (ML convention,\n                              rows = samples, cols = features)\nlayernorm(M, dim)          — dim=2 per-row (default), dim=1 per-column\nlayernorm(M, dim, eps)     — full form\n\n  Uses population variance (divides by N, not N-1).\n  1-D-shaped matrices (1×N or N×1) are treated as vectors regardless\n  of dim, matching how sum/mean handle the same shapes.\n\n  Note: layernorm's per-row default diverges from sum/mean/std (which\n  default to dim=1). The ML convention dominates here." },
    HelpEntry { name: "print", brief: "Print values to stdout",
        detail: "print(a, b, ...)  — prints space-separated values followed by newline" },
    HelpEntry { name: "plot",  brief: "Plot a vector in the terminal",
        detail: "plot(v)  or  plot(v, \"title\")  — opens a ratatui terminal chart; press any key to close\n  plot(v, \"title\", \"color\")  — color: r, g, b, c, m, y, k, w\n  plot(v, \"title\", \"color\", \"dashed\")" },
    HelpEntry { name: "stem",  brief: "Stem plot of a vector",
        detail: "stem(v)  or  stem(v, \"title\")  — discrete-sample stem chart" },
    HelpEntry { name: "bar",       brief: "Bar chart in the terminal",
        detail: "bar(y)                — bars at positions 0,1,2,…\nbar(x, y)             — bars at explicit x positions\nbar(labels, y)        — categorical bars with string array labels\nbar(labels, y, title) — categorical bars with title\nbar(y, \"title\")        — with title\nbar(x, y, \"title\")     — explicit positions with title\nbar(M)                — grouped bar chart (each column = group)\nbar(x, M)             — grouped bars at explicit x positions\nbar(x, M, \"title\")    — grouped bars with title\n  Negative heights supported (bars extend below zero).\n\nExamples:\n  bar([10, 20, 30])\n  bar({'Jan','Feb','Mar'}, [10, 20, 30], 'Sales')" },
    HelpEntry { name: "scatter",   brief: "Scatter plot in the terminal",
        detail: "scatter(x, y)          — plot (x,y) points as dots\nscatter(x, y, \"title\") — with title\n  No lines drawn between points.\n  Press any key to close." },
    HelpEntry { name: "hline",     brief: "Horizontal reference line",
        detail: "hline(y)               — dashed horizontal line at y\nhline(y, \"color\")       — with color (\"r\", \"b\", \"green\", etc.)\nhline(y, \"color\", \"label\")  — with color and legend label\nhline([y1, y2, ...])   — multiple horizontal lines\n  Best used with hold(\"on\") to overlay on existing plots.\n  yline() is an alias." },
    HelpEntry { name: "loglog",    brief: "Log-log line plot (data pre-transformed via log10)",
        detail: "loglog(x, y [, opts])  — straight lines on power-law data\n  Both x and y must be strictly positive (negatives or zero error).\n  Pre-transforms via log10 and labels axes \"log10(x)\", \"log10(y)\".\n  Same option syntax as plot().\n\nExample:\n  x = logspace(0, 3, 50);\n  y = x .^ 2;             % power law\n  loglog(x, y)            % straight line, slope 2" },
    HelpEntry { name: "semilogx",  brief: "Log-x, linear-y line plot",
        detail: "semilogx(x, y [, opts])  — log-spaced x-axis (Bode-style)\n  x must be strictly positive. Same option syntax as plot()." },
    HelpEntry { name: "semilogy",  brief: "Linear-x, log-y line plot",
        detail: "semilogy(x, y [, opts])  — log-spaced y-axis (decay / dB plots)\n  y must be strictly positive. Same option syntax as plot()." },
    HelpEntry { name: "polar",     brief: "Polar coordinate plot via Cartesian pre-transform",
        detail: "polar(theta, r [, opts])  — plot (r·cos θ, r·sin θ)\n  theta in radians, r in arbitrary units. Both real-valued.\n  Axes are labeled \"r·cos(θ)\", \"r·sin(θ)\". A future enhancement will\n  add radial gridlines via a proper polar plot kind in the renderer.\n\nExample:\n  theta = linspace(0, 2*pi, 360);\n  r = 1 + 0.3 * cos(3 * theta);   % three-petal rose\n  polar(theta, r)" },
    HelpEntry { name: "yline",     brief: "Horizontal reference line (alias for hline)",
        detail: "yline(y) / yline(y, \"color\") / yline(y, \"color\", \"label\")\n  Same as hline. Convenient when reading MATLAB-flavoured code where\n  yline is the more common spelling for a horizontal threshold line.\n\nExample:\n  yline(-3, \"r\", \"-3 dB\")" },
    HelpEntry { name: "histogram", brief: "Histogram bar chart (alias for hist)",
        detail: "histogram(v) / histogram(v, n_bins)\n  Same builtin as hist(). Both names are registered for ergonomic\n  parity with MATLAB / Octave.\n\nExample:\n  histogram(randn(1000), 30)" },
    HelpEntry { name: "plotdb",   brief: "Terminal dB frequency response plot",
        detail: "plotdb(Hz)  or  plotdb(Hz, \"title\")\n  Hz is the 2×n matrix returned by freqz()\n  x-axis: Hz, y-axis: dB magnitude" },
    HelpEntry { name: "savefig",  brief: "Save current figure to PNG, SVG, or interactive HTML",
        detail: "savefig(\"file.svg\")    — save current figure as SVG\nsavefig(\"file.png\")    — save current figure as PNG\nsavefig(\"file.html\")   — save as interactive Plotly HTML\n\n  Extension determines format: .svg, .png, or .html\n  .html exports all subplots with interactive zoom/pan/hover (Plotly CDN)\n  Build the figure first with plot/stem/bar/scatter/plotdb/imagesc/histogram." },
    HelpEntry { name: "frame",    brief: "Snapshot current figure into the animation frame buffer",
        detail: "frame()\n  Clones the current figure into the per-thread animation buffer, then\n  clears trace data on the active figure (series, heatmap, surface,\n  contours, quivers, streamlines) so the next loop iteration starts\n  with a clean canvas. Subplot layout, axis labels, titles, limits,\n  hold state, and grid setting are preserved.\n\n  Pair with saveanim() to flush. Calling figure() also clears the\n  buffer.\n\nExample:\n  figure()\n  for k = 1:60\n    Ez = step(k); imagesc(Ez, \"viridis\")\n    title(sprintf(\"frame %d\", k))   % set AFTER imagesc\n    frame()\n  end\n  saveanim(\"wave.html\", 30)" },
    HelpEntry { name: "saveanim", brief: "Flush the animation frame buffer to a Plotly HTML or animated GIF file",
        detail: "saveanim(\"file.html\")        — Plotly HTML at default 10 fps\nsaveanim(\"file.html\", fps)   — Plotly HTML at given frame rate\nsaveanim(\"file.gif\", fps)    — animated GIF, per-frame NeuQuant palette\n\n  .html / .htm: self-contained Plotly document with play/pause + slider.\n  .gif: portable GIF that embeds in markdown / PDFs / chat.\n  Buffer is drained on success.\n  Errors on empty buffer or unsupported extension. MP4 / SVG animation\n  not supported in this release." },
    HelpEntry { name: "imagesc",  brief: "Display matrix as a colour heatmap in the terminal",
        detail: "imagesc(M)\nimagesc(M, colormap)\n  colormap: \"viridis\" (default), \"jet\", \"hot\", \"gray\"\n  Press any key to close." },
    HelpEntry { name: "heatmap",  brief: "Heatmap with categorical axis labels",
        detail: "heatmap(M)\nheatmap(M, \"title\")\nheatmap(xlabels, ylabels, M)\nheatmap(xlabels, ylabels, M, \"title\")\nheatmap(xlabels, ylabels, M, \"title\", \"colormap\")\n\n  xlabels, ylabels: string arrays such as {\"Mon\", \"Tue\", \"Wed\"}.\n  colormap: \"viridis\" (default), \"jet\", \"hot\", \"gray\".\n  Row 0 is rendered at the top (image/data orientation).\n\nExample:\n  heatmap({\"A\",\"B\",\"C\"}, {\"X\",\"Y\"}, [1,2,3;4,5,6], \"demo\");" },
    HelpEntry { name: "image",    brief: "Raw pixel display (no normalisation, values 0–255)",
        detail: "image(M)              — grayscale (values 0..255)\nimage(M, \"colormap\")  — single channel mapped through colormap\nimage(R, G, B)        — true-colour RGB (each channel 0..255, real-valued)\n\n  Values are clamped to [0, 255]; no min/max normalisation (unlike imagesc).\n  RGB form requires three real matrices of identical shape.\n  Row 0 is rendered at the top." },
    HelpEntry { name: "surf",     brief: "3D surface plot of a Z matrix",
        detail: "surf(Z)              — plot Z with x=1..cols, y=1..rows\nsurf(X, Y, Z)        — X, Y may be vectors or meshgrid matrices\nsurf(X, Y, Z, cmap)  — with colormap \"viridis\"|\"jet\"|\"hot\"|\"gray\"\n\nTerminal:  renders as a heatmap of Z.\nViewer:    interactive 3D surface — left-drag rotate, scroll zoom,\n           shift+scroll scale Z, right-drag pan, R resets.\nHTML:      Plotly 3D surface (draggable in browser).\nSVG/PNG:   static isometric wireframe.\n\nExample:\n  [X, Y] = meshgrid(linspace(-3, 3, 40), linspace(-3, 3, 40));\n  Z = sin(X.^2 + Y.^2); surf(X, Y, Z);" },
    HelpEntry { name: "contour",  brief: "Line contour plot of a 2-D scalar field",
        detail: "contour(Z)\ncontour(X, Y, Z)\ncontour(X, Y, Z, nlevels)\ncontour(X, Y, Z, levels)         — explicit level vector\ncontour(X, Y, Z, [..], \"k\")     — single line colour (k/r/g/b/c/m/y/w)\ncontour(X, Y, Z, \"title\")\n\n  X, Y may be 1-D vectors or meshgrid matrices.\n  Default is 10 auto-spaced round-number levels.\n  Honours hold on so contour can overlay imagesc heatmaps and other contour layers.\n\n  Terminal: not rendered (issues a one-time warning) — use savefig to view.\n  HTML:     Plotly contour trace (exact).\n  SVG/PNG:  marching-squares line segments.\n\nExample:\n  [X, Y] = meshgrid(linspace(-2, 2, 41), linspace(-2, 2, 41));\n  Z = X .^ 2 + Y .^ 2;\n  contour(X, Y, Z);  savefig(\"contour.svg\");" },
    HelpEntry { name: "contourf", brief: "Filled contour plot of a 2-D scalar field",
        detail: "contourf(Z)\ncontourf(X, Y, Z)\ncontourf(X, Y, Z, nlevels)\ncontourf(X, Y, Z, levels)         — explicit level vector\ncontourf(X, Y, Z, \"title\")\n\n  Default is 10 auto-spaced round-number levels.\n  Colormap follows the heatmap convention (currently always viridis).\n  Honours hold on for overlay with imagesc / contour.\n\n  HTML:     Plotly contour trace with coloring='fill' (exact polygon fill).\n  SVG/PNG:  per-cell discrete-band approximation (exact polygon fill is\n            HTML-only in v1).\n  Terminal: not rendered.\n\nExample:\n  contourf(X, Y, Z, 12);  savefig(\"fill.html\");" },
    HelpEntry { name: "quiver",   brief: "Arrow plot of a 2-D vector field",
        detail: "quiver(X, Y, U, V)\nquiver(X, Y, U, V, scale)         — shaft-length multiplier (default 1)\nquiver(X, Y, U, V, \"title\")\nquiver(U, V)                      — X, Y default to 1..ncols / 1..nrows\n\n  X, Y may be 1-D vectors or meshgrid matrices. U and V are matrices of\n  the same shape; NaN entries are skipped. Arrows auto-scale so the\n  longest one equals the nearest-neighbour cell distance; the optional\n  `scale` multiplier is applied on top.\n  Honours hold on for overlay with imagesc / contour.\n\n  Terminal: not rendered (issues a one-time warning) — use savefig to view.\n  HTML:     scatter lines with arrowhead polylines.\n  SVG/PNG:  plotters line + polygon arrow per grid cell.\n\nExample:\n  [X, Y] = meshgrid(linspace(-2, 2, 16), linspace(-2, 2, 16));\n  U = -Y; V = X; quiver(X, Y, U, V);  savefig(\"vortex.svg\");" },
    HelpEntry { name: "streamplot", brief: "Streamline plot of a 2-D vector field",
        detail: "streamplot(X, Y, U, V)\nstreamplot(X, Y, U, V, density)    — seeds per grid cell (default 1)\nstreamplot(X, Y, U, V, \"title\")\nstreamplot(X, Y, U, V, seeds)      — explicit Nx2 seed matrix (x, y)\n\n  Integrates streamlines by RK4 forward and backward from each seed,\n  clipping at the domain boundary and terminating on NaN or near-zero\n  field magnitude. NaN entries in U or V end the trace locally.\n  Each streamline carries a midpoint arrowhead.\n  Honours hold on for overlay with imagesc / contour.\n\n  Terminal: not rendered (issues a one-time warning) — use savefig to view.\n  HTML:     scatter lines with null-separated polylines.\n  SVG/PNG:  plotters path per streamline.\n\nExample:\n  [X, Y] = meshgrid(linspace(-2, 2, 40), linspace(-2, 2, 40));\n  U = -Y; V = X; streamplot(X, Y, U, V);  savefig(\"stream.html\");" },
    // Figure controls
    HelpEntry { name: "figure",   brief: "Create/switch figures (returns numeric handle)",
        detail: "fig = figure()              — new figure, returns handle (numeric ID)\nfig = figure(\"file.html\")   — new figure in HTML output mode\nfigure(N)                   — switch to figure N (creates if needed)\n\nMultiple figures can coexist. Each figure has its own plot data,\nlabels, and output mode (TUI, HTML, or viewer).\n\nExamples:\n  fig1 = figure()\n  plot(sin(linspace(0,10,100)))\n  fig2 = figure(\"temp.html\")\n  plot(cos(linspace(0,10,100)))\n  figure(fig1)  % switch back to fig1" },
    HelpEntry { name: "hold",     brief: "Keep existing series when adding new ones",
        detail: "hold(\"on\")   — subsequent plot() calls overlay on the current subplot\nhold(\"off\")  — each plot() replaces the previous series (default)\nhold(1) / hold(0) also accepted" },
    HelpEntry { name: "grid",     brief: "Show or hide grid lines",
        detail: "grid(\"on\")   — enable grid lines (default)\ngrid(\"off\")  — disable grid lines\ngrid(1) / grid(0) also accepted" },
    HelpEntry { name: "viewer",   brief: "Connect/disconnect external rustlab-viewer (bare form shows status)",
        detail: "viewer            — show connection status and where the current figure will render\nviewer on         — connect to the default rustlab-viewer\nviewer on <name>  — connect to a named session (e.g. viewer on work)\nviewer off        — disconnect, return to terminal plotting\n\nWhen connected, all plot commands render in the external\nviewer window (egui) with zoom/pan/crosshairs instead of\nthe terminal. Start rustlab-viewer first, then type 'viewer on'.\n\nNamed sessions allow multiple viewers to run simultaneously:\n  Terminal 1:  rustlab-viewer --name filters\n  Terminal 2:  rustlab-viewer --name analysis\n  REPL 1:     viewer on filters\n  REPL 2:     viewer on analysis\n\nIf the viewer window is closed while connected, the next plot call\ndetects the lost connection, disconnects, warns, and falls back to\nterminal rendering for subsequent plots.\n\nRequires the 'viewer' feature (included in 'make install')." },
    HelpEntry { name: "xlabel",   brief: "Set x-axis label",
        detail: "xlabel(\"text\")  — sets the x-axis label on the current subplot" },
    HelpEntry { name: "ylabel",   brief: "Set y-axis label",
        detail: "ylabel(\"text\")  — sets the y-axis label on the current subplot" },
    HelpEntry { name: "title",    brief: "Set subplot title",
        detail: "title(\"text\")  — sets the title of the current subplot" },
    HelpEntry { name: "xlim",     brief: "Set x-axis limits",
        detail: "xlim([lo, hi])  — fixes the x-axis range on the current subplot" },
    HelpEntry { name: "ylim",     brief: "Set y-axis limits",
        detail: "ylim([lo, hi])  — fixes the y-axis range on the current subplot" },
    HelpEntry { name: "axis",     brief: "Lock visual aspect or set both axis limits",
        detail: "axis(\"equal\")                       — lock visual aspect to 1:1 (one data unit on x = one data unit on y)\naxis(\"auto\")                        — release the aspect lock (default)\naxis([xmin, xmax, ymin, ymax])      — set both axis limits at once\n\n  axis(\"equal\") is honored across all four rendering backends (SVG, Plotly\n  HTML, ratatui terminal, viewer). Use it when geometric shape matters —\n  e.g. nyquist plots, parametric circles, complex-plane scatters." },
    HelpEntry { name: "subplot",  brief: "Switch to a subplot panel",
        detail: "subplot(rows, cols, idx)  — divides the figure into rows×cols panels\n  idx is 1-based, counts left-to-right then top-to-bottom\n  Example: subplot(2, 1, 1)  — top panel of a 2-row layout" },
    HelpEntry { name: "legend",   brief: "Label series in the current subplot",
        detail: "legend(\"s1\", \"s2\", ...)  — assigns labels to series in the order they were added\n  Labels appear in the chart legend." },
    // I/O
    HelpEntry { name: "save", brief: "Save a variable to NPY, NPZ, CSV, or TOML",
        detail: "save(\"file.npy\", x)                          — single array, NumPy format\nsave(\"file.csv\", x)                          — single array, CSV text\nsave(\"file.toml\", s)                         — struct to TOML (settings/config)\nsave(\"file.npz\", \"a\", a, \"b\", b, ...)        — multiple named arrays\n\nNPY/NPZ files are compatible with numpy.load() in Python." },
    HelpEntry { name: "load", brief: "Load variables from NPY, NPZ, CSV, or TOML",
        detail: "load(\"file.npz\")              — loads ALL variables into the workspace (bare call only)\nload(\"file.npz\", \"varname\")   — returns one named array from the archive\nload(\"file.npy\")              — returns the array as a value\nload(\"file.csv\")              — returns scalar / vector / matrix\nload(\"file.toml\")             — returns a struct from a TOML file" },
    HelpEntry { name: "whos", brief: "List workspace variables or inspect an NPZ file",
        detail: "whos                          — list all workspace variables\nwhos(\"file.npz\")              — list arrays stored in an NPZ file\n  Shows name, type (real/complex), and size for each array." },
    // Language
    HelpEntry { name: "i / j", brief: "Imaginary unit constant  (0 + 1i)",
        detail: "i and j are both pre-defined constants equal to sqrt(-1)\n  Example: z = 3 + j*4   or   z = 3 + i*4" },
    HelpEntry { name: "pi",   brief: "π  (3.14159…)",  detail: "pi  — pre-defined constant" },
    HelpEntry { name: "e",    brief: "Euler's number (2.71828…)", detail: "e  — pre-defined constant" },
    HelpEntry { name: "Inf",  brief: "IEEE positive infinity",    detail: "Inf  — pre-defined constant (f64::INFINITY)\n  Useful with norm(v, Inf) for the infinity-norm." },
    HelpEntry { name: "NaN",  brief: "IEEE Not-a-Number",         detail: "NaN  — pre-defined constant (f64::NAN)\n  NaN != NaN is true (IEEE semantics)." },
    HelpEntry { name: "underscores", brief: "Underscore digit separators in numeric literals",
        detail: "Underscores can be used as digit separators for readability (like Rust, Python, C++).\n  x = 1_000_000       → 1000000\n  fs = 48_000          → 48000\n  y = 3.141_592_653    → 3.141592653\n  z = 1_234.567_89     → 1234.56789\n  w = 1_000e3          → 1000000\n  Underscores are stripped during parsing and have no effect on the value." },
    HelpEntry { name: "range", brief: "Range syntax: start:stop  or  start:step:stop",
        detail: "1:5       → [1, 2, 3, 4, 5]\n0:0.5:2   → [0, 0.5, 1.0, 1.5, 2.0]\nUse v(end) for last element." },
    HelpEntry { name: "index", brief: "1-based indexing: v(i)  or  v(1:3)",
        detail: "v(1)      — first element\nv(end)    — last element\nv(2:4)    — elements 2 through 4" },
    HelpEntry { name: "str_index", brief: "String indexing: s(i), s(1:5), s(:)",
        detail: "s = 'hello world'\ns(1)    → 'h'         single character (as string)\ns(1:5)  → 'hello'     substring via range\ns(:)    → 'hello world'  full copy\ns(end)  → 'd'         last character\n  1-based, consistent with vector indexing." },
    HelpEntry { name: "clear", brief: "Remove all variables and functions from the session",
        detail: "clear  — deletes every user-defined variable and function; built-in constants (j, pi, e) are kept\n  Works in both REPL and scripts. No parentheses needed." },
    HelpEntry { name: "clf", brief: "Clear current figure",
        detail: "clf  — reset the figure state (clear all subplot series, titles, labels)\n  Works in both REPL and scripts. No parentheses needed." },
    HelpEntry { name: "close", brief: "Dismiss figures (current, by ID, or all)",
        detail: "close            — dismiss the current figure\nclose all        — dismiss every open figure\nclose(N)         — dismiss figure with handle N (returned by `figure()`)\nclose(\"all\")     — same as `close all` (function-call form)\n\nWith the external rustlab-viewer connected, `close` also closes the\ncorresponding viewer window; `close all` clears every viewer window in\nthe session in one Reset message. The viewer connection itself stays\nopen — subsequent plots route to fresh viewer figures.\n\nClosing the active figure switches to the most-recently-used remaining\nfigure; closing the last one resets to a fresh anonymous figure routed\nto the terminal.\n\nNote: `figure_close(fig)` is a different builtin that releases an\nanimation `LiveFigure` handle (see `figure_live`). Use `close` for the\nregular figures returned by `figure()`." },
    HelpEntry { name: "compound_assign", brief: "Compound assignment operators (+=, -=, *=, /=)",
        detail: "x += expr   — equivalent to x = x + expr\nx -= expr   — equivalent to x = x - expr\nx *= expr   — equivalent to x = x * expr\nx /= expr   — equivalent to x = x / expr\n\n  s = 0\n  for i = 1:10\n    s += i\n  end" },
    // Structs
    HelpEntry { name: "struct", brief: "Create a struct from field-value pairs",
        detail: "struct(\"x\", 1, \"y\", 2)  — creates a struct with fields x=1, y=2\n  Access: s.x\n  Assign: s.z = 3  (auto-creates struct if s is undefined)" },
    HelpEntry { name: "isstruct", brief: "Test if a value is a struct",
        detail: "isstruct(x)  — returns true if x is a struct, false otherwise" },
    HelpEntry { name: "fieldnames", brief: "List field names of a struct",
        detail: "fieldnames(s)  — prints all field names of struct s" },
    HelpEntry { name: "isfield", brief: "Test if a struct has a given field",
        detail: "isfield(s, \"x\")  — returns true if struct s has field 'x'" },
    HelpEntry { name: "rmfield", brief: "Remove a field from a struct (returns new struct)",
        detail: "s2 = rmfield(s, \"x\")  — returns a copy of s with field 'x' removed" },
    // Output
    HelpEntry { name: "disp", brief: "Display a value (always prints newline)",
        detail: "disp(x)  — prints x followed by a newline\n  Equivalent to print(x) but guaranteed to end with \\n." },
    HelpEntry { name: "fprintf", brief: "Formatted print (C-style)",
        detail: "fprintf(fmt, arg1, arg2, ...)\n  Specifiers: %d %i %f %g %e %s %%\n  Flags:      - + 0 # , (comma inserts thousands separators)\n  Escapes:    \\n \\t \\\\\n  Width/precision: %8.2f  %-10s\n  Comma flag: fprintf(\"%,d\\n\", 1234567)  →  1,234,567\n              fprintf(\"%,.2f\\n\", 1234567.89)  →  1,234,567.89\n  Example: fprintf(\"x = %.3f\\n\", 3.14159)" },
    HelpEntry { name: "sprintf", brief: "Formatted string (C-style, returns string)",
        detail: "sprintf(fmt, arg1, arg2, ...)\n  Same format specifiers as fprintf, but returns the string instead of printing.\n  s = sprintf(\"%,.2f\", 1234567.89)  →  \"1,234,567.89\"" },
    HelpEntry { name: "commas", brief: "Format number with thousands separators",
        detail: "commas(x)  — format number with comma separators, returns string\n  commas(1234567)       →  \"1,234,567\"\n  commas(1234567.89)    →  \"1,234,567.89\"\n  commas(1234567.89, 2) →  \"1,234,567.89\"  (with precision)\n  commas(1234567, 0)    →  \"1,234,567\"" },
    // Formatting
    HelpEntry { name: "format", brief: "Set display format (short, long, hex, commas)",
        detail: "format short    — default display (4-6 digits)\n  format long     — full f64 precision (15 digits)\n  format hex      — IEEE-754 hex encoding of float bits\n  format commas   — thousands separators\n  format default  — alias for short\n  format          — show current mode\n  Example:\n    format long\n    x = pi\n    x = 3.141592653589793" },
    // Aggregates
    HelpEntry { name: "all", brief: "True if all elements are nonzero",
        detail: "all(v)  — true if every element of v is nonzero\n  Works on scalars, bools, and vectors." },
    HelpEntry { name: "any", brief: "True if any element is nonzero",
        detail: "any(v)  — true if at least one element of v is nonzero" },
    // Matrix analysis
    HelpEntry { name: "rank", brief: "Matrix rank (SVD threshold)",
        detail: "rank(M)  — number of linearly independent rows/columns\n  Uses SVD-based threshold: eps * max(size) * max_sv" },
    HelpEntry { name: "roots", brief: "Roots of a polynomial",
        detail: "roots(p)  — roots of polynomial with coefficients p (descending power)\n  roots([1, -3, 2])  →  [2, 1]  (roots of x²-3x+2)\n  roots([1, 2, 10])  →  [-1+3j, -1-3j]" },
    // Control Systems
    HelpEntry { name: "tf", brief: "Create a transfer function",
        detail: "tf(\"s\")              — Laplace variable s\ntf(num, den)         — TF from numerator/denominator coefficient vectors (descending power)\ntf(sys)              — convert state-space (from ss(...)) to TF (SISO; Faddeev–LeVerrier)\ntf(A, B, C, D)       — convert raw matrices to TF (SISO; sugar for tf(ss(A,B,C,D)))\n\nExample:\n  s = tf(\"s\")\n  G = 10 / (s^2 + 2*s + 10)\n  G = tf([10], [1, 2, 10])      % equivalent\n  G = tf([0,1; -4,-0.5], [0;1], [1,0], 0)   % from physics-derived (A,B,C,D)" },
    HelpEntry { name: "tfdata", brief: "Extract numerator and denominator from a TF",
        detail: "[num, den] = tfdata(G)  — numerator and denominator coefficient vectors\n  Coefficients are in descending-power order (index 0 = highest power).\n\nExample:\n  G = tf([1,2], [1,3,5])\n  [n, d] = tfdata(G)   % n = [1,2], d = [1,3,5]" },
    HelpEntry { name: "pole", brief: "Poles of a transfer function",
        detail: "pole(G)  — complex vector of closed-loop poles (roots of denominator)\n\nExample:\n  G = tf([10], [1, 2, 10])\n  p = pole(G)  % ≈ [-1+3j, -1-3j]" },
    HelpEntry { name: "zero", brief: "Zeros of a transfer function",
        detail: "zero(G)  — complex vector of transmission zeros (roots of numerator)\n\nExample:\n  G = tf([1, 1], [1, 2, 10])\n  z = zero(G)  % ≈ -1" },
    HelpEntry { name: "ss", brief: "Construct or convert to state-space",
        detail: "ss(G)            — convert transfer function to observable canonical form\nss(A, B, C, D)   — build state-space directly from matrices (SISO or MIMO shapes)\n\nValidation: A is n×n; B is n×m; C is p×n; D is p×m (scalar 0 broadcast to p×m).\nAccess fields: sys.A, sys.B, sys.C, sys.D\n\nExample:\n  G   = tf([10], [1, 2, 10])\n  sys = ss(G)\n  sys = ss([0,1; -4,-0.5], [0;1], [1,0], 0)" },
    HelpEntry { name: "ctrb", brief: "Controllability matrix",
        detail: "ctrb(A, B)  — [B, AB, A²B, …]  (n × n·p matrix)\n\nFull column rank ↔ system is controllable.\n\nExample:\n  sys = ss(G)\n  Wc  = ctrb(sys.A, sys.B)\n  rank(Wc)   % should equal n for controllable system" },
    HelpEntry { name: "obsv", brief: "Observability matrix",
        detail: "obsv(A, C)  — [C; CA; CA²; …]  (n·q × n matrix)\n\nFull row rank ↔ system is observable.\n\nExample:\n  sys = ss(G)\n  Wo  = obsv(sys.A, sys.C)\n  rank(Wo)" },
    HelpEntry { name: "bode", brief: "Bode magnitude and phase plot",
        detail: "bode(G)         — plot magnitude (dB) and phase (deg) vs log10(ω)\nbode(G, w)      — use supplied frequency vector w (rad/s)\n[mag, ph, w] = bode(G)  — return data without plotting\n\nExample:\n  G = tf([10], [1, 2, 10])\n  bode(G)\n  [m, p, w] = bode(G)" },
    HelpEntry { name: "nyquist", brief: "Nyquist plot of L(jω) in the complex plane",
        detail: "nyquist(G)                — plot L(jω) vs L(-jω) (closed contour)\nnyquist(G, w)             — supply the frequency grid (rad/s)\nnyquist(G, \"pos-only\")    — omit the negative-frequency mirror\n[re, im, w] = nyquist(G)  — return positive-frequency locus\n\nThe -1 marker, equal aspect, and densification near s = -1 are all\nautomatic. Use it for stability margins (encirclements, sensitivity\npeak 1/|1+L|), Kalman frequency-domain inequality verification, and\nloop shaping. Accepts tf or ss inputs.\n\nExample:\n  L = tf([1], [1, 0.3, 1])\n  nyquist(L)\n  [re, im, w] = nyquist(L)" },
    HelpEntry { name: "step", brief: "Step response plot",
        detail: "step(G)              — plot unit step response\n[y, t] = step(G)     — return output and time vectors\n[y, t] = step(G, tf) — specify final time\n\nExample:\n  G = tf([10], [1, 2, 10])\n  step(G)\n  [y, t] = step(G, 5)" },
    HelpEntry { name: "margin", brief: "Gain and phase margins",
        detail: "[Gm, Pm, Wcg, Wcp] = margin(G)\n  Gm  — gain margin (linear ratio)\n  Pm  — phase margin (degrees)\n  Wcg — gain crossover frequency (rad/s)\n  Wcp — phase crossover frequency (rad/s)\n\nExample:\n  G = tf([10], [1, 2, 10])\n  [Gm, Pm, Wcg, Wcp] = margin(G)" },
    HelpEntry { name: "lqr", brief: "Linear-Quadratic Regulator design",
        detail: "[K, S, e] = lqr(sys, Q, R)\n  sys — state-space system (from ss())\n  Q   — state weighting matrix (n×n, positive semi-definite)\n  R   — input weighting matrix (m×m, positive definite)\n  K   — optimal gain matrix\n  S   — Riccati solution (cost matrix)\n  e   — closed-loop eigenvalues\n\nSolves the continuous-time algebraic Riccati equation (CARE).\n\nExample:\n  sys = ss(tf([1], [1, 0, 0]))   % double integrator\n  [K, S, e] = lqr(sys, eye(2), 1)" },
    HelpEntry { name: "rlocus", brief: "Root locus plot",
        detail: "rlocus(G)  — plot closed-loop pole trajectories as loop gain K sweeps 0 → ∞\n\nEach coloured path shows where one pole moves as K increases.\nOpen-loop poles are the starting points (K=0).\n\nExample:\n  s = tf(\"s\")\n  G = 1 / (s * (s + 1))\n  rlocus(G)" },
    // Control flow
    HelpEntry { name: "if", brief: "Conditional branching",
        detail: "if cond\n  body\nend\n\nif cond\n  then_body\nelseif cond2\n  body2\nelse\n  else_body\nend\n\nSingle-line form:  if cond, body; end\nCondition may be a Bool or scalar (0 = false, nonzero = true)." },
    HelpEntry { name: "for", brief: "Iterate over a range or vector",
        detail: "for i = 1:10\n  body\nend\n\nfor i = 1:step:stop\n  body\nend\n\nfor i = some_vector\n  body\nend\n\n  The loop variable stays in scope after end.\n  Use reverse step for countdown: for i = n:-1:1" },
    HelpEntry { name: "index_assign", brief: "Assign to a vector or matrix element",
        detail: "v(i) = val       — 1-based; vector auto-created and grown as needed\nM(r, c) = val   — matrix must already exist with sufficient size\n\nExample:\n  for i = 1:5\n    x(i) = i ^ 2\n  end\n  # x = [1, 4, 9, 16, 25]" },
    HelpEntry { name: "chained_index", brief: "Index a function return value inline",
        detail: "f(args)(i)  — no temporary variable needed\n\nExample:\n  v = linspace(0, 1, 10)(3)   # third element\n  loss = gd_step(w, b, x, y)(3)" },
    HelpEntry { name: "switch", brief: "Match a value against cases",
        detail: "switch expr\n  case val1\n    body1\n  case val2\n    body2\n  otherwise\n    default_body\nend\n\nExecutes the first matching case. Falls through to 'otherwise' if no case matches." },
    HelpEntry { name: "elseif", brief: "Chained conditional (used inside if)",
        detail: "if cond1\n  body1\nelseif cond2\n  body2\nelseif cond3\n  body3\nelse\n  default\nend\n\nMultiple elseif arms are allowed; first true condition wins." },
    HelpEntry { name: "error", brief: "Halt execution with an error message",
        detail: "error('msg')  — stop the script and display the message\n  error('Invalid input')  → runtime error: Invalid input" },
    HelpEntry { name: "sleep", brief: "Pause execution for a duration in seconds",
        detail: "sleep(seconds)\n  sleep(0.01)   — pause for 10 ms\n  sleep(1.5)    — pause for 1.5 seconds\n\nUseful for real-time control loops and animation pacing." },
    // User-defined functions
    HelpEntry { name: "function", brief: "Define a named function",
        detail: "function y = foo(x)\n  y = x * 2\nend\n\nfunction bar(a, b)\n  print(a + b)\nend\n\nSyntax:\n  function retvar = name(param1, param2, ...)\n    body\n  end\n  function name(param, ...)   % no return value\n    body\n  end\n\nuse 'return' to exit early." },
    // Filesystem / script loading
    HelpEntry { name: "run", brief: "Run a .rlab script file in the current session",
        detail: "run <file>  — execute a script file; its variables and functions merge into the current scope\n  Works in both the REPL and inside .rlab scripts (for sourcing shared functions).\n  Example: run calculate_helpers.rlab" },
    HelpEntry { name: "ls",  brief: "List directory contents",
        detail: "ls          — list current directory\nls <path>   — list the given directory" },
    HelpEntry { name: "cd",  brief: "Change working directory",
        detail: "cd          — change to home directory\ncd <path>   — change to the given path" },
    HelpEntry { name: "pwd", brief: "Print working directory",
        detail: "pwd  — show the current working directory" },
    // Math (additional)
    HelpEntry { name: "atan2", brief: "Two-argument inverse tangent  atan2(y, x)",
        detail: "atan2(y, x)  — angle in radians in the range (-π, π]\n  Element-wise; accepts scalars, vectors, or matrices.\n  atan2(1, 1)   →  π/4\n  atan2(0, -1)  →  π" },
    HelpEntry { name: "prod", brief: "Product of all elements",
        detail: "prod(v)  — product of every element in v; returns a scalar\n  prod([1, 2, 3, 4])  →  24\n  prod([1:5])         →  120" },
    HelpEntry { name: "logspace", brief: "Logarithmically spaced vector",
        detail: "logspace(a, b, n)  — n points from 10^a to 10^b (inclusive)\n  Equivalent to 10 .^ linspace(a, b, n)\n  logspace(0, 3, 4)  →  [1, 10, 100, 1000]" },
    HelpEntry { name: "meshgrid", brief: "Create 2-D coordinate matrices from two vectors",
        detail: "[X, Y] = meshgrid(x, y)\n  x — length-m vector (column values)\n  y — length-n vector (row values)\n  Returns Tuple [X, Y] where X and Y are n×m matrices.\n  X[i,j] = x[j]  (x repeats across rows)\n  Y[i,j] = y[i]  (y repeats across columns)\n\nExample:\n  [X, Y] = meshgrid(1:3, 1:2)\n  X  →  [1,2,3; 1,2,3]\n  Y  →  [1,1,1; 2,2,2]" },
    // Geometry / shape rasterization masks
    HelpEntry { name: "rect_mask", brief: "Axis-aligned rectangle mask on a meshgrid",
        detail: "M = rect_mask(X, Y, x0, y0, w, h)\n  X, Y — meshgrid coordinate matrices (same shape).\n  x0, y0 — rectangle origin (lower-left corner).\n  w, h   — width and height (must be finite, non-negative).\n  Returns ny×nx matrix with 1.0 inside [x0, x0+w] × [y0, y0+h] (inclusive)\n  and 0.0 outside.\n\nCompose with element-wise math:\n  M1 .* M2     intersection\n  1 - M        complement\n  max(M1, M2)  union\n\nExample:\n  [X, Y] = meshgrid(linspace(0,1,21), linspace(0,1,21))\n  M = rect_mask(X, Y, 0.25, 0.25, 0.5, 0.5)" },
    HelpEntry { name: "disk_mask", brief: "Closed-disk mask on a meshgrid",
        detail: "M = disk_mask(X, Y, xc, yc, r)\n  X, Y — meshgrid coordinate matrices (same shape).\n  xc, yc — disk centre.\n  r      — radius (finite, non-negative; r=0 matches the centre cell only).\n  Returns ny×nx matrix with 1.0 where (X-xc)^2 + (Y-yc)^2 ≤ r^2 and 0.0 elsewhere.\n\nExample:\n  [X, Y] = meshgrid(linspace(-1.5,1.5,200), linspace(-1.5,1.5,200))\n  D = disk_mask(X, Y, 0, 0, 1)\n  area = sum(sum(D)) * (3/199)^2   # ≈ π" },
    HelpEntry { name: "polygon_mask", brief: "Polygon mask via even-odd ray casting",
        detail: "M = polygon_mask(X, Y, verts)\n  X, Y — meshgrid coordinate matrices (same shape).\n  verts — N×2 matrix; each row is [x, y]. Polygon is implicitly closed.\n  Returns ny×nx matrix with 1.0 inside the polygon and 0.0 outside.\n\nDegenerate inputs (fewer than 3 vertices, or all vertices collinear) return an\nall-zero mask. Behaviour at points exactly on a polygon edge is implementation-\ndefined.\n\nExample:\n  [X, Y] = meshgrid(linspace(0,1,50), linspace(0,1,50))\n  T = polygon_mask(X, Y, [0 0; 1 0; 0.5 1])   # right triangle" },
    // Vector calculus
    HelpEntry { name: "gradient", brief: "Gradient of a scalar field on a uniform 2-D grid",
        detail: "[Fx, Fy] = gradient(F)\n[Fx, Fy] = gradient(F, dx, dy)\n  F   — ny×nx scalar field (real or complex). Rows index y, columns index x.\n  dx, dy — grid spacing (default 1.0). Both must be > 0.\n  Returns Tuple [Fx, Fy] same shape as F.\n  Stencils: 2nd-order central interior, 2nd-order one-sided at boundaries.\n  Each axis must have length ≥ 3.\n\nExample:\n  [X, Y] = meshgrid(linspace(-1,1,21), linspace(-1,1,21))\n  F = X.^2 + Y.^2\n  [Fx, Fy] = gradient(F, 0.1, 0.1)" },
    HelpEntry { name: "divergence", brief: "Divergence of a 2-D vector field  ∂Fx/∂x + ∂Fy/∂y",
        detail: "D = divergence(Fx, Fy)\nD = divergence(Fx, Fy, dx, dy)\n  Fx, Fy — ny×nx components on the same grid (same shape).\n  dx, dy — grid spacing (default 1.0).\n  Returns scalar field D, same shape as Fx.\n  Same stencils and shape requirements as gradient.\n\nExample:\n  D = divergence(Fx, Fy, 0.1, 0.1)" },
    HelpEntry { name: "curl", brief: "Scalar curl of a 2-D vector field  ∂Fy/∂x − ∂Fx/∂y",
        detail: "Cz = curl(Fx, Fy)\nCz = curl(Fx, Fy, dx, dy)\n  Returns the z-component of ∇×F (a scalar field, same shape as Fx).\n  Same stencils and shape requirements as gradient.\n\nExample:\n  Cz = curl(Fx, Fy, 0.1, 0.1)" },
    HelpEntry { name: "gradient3", brief: "Gradient of a scalar field on a uniform 3-D grid",
        detail: "[Fx, Fy, Fz] = gradient3(F)\n[Fx, Fy, Fz] = gradient3(F, dx, dy, dz)\n  F — m×n×p Tensor3 (real or complex).\n    Axis 0 = y (rows), axis 1 = x (cols), axis 2 = z (pages).\n  dx, dy, dz — grid spacing (default 1.0). All must be > 0.\n  Returns Tuple [Fx, Fy, Fz], each a Tensor3 of the same shape as F.\n  Stencils: 2nd-order central interior, 2nd-order one-sided at boundaries.\n  Each axis must have length ≥ 3.\n\nExample:\n  T = reshape(1:60, 3, 4, 5)\n  [Fx, Fy, Fz] = gradient3(T, 0.1, 0.1, 0.1)" },
    HelpEntry { name: "divergence3", brief: "3-D divergence  ∂Fx/∂x + ∂Fy/∂y + ∂Fz/∂z",
        detail: "D = divergence3(Fx, Fy, Fz)\nD = divergence3(Fx, Fy, Fz, dx, dy, dz)\n  Fx, Fy, Fz — Tensor3 components of the same shape.\n  Returns a Tensor3 scalar field, same shape as Fx.\n  Same stencils and shape requirements as gradient3." },
    HelpEntry { name: "curl3", brief: "3-D curl  ∇×F  → (Cx, Cy, Cz)",
        detail: "[Cx, Cy, Cz] = curl3(Fx, Fy, Fz)\n[Cx, Cy, Cz] = curl3(Fx, Fy, Fz, dx, dy, dz)\n  Returns Tuple [Cx, Cy, Cz] with each component a Tensor3 of the same shape as Fx.\n    Cx = ∂Fz/∂y − ∂Fy/∂z\n    Cy = ∂Fx/∂z − ∂Fz/∂x\n    Cz = ∂Fy/∂x − ∂Fx/∂y\n  Same stencils and shape requirements as gradient3." },
    // DSP (additional)
    HelpEntry { name: "filtfilt", brief: "Zero-phase forward-backward filter",
        detail: "filtfilt(b, a, x)\n  b — numerator coefficients (FIR: filter taps)\n  a — denominator coefficients (FIR: use [1])\n  x — real input signal vector\n\nApplies the filter forward then backward so phase distortion cancels exactly.\nEffective filter order is doubled; no startup transient.\n\nExample (FIR lowpass):\n  h = fir_lowpass(63, 2000, 44100, \"hann\")\n  y = filtfilt(h, [1], x)" },
    HelpEntry { name: "firpmq", brief: "Integer-coefficient Parks-McClellan equiripple FIR",
        detail: "firpmq(n_taps, bands, desired)\nfirpmq(n_taps, bands, desired, weights)\nfirpmq(n_taps, bands, desired, weights, bits)\nfirpmq(n_taps, bands, desired, weights, bits, n_iter)\n  bands   — normalized frequency edges [0,1], 1 = Nyquist; pairs define each band\n  desired — target amplitude at each band edge (piecewise-linear)\n  weights — per-band weights (default: all 1.0)\n  bits    — coefficient word width (default: 16)\n  n_iter  — optimization iterations (default: 8)\n\nReturns integer-valued taps. For unit-gain passband, sum(h_int) is the scale\nfactor — use freqz(h_int / sum(h_int), ...) to verify.\n\nExample (lowpass): firpmq(63, [0,0.2,0.3,1], [1,1,0,0])" },
    // Linear algebra (additional)
    HelpEntry { name: "svd", brief: "Singular Value Decomposition  A = U·diag(σ)·V'",
        detail: "svd(A)  — Jacobi SVD (real matrices)\n  Returns Tuple [U, sigma, V] where:\n    U     — left singular vectors (m×m orthogonal)\n    sigma — singular values as a vector (descending order)\n    V     — right singular vectors (n×n orthogonal)\n\nReconstruction: U * diag(sigma) * V'  ≈  A\n\nExample:\n  [U, s, V] = svd(A)\n  rank_est = sum(s .> 1e-10)   % numerical rank" },
    // Controls (additional)
    HelpEntry { name: "rk4", brief: "Fixed-step 4th-order Runge-Kutta ODE solver",
        detail: "rk4(f, x0, t)\n  f  — function f(x, t) → x_dot (state derivative); use @(x,t) ...\n  x0 — initial state (scalar or vector)\n  t  — uniformly spaced time vector\n\nReturns:\n  scalar x0 → Vector of states at each time step\n  vector x0 → n×T matrix (rows = states, columns = time steps)\n\nExample:\n  f = @(x, t) -x\n  t = linspace(0, 5, 100)\n  y = rk4(f, 1.0, t)" },
    HelpEntry { name: "lyap", brief: "Solve the continuous Lyapunov equation  A*X + X*A' + Q = 0",
        detail: "lyap(A, Q)  — solves A*X + X*A' + Q = 0 for X\n  A — n×n real square matrix (must be stable: all eigenvalues have negative real part)\n  Q — n×n real symmetric positive semi-definite matrix\n\nUses Kronecker vectorization. Practical for n ≤ 50.\n\nExample:\n  A = [-1, 0; 0, -2]\n  Q = eye(2)\n  X = lyap(A, Q)" },
    HelpEntry { name: "gram", brief: "Controllability or observability Gramian",
        detail: "gram(A, B, \"c\")  — controllability Gramian: solve A*Wc + Wc*A' + B*B' = 0\ngram(A, C, \"o\")  — observability Gramian:  solve A'*Wo + Wo*A + C'*C = 0\n  Third argument is the string \"c\" or \"o\".\n\nEigenvalues of the Gramian indicate how controllable/observable each mode is.\nSolved via lyap().\n\nExample:\n  sys = ss(tf([1], [1, 2, 1]))\n  Wc  = gram(sys.A, sys.B, \"c\")" },
    HelpEntry { name: "care", brief: "Solve the Continuous Algebraic Riccati Equation",
        detail: "care(A, B, Q, R)  — solves A'*P + P*A - P*B*inv(R)*B'*P + Q = 0\n  A — n×n system matrix\n  B — n×m input matrix\n  Q — n×n state cost (positive semi-definite)\n  R — m×m input cost (positive definite)\n\nReturns P (the cost matrix). Optimal LQR gain: K = inv(R)*B'*P\n\nExample:\n  sys = ss(tf([1], [1, 0, 0]))\n  P = care(sys.A, sys.B, eye(2), 1)" },
    HelpEntry { name: "dare", brief: "Solve the Discrete Algebraic Riccati Equation",
        detail: "dare(A, B, Q, R)  — solves P = A'*P*A - A'*P*B*inv(R+B'*P*B)*B'*P*A + Q\n  A — n×n discrete-time system matrix\n  B — n×m input matrix\n  Q — n×n state cost (positive semi-definite)\n  R — m×m input cost (positive definite)\n\nReturns P. Optimal discrete LQR gain: K = inv(R + B'*P*B)*B'*P*A\n\nExample:\n  P = dare(Ad, Bd, eye(2), 1)" },
    HelpEntry { name: "place", brief: "Ackermann pole placement (SISO)",
        detail: "place(A, B, poles)  — state feedback gain K such that eig(A - B*K) = poles\n  A     — n×n system matrix\n  B     — n×1 input vector (SISO only)\n  poles — desired closed-loop pole locations (complex vector, length n)\n\nReturns K as a row vector. Uses Ackermann's formula.\n\nExample:\n  sys = ss(tf([1], [1, 0, 0]))\n  K   = place(sys.A, sys.B, [-1+j, -1-j])" },
    HelpEntry { name: "freqresp", brief: "Frequency response of a state-space system at given frequencies",
        detail: "freqresp(A, B, C, D, w)  — evaluate H(jω) at each frequency in w\n  A, B, C, D — state-space matrices (from ss())\n  w          — frequency vector (rad/s), e.g. logspace(-1, 2, 200)\n\nSISO: returns complex Vector (one value per frequency)\nMIMO: returns complex Matrix\n\nH(jω) = C*(jω*I - A)^-1*B + D\n\nExample:\n  sys = ss(tf([10], [1, 2, 10]))\n  w   = logspace(-1, 2, 200)\n  H   = freqresp(sys.A, sys.B, sys.C, sys.D, w)" },
    // Higher-order / meta
    HelpEntry { name: "arrayfun", brief: "Map a callable over every element of a vector",
        detail: "arrayfun(f, v)  — applies f to each element of v\n  f may be a lambda (@(x) ...), a function handle (@sin), or a user function.\n\nOutput rules:\n  All scalar outputs   → Vector\n  Equal-length vectors → Matrix (one row per input element)\n\nExample:\n  arrayfun(@(x) x^2, [1,2,3,4])  →  [1, 4, 9, 16]\n  arrayfun(@sin, linspace(0, pi, 5))" },
    HelpEntry { name: "feval", brief: "Call a function by string name",
        detail: "feval(\"name\", arg1, arg2, ...)  — invoke any builtin or user function by name\n  Useful for dynamic/generic dispatch.\n\nExample:\n  feval(\"sin\", pi/2)   →  1.0\n  feval(\"my_fn\", x)" },
    // Profiling
    HelpEntry { name: "profile", brief: "Enable in-script call profiling",
        detail: "profile(fn1, fn2, ...)  — track only the named functions\nprofile()              — track all function calls\n\nStats accumulate across multiple calls to profile().\nA final report is printed to stderr on script exit.\nFor CLI-flag profiling without source changes: rustlab run --profile script.rlab" },
    HelpEntry { name: "profile_report", brief: "Print the accumulated profiling table to stderr",
        detail: "profile_report()  — prints the profiling table at this point in the script\n  Useful for mid-script snapshots.\n  A final report is always printed automatically at script exit when profiling is active." },
    // Streaming DSP
    HelpEntry { name: "state_init", brief: "Allocate a FIR history buffer of n zeros",
        detail: "state_init(n)  — allocate FIR state for a filter with n+1 taps\n  n = length(h) - 1  where h is the coefficient vector\n\nReturns an opaque fir_state handle. Pass it to filter_stream each frame.\nTwo independent handles allow stereo (or any multi-channel) processing\nwith no shared state.\n\nExample:\n  h  = firpm(64, [0, 0.04, 0.05, 1.0], [1, 1, 0, 0])\n  st = state_init(length(h) - 1)" },
    HelpEntry { name: "filter_stream", brief: "Overlap-save FIR filtering — one frame at a time",
        detail: "filter_stream(frame, h, state)  →  [output_frame, state]\n  frame  — input samples (Vector, length N)\n  h      — FIR coefficients (Vector, length M)\n  state  — fir_state handle from state_init(length(h)-1)\n\nReturns a Tuple: output frame (length N) and the updated state handle.\nThe state is mutated in place — no heap reallocation per frame.\nOutput matches convolve(full_signal, h) to within floating-point precision.\n\nRun with external audio bridge:\n  sox -d -t raw -r 44100 -e float -b 32 -c 1 - \\\n    | rustlab run filter.rlab \\\n    | sox -t raw -r 44100 -e float -b 32 -c 1 - -d\n\nExample:\n  [out, st] = filter_stream(frame, h, st)" },
    // Audio I/O
    HelpEntry { name: "audio_in", brief: "Create a stdin PCM input handle",
        detail: "audio_in(sr, n)  — metadata handle for reading audio from stdin\n  sr — sample rate in Hz (e.g. 44100.0)\n  n  — frame size in samples (e.g. 256)\n\nOpens no hardware. audio_read(adc) reads n × 4 bytes of f32-LE PCM\nfrom stdin and blocks until the full frame arrives.\n\nExample:\n  adc = audio_in(44100.0, 256)" },
    HelpEntry { name: "audio_out", brief: "Create a stdout PCM output handle",
        detail: "audio_out(sr, n)  — metadata handle for writing audio to stdout\n  sr — sample rate in Hz\n  n  — frame size in samples\n\nOpens no hardware. audio_write(dac, frame) writes n × 4 bytes of f32-LE PCM\nto stdout (real part only).\n\nExample:\n  dac = audio_out(44100.0, 256)" },
    HelpEntry { name: "audio_read", brief: "Read one frame of f32-LE PCM from stdin",
        detail: "audio_read(adc)  — blocking read of one frame from stdin\n  adc — audio_in handle\n\nBlocks until the full frame is available. Returns a real-valued Vector.\nIf stdin closes, raises a runtime error and the script exits cleanly.\n\nExample:\n  frame = audio_read(adc)" },
    HelpEntry { name: "audio_write", brief: "Write one frame of f32-LE PCM to stdout",
        detail: "audio_write(dac, frame)  — write one frame to stdout\n  dac   — audio_out handle\n  frame — Vector of samples (real part written as f32-LE)\n\nFlushes stdout after each frame so the downstream consumer receives\ndata promptly.\n\nExample:\n  audio_write(dac, out)" },
    // Live plotting
    HelpEntry { name: "figure_live", brief: "Open a persistent live terminal plot",
        detail: "figure_live(rows, cols)  — create a live figure with rows × cols panels\n  rows, cols — grid dimensions\n\nKeeps the alternate screen open across multiple draw calls.\nErrors if stdout is not a real tty.\n\nExample:\n  fig = figure_live(2, 1)" },
    HelpEntry { name: "plot_update", brief: "Update panel data (no immediate redraw)",
        detail: "plot_update(fig, panel, y)      — auto x-axis (1..N)\nplot_update(fig, panel, x, y)  — explicit x-axis\n  panel — 1-based index\n\nCall figure_draw(fig) after updating all panels for one atomic refresh.\n\nExample:\n  plot_update(fig, 1, frame)\n  plot_update(fig, 2, freqs, mag2db(X))" },
    HelpEntry { name: "plot_labels", brief: "Set title and axis labels on a live panel",
        detail: "plot_labels(fig, panel, title, xlabel, ylabel)\n  panel — 1-based index\n\nLabels persist across redraws. Set once after figure_live().\n\nExample:\n  plot_labels(fig, 1, \"Spectrum\", \"Frequency (Hz)\", \"Magnitude (dB)\")" },
    HelpEntry { name: "plot_limits", brief: "Set fixed axis limits on a live panel",
        detail: "plot_limits(fig, panel, xlim, ylim)\n  panel — 1-based index\n  xlim, ylim — [lo, hi] vectors\n\nExample:\n  plot_limits(fig, 1, [0, 22050], [-120, 0])" },
    HelpEntry { name: "figure_draw", brief: "Redraw all panels to the terminal",
        detail: "figure_draw(fig)  — one atomic screen refresh\n\nCall after all plot_update calls to avoid partial-state flicker.\n\nExample:\n  figure_draw(fig)" },
    HelpEntry { name: "figure_close", brief: "Close live figure and restore terminal",
        detail: "figure_close(fig)  — drop live figure, restore normal terminal\n\nAlso fires automatically on script end or Ctrl-C via Drop.\n\nExample:\n  figure_close(fig)" },
    HelpEntry { name: "mag2db", brief: "Convert magnitude to dB: 20·log10(|X|)",
        detail: "mag2db(X)  — element-wise, floored at −200 dB (1e-10 floor)\n  X — scalar, complex, vector, or matrix\n\nExamples:\n  mag2db(1.0)         % 0 dB\n  mag2db(0.0)         % -200 dB\n  mag2db(fft(frame))  % spectrum in dB" },
    // Cell / string arrays
    HelpEntry { name: "iscell", brief: "True if argument is a string array",
        detail: "iscell(x) — returns true if x is a string array ({...}), false otherwise\n\nExamples:\n  iscell({'a', 'b'})  % true\n  iscell([1, 2])      % false" },
    // Sparse
    HelpEntry { name: "sparse", brief: "Build sparse matrix or convert dense→sparse",
        detail: "sparse(I, J, V, m, n)  — build m×n sparse matrix from 1-based row/col/value vectors\nsparse(A)              — convert dense matrix/vector to sparse (drops near-zeros)\n\nDuplicate (i,j) entries are summed.\n\nExamples:\n  S = sparse([1,2,3], [1,2,3], [10,20,30], 3, 3)\n  S2 = sparse(eye(3))" },
    HelpEntry { name: "sparsevec", brief: "Build sparse vector from indices and values",
        detail: "sparsevec(I, V, n)  — build sparse vector of length n\n  I — 1-based index vector\n  V — value vector (same length as I)\n  n — total length\n\nExample:\n  sv = sparsevec([1, 5, 9], [1.0, -2.0, 3.0], 10)" },
    HelpEntry { name: "speye", brief: "Sparse identity matrix",
        detail: "speye(n)  — n×n sparse identity matrix (nnz = n)\n\nExample:\n  I5 = speye(5)" },
    HelpEntry { name: "spzeros", brief: "All-zero sparse matrix",
        detail: "spzeros(m, n)  — m×n sparse matrix with no stored entries\n\nExample:\n  Z = spzeros(100, 100)" },
    HelpEntry { name: "nnz", brief: "Number of non-zero entries",
        detail: "nnz(S)  — number of stored non-zero entries\n  For dense inputs, returns numel.\n\nExample:\n  nnz(speye(5))  → 5" },
    HelpEntry { name: "issparse", brief: "Test if value is sparse",
        detail: "issparse(x)  — returns 1 if x is a sparse vector or matrix, 0 otherwise\n\nExample:\n  issparse(speye(3))  → 1\n  issparse(eye(3))    → 0" },
    HelpEntry { name: "full", brief: "Convert sparse to dense",
        detail: "full(S)  — convert sparse vector/matrix to dense\n  Dense inputs pass through unchanged.\n\nExample:\n  D = full(speye(3))  → 3×3 identity matrix" },
    HelpEntry { name: "nonzeros", brief: "Extract non-zero values from sparse",
        detail: "nonzeros(S)  — return a vector of the stored non-zero values (in storage order)\n\nExample:\n  nonzeros(speye(3))  → [1, 1, 1]" },
    HelpEntry { name: "find", brief: "Find non-zero element positions (nargout-aware)",
        detail: "find(v)            — dense vector → 1-based element indices\nfind(M)            — dense matrix → 1-based column-major linear indices\n[I, V] = find(v)   — indices + values\n[I, J] = find(M)   — row + column subscripts (column-major order)\n[I, J, V] = find(M) — adds the nonzero values\nfind(S)            — sparse vector → tuple [I, V]\nfind(S)            — sparse matrix → tuple [I, J, V]\n\nDense matrix linear indexing follows the octave/matlab convention:\nelement M(i, j) sits at linear index (j - 1) * nrows + i.\n\nExamples:\n  find([0, 5, 0, -3])      → [2, 4]\n  find([0, 2; 3, 0])       → [2, 3]\n  [I, J] = find([0, 2; 3, 0])     % I=[2,1], J=[1,2]\n  [I, J, V] = find([0, 2; 3, 0])  % adds V=[3, 2]" },
    HelpEntry { name: "spsolve", brief: "Solve sparse linear system  A*x = b",
        detail: "spsolve(A, b)\nspsolve(A, b, mode)\n  A    — sparse or dense square matrix\n  b    — right-hand side vector\n  mode — \"auto\" (default), \"cholesky\", or \"lu\"\n\nDispatch:\n  \"auto\"     — detect Hermitian-positive-definite structure; route SPD\n              inputs through hand-rolled sparse Cholesky, others\n              through hand-rolled sparse LU with partial pivoting.\n  \"cholesky\" — force the sparse Cholesky path; errors if A is not SPD.\n  \"lu\"       — force the sparse LU path (always works, slightly more fill).\n\nBoth paths stay sparse end-to-end and use AMD ordering by default.\nDense Value::Matrix input still uses the legacy dense Gaussian elimination.\n\nExamples:\n  x = spsolve(speye(3), [1, 2, 3])\n  L = -1 * laplacian_2d(50, 50);\n  x = spsolve(L, ones(2500, 1));               % auto picks Cholesky\n  x = spsolve(L, ones(2500, 1), \"cholesky\");   % explicit\n\n  A = [1, 2; 2, 1];                            % indefinite\n  x = spsolve(sparse(A), [1; 1]);              % auto routes through LU\n  x = spsolve(sparse(A), [1; 1], \"lu\");        % explicit" },
    HelpEntry { name: "spdiags", brief: "Build sparse matrix from diagonals",
        detail: "spdiags(V, D, m, n)  — place diagonals into an m×n sparse matrix\n  V — vector (single diag) or matrix (one column per diag)\n  D — scalar or vector of offsets (0=main, >0 super, <0 sub)\n\nExamples:\n  S = spdiags([1,2,3], 0, 3, 3)   — diagonal\n  T = spdiags([-ones(5,1), 2*ones(5,1), -ones(5,1)], [-1,0,1], 5, 5)" },
    HelpEntry { name: "sprand", brief: "Random sparse matrix with given density",
        detail: "sprand(m, n, density)  — m×n sparse matrix with ~density*m*n non-zeros\n  Values are uniform in [0, 1). Density must be in [0, 1].\n\nExample:\n  S = sprand(100, 100, 0.05)  → ~500 non-zeros" },
    HelpEntry { name: "laplacian_2d", brief: "5-point sparse Laplacian on a 2-D grid",
        detail: "laplacian_2d(nx, ny)                  — dx = dy = 1, Dirichlet\nlaplacian_2d(nx, ny, dx, dy)          — uniform spacing\nlaplacian_2d(nx, ny [, dx, dy], bc)   — bc = \"dirichlet\"|\"neumann\"|\"periodic\"\n\n  Returns an (nx*ny) × (nx*ny) sparse matrix L approximating +∇² on a\n  uniform grid. Sign: Poisson ∇²V = -rho/eps0 solves as V = spsolve(L, -rho/eps0).\n\n  Node ordering: column-major, V(i, j) → k = (j-1)*ny + i (1-based).\n  Use ij2k(i, j, ny) / k2ij(k, ny) for index sugar.\n\n  Boundary conditions:\n    \"dirichlet\" (default) — V = 0 outside grid; standard banded stencil.\n    \"neumann\"             — zero-flux; boundary cells absorb missing\n                            coefficient into diagonal. Constants in null space.\n    \"periodic\"            — wrap. Constants in null space.\n\nExample:\n  nx = 8; ny = 6;\n  L = laplacian_2d(nx, ny);\n  rho = zeros(ny, nx);  rho(ny/2, nx/2) = 1;\n  V = spsolve(L, -rho(:));  V_grid = reshape(V, ny, nx);" },
    HelpEntry { name: "laplacian_1d", brief: "Sparse tridiagonal Laplacian on a 1-D grid",
        detail: "laplacian_1d(n)                — dx = 1, Dirichlet\nlaplacian_1d(n, dx)            — explicit spacing\nlaplacian_1d(n [, dx], bc)     — bc = \"dirichlet\"|\"neumann\"|\"periodic\"\n\n  Returns an n × n sparse matrix approximating +d²/dx². Same boundary\n  semantics as laplacian_2d.\n\nExample:\n  L = laplacian_1d(64, 0.01, \"periodic\")" },
    HelpEntry { name: "laplacian_3d", brief: "7-point sparse Laplacian on a 3-D grid",
        detail: "laplacian_3d(nx, ny, nz)                       — unit spacing, Dirichlet\nlaplacian_3d(nx, ny, nz, dx, dy, dz)           — anisotropic spacing\nlaplacian_3d(nx, ny, nz [, dx, dy, dz], bc)    — bc selector\n\n  Returns an (nx*ny*nz) × (nx*ny*nz) sparse matrix on the Tensor3\n  grid (axis 0 = y, axis 1 = x, axis 2 = z). Flat index:\n    V(i, j, kk) → k = ((kk-1)*nx + (j-1))*ny + i  (1-based)\n  Use ijk2k / k2ijk for index sugar.\n\nExample:\n  L = laplacian_3d(8, 8, 8, \"neumann\")" },
    HelpEntry { name: "laplacian_eps_2d", brief: "Variable-coefficient Laplacian ∇·(ε∇)",
        detail: "laplacian_eps_2d(eps_map)                  — unit spacing, Dirichlet\nlaplacian_eps_2d(eps_map, dx, dy)          — uniform spacing\nlaplacian_eps_2d(eps_map [, dx, dy], bc)   — bc selector\n\n  Flux-conservative discretization with harmonic-mean half-cell\n  coefficients — preserves flux continuity across material interfaces.\n  eps_map is shape (ny, nx) matching meshgrid / imagesc; real or complex.\n\n  Setting eps_map ≡ 1 reproduces laplacian_2d. For magnetostatics use\n  1./mu_map. For lossy materials, use complex eps with negative imag.\n\nExample:\n  eps = ones(ny, nx);\n  eps(:, 1:nx/2) = 4.0;       % left half is dielectric\n  L = laplacian_eps_2d(eps, dx, dy)" },
    HelpEntry { name: "ij2k", brief: "Column-major grid (i, j) → flat index",
        detail: "ij2k(i, j, ny)  — return 1-based flat index k = (j-1)*ny + i\n  Third argument is ny (row count), not nx. Matches reshape(V_flat, ny, nx).\n\nExample:\n  k = ij2k(3, 4, 6)  → (4-1)*6 + 3 = 21" },
    HelpEntry { name: "k2ij", brief: "Column-major flat index → grid (i, j)",
        detail: "[i, j] = k2ij(k, ny)  — inverse of ij2k. 1-based.\n  i = ((k-1) mod ny) + 1\n  j = ((k-1) div ny) + 1\n\nExample:\n  [i, j] = k2ij(21, 6)  → i = 3, j = 4" },
    HelpEntry { name: "ijk2k", brief: "Column-major-of-pages 3-D (i, j, kk) → flat index",
        detail: "ijk2k(i, j, kk, ny, nx)  — return 1-based flat index\n  k = ((kk-1)*nx + (j-1))*ny + i\n  Last two arguments are ny (rows), nx (cols) — same as Tensor3 / laplacian_3d.\n\nExample:\n  k = ijk2k(2, 3, 4, 5, 6)  → 102" },
    HelpEntry { name: "k2ijk", brief: "Column-major-of-pages 3-D flat index → (i, j, kk)",
        detail: "[i, j, kk] = k2ijk(k, ny, nx)  — inverse of ijk2k. 1-based.\n\nExample:\n  [i, j, kk] = k2ijk(102, 5, 6)  → i = 2, j = 3, kk = 4" },
    HelpEntry { name: "plot_limits", brief: "Set axis limits for a live figure panel",
        detail: "plot_limits(fig, panel, xmin, xmax, ymin, ymax)  — fix axes for one panel\n\nExample:\n  plot_limits(fig, 1, 0, 1000, -100, 0)" },
];

fn whos_type(v: &rustlab_script::Value) -> &'static str {
    use rustlab_script::Value;
    match v {
        Value::Scalar(_) => "scalar",
        Value::Complex(_) => "complex",
        Value::Vector(_) => "vector",
        Value::Matrix(_) => "matrix",
        Value::Tensor3(_) => "tensor3",
        Value::Bool(_) => "bool",
        Value::Str(_) => "string",
        Value::QFmt(_) => "qfmt",
        Value::Struct(_) => "struct",
        Value::Tuple(_) => "tuple",
        Value::All => "all-index",
        Value::None => "none",
        Value::TransferFn { .. } => "tf",
        Value::StateSpace { .. } => "ss",
        Value::Lambda { .. } => "lambda",
        Value::FuncHandle(_) => "function_handle",
        Value::FirState(_) => "fir_state",
        Value::AudioIn { .. } => "audio_in",
        Value::AudioOut { .. } => "audio_out",
        Value::LiveFigure(_) => "live_figure",
        Value::SparseVector(_) => "sparse_vector",
        Value::SparseMatrix(_) => "sparse_matrix",
        Value::StringArray(_) => "string_array",
    }
}

fn whos_size(v: &rustlab_script::Value) -> String {
    use rustlab_script::Value;
    match v {
        Value::Vector(v) => format!("1×{}", v.len()),
        Value::Matrix(m) => format!("{}×{}", m.nrows(), m.ncols()),
        Value::Tensor3(t) => {
            let s = t.shape();
            format!("{}×{}×{}", s[0], s[1], s[2])
        }
        Value::Str(s) => format!("1×{}", s.len()),
        Value::Struct(f) => format!("1×1 ({} fields)", f.len()),
        Value::Tuple(v) => format!("1×{}", v.len()),
        Value::StateSpace { a, .. } => format!("{}×{}", a.nrows(), a.ncols()),
        Value::SparseVector(sv) => {
            let fill = if sv.len > 0 {
                100.0 * sv.nnz() as f64 / sv.len as f64
            } else {
                0.0
            };
            format!("1×{}, nnz={}, fill={:.0}%", sv.len, sv.nnz(), fill)
        }
        Value::SparseMatrix(sm) => {
            let total = sm.rows * sm.cols;
            let fill = if total > 0 {
                100.0 * sm.nnz() as f64 / total as f64
            } else {
                0.0
            };
            format!(
                "{}×{}, nnz={}, fill={:.0}%",
                sm.rows,
                sm.cols,
                sm.nnz(),
                fill
            )
        }
        Value::StringArray(v) => format!("1×{}", v.len()),
        Value::All => "—".to_string(),
        _ => "1×1".to_string(),
    }
}

fn whos_preview(v: &rustlab_script::Value) -> String {
    use rustlab_script::Value;
    match v {
        Value::Scalar(n) => format!("{n}"),
        Value::Complex(c) => {
            if c.im >= 0.0 {
                format!("{}+{}j", c.re, c.im)
            } else {
                format!("{}{}j", c.re, c.im)
            }
        }
        Value::Bool(b) => format!("{b}"),
        Value::Str(s) => {
            if s.len() <= 40 {
                format!("\"{s}\"")
            } else {
                format!("\"{}…\"", &s[..37])
            }
        }
        Value::Vector(v) => {
            let preview: Vec<String> = v
                .iter()
                .take(3)
                .map(|c| {
                    if c.im == 0.0 {
                        format!("{:.4}", c.re)
                    } else {
                        format!("{:.4}+{:.4}j", c.re, c.im)
                    }
                })
                .collect();
            let suffix = if v.len() > 3 { ", …" } else { "" };
            format!("[{}{}]", preview.join(", "), suffix)
        }
        Value::Matrix(m) => format!("[{}×{} matrix]", m.nrows(), m.ncols()),
        Value::Tensor3(t) => {
            let s = t.shape();
            format!("[{}×{}×{} tensor3]", s[0], s[1], s[2])
        }
        Value::Struct(f) => {
            let mut names: Vec<&str> = f.keys().map(|s| s.as_str()).collect();
            names.sort();
            let preview = names.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            let suffix = if names.len() > 3 { ", …" } else { "" };
            format!("{{{}{}}}", preview, suffix)
        }
        Value::QFmt(spec) => format!("{}", rustlab_script::Value::QFmt(spec.clone())),
        Value::Tuple(v) => format!("({} values)", v.len()),
        Value::All => ":".to_string(),
        Value::None => "none".to_string(),
        Value::TransferFn { num, den } => format!("{} / ({} terms)", num.len(), den.len()),
        Value::StateSpace { a, b, c, .. } => format!(
            "{}-state, {} input, {} output",
            a.nrows(),
            b.ncols(),
            c.nrows()
        ),
        Value::Lambda { params, .. } => format!("@({}) <expr>", params.join(", ")),
        Value::FuncHandle(name) => format!("@{}", name),
        Value::FirState(buf) => format!("<fir_state {}>", buf.lock().unwrap().len()),
        Value::AudioIn {
            sample_rate,
            frame_size,
        } => format!("<audio_in {:.0} Hz / {}>", sample_rate, frame_size),
        Value::AudioOut {
            sample_rate,
            frame_size,
        } => format!("<audio_out {:.0} Hz / {}>", sample_rate, frame_size),
        Value::LiveFigure(fig) => {
            if fig.lock().unwrap().is_some() {
                "<live_figure>".to_string()
            } else {
                "<live_figure closed>".to_string()
            }
        }
        Value::SparseVector(sv) => format!("sparse [1×{}, nnz={}]", sv.len, sv.nnz()),
        Value::SparseMatrix(sm) => format!("sparse [{}×{}, nnz={}]", sm.rows, sm.cols, sm.nnz()),
        Value::StringArray(arr) => {
            let preview: Vec<String> = arr.iter().take(3).map(|s| format!("\"{}\"", s)).collect();
            let suffix = if arr.len() > 3 { ", …" } else { "" };
            format!("{{{}{}}}", preview.join(", "), suffix)
        }
    }
}

fn print_whos(ev: &rustlab_script::Evaluator) {
    let vars = ev.vars();
    let fns = ev.user_fn_names();
    if vars.is_empty() && fns.is_empty() {
        println!("  {}", color::dim("(no variables defined)"));
        return;
    }
    // Compute column widths from actual data
    let name_w = vars
        .iter()
        .map(|(n, _)| n.len())
        .chain(fns.iter().map(|n| n.len()))
        .max()
        .unwrap_or(4)
        .max(4);
    let type_w = vars
        .iter()
        .map(|(_, v)| whos_type(v).len())
        .max()
        .unwrap_or(4)
        .max(4);
    let size_w = vars
        .iter()
        .map(|(_, v)| whos_size(v).len())
        .max()
        .unwrap_or(4)
        .max(4);
    println!();
    println!(
        "  {}  {}  {}  {}",
        color::bold(&format!("{:<nw$}", "Name", nw = name_w)),
        color::bold(&format!("{:<tw$}", "Type", tw = type_w)),
        color::bold(&format!("{:<sw$}", "Size", sw = size_w)),
        color::bold("Value")
    );
    let total_w = name_w + type_w + size_w + 12; // 12 = padding between columns + "Value"
    println!("  {}", color::dim(&"─".repeat(total_w.max(50))));
    for (name, val) in &vars {
        println!(
            "  {}  {}  {}  {}",
            color::green(&format!("{:<nw$}", name, nw = name_w)),
            color::cyan(&format!("{:<tw$}", whos_type(val), tw = type_w)),
            format!("{:<sw$}", whos_size(val), sw = size_w),
            whos_preview(val),
        );
    }
    for name in &fns {
        println!(
            "  {}  {}  {}  {}",
            color::green(&format!("{:<nw$}", name, nw = name_w)),
            color::cyan(&format!("{:<tw$}", "function", tw = type_w)),
            format!("{:<sw$}", "", sw = size_w),
            color::dim("<user-defined>")
        );
    }
    println!();
}

fn cmd_pwd() {
    match std::env::current_dir() {
        Ok(p) => println!("{}", p.display()),
        Err(e) => eprintln!("pwd: {e}"),
    }
}

fn cmd_cd(path: &str) {
    let target = if path.is_empty() {
        std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
    } else {
        path.to_string()
    };
    if let Err(e) = std::env::set_current_dir(&target) {
        eprintln!("cd: {target}: {e}");
    }
}

fn cmd_ls(path: &str) {
    let target = if path.is_empty() { "." } else { path };
    let dir = std::path::Path::new(target);

    let mut entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect::<Vec<_>>(),
        Err(e) => {
            eprintln!("ls: {target}: {e}");
            return;
        }
    };
    entries.sort_by_key(|e| e.file_name());

    let mut dirs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();

    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            dirs.push(format!("{name}/"));
        } else {
            files.push(name);
        }
    }

    // Print directories first, then files, in columns of 4
    let all: Vec<String> = dirs.into_iter().chain(files).collect();
    if all.is_empty() {
        return;
    }

    let col_w = all.iter().map(|s| s.len()).max().unwrap_or(0) + 2;
    let cols = (80 / col_w).max(1);

    println!();
    for (i, name) in all.iter().enumerate() {
        if i > 0 && i % cols == 0 {
            println!();
        }
        print!("  {:<width$}", name, width = col_w);
    }
    println!("\n");
}

/// Run a script source string through the evaluator.
pub(crate) fn run_script_source(src: &str, ev: &mut Evaluator) {
    match lexer::tokenize(src).and_then(|t| parser::parse(t)) {
        Err(e) => eprintln!("error: {e}"),
        Ok(stmts) => {
            for stmt in &stmts {
                if let Err(e) = ev.exec_stmt(stmt) {
                    eprintln!("error: {e}");
                    break;
                }
            }
        }
    }
}

/// Categorical grouping of `HELP` entries used by `print_help_list` and the
/// `rustlab docs` subcommand. Each tuple is `(category-name, &[entry-names])`.
/// Public so that out-of-crate tooling and `commands/docs.rs` can iterate.
pub static CATEGORIES: &[(&str, &[&str])] = &[
        (
            "Math",
            &[
                "abs", "angle", "real", "imag", "conj", "cos", "sin", "acos", "asin", "atan",
                "atan2", "tanh", "sinh", "cosh", "sqrt", "exp", "log", "log10", "log2", "floor",
                "ceil", "round", "sign", "mod",
            ],
        ),
        (
            "ML / Activation",
            &["softmax", "relu", "gelu", "layernorm", "tanh"],
        ),
        (
            "Array / Stats",
            &[
                "zeros", "ones", "linspace", "logspace", "rand", "randn", "randi", "seed", "min", "max",
                "sum", "prod", "cumsum", "argmin", "argmax", "sort", "trapz", "mean", "median",
                "std", "hist", "len", "length", "numel", "size", "ndims", "meshgrid", "all", "any",
            ],
        ),
        (
            "Matrix",
            &[
                "eye",
                "transpose",
                "diag",
                "trace",
                "reshape",
                "repmat",
                "horzcat",
                "vertcat",
                "cat",
                "rank",
            ],
        ),
        (
            "Tensor3 (rank-3)",
            &["zeros3", "ones3", "rand3", "randn3", "permute", "squeeze"],
        ),
        (
            "Linear Algebra",
            &[
                "dot", "cross", "outer", "kron", "norm", "det", "inv", "expm", "linsolve",
                "eig", "eigs",
                "svd", "laguerre", "legendre", "factor", "roots",
            ],
        ),
        (
            "Vector Calculus",
            &[
                "gradient",
                "divergence",
                "curl",
                "gradient3",
                "divergence3",
                "curl3",
            ],
        ),
        (
            "Geometry / Masks",
            &["rect_mask", "disk_mask", "polygon_mask"],
        ),
        (
            "DSP",
            &[
                "fir_lowpass",
                "fir_highpass",
                "fir_bandpass",
                "fir_lowpass_kaiser",
                "fir_highpass_kaiser",
                "fir_bandpass_kaiser",
                "fir_notch",
                "firpm",
                "firpmq",
                "freqz",
                "butterworth_lowpass",
                "butterworth_highpass",
                "filtfilt",
                "convolve",
                "upfirdn",
                "window",
                "fft",
                "ifft",
                "fftshift",
                "fftfreq",
                "spectrum",
                "mag2db",
            ],
        ),
        ("Streaming DSP", &["state_init", "filter_stream"]),
        (
            "Audio I/O",
            &["audio_in", "audio_out", "audio_read", "audio_write"],
        ),
        (
            "Live Plotting",
            &[
                "figure_live",
                "plot_update",
                "plot_labels",
                "plot_limits",
                "figure_draw",
                "figure_close",
            ],
        ),
        (
            "Fixed-point",
            &["qfmt", "quantize", "qadd", "qmul", "qconv", "snr"],
        ),
        (
            "Plotting",
            &[
                "plot", "stem", "bar", "scatter", "hline", "yline", "plotdb", "imagesc",
                "heatmap", "image", "surf",
                "loglog", "semilogx", "semilogy", "polar",
                "contour", "contourf", "quiver", "streamplot", "savefig", "hist",
                "frame", "saveanim",
            ],
        ),
        (
            "Figure Controls",
            &[
                "figure", "clf", "close", "hold", "grid", "axis", "viewer", "xlabel", "ylabel",
                "title", "xlim", "ylim", "subplot", "legend",
            ],
        ),
        (
            "Controls",
            &[
                "tf", "tfdata", "pole", "zero", "ss", "ctrb", "obsv", "bode", "nyquist", "step",
                "margin", "lqr", "rlocus", "rk4", "lyap", "gram", "care", "dare", "place",
                "freqresp",
            ],
        ),
        (
            "Sparse",
            &[
                "sparse",
                "sparsevec",
                "speye",
                "spzeros",
                "spdiags",
                "sprand",
                "full",
                "nnz",
                "issparse",
                "nonzeros",
                "find",
                "spsolve",
                "laplacian_1d",
                "laplacian_2d",
                "laplacian_3d",
                "laplacian_eps_2d",
                "ij2k",
                "k2ij",
                "ijk2k",
                "k2ijk",
            ],
        ),
        (
            "Structs",
            &["struct", "isstruct", "fieldnames", "isfield", "rmfield"],
        ),
        ("Cell Arrays", &["iscell"]),
        (
            "Control Flow",
            &[
                "if",
                "elseif",
                "switch",
                "for",
                "function",
                "error",
                "index_assign",
                "chained_index",
            ],
        ),
        ("Output", &["disp", "fprintf", "sprintf", "commas", "print"]),
        ("Formatting", &["format", "underscores"]),
        ("I/O", &["print", "save", "load", "whos", "sleep"]),
        (
            "Language / REPL",
            &[
                "i / j",
                "pi",
                "e",
                "Inf",
                "NaN",
                "range",
                "index",
                "index_assign",
                "chained_index",
                "compound_assign",
                "clear",
                "whos",
                "arrayfun",
                "feval",
                "profile",
                "profile_report",
            ],
        ),
        ("Filesystem", &["run", "ls", "cd", "pwd"]),
];

pub fn print_help_list() {
    println!();
    println!(
        "  {:<26}  {}",
        color::bold("Command / Topic"),
        color::bold("Description")
    );
    println!("  {}", color::dim(&"-".repeat(60)));

    for (cat, names) in CATEGORIES {
        println!("\n  {}:", color::bold_yellow(cat));
        for &n in *names {
            if let Some(e) = HELP.iter().find(|e| e.name == n) {
                println!("    {:<24}  {}", color::cyan(e.name), e.brief);
            }
        }
    }
    println!();
    println!(
        "  Type  {}  or  {}  for details.",
        color::bold("help <command>"),
        color::bold("? <command>")
    );
    println!();
}

/// Print the detail block for one builtin. If `topic` doesn't match an entry
/// name, fall back to checking the categories list (case-insensitive) and list
/// that category's entries instead. Returns `true` on a hit, `false` when
/// neither name nor category matched.
pub fn print_help_detail(topic: &str) -> bool {
    if let Some(e) = HELP.iter().find(|e| e.name == topic) {
        println!();
        println!("  {}  —  {}", color::bold_cyan(e.name), e.brief);
        println!();
        for line in e.detail.lines() {
            println!("  {}", line);
        }
        println!();
        return true;
    }

    // Try as a category name (case-insensitive).
    if let Some((cat, names)) = CATEGORIES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(topic))
    {
        println!();
        println!("  {}:", color::bold_yellow(cat));
        for &n in *names {
            if let Some(e) = HELP.iter().find(|e| e.name == n) {
                println!("    {:<24}  {}", color::cyan(e.name), e.brief);
            }
        }
        println!();
        return true;
    }

    println!(
        "No help found for '{}'.  Type {} for a full list.",
        color::yellow(&format!("'{}'", topic)),
        color::bold("'help'")
    );
    false
}

// ─── Tab completion helper ────────────────────────────────────────────────────

#[derive(Helper, Hinter, Validator)]
struct ReplHelper {
    file_completer: FilenameCompleter,
    #[rustyline(Hinter)]
    hinter: HistoryHinter,
    /// Workspace identifiers (vars + user fns) — refreshed after each eval.
    names: Vec<String>,
}

impl ReplHelper {
    fn new() -> Self {
        Self {
            file_completer: FilenameCompleter::new(),
            hinter: HistoryHinter::new(),
            names: Vec::new(),
        }
    }

    fn sync(&mut self, ev: &Evaluator) {
        self.names = ev.vars().iter().map(|(n, _)| n.to_string()).collect();
        self.names
            .extend(ev.user_fn_names().iter().map(|n| n.to_string()));
        self.names.sort();
    }
}

impl Highlighter for ReplHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> std::borrow::Cow<'b, str> {
        if color::is_color_enabled() {
            if prompt == ">> " {
                std::borrow::Cow::Owned(color::bold_cyan(prompt))
            } else if prompt == ".. " {
                std::borrow::Cow::Owned(color::dim(prompt))
            } else {
                std::borrow::Cow::Borrowed(prompt)
            }
        } else {
            std::borrow::Cow::Borrowed(prompt)
        }
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        // Dim the inline history hint so it reads as a ghost suggestion.
        std::borrow::Cow::Owned(format!("\x1b[2m{hint}\x1b[0m"))
    }
}

/// Returns true when the cursor is inside an unclosed double-quoted string,
/// meaning Tab should complete a file path.
fn inside_string(s: &str) -> bool {
    s.chars().filter(|&c| c == '"').count() % 2 == 1
}

/// Builtin names drawn from the help table, for identifier completion.
fn builtin_names() -> Vec<&'static str> {
    HELP.iter().map(|e| e.name).collect()
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let s = &line[..pos];

        // ── run <path>  or  ls/cd <path> — filesystem, no quotes ─────────────
        let is_path_cmd = s.starts_with("run ") || s.starts_with("ls ") || s.starts_with("cd ");
        if is_path_cmd || inside_string(s) {
            return self.file_completer.complete(line, pos, ctx);
        }

        // ── help <topic> ──────────────────────────────────────────────────────
        let help_prefix = s.strip_prefix("help ").or_else(|| s.strip_prefix("? "));
        if let Some(rest) = help_prefix {
            let candidates = builtin_names()
                .into_iter()
                .filter(|n| n.starts_with(rest))
                .map(|n| Pair {
                    display: n.to_string(),
                    replacement: n.to_string(),
                })
                .collect();
            return Ok((pos - rest.len(), candidates));
        }

        // ── bare identifier — workspace vars/fns + builtins ───────────────────
        let word_start = s
            .rfind(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|i| i + 1)
            .unwrap_or(0);
        let prefix = &s[word_start..];

        if prefix.is_empty() {
            return Ok((pos, vec![]));
        }

        let builtins = builtin_names();
        let mut candidates: Vec<Pair> = self
            .names
            .iter()
            .filter(|n| n.starts_with(prefix))
            .map(|n| Pair {
                display: n.clone(),
                replacement: n.clone(),
            })
            .collect();
        for name in builtins {
            if name.starts_with(prefix) && !self.names.iter().any(|n| n == name) {
                candidates.push(Pair {
                    display: name.to_string(),
                    replacement: name.to_string(),
                });
            }
        }
        candidates.sort_by(|a, b| a.replacement.cmp(&b.replacement));
        Ok((word_start, candidates))
    }
}

// ─── REPL ─────────────────────────────────────────────────────────────────────

pub fn execute() -> Result<()> {
    println!(
        "rustlab {} — type {} or {} for help, {} or Ctrl+D to quit",
        color::bold_green(env!("CARGO_PKG_VERSION")),
        color::bold("'help'"),
        color::bold("'?'"),
        color::bold("'exit'")
    );
    println!(
        "{}\n",
        color::dim("Tip: end a line with ; to suppress output")
    );

    let config = Config::builder()
        .completion_type(CompletionType::List)
        .build();
    let mut rl = Editor::with_config(config)?;
    rl.set_helper(Some(ReplHelper::new()));
    let mut ev = Evaluator::new();
    ev.color_output = color::is_color_enabled();

    let hist_path = std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".rustlab_history"))
        .unwrap_or_else(|| std::path::PathBuf::from(".rustlab_history"));
    let _ = rl.load_history(&hist_path);

    let prompt = ">> ";
    let cont_prompt = ".. ";

    loop {
        match rl.readline(&prompt) {
            Ok(line) => {
                let trimmed = line.trim();

                if trimmed.is_empty() {
                    continue;
                }

                rl.add_history_entry(trimmed).ok();

                if trimmed == "exit" || trimmed == "quit" {
                    break;
                }

                // help / ?
                if trimmed == "help" || trimmed == "?" {
                    print_help_list();
                    continue;
                }
                if let Some(topic) = trimmed
                    .strip_prefix("help ")
                    .or_else(|| trimmed.strip_prefix("? "))
                {
                    print_help_detail(topic.trim());
                    continue;
                }

                // whos
                if trimmed == "whos" {
                    print_whos(&ev);
                    continue;
                }

                // clear
                if trimmed == "clear" {
                    ev.clear_vars();
                    if let Some(h) = rl.helper_mut() {
                        h.sync(&ev);
                    }
                    continue;
                }

                // run <file>
                if let Some(path) = trimmed.strip_prefix("run ") {
                    let path = path.trim();
                    match std::fs::read_to_string(path) {
                        Err(e) => eprintln!("run: {path}: {e}"),
                        Ok(src) => {
                            run_script_source(&src, &mut ev);
                        }
                    }
                    if let Some(h) = rl.helper_mut() {
                        h.sync(&ev);
                    }
                    continue;
                }

                // directory commands
                if trimmed == "pwd" {
                    cmd_pwd();
                    continue;
                }
                if trimmed == "cd" {
                    cmd_cd("");
                    continue;
                }
                if let Some(path) = trimmed.strip_prefix("cd ") {
                    cmd_cd(path.trim());
                    continue;
                }
                if trimmed == "ls" {
                    cmd_ls("");
                    continue;
                }
                if let Some(path) = trimmed.strip_prefix("ls ") {
                    cmd_ls(path.trim());
                    continue;
                }

                // Multi-line input for function definitions
                let source = if trimmed.starts_with("function ") || trimmed == "function" {
                    let mut buf = format!("{}\n", trimmed);
                    let mut depth: i32 = 1;
                    loop {
                        match rl.readline(&cont_prompt) {
                            Ok(cont) => {
                                let ct = cont.trim();
                                rl.add_history_entry(ct).ok();
                                // Track nesting for nested function defs
                                if ct.starts_with("function ") || ct == "function" {
                                    depth += 1;
                                }
                                buf.push_str(ct);
                                buf.push('\n');
                                if ct == "end" || ct == "end;" {
                                    depth -= 1;
                                    if depth <= 0 {
                                        break;
                                    }
                                }
                            }
                            Err(ReadlineError::Interrupted) => {
                                println!("(interrupted)");
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    buf
                } else {
                    format!("{}\n", trimmed)
                };

                match lexer::tokenize(&source).and_then(|tokens| parser::parse(tokens)) {
                    Ok(stmts) => {
                        for stmt in &stmts {
                            if let Err(e) = ev.exec_stmt(stmt) {
                                eprintln!("{} {}", color::bold_red("error:"), e);
                            }
                        }
                        if let Some(h) = rl.helper_mut() {
                            h.sync(&ev);
                        }
                    }
                    Err(e) => eprintln!("{} {}", color::bold_red("error:"), e),
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl+C — clear current input, continue
                println!("(interrupted)");
            }
            Err(ReadlineError::Eof) => {
                // Ctrl+D — exit
                break;
            }
            Err(e) => {
                eprintln!("readline error: {}", e);
                break;
            }
        }
    }

    let _ = rl.save_history(&hist_path);
    println!("{}", color::dim("bye"));
    Ok(())
}
