# pole() and zero() — system poles and zeros

s = tf("s")
G = 10 * (s + 2) / (s^2 + 2*s + 10)   # zero at s=-2, poles at s=-1±3j

p = pole(G)
z = zero(G)

print(p)   # [-1+3j; -1-3j]
print(z)   # [-2]

# Stability: all poles must be in the left half-plane
if all(real(p) < 0)
    print("stable")
else
    print("unstable")
end
