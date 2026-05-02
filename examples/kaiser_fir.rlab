# Kaiser-window FIR filter design
# Automatically selects beta and tap count from the desired spec

sr   = 8000.0
tbw  = 200.0    # transition bandwidth in Hz
attn = 60.0     # stopband attenuation in dB

# Lowpass at 1 kHz — auto-designed Kaiser window
h_lp = fir_lowpass_kaiser(1000.0, tbw, attn, sr)
print(len(h_lp))     # number of taps chosen by the Kaiser formula
stem(real(h_lp), "Lowpass Kaiser Impulse Response")
savefig("kaiser_lp_impulse.svg")

# Frequency response of the lowpass
H_lp = freqz(h_lp, 512, sr)
plotdb(H_lp, "Lowpass Kaiser Frequency Response")
savefig("kaiser_lp_response.svg")

# Highpass at 3 kHz
h_hp = fir_highpass_kaiser(3000.0, tbw, attn, sr)
stem(real(h_hp), "Highpass Kaiser Impulse Response")
savefig("kaiser_hp_impulse.svg")
H_hp = freqz(h_hp, 512, sr)
plotdb(H_hp, "Highpass Kaiser Frequency Response")
savefig("kaiser_hp_response.svg")

# Bandpass 1 kHz – 2.5 kHz
h_bp = fir_bandpass_kaiser(1000.0, 2500.0, tbw, attn, sr)
stem(real(h_bp), "Bandpass Kaiser Impulse Response")
savefig("kaiser_bp_impulse.svg")
H_bp = freqz(h_bp, 512, sr)
plotdb(H_bp, "Bandpass Kaiser Frequency Response")
savefig("kaiser_bp_response.svg")

# Notch at 1 kHz (200 Hz wide), manual tap count
h_notch = fir_notch(1000.0, 200.0, sr, 65, "hann")
stem(real(h_notch), "Notch Filter Impulse Response")
savefig("kaiser_notch_impulse.svg")
H_notch = freqz(h_notch, 512, sr)
plotdb(H_notch, "Notch Filter Frequency Response")
savefig("kaiser_notch_response.svg")

# Apply the lowpass to a two-tone signal
t  = linspace(0.0, 0.5, 4000)
x  = cos(t * 2.0 * pi * 500.0) + cos(t * 2.0 * pi * 3000.0)
y  = convolve(x, h_lp)
plot(real(y), "Lowpass Output (500 Hz passes, 3 kHz attenuated)")
savefig("kaiser_lp_output.svg")
