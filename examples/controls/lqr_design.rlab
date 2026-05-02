# lqr() — optimal LQR state-feedback gain

s = tf("s")
G = 10 / (s^2 + 2*s + 10)
sys = ss(G)

Q = diag([10, 1])   # state cost: position weighted 10x over velocity
R = 1               # control effort cost

[K, S, e] = lqr(sys, Q, R)
print(K)   # optimal feedback gain
print(e)   # closed-loop eigenvalues

if all(real(e) < 0)
    print("closed-loop stable")
end
