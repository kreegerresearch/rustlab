# Rustlab test script — produces out_*.csv files matching reference.m

# ── 1. FFT (real input) ───────────────────────────────────────────────────────
x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
X = fft(x)
save("out_fft_re.csv", real(X))
save("out_fft_im.csv", imag(X))

# ── 2. IFFT round-trip ────────────────────────────────────────────────────────
save("out_ifft.csv", real(ifft(X)))

# ── 3. FFT (complex input) ────────────────────────────────────────────────────
xc = [1.0+j*2.0, 3.0-j*1.0, -2.0+j*0.0, 0.0+j*4.0]
Xc = fft(xc)
save("out_fft_complex_re.csv", real(Xc))
save("out_fft_complex_im.csv", imag(Xc))

# ── 4. fftshift ───────────────────────────────────────────────────────────────
v8 = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]
save("out_fftshift_8.csv", real(fftshift(v8)))
v7 = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
save("out_fftshift_7.csv", real(fftshift(v7)))

# ── 5. Convolution ────────────────────────────────────────────────────────────
a = [1.0, 2.0, 3.0]
b = [0.5, -0.5, 0.25]
save("out_conv.csv", real(convolve(a, b)))

# ── 7. fir_lowpass (Hann, 33 taps, fc=1000 Hz, sr=8000 Hz) ─────────────────
h_lp = fir_lowpass(33, 1000.0, 8000.0, "hann")
save("out_fir_lp.csv", real(h_lp))

# ── 8. fir_highpass (Hann, 33 taps, fc=1000 Hz, sr=8000 Hz) ─────────────────
h_hp = fir_highpass(33, 1000.0, 8000.0, "hann")
save("out_fir_hp.csv", real(h_hp))

# ── 9. fir_bandpass (Hann, 33 taps, 500-2000 Hz, sr=8000 Hz) ────────────────
h_bp = fir_bandpass(33, 500.0, 2000.0, 8000.0, "hann")
save("out_fir_bp.csv", real(h_bp))

# ── 10. freqz magnitude (512 points, LP 33-tap Hann at 1000 Hz / 8000 sr) ───
Hz = freqz(h_lp, 512, 8000.0)
save("out_freqz_hz.csv",  real(Hz(1,:)))
save("out_freqz_mag.csv", abs(Hz(2,:)))

# ── 11. Hamming window filter (33 taps, LP at 1000 Hz, sr=8000) ──────────────
h_hamming = fir_lowpass(33, 1000.0, 8000.0, "hamming")
save("out_fir_hamming.csv", real(h_hamming))

# ── 12. firpm (Parks-McClellan LP, 63 taps) ───────────────────────────────────
h_pm = firpm(63, [0.0, 0.20, 0.30, 1.0], [1.0, 1.0, 0.0, 0.0])
save("out_firpm_lp.csv", real(h_pm))

# firpm bandpass (79 taps)
h_bp_pm = firpm(79, [0.0, 0.25, 0.30, 0.50, 0.55, 1.0], [0.0, 0.0, 1.0, 1.0, 0.0, 0.0])
save("out_firpm_bp.csv", real(h_bp_pm))

# ── 13. Kaiser lowpass (fc=1000, tbw=200, attn=60, sr=8000) ──────────────────
h_kai_lp = fir_lowpass_kaiser(1000.0, 200.0, 60.0, 8000.0)
save("out_kaiser_lp.csv", real(h_kai_lp))

# ── 14. Kaiser highpass (fc=3000, tbw=200, attn=60, sr=8000) ─────────────────
h_kai_hp = fir_highpass_kaiser(3000.0, 200.0, 60.0, 8000.0)
save("out_kaiser_hp.csv", real(h_kai_hp))

# ── 15. SNR formula test ─────────────────────────────────────────────────────
ref   = [1.0, 0.5, -0.5, -1.0, 0.0, 0.75, -0.75, 0.25]
noisy = [0.875, 0.5, -0.625, -0.875, 0.125, 0.75, -0.75, 0.25]
snr_v = snr(ref, noisy)
print("SNR:", snr_v)
save("out_snr.csv", snr_v)
