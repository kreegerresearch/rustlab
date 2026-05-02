# Benchmark: element-wise builtin throughput on large vectors
#
# Each test applies a single built-in to a 100 K-element vector and
# reports the output length as a sanity check.

N = 100000

x = randn(N);

print("abs   n=100000")
y = abs(x);
print("  out: ", len(y))

print("exp   n=100000")
y = exp(x);
print("  out: ", len(y))

print("log   n=100000 (positive input)")
xp = abs(x);
y = log(xp);
print("  out: ", len(y))

print("sqrt  n=100000 (positive input)")
y = sqrt(xp);
print("  out: ", len(y))

print("sin   n=100000")
y = sin(x);
print("  out: ", len(y))

print("cos   n=100000")
y = cos(x);
print("  out: ", len(y))

print("tanh  n=100000")
y = tanh(x);
print("  out: ", len(y))

# ── Statistics on large vector ───────────────────────────────────────────────
print("sum   n=100000")
s = sum(x);
print("  sum: ", s)

print("mean  n=100000")
m = mean(x);
print("  mean: ", m)

print("std   n=100000")
sd = std(x);
print("  std: ", sd)

print("sort  n=100000")
xs = sort(x);
print("  out: ", len(xs))

print("done")
