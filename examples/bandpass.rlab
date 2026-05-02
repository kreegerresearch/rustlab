# FIR bandpass filter: 500 Hz – 2000 Hz, 44.1 kHz sample rate

h = fir_bandpass(64, 500.0, 2000.0, 44100.0, "hamming")

# Impulse response → save
stem(real(h), "Bandpass Impulse Response")
savefig("bandpass_impulse.svg")

# Frequency response → save
Hz = freqz(h, 512, 44100.0)
plotdb(Hz, "Bandpass Frequency Response")
savefig("bandpass_response.svg")

# Apply to a test signal: sum of tones
t  = linspace(0.0, 1.0, 4410)
x1 = cos(t * 2.0 * pi * 250.0)    # 250 Hz — should be attenuated
x2 = cos(t * 2.0 * pi * 1000.0)   # 1 kHz  — should pass
x  = x1 + x2
y  = convolve(x, h)

# Filtered output → save
plot(real(y), "Bandpass Output (1 kHz passes, 250 Hz attenuated)")
savefig("bandpass_output.svg")
