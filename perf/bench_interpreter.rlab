# Benchmark: interpreter overhead
#
# Measures the cost of the scripting engine itself — variable lookup,
# arithmetic dispatch, for-loop iteration — independent of numeric kernels.

# ── Scalar arithmetic loop (10 000 iterations) ───────────────────────────────
print("scalar loop 10000 iterations")
acc = 0.0;
for i = 1:10000
    acc = acc + i;
end
print("  result: ", acc)

# ── Vector build via indexed assign (1 000 elements) ─────────────────────────
print("indexed assign build n=1000")
v = zeros(1000);
for i = 1:1000
    v(i) = i * 2.0;
end
print("  v(1000): ", v(1000))

# ── Deeply nested expression chain ───────────────────────────────────────────
print("deep expression chain n=500")
x = randn(500);
y = abs(sqrt(abs(exp(abs(x)))));
print("  out: ", len(y))

# ── Many small function calls ─────────────────────────────────────────────────
print("1000 calls to len()")
total = 0.0;
v2 = randn(64);
for i = 1:1000
    total = total + len(v2);
end
print("  total: ", total)

print("done")
