# ss() — transfer function to state-space conversion
# Returns observable canonical form with fields A, B, C, D

s = tf("s")
G = 10 / (s^2 + 2*s + 10)

sys = ss(G)
print(sys.A)
print(sys.B)
print(sys.C)
print(sys.D)
