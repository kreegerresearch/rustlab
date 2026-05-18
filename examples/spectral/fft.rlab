# FFT example
# Compute the spectrum of a two-tone signal at 8 kHz sample rate

sr = 8000.0
n  = 256

# Build a time vector and a signal with tones at 500 Hz and 1500 Hz
t = linspace(0.0, (n - 1) / sr, n)
x = cos(t * 2.0 * pi * 500.0) + cos(t * 2.0 * pi * 1500.0)

# Save the input signal
plot(real(x), "Input Signal (500 Hz + 1500 Hz)")
savefig("fft_input.svg")

# Forward FFT
X = fft(x)

# spectrum() applies fftshift and pairs with the Hz frequency axis,
# returning a 2×n matrix that plugs straight into plotdb.
H = spectrum(X, sr)

# dB magnitude spectrum → save
plotdb(H, "Magnitude Spectrum")
savefig("fft_spectrum.svg")

# Round-trip: reconstruct original signal from spectrum
x_rec = real(ifft(X))
plot(x_rec, "Reconstructed Signal")
savefig("fft_reconstructed.svg")
