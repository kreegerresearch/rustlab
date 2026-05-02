# User-defined functions and structs example

# ── Basic function with return variable ─────────────────────────────────────
function y = square(x)
  y = x * x
end

function y = add(a, b)
  y = a + b
end

a = square(4)
b = add(3, 7)
c = square(add(2, 3))

# ── Vector DSP helper ────────────────────────────────────────────────────────
function y = normalize(v)
  y = v / max(abs(v))
end

sig = randn(64);
norm_sig = normalize(sig);

# ── Struct creation and field access ─────────────────────────────────────────
filter_spec = struct("cutoff", 0.25, "order", 32, "type", "lowpass")
print(filter_spec)

# Read fields
fc = filter_spec.cutoff
n  = filter_spec.order

# Assign into a new struct field-by-field
result.min = min(norm_sig)
result.max = max(norm_sig)
result.mean = mean(real(norm_sig))
print(result)

# ── Method-call sugar: obj.method(args) → method(obj, args) ──────────────────
function y = scale(v, factor)
  y = v * factor
end

w = 1:5;
doubled = w.scale(2)

# ── Struct inspection builtins ───────────────────────────────────────────────
b1 = isstruct(filter_spec)
b2 = isstruct(42)
b3 = isfield(filter_spec, "cutoff")
fieldnames(filter_spec)

# Remove a field (returns new struct)
trimmed = rmfield(filter_spec, "type")
print(trimmed)
