# Save and load example
# Demonstrates: NPY binary, NPZ multi-variable archive, and CSV round-trips

sr   = 8000.0
n    = 256
t    = linspace(0.0, (n - 1) / sr, n)

# Build a two-tone signal and a lowpass filter
x = cos(t * 2.0 * pi * 440.0) + cos(t * 2.0 * pi * 1200.0)
h = fir_lowpass(64, 800.0, sr, "hann")

# ── Single-array NPY round-trip ─────────────────────────────────────────────

save("signal.npy", x)
x2 = load("signal.npy")
print("NPY round-trip max error:", max(abs(real(x2) - real(x))))

# ── CSV round-trip ───────────────────────────────────────────────────────────

save("filter.csv", h)
h2 = load("filter.csv")
print("CSV round-trip max error:", max(abs(real(h2) - real(h))))

# ── Multi-variable NPZ archive ───────────────────────────────────────────────

# Compute the frequency response so we can archive it alongside the signal
Hz = freqz(h, 512, sr)

save("session.npz", "signal", x, "filter", h, "response", Hz)

# Inspect what we saved
whos("session.npz")

# Load individual arrays back by name
x_back  = load("session.npz", "signal")
h_back  = load("session.npz", "filter")
Hz_back = load("session.npz", "response")

print("NPZ signal round-trip max error:", max(abs(real(x_back)  - real(x))))
print("NPZ filter round-trip max error:", max(abs(real(h_back)  - real(h))))

# Use the reloaded frequency response for a plot
plotdb(Hz_back, "Reloaded Frequency Response")
savefig("session_response.svg")
print("Saved session_response.svg")
