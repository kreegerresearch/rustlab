# Matrix construction and manipulation
#
# Demonstrates: randn(m,n), reshape, outer, kron, expm, transpose, diag
#
# Each section is self-contained so individual lines can be pasted into the
# REPL for exploration.

# ── randn(m, n) ───────────────────────────────────────────────────────────────
# Weight-matrix initialisation: common in neural network layers.
# randn(m, n) returns an m×n matrix of N(0,1) samples.
W1 = randn(4, 8) * 0.1               # first layer  (4 outputs, 8 inputs)
W2 = randn(2, 4) * 0.1               # second layer (2 outputs, 4 inputs)
print(size(W1))                        # → [4, 8]
print(size(W2))                        # → [2, 4]

# Kaiming initialisation: scale by sqrt(2 / fan_in)
fan_in = 8.0
W_kaiming = randn(4, 8) * sqrt(2.0 / fan_in)

imagesc(W1)

# ── reshape ───────────────────────────────────────────────────────────────────
# Reinterpret data layout without copying values (column-major fill order).
flat = 1:12
M    = reshape(flat, 3, 4)            # 3×4 matrix, columns filled first
print(M)

# Flatten a matrix back to a vector
v_again = reshape(M, 1, 12)           # 1×12 → returned as vector
print(v_again)

# Partition a 256-sample frame into an 8×32 spectrogram-style layout
frame = randn(256)
gram  = reshape(frame, 8, 32)
imagesc(gram)

# ── outer product ─────────────────────────────────────────────────────────────
# outer(a, b)[i, j] = a[i] * b[j] — rank-1 matrix.
a   = 1:4
b   = 1:3
R   = outer(a, b)                     # 4×3 matrix: multiplication table
print(R)

# Useful in signal processing: outer product of two windows gives a 2-D kernel
win = window("hann", 8)
K   = outer(win, win)                 # 8×8 separable 2-D Hann window
imagesc(K)

# ── Kronecker product ─────────────────────────────────────────────────────────
# kron(A, B) tiles B scaled by each element of A — builds block matrices.
I2  = eye(2)
blk = [1, 2; 3, 4]
K2  = kron(I2, blk)                   # 4×4 block-diagonal: blk on the diagonal
print(K2)

# Upsample a 2×2 matrix 3× in each dimension using kron with a 3×3 block of ones
small  = [1, 2; 3, 4]
block3 = outer(ones(3), ones(3))       # 3×3 all-ones matrix
big    = kron(small, block3)           # each element of small → scaled 3×3 block
print(size(big))                       # → [6, 6]

# ── Matrix exponential ────────────────────────────────────────────────────────
# expm(M) = e^M (via Padé approximant) — not the same as exp(M) element-wise.

# Rotation generator: expm(t * [0,-1;1,0]) = rotation matrix by angle t
t   = pi / 4.0                        # 45 degrees
gen = [0, -1; 1, 0]                   # so(2) generator
Rot = expm(t * gen)
print(Rot)                             # → [cos(π/4), -sin(π/4); sin(π/4), cos(π/4)]
print(abs(det(Rot) - 1.0))            # → ≈ 0  (rotation preserves volume)

# Verify: Rot * Rot' ≈ I₂
I_check = Rot * Rot'
print(I_check)

# Time-evolution operator: expm(-j * H * t) for Hamiltonian H
H   = [0, 1; 1, 0]                    # Pauli-X / spin-flip Hamiltonian
dt  = pi / 2.0
U   = expm(-j * H * dt)               # quarter-period evolution
print(U)

# ── diag ─────────────────────────────────────────────────────────────────────
d   = diag([3, 1, 4, 1, 5])          # 5×5 diagonal matrix
print(d)
print(diag(d))                         # extract diagonal back → [3, 1, 4, 1, 5]

# ── transpose ─────────────────────────────────────────────────────────────────
A   = [1, 2, 3; 4, 5, 6]             # 2×3 matrix
At  = transpose(A)                    # 3×2 non-conjugate transpose
print(size(A))                         # → [2, 3]
print(size(At))                        # → [3, 2]

# For real matrices, .' and ' are identical
Ah  = A'                              # conjugate transpose (same here — all real)
print(At)
