# Fixed-point arithmetic simulation for FPGA/ASIC bitwidth studies
#
# Demonstrates:
#   qfmt()     - define a Q-format spec (word bits, frac bits, rounding, overflow)
#   quantize() - snap float data to a Q-format grid
#   qconv()    - fixed-point FIR convolution
#   snr()      - signal-to-noise ratio in dB
#
# Goal: find the minimum coefficient bitwidth for a low-pass FIR filter
# that meets a 50 dB SNR target.

sr   = 48000.0
n    = 2048

# -- 1. Design a float-precision low-pass FIR (Parks-McClellan) ---------------
#    Pass band: 0 - 0.20 Nyquist, stop band: 0.30 - 1.0 Nyquist
h_float = firpm(63,
                [0.0, 0.20, 0.30, 1.0],
                [1.0, 1.0,  0.0,  0.0]);

print("Filter tap count:");
print(length(h_float));

# -- 2. Generate a broadband test signal (white noise) ------------------------
#    A single sinusoid gives unreliable SNR estimates because coefficient
#    quantization shifts the filter phase, causing the phasor difference to
#    dominate.  White noise exercises the full passband uniformly.
#    Scale to 0.3 sigma so samples stay inside the Q1.14 range (-2 to ~2).
x    = randn(n) * 0.3;

# Float reference output
y_ref = real(convolve(x, real(h_float)));

# -- 3. Define data format: Q1.14 (16-bit, handles amplitudes up to ~2.0) ----
fmt_data = qfmt(16, 14, "round_even", "saturate");

print("Data format:");
print(fmt_data);

xq = quantize(x, fmt_data);

# -- 4. Sweep coefficient word width: 8, 10, 12, 14, 16 bits -----------------
#    For each width use Q(word-1) format (pure fractional, range -1 to ~1)

fmt8  = qfmt(8,  7,  "round_even", "saturate");
fmt10 = qfmt(10, 9,  "round_even", "saturate");
fmt12 = qfmt(12, 11, "round_even", "saturate");
fmt14 = qfmt(14, 13, "round_even", "saturate");
fmt16 = qfmt(16, 15, "round_even", "saturate");

hq8  = quantize(h_float, fmt8);
hq10 = quantize(h_float, fmt10);
hq12 = quantize(h_float, fmt12);
hq14 = quantize(h_float, fmt14);
hq16 = quantize(h_float, fmt16);

# Fixed-point FIR: convolve quantized input with quantized coefficients
# Output quantized to same data format
y8  = qconv(xq, real(hq8),  fmt_data);
y10 = qconv(xq, real(hq10), fmt_data);
y12 = qconv(xq, real(hq12), fmt_data);
y14 = qconv(xq, real(hq14), fmt_data);
y16 = qconv(xq, real(hq16), fmt_data);

# Trim reference to match qconv output length (len(x) + len(h) - 1)
n_out = length(h_float) + n - 1;

# -- 5. Compute SNR for each bitwidth -----------------------------------------
print("--- Coefficient bitwidth sweep ---");
print(" 8-bit coeff SNR (dB):");
print(snr(y_ref, y8));
print("10-bit coeff SNR (dB):");
print(snr(y_ref, y10));
print("12-bit coeff SNR (dB):");
print(snr(y_ref, y12));
print("14-bit coeff SNR (dB):");
print(snr(y_ref, y14));
print("16-bit coeff SNR (dB):");
print(snr(y_ref, y16));

# -- 6. Coefficient quantization error spectrum for 8-bit vs 16-bit -----------
h_err8  = real(h_float) - real(hq8);
h_err16 = real(h_float) - real(hq16);

print("Max coeff error  8-bit:");
print(max(abs(h_err8)));
print("Max coeff error 16-bit:");
print(max(abs(h_err16)));

# -- 7. Save frequency response comparison ------------------------------------
Hz_float = freqz(real(h_float), 512, sr);
Hz_q8    = freqz(real(hq8),     512, sr);
Hz_q16   = freqz(real(hq16),    512, sr);

plotdb(Hz_float, "Float reference (63-tap LP)");
plotdb(Hz_q8, "Q8 coefficients");
plotdb(Hz_q16, "Q16 coefficients");
