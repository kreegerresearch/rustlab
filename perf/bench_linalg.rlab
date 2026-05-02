# Benchmark: matrix operations

# ── Matrix multiply (ndarray BLAS) ───────────────────────────────────────────
print("Matrix multiply 64x64")
A = randn(64, 64);
B = randn(64, 64);
C = A * B;
print("  result size: ", size(C))

print("Matrix multiply 256x256")
A2 = randn(256, 256);
B2 = randn(256, 256);
C2 = A2 * B2;
print("  result size: ", size(C2))

# ── Matrix inverse ───────────────────────────────────────────────────────────
print("inv 64x64")
I64 = inv(A);
print("  result size: ", size(I64))

# ── Eigenvalues ──────────────────────────────────────────────────────────────
print("eig 32x32")
S = randn(32, 32);
ev = eig(S);
print("  eigenvalue count: ", len(ev))

print("done")
