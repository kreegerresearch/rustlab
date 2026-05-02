# tf() — transfer function creation and arithmetic

# Build from the Laplace variable s
s = tf("s")
G = 10 / (s^2 + 2*s + 10)
disp(G)   # 10 / (s^2 + 2s + 10)

# Series connection: G1 * G2
G1 = 1 / (s + 1)
G2 = 1 / (s + 2)
disp(G1 * G2)   # 1 / (s^2 + 3s + 2)

# Closed-loop under unity feedback: G / (1 + G)
G_cl = G / (1 + G)
disp(G_cl)

# Explicit numerator/denominator: (2s + 1) / (s^2 + 3s + 2)
H = tf([2, 1], [1, 3, 2])
disp(H)
