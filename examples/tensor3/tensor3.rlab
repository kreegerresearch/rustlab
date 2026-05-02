# ── Rank-3 Tensors (Value::Tensor3) ──────────────────────────────
# Demonstrates construction, indexing, arithmetic, reshape/permute/squeeze,
# and cat(3, ...) for stacking matrices into pages.

# ── Construction ─────────────────────────────────────────────────
A = zeros3(2, 3, 4)
print(size(A))        # [2, 3, 4]
print(ndims(A))       # 3
print(numel(A))       # 24

B = ones3(2, 3, 4)
C = rand3(2, 3, 4)
D = randn3(2, 3, 4)

# Build a known tensor with encoded values A(i, j, k) = 100*i + 10*j + k
T = reshape(1:24, 2, 3, 4)    # column-major walk (Octave convention)

# ── Indexing ─────────────────────────────────────────────────────
# Single element
print(T(1, 1, 1))             # 1
print(T(2, 3, 4))             # 24

# Page slice — returns a Matrix (trailing singleton dropped)
page2 = T(:, :, 2)
print(size(page2))            # [2, 3]

# Row/column slabs across the page axis
row1 = T(1, :, :)             # Matrix (3, 4)
col2 = T(:, 2, :)             # Matrix (2, 4)

# Range slice stays rank-3 if the result has >1 non-singleton dims
T_first_two_pages = T(:, :, 1:2)
print(size(T_first_two_pages))  # [2, 3, 2]

# ── Arithmetic ───────────────────────────────────────────────────
# Scalar broadcast
E = T * 2
F = T + 10
G = T .^ 2

# Element-wise between tensors of the same shape
H = B + T               # ones-plus-encoded
J = B .* T              # element-wise multiply

# NB: `*` between two tensor3s is *not* matmul — it errors.
# Use `.*` for element-wise. Matrix + Tensor3 also errors (no broadcasting).

# ── Assignment ───────────────────────────────────────────────────
U = zeros3(2, 2, 3)
U(:, :, 2) = [1, 2; 3, 4]      # Page write
U(1, 1, 1) = 99                # Scalar write
print(U(1, 1, 1))              # 99
print(U(:, :, 2))              # [1, 2; 3, 4]

# ── Reshape / Permute / Squeeze ──────────────────────────────────
# Reshape accepts Tensor3 as input; column-major walk preserved
flat = reshape(T, 24, 1)       # Column vector (length 24)
back = reshape(flat, 2, 3, 4)  # Same as T

# Permute: swap axes 1 and 2 (rows ↔ cols)
P = permute(T, [2, 1, 3])
print(size(P))                 # [3, 2, 4]

# Squeeze drops singleton dimensions
S1 = reshape(1:6, 2, 3, 1)
M1 = squeeze(S1)
print(size(M1))                # [2, 3] — became a Matrix

S2 = reshape(1:5, 1, 1, 5)
V1 = squeeze(S2)
print(numel(V1))               # 5 — became a Vector

# ── cat along the page axis ──────────────────────────────────────
# Stack two matrices into a 2-page tensor3
M1 = [1, 2; 3, 4]
M2 = [5, 6; 7, 8]
stacked = cat(3, M1, M2)
print(size(stacked))           # [2, 2, 2]
print(stacked(:, :, 1))        # [1, 2; 3, 4]
print(stacked(:, :, 2))        # [5, 6; 7, 8]

# Mix: append a matrix as one extra page on a tensor3
more = cat(3, stacked, [9, 10; 11, 12])
print(size(more))              # [2, 2, 3]

# ── I/O round-trip ───────────────────────────────────────────────
# NPY preserves the rank-3 shape natively.
save("/tmp/rustlab_demo_tensor3.npy", T)
T_loaded = load("/tmp/rustlab_demo_tensor3.npy")
print(ndims(T_loaded))         # 3
print(size(T_loaded))          # [2, 3, 4]
