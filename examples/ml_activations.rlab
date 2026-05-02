# ML activation functions
#
# Demonstrates: relu, gelu, tanh, softmax, layernorm, sort
#
# Activations are evaluated on a shared input range and saved to SVG so the
# shapes can be compared side-by-side.  The final section shows a top-K
# sampling pattern built from sort + softmax.

x = linspace(-3.0, 3.0, 64)

# ── ReLU ─────────────────────────────────────────────────────────────────────
# max(0, x): zero for negatives, identity for positives
r = relu(x)
plot(real(r), "ReLU: max(0, x)")

# ── GELU ─────────────────────────────────────────────────────────────────────
# Smooth approximation of ReLU used in transformers (BERT, GPT)
g = gelu(x)
plot(real(g), "GELU: 0.5·x·(1 + tanh(√(2/π)·(x + 0.044715·x³)))")

# ── tanh ─────────────────────────────────────────────────────────────────────
# Classic bounded activation: saturates at ±1, zero-centred unlike sigmoid
t = tanh(x)
plot(real(t), "tanh: (eˣ − e⁻ˣ) / (eˣ + e⁻ˣ)")

# ── Side-by-side comparison: ReLU vs GELU vs tanh ────────────────────────────
# All three on the same axes — save each curve individually then inspect
plot(real(r - g), "ReLU − GELU  (difference)")

# ── softmax ───────────────────────────────────────────────────────────────────
# Converts a logit vector to a probability distribution summing to 1.
logits = [2.0, 1.0, 0.1, 3.5, -0.5]
probs  = softmax(logits)
print(probs)
print(sum(probs))                      # → 1.0

bar(real(probs), "softmax([2, 1, 0.1, 3.5, -0.5])")

# ── Top-K sampling pattern ────────────────────────────────────────────────────
# sort the logits descending, keep the top-3, re-normalise with softmax.
# sort() returns ascending — reverse with end:-1:1 indexing.
logits8  = [0.5, 2.1, -1.0, 3.8, 0.0, 1.4, -0.3, 2.9]
sorted   = sort(logits8)
top3_idx = len(sorted) - 2            # 1-based: last three are indices end-2:end
top3     = sorted(top3_idx:len(sorted))
top3_p   = softmax(top3)
print(top3)
print(top3_p)

# ── layernorm ─────────────────────────────────────────────────────────────────
# Subtracts mean, divides by population std.  Output has zero mean, unit variance.
raw = randn(16)
ln  = layernorm(raw)
print(mean(real(ln)))                  # → ≈ 0.0
print(std(real(ln)))                   # → ≈ 1.0  (sample std; layernorm targets population std = 1)

# layernorm then softmax — a common transformer sub-block pattern
logits2 = randn(8) * 3.0
normed  = layernorm(logits2)
p2      = softmax(normed)
print(sum(p2))                         # → 1.0
