# Parks-McClellan optimal equiripple FIR filter design
#
# firpm(n_taps, bands, desired)          - uniform band weights
# firpm(n_taps, bands, desired, weights) - per-band weights
#
# bands   : normalized frequency edges, 0 = DC, 1 = Nyquist (f_s/2)
# desired : target amplitude at each band edge (piecewise-linear)
# weights : one value per band (default = 1.0 for each)

fs = 8000.0

# -- 1. Low-pass: 63-tap, cutoff 0.25 Nyquist (1 kHz)
#    Pass [0, 0.20], transition (0.20, 0.30), stop [0.30, 1.0]
h_lp = firpm(63,
             [0.0, 0.20, 0.30, 1.0],
             [1.0, 1.0,  0.0, 0.0])

print("Low-pass (63 taps):")
print("  tap count =")
print(length(h_lp))

Hz_lp = freqz(h_lp, 512, fs)
plotdb(Hz_lp, "PM Low-pass Frequency Response")

# -- 2. Band-pass: 79-tap, pass-band 0.30-0.50 Nyquist (2.4-4 kHz)
h_bp = firpm(79,
             [0.0, 0.25, 0.30, 0.50, 0.55, 1.0],
             [0.0, 0.0,  1.0,  1.0,  0.0,  0.0])

print("Band-pass (79 taps):")
print("  tap count =")
print(length(h_bp))

Hz_bp = freqz(h_bp, 512, fs)
plotdb(Hz_bp, "PM Band-pass Frequency Response")

# -- 3. Weighted low-pass: 51-tap, 10x heavier stop-band constraint
h_w = firpm(51,
            [0.0, 0.25, 0.35, 1.0],
            [1.0, 1.0,  0.0, 0.0],
            [1.0, 10.0])

print("Weighted low-pass (51 taps):")
print("  tap count =")
print(length(h_w))

Hz_w = freqz(h_w, 512, fs)
plotdb(Hz_w, "PM Weighted Low-pass Frequency Response")

# -- 4. Tap count comparison: Parks-McClellan vs Kaiser for same spec
h_kai = fir_lowpass_kaiser(0.25 * fs, 400.0, 60.0, fs)
print("Kaiser tap count for similar LP spec:")
print(length(h_kai))

# -- 5. Integer-coefficient low-pass: 8-bit and 16-bit comparison
#    firpmq iterates Remez with quantized coefs so rounding error is absorbed.
#    The returned coefficients are integer-valued (e.g. 127, -256).
#    To normalize for freqz: for a unit-gain passband, sum(h_int) == scale,
#    so divide by sum(h_int) to recover the unit-gain float response.

h_int8 = firpmq(63,
                [0.0, 0.20, 0.30, 1.0],
                [1.0, 1.0,  0.0, 0.0],
                [],
                8)

print("Integer LP (8-bit, 63 taps):")
print("  peak coefficient (should be 127):")
print(max(abs(h_int8)))
print("  scale factor:")
print(sum(h_int8))

Hz_int8 = freqz(h_int8 / sum(h_int8), 512, fs)
plotdb(Hz_int8, "Integer LP Frequency Response (8-bit)")

h_int16 = firpmq(63,
                 [0.0, 0.20, 0.30, 1.0],
                 [1.0, 1.0,  0.0, 0.0],
                 [],
                 16)

print("Integer LP (16-bit, 63 taps):")
print("  peak coefficient (should be 32767):")
print(max(abs(h_int16)))
print("  scale factor:")
print(sum(h_int16))

Hz_int16 = freqz(h_int16 / sum(h_int16), 512, fs)
plotdb(Hz_int16, "Integer LP Frequency Response (16-bit)")
