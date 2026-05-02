# margin() — gain margin and phase margin from Bode data

s = tf("s")
G = 10 / (s^2 + 2*s + 10)

[Gm, Pm, Wcg, Wcp] = margin(G)
fprintf("Gain Margin:  %.2f dB  at %.4f rad/s\n", 20*log10(Gm), Wcg)
fprintf("Phase Margin: %.2f deg at %.4f rad/s\n", Pm, Wcp)
