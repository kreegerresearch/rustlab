# Vector operations
# Demonstrates: range operator, indexing, concatenation, element-wise ops, transpose

# Range operator: start:stop and start:step:stop
t = 0:0.1:1                         # 11 points: 0.0, 0.1, ..., 1.0
n = 1:8                              # integer sequence [1, 2, ..., 8]
print(n)

# 1-based indexing
first = n(1)
third = n(3)
last  = n(end)
slice = n(3:6)
print(slice)
print(last)

# Element-wise operators
sq   = n .^ 2                        # [1, 4, 9, 16, 25, 36, 49, 64]
inv  = 1.0 ./ n                      # harmonic series
prod = n .* n
print(sq)

# Concatenation
a = 1:4
b = 5:8
c = [a, b]                           # [1, 2, 3, 4, 5, 6, 7, 8]
print(c)

# Transpose
row = [1.0 + j*0.0, 2.0 + j*1.0, 3.0 + j*2.0]
col = row'
print(col)

# Complex sinusoid using range + element-wise ops
omega  = 2.0 * pi * 440.0
signal = cos(t * omega) + j * sin(t * omega)
mag    = abs(signal)                 # all ones — unit circle
print(mag)

# Interactive terminal plot
plot(signal, "440 Hz Complex Sinusoid")

# Save magnitude and real part to files
plot(real(signal), "440 Hz Sinusoid (Real Part)")
savefig("sinusoid_real.svg")
plot(mag, "Sinusoid Magnitude (unit circle)")
savefig("sinusoid_magnitude.svg")
