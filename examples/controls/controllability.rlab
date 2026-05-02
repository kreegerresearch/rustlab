# ctrb() and obsv() — controllability and observability matrices

s = tf("s")
G = 10 / (s^2 + 2*s + 10)
sys = ss(G)
n = length(sys.A)

Co = ctrb(sys.A, sys.B)
if rank(Co) == n
    print("fully controllable")
else
    fprintf("%d uncontrollable states\n", n - rank(Co))
end

Ob = obsv(sys.A, sys.C)
if rank(Ob) == n
    print("fully observable")
else
    fprintf("%d unobservable states\n", n - rank(Ob))
end
