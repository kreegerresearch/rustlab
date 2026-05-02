# bode() — Bode magnitude and phase frequency response

s = tf("s")
G = 10 / (s^2 + 2*s + 10)

# Return magnitude (dB), phase (deg), and frequency (rad/s) vectors
[mag, phase, w] = bode(G)
plot(mag, "Bode Magnitude |G(jw)| (dB)")
plot(phase, "Bode Phase /G(jw) (deg)")
