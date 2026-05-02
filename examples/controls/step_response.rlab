# step() — unit step response simulation

s = tf("s")
G = 10 / (s^2 + 2*s + 10)   # underdamped second-order system

# Simulate and return time/output vectors
[y, t] = step(G, 5.0)
plot(y, "Step Response: 10/(s^2+2s+10)")
