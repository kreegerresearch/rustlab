# ── Sparse Vectors & Matrices ─────────────────────────────────────
# Demonstrates construction, arithmetic, conversion, and solving.

# Build a 3×3 sparse matrix from (row, col, value) triples
S = sparse([1,2,3,1], [1,2,3,3], [10, 20, 30, 5], 3, 3)

# Sparse + sparse stays sparse
T = S + speye(3)
print(full(T))

# Sparse × dense vector (SpMV) — use column vector with '
x = [1, 2, 3]
y = S * x'
print(y)

# Convert dense → sparse → dense round-trip
D  = full(S)
S2 = sparse(D)
print(issparse(S2))   # 1
print(nnz(S2))        # 4

# Diagonal sparse matrix via spdiags
diag5 = spdiags([1, 2, 3, 4, 5], 0, 5, 5)
print(full(diag5))

# Random 10×10 sparse matrix (~10% fill)
R = sprand(10, 10, 0.1)
print(R)

# ── Tridiagonal solve ────────────────────────────────────────────
# Build the classic [-1, 2, -1] tridiagonal and solve T*x = b
T2 = spdiags(-1, -1, 5, 5) + spdiags(2, 0, 5, 5) + spdiags(-1, 1, 5, 5)
b  = [1, 0, 0, 0, 1]
x  = spsolve(T2, b)
print(x)              # [1, 1, 1, 1, 1]
