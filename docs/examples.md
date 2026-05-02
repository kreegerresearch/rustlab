# Example Walkthroughs

This document walks through the scripts in `examples/` step by step.

---

## `examples/complex_basics.rlab`

**Full script:**

```
# Complex number basics
# j is the imaginary unit: Complex(0, 1)

a = 123.2 + j*123.0
b = 2.0 * j
c = a + b
print(a)
print(b)
print(c)

# Magnitude and phase
print(abs(a))
print(angle(a))

# Vector of complex numbers
v = [1.0 + j*0.5, 2.0 + j*1.0, 3.0 + j*1.5]
print(v)

# Plot magnitude and save to file
mag = abs(v)
print(mag)
plot(mag, "Complex Vector Magnitudes")
savefig("complex_magnitude.svg")
```

**Step-by-step explanation:**

1. **`j` is predefined** as `Complex(0.0, 1.0)`. You never need to declare it.

2. **`a = 123.2 + j*123.0`** — scalar multiplication `j*123.0` gives `Complex(0, 123)`, then addition with `123.2` gives `Complex(123.2, 123.0)`.

3. **`b = 2.0 * j`** — shorthand for `Complex(0.0, 2.0)`.

4. **`c = a + b`** — element-wise complex addition: `Complex(123.2, 125.0)`.

5. **`abs(a)`** — computes the L2 magnitude `sqrt(123.2^2 + 123.0^2) ≈ 174.06`.

6. **`angle(a)`** — computes the phase angle in radians using `atan2(im, re) ≈ 0.7998` (about 45.8°).

7. **`v = [...]`** — constructs a `CVector` (complex ndarray). Each element can be any complex expression.

8. **`abs(v)`** — element-wise magnitude over the whole vector, returning a real `RVector`.

9. **`plot(mag, ...); savefig("complex_magnitude.svg")`** — plots the magnitude vector then writes the chart to `complex_magnitude.svg` in the current directory. Use `.png` extension for a raster image.

**Run it:**

```sh
rustlab run examples/complex_basics.rlab
```

---

## `examples/vectors.rlab`

**Full script:**

```
# Vector operations
# Demonstrates: range operator, indexing, concatenation, element-wise ops, transpose

# Range operator: start:stop and start:step:stop
t = 0:0.1:1                         # 11 points: 0.0, 0.1, ..., 1.0
n = 1:8                              # integer sequence [1, 2, ..., 8]
print(n)

# 1-based indexing
first = n(1)
third = n(3)
last  = n(end)
slice = n(3:6)
print(slice)
print(last)

# Element-wise operators
sq   = n .^ 2                        # [1, 4, 9, 16, 25, 36, 49, 64]
inv  = 1.0 ./ n                      # harmonic series
prod = n .* n
print(sq)

# Concatenation
a = 1:4
b = 5:8
c = [a, b]                           # [1, 2, 3, 4, 5, 6, 7, 8]
print(c)

# Transpose
row = [1.0 + j*0.0, 2.0 + j*1.0, 3.0 + j*2.0]
col = row'
print(col)

# Complex sinusoid using range + element-wise ops
omega  = 2.0 * pi * 440.0
signal = cos(t * omega) + j * sin(t * omega)
mag    = abs(signal)                 # all ones — unit circle
print(mag)

# Interactive terminal plot
plot(signal, "440 Hz Complex Sinusoid")

# Save magnitude and real part to files
plot(real(signal), "440 Hz Sinusoid (Real Part)")
savefig("sinusoid_real.svg")
plot(mag, "Sinusoid Magnitude (unit circle)")
savefig("sinusoid_magnitude.svg")
```

**Step-by-step explanation:**

1. **`t = 0:0.1:1`** — three-part range `start:step:stop`. Generates `[0.0, 0.1, 0.2, ..., 1.0]` (11 values). The step can be fractional or negative.

2. **`n(1)`, `n(end)`, `n(3:6)`** — indexing is **1-based**. `end` is a special keyword that expands to the length of the vector, so `n(end-1:end)` gives the last two elements.

3. **`.^`, `.*`, `./`** — dot operators always work element-wise. Without the dot, `*` and `^` broadcast a scalar onto a vector. With the dot, both operands must have the same length.

4. **`[a, b]`** — when `a` and `b` are vectors, placing them inside `[...]` concatenates them into a single longer vector. Mix scalars and vectors freely: `[0, n, 100]` prepends 0 and appends 100 to `n`.

5. **`row'`** — the apostrophe is the conjugate transpose operator. For real vectors it returns the same values. For complex vectors it conjugates each element.

6. **`cos(t * omega) + j * sin(t * omega)`** — Euler's formula `e^(jωt)` expressed using the `j` constant and element-wise arithmetic. The result is a complex sinusoid rotating on the unit circle, so `abs(signal)` is all ones.

7. **`plot(signal, ...)`** — renders the complex signal interactively in the terminal (magnitude and phase side by side). The REPL waits for a keypress before returning.

8. **`plot(...); savefig(...)`** — plots data then writes the chart to an SVG or PNG file for use in reports without interrupting a script run.

**Run it:**

```sh
rustlab run examples/vectors.rlab
```

---

## `examples/lowpass.rlab`

**Full script:**

```
# FIR lowpass filter design
# 32-tap Hann-windowed sinc at 1 kHz, 44.1 kHz sample rate

h = fir_lowpass(32, 1000.0, 44100.0, "hann")
print(h)

# Interactive: impulse response stem plot
stem(real(h), "Lowpass Impulse Response")

# Save impulse response to file
stem(real(h), "Lowpass Impulse Response")
savefig("lowpass_impulse.svg")

# Frequency response
Hz = freqz(h, 512, 44100.0)

# Interactive: dB magnitude response with Hz x-axis
plotdb(Hz, "Lowpass Frequency Response")

# Save frequency response to file
savefig("lowpass_response.svg")
```

**Step-by-step explanation:**

1. **`fir_lowpass(32, 1000.0, 44100.0, "hann")`**

   Designs a 32-tap windowed-sinc lowpass filter. Arguments:
   - `32` — number of taps (filter length). More taps means a sharper transition.
   - `1000.0` — cutoff frequency in Hz.
   - `44100.0` — sample rate in Hz (CD quality). Normalized cutoff = 1000/44100 ≈ 0.0227.
   - `"hann"` — apply a Hann (raised-cosine) window to the ideal sinc kernel to reduce Gibbs ripple.

   The function returns a `FirFilter` whose coefficients form a `CVector`. Because this is a real-valued filter, the imaginary parts of all coefficients are zero.

2. **`print(h)`** — prints all 32 complex coefficients.

3. **`stem(real(h), ...)`** — interactive terminal stem plot of the impulse response. Each tap is a vertical bar. The bell-shaped envelope is the Hann window multiplied onto the sinc function.

4. **`stem(real(h), ...); savefig("lowpass_impulse.svg")`** — saves the same stem plot as a vector SVG file.

5. **`Hz = freqz(h, 512, 44100.0)`** — evaluates the frequency response at 512 points from 0 to the Nyquist frequency. Returns a 2×512 matrix:
   - Row 1 (`Hz(1)`) — frequency axis in Hz
   - Row 2 (`Hz(2)`) — complex frequency response H(f)

6. **`plotdb(Hz, ...)`** — interactive terminal chart showing 20·log₁₀|H(f)| (dB magnitude) on the y-axis and frequency in Hz on the x-axis.

7. **`savefig("lowpass_response.svg")`** — saves the dB frequency response chart (from the preceding `plotdb`) to an SVG file.

**Using the CLI instead of scripting:**

```sh
rustlab filter fir --taps 32 --cutoff 1000 --sr 44100 --window hann
```

**Run it:**

```sh
rustlab run examples/lowpass.rlab
```

---

## `examples/bandpass.rlab`

**Full script:**

```
# FIR bandpass filter: 500 Hz – 2000 Hz, 44.1 kHz sample rate

h = fir_bandpass(64, 500.0, 2000.0, 44100.0, "hamming")

# Save impulse response
stem(real(h), "Bandpass Impulse Response")
savefig("bandpass_impulse.svg")

# Frequency response — interactive and saved
Hz = freqz(h, 512, 44100.0)
plotdb(Hz, "Bandpass Frequency Response")
savefig("bandpass_response.svg")

# Apply to a test signal: sum of tones
t  = linspace(0.0, 1.0, 4410)
x1 = cos(t * 2.0 * pi * 250.0)    # 250 Hz — should be attenuated
x2 = cos(t * 2.0 * pi * 1000.0)   # 1 kHz  — should pass
x  = x1 + x2
y  = convolve(x, h)

# Interactive: filtered output
plot(real(y), "Bandpass Output (1 kHz passes, 250 Hz attenuated)")

# Save filtered output to file
savefig("bandpass_output.svg")
```

**Step-by-step explanation:**

1. **`fir_bandpass(64, 500.0, 2000.0, 44100.0, "hamming")`**

   Designs a 64-tap Hamming-windowed FIR bandpass filter. Arguments:
   - `64` — number of taps. Higher tap count improves stopband attenuation.
   - `500.0` — lower passband edge in Hz.
   - `2000.0` — upper passband edge in Hz.
   - `44100.0` — sample rate in Hz.
   - `"hamming"` — Hamming window, which offers ~41 dB stopband attenuation.

   Internally the filter is constructed as `h_lp(2000) - h_lp(500)`: the difference of two lowpass filters (spectral subtraction).

2. **`stem(real(h), ...); savefig("bandpass_impulse.svg")`** — saves the impulse response stem plot to file. For bandpass filters, the stem plot shows the characteristic alternating-sign oscillation of the underlying sinc difference.

3. **`Hz = freqz(h, 512, 44100.0)`** — computes the frequency response. `plotdb` shows the passband (500–2000 Hz) at 0 dB, with the stopband rolling off below and above.

4. **`linspace(0.0, 1.0, 4410)`** — generates 4410 evenly-spaced time samples from 0 to 1 second. With a 44100 Hz sample rate, `4410` samples represents exactly 0.1 seconds.

5. **`x1 = cos(t * 2.0 * pi * 250.0)`** — a 250 Hz sinusoid. This frequency is below the passband (500–2000 Hz) so it will be attenuated by the filter.

6. **`x2 = cos(t * 2.0 * pi * 1000.0)`** — a 1 kHz sinusoid. This frequency sits in the middle of the passband and will pass through with minimal attenuation.

7. **`y = convolve(x, h)`** — linearly convolve `x` with the filter kernel `h`. The output `y` has length `len(x) + len(h) - 1 = 4410 + 64 - 1 = 4473`. The 250 Hz component is attenuated; the 1 kHz component survives.

8. **`plot(real(y), ...)` + `savefig("bandpass_output.svg")`** — view the filtered output interactively and save it for a report.

**Experiment:** change the cutoff frequencies to `[200.0, 800.0]` and observe that the 250 Hz tone now passes while the 1 kHz tone is suppressed.

**Run it:**

```sh
rustlab run examples/bandpass.rlab
```

---

## `examples/fft.rlab`

**Full script:**

```
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

# Interactive: dB magnitude spectrum with Hz x-axis
plotdb(H, "Magnitude Spectrum")

# Save to file
savefig("fft_spectrum.svg")

# Round-trip: reconstruct original signal from spectrum
x_rec = real(ifft(X))
plot(x_rec, "Reconstructed Signal")
savefig("fft_reconstructed.svg")
```

**Step-by-step explanation:**

1. **`t = linspace(0.0, (n-1)/sr, n)`** — builds a 256-sample time axis from 0 to 255/8000 ≈ 0.031875 seconds.

2. **`x = cos(...500...) + cos(...1500...)`** — sum of a 500 Hz and a 1500 Hz cosine, giving a signal with two spectral peaks.

3. **`plot(real(x), ...); savefig("fft_input.svg")`** — saves the input signal waveform to SVG.

4. **`X = fft(x)`** — computes the forward FFT. Because `n = 256` is already a power of two, no zero-padding is needed and `len(X) == 256`. For non-power-of-two inputs, `fft` automatically pads to the next power of two.

5. **`H = spectrum(X, sr)`** — the key step. `spectrum` applies `fftshift` to center DC, pairs the result with the Hz frequency axis, and returns a **2×n matrix** compatible with `plotdb`. This is the standard way to display FFT output with a correct frequency axis. Internally it is equivalent to:
   ```
   Xs    = fftshift(X)
   freqs = fftshift(fftfreq(len(X), sr))
   # then packaged as a 2×n matrix
   ```

6. **`plotdb(H, ...)`** — interactive terminal dB chart with Hz on the x-axis. You will see two symmetric peaks at ±500 Hz and ±1500 Hz sitting above a noise floor.

7. **`savefig("fft_spectrum.svg")`** — saves the dB spectrum chart (from the preceding `plotdb`) as an SVG file.

8. **`real(ifft(X))`** — inverse FFT followed by discarding the (numerically tiny) imaginary parts. The result matches the original signal `x` to within floating-point precision.

**Key identities:**

| Expression | What you get |
|-----------|--------------|
| `spectrum(fft(x), sr)` | DC-centered dB spectrum with Hz axis (for plotdb) |
| `abs(fft(x))` | Raw magnitude spectrum (sample-indexed) |
| `angle(fft(x))` | Phase spectrum |
| `real(ifft(fft(x)))` | Reconstructed signal (round-trip) |
| `fftfreq(n, sr)` | Raw frequency axis in Hz (not shifted) |

**Run it:**

```sh
rustlab run examples/fft.rlab
```

---

## `examples/save_load.rlab`

**Full script:**

```
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

Hz = freqz(h, 512, sr)

save("session.npz", "signal", x, "filter", h, "response", Hz)

whos("session.npz")

x_back  = load("session.npz", "signal")
h_back  = load("session.npz", "filter")
Hz_back = load("session.npz", "response")

print("NPZ signal round-trip max error:", max(abs(real(x_back)  - real(x))))
print("NPZ filter round-trip max error:", max(abs(real(h_back)  - real(h))))

plotdb(Hz_back, "Reloaded Frequency Response")
savefig("session_response.svg")
print("Saved session_response.svg")
```

**Step-by-step explanation:**

1. **`save("signal.npy", x)`** — writes `x` (a real-valued vector) to a NumPy-format binary file. Real arrays are stored as `float64` (`<f8` dtype); complex arrays are stored as `complex128` (`<c16`). The file is identical to what `np.save("signal.npy", x)` would produce in Python, so it can be passed directly to numpy, Julia, or any other tool that reads the NPY format.

2. **`x2 = load("signal.npy")`** — reads the file back. The shape (vector/matrix) and type (real/complex) are recovered from the file header automatically. The round-trip error printed here is zero to floating-point precision.

3. **`save("filter.csv", h)` / `load("filter.csv")`** — the CSV path stores values as text. Real values are written as plain decimals. Complex values use `a+bi` notation (e.g., `1.0+2.0i`). A single-column file loads back as a vector; a multi-column file as a matrix; a single value as a scalar. CSV is useful when you need a human-readable format or compatibility with spreadsheets.

4. **`save("session.npz", "signal", x, "filter", h, "response", Hz)`** — saves three arrays into a single `.npz` archive under the names `"signal"`, `"filter"`, and `"response"`. The `.npz` format is a standard ZIP file containing one `.npy` entry per variable. In Python: `d = np.load("session.npz"); d["signal"]`.

   The argument pattern after the filename is **alternating name/value pairs**:
   ```
   save("out.npz", "a", a, "b", b, "c", c)
   ```
   Any number of pairs can be given.

5. **`whos("session.npz")`** — lists the contents of the archive without loading any data:
   ```
     Name                 Type       Size
     ────────────────────────────────────────────
     signal               real       256
     filter               real       65
     response             complex    2×512
   ```
   `response` is a 2×512 complex matrix (as returned by `freqz`), so it is stored as `complex128`.

6. **`load("session.npz", "signal")`** — loads a specific named variable from the archive. Call `load` once per variable you need; the archive is not fully extracted.

7. **`plotdb(Hz_back, ...); savefig("session_response.svg")`** — demonstrates that the reloaded `response` matrix feeds directly back into `plotdb`, just as the original `freqz` output would.

**Supported formats summary:**

| Function | `.npy` | `.csv` | `.npz` |
|----------|--------|--------|--------|
| `save`   | single value | single value | multiple named values |
| `load`   | `load(file)` | `load(file)` | `load(file, "name")` |
| `whos`   | — | — | `whos(file)` |

**Run it:**

```sh
rustlab run examples/save_load.rlab
```

---

## `examples/kaiser_fir.rlab`

**Full script:**

```
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
plot(real(y), "Lowpass output: 500 Hz passes, 3 kHz attenuated")
savefig("kaiser_lp_output.svg")
```

**Step-by-step explanation:**

1. **`fir_lowpass_kaiser(1000.0, tbw, attn, sr)`**

   The three key parameters are:
   - `tbw = 200.0` — transition bandwidth (Hz). The filter rolls off from −3 dB at 1000 Hz to `attn` dB of attenuation by 1200 Hz. A narrower transition costs more taps.
   - `attn = 60.0` — target stopband attenuation in dB. Common values: 40 dB (Hamming-equivalent), 60 dB (good general use), 80 dB (demanding applications).

   Internally, the Kaiser formulas (Harris 1978) compute:
   - **β (beta)** — controls the window shape: `β = 0.1102 × (A − 8.7)` for `A ≥ 50 dB`
   - **N (tap count)** — rounded up to the nearest odd integer: `N = ⌈D / Δf⌉ + 1` where `D = (A − 7.95) / 14.36` and `Δf = tbw / sr`

   For `attn = 60 dB` and `tbw = 200 Hz` at 8 kHz: β ≈ 5.65, N ≈ 185 taps.

2. **`stem(real(h_lp), ...); savefig("kaiser_lp_impulse.svg")`** — saves the impulse response as a stem plot SVG. For Kaiser-windowed filters, the taper is more pronounced than a Hann window, reflecting the higher stopband attenuation.

3. **`H_lp = freqz(h_lp, 512, sr)`** — evaluates `H(f)` at 512 frequency points. Returns a 2×512 matrix:
   - Row 1 (`H_lp(1)`) — frequency axis in Hz
   - Row 2 (`H_lp(2)`) — complex frequency response H(f)

4. **`plotdb(H_lp, ...)`** — interactive terminal dB-magnitude chart with a Hz x-axis. The Kaiser filter's equiripple stopband is visible as a flat floor at −60 dB.

5. **`savefig("kaiser_lp_response.svg")`** — saves the dB frequency response (from the preceding `plotdb`) to SVG for reports.

6. **Highpass and bandpass** — the same `freqz`/`plotdb`/`savefig` pattern is applied to each filter type. The interactive `plotdb` pushes data to the figure, then `savefig` renders it to SVG.

7. **`fir_notch(1000.0, 200.0, sr, 65, "hann")`** — designs a bandpass at 900–1100 Hz then inverts the spectrum: `h_notch[n] = −h_bp[n]`, `h_notch[center] += 1`. The result passes all frequencies *except* the 200 Hz-wide notch around 1 kHz. Unlike the Kaiser variants, `fir_notch` requires you to specify `num_taps` and `window` explicitly.

8. **Two-tone filtering test** — a 500 Hz tone and a 3 kHz tone are mixed and passed through `h_lp`. After filtering, the 3 kHz component is attenuated by ~60 dB relative to the 500 Hz component. Both interactive and SVG outputs are produced.

**Design guidelines:**

| Attenuation | Typical β | Use case |
|-------------|-----------|----------|
| 40 dB | 3.40 | General audio, modest requirements |
| 60 dB | 5.65 | Most signal processing tasks |
| 80 dB | 7.86 | High-fidelity or scientific measurement |
| 100 dB | 10.06 | Demanding interference rejection |

Narrowing `tbw` (sharper transition) or increasing `attn` both increase the tap count and computational cost.

**Files produced by this script:**

| File | Content |
|------|---------|
| `kaiser_lp_impulse.svg` | Lowpass impulse response stem plot |
| `kaiser_lp_response.svg` | Lowpass dB frequency response |
| `kaiser_hp_impulse.svg` | Highpass impulse response stem plot |
| `kaiser_hp_response.svg` | Highpass dB frequency response |
| `kaiser_bp_impulse.svg` | Bandpass impulse response stem plot |
| `kaiser_bp_response.svg` | Bandpass dB frequency response |
| `kaiser_notch_impulse.svg` | Notch impulse response stem plot |
| `kaiser_notch_response.svg` | Notch dB frequency response |
| `kaiser_lp_output.svg` | Filtered two-tone signal |

**Run it:**

```sh
rustlab run examples/kaiser_fir.rlab
```

---

## `examples/upfirdn.rlab`

Demonstrates the three fundamental use cases of `upfirdn`: interpolation,
decimation, and rational sample-rate conversion. All three share the same
pattern — design a lowpass anti-aliasing / anti-imaging filter, then call
`upfirdn` with the appropriate `p` and `q`.

**Full script:**

```
# upfirdn — polyphase upsample / filter / downsample

sr   = 8000.0
n    = 64
tone = 300.0

t = linspace(0.0, (n - 1) / sr, n);
x = real(cos(2.0 * pi * tone * t));

plot(x, "Input — 300 Hz at 8 kHz")
savefig("upfirdn_input.svg")

# 1. Interpolation by 4
h_interp = fir_lowpass(64, sr / 2.0 / 4, sr, "hann");
y_up = upfirdn(x, h_interp, 4, 1);
print("Interpolated length: ", len(y_up))
plot(real(y_up), "Interpolated x4 (32 kHz)")
savefig("upfirdn_interp4.svg")

# 2. Decimation by 4
h_decim = fir_lowpass(64, sr / 2.0 / 4, sr, "hann");
y_down = upfirdn(x, h_decim, 1, 4);
print("Decimated length:  ", len(y_down))
plot(real(y_down), "Decimated x4 (2 kHz)")
savefig("upfirdn_decim4.svg")

# 3. Rational SRC 3/2
cutoff = (sr / 2.0) / 3.0
h_src = fir_lowpass(128, cutoff, sr, "hann");
y_src = upfirdn(x, h_src, 3, 2);
print("SRC 3/2 length:  ", len(y_src))
plot(real(y_src), "Rate conversion 3/2")
savefig("upfirdn_src32.svg")
```

**Step-by-step explanation:**

1. **Test signal** — a 300 Hz cosine at 8 kHz, 64 samples long.

2. **Interpolation by 4** (`p=4, q=1`):
   - The output sample rate becomes `8000 × 4 = 32 kHz`.
   - The anti-imaging filter cuts off at `sr/2/p = 1 kHz` — the original
     signal band — to suppress the spectral images created by upsampling.
   - Output length: `(64−1)·4 + 64 − 1)/1 + 1 = 316`.

3. **Decimation by 4** (`p=1, q=4`):
   - The output sample rate becomes `8000 / 4 = 2 kHz`.
   - The anti-aliasing filter cuts off at `sr/2/q = 1 kHz` — the new
     Nyquist — to prevent aliasing from the downsampled images.
   - Output length: `(64−1)·1 + 64 − 1)/4 + 1 = 32`.

4. **Rational SRC 3/2** (`p=3, q=2`):
   - Effective rate change: `× 3/2`, so 8 kHz → 12 kHz.
   - Cutoff is `sr/2/max(p,q) = sr/2/3 ≈ 1333 Hz` — the lower of the two
     Nyquist limits after up and down conversion.
   - A longer filter (128 taps) is used to maintain stopband attenuation
     after the narrower transition band.
   - Output length: `(64−1)·3 + 128 − 1)/2 + 1 = 159`.

5. **Filter design rule of thumb:**
   - Interpolation `p`: cutoff = `sr / (2·p)`
   - Decimation `q`: cutoff = `sr / (2·q)`
   - Rational `p/q`: cutoff = `sr / (2·max(p, q))`

**Run it:**

```sh
rustlab run examples/upfirdn.rlab
```

---

## `examples/audio/filter.rlab` — Real-time FIR streaming

**Full script:**

```r
sr     = 44100.0;
cutoff = 1000.0 / (sr / 2.0);

h  = firpm(64, [0, cutoff * 0.9, cutoff, 1.0], [1, 1, 0, 0]);
st = state_init(length(h) - 1);

adc = audio_in(sr, 256);
dac = audio_out(sr, 256);

while true
    frame     = audio_read(adc);
    [out, st] = filter_stream(frame, h, st);
    audio_write(dac, out);
end
```

**Step by step:**

1. **Filter design** — `firpm` designs a 64-tap Parks-McClellan equiripple lowpass.
   Band edges are normalised to [0, 1] where 1 = Nyquist. The 10% margin between
   passband edge (`cutoff * 0.9`) and stopband edge (`cutoff`) gives a clean transition.

2. **`state_init(length(h) - 1)`** — allocates a zero-filled history buffer of
   M−1 = 63 samples. This buffer carries the tail of each input frame into the next,
   making the output indistinguishable from a single offline `convolve(full_signal, h)`.

3. **`audio_in` / `audio_out`** — lightweight metadata descriptors. They open no
   files or devices; all I/O goes through stdin/stdout as raw f32 LE mono PCM.

4. **`while true` loop** — runs until stdin closes. `audio_read` raises `AudioEof`
   on a clean EOF, which `rustlab run` silently maps to exit code 0.

5. **`filter_stream(frame, h, st)`** — overlap-save algorithm, one frame at a time:
   - Prepend M−1 history samples → extended buffer of length `FRAME + M − 1`
   - Direct-form convolution with `h` → FRAME output samples
   - Store last M−1 input samples as new history

**Running it:**

```sh
# macOS (sox required):
sox -d -r 44100 -c 1 -b 32 -e float -t raw - \
  | rustlab run examples/audio/filter.rlab \
  | sox -r 44100 -c 1 -b 32 -e float -t raw - -d

# Hardware-free test:
bash examples/audio/test_filter.sh
```

---

## `examples/audio/spectrum_monitor.rlab` — Live FFT spectrum monitor

Captures microphone input and displays a continuously updating two-panel
terminal plot using `figure_live`:

- **Panel 1** — time-domain waveform (raw input)
- **Panel 2** — Hann-windowed FFT magnitude in dB (DC to Nyquist)

**Key patterns:**

```r
sr       = 44100.0;
fft_size = 1024;
half     = fft_size / 2;

h    = window(fft_size, "hann");
t_ms = linspace(0.0, fft_size / sr * 1000.0, fft_size);
freqs = fftfreq(fft_size, sr);
f_hz  = freqs(1:half);

adc = audio_in(sr, fft_size);
fig = figure_live(2, 1);

while true
    frame = audio_read(adc);

    X  = fft(frame .* h);
    Xd = mag2db(X(1:half));

    plot_update(fig, 1, t_ms, frame);    # waveform
    plot_update(fig, 2, f_hz, Xd);       # spectrum
    figure_draw(fig);
end
```

**Running it:**

```sh
# macOS / Linux / synthetic test:
./examples/audio/spectrum_monitor.sh
./examples/audio/spectrum_monitor.sh --test   # no hardware needed
```

---

## `examples/vector_calc.rlab`

**Full script:**

```
# Vector calculus on uniform 2-D grids
# Demonstrates: gradient(F), divergence(Fx, Fy), curl(Fx, Fy)
#
# Grid convention: F(i, j) ↔ position (x = (j-1)*dx, y = (i-1)*dy).
# Rows index y, columns index x — matches Octave / NumPy.
# Trailing `;` suppresses implicit echo for assignments.

dx = 0.1;
dy = 0.1;
xs = -1:dx:1;
ys = -1:dy:1;
[X, Y] = meshgrid(xs, ys);

# 1. gradient: F = x² + y² → ∇F = (2x, 2y)
F = X .^ 2 + Y .^ 2;
[Fx, Fy] = gradient(F, dx, dy);
print(Fx(11, 11))      # ≈ 0
print(Fy(21, 21))      # ≈ 2 (boundary)

# 2. divergence: F = (x, y) → ∇·F = 2 everywhere
D = divergence(X, Y, dx, dy);
print(D(11, 11))       # ≈ 2

# 3. curl: F = (-y, x) → ∇×F · ẑ = 2
Cz = curl(-Y, X, dx, dy);
print(Cz(11, 11))      # ≈ 2

# 4. Laplacian via composition: ∇²V = ∇·(∇V); for V = x² + y²  → 4
[Vx, Vy] = gradient(F, dx, dy);
laplV = divergence(Vx, Vy, dx, dy);
print(laplV(11, 11))   # ≈ 4

# 5. Complex inputs: F = exp(j*x) → ∂F/∂x = j*exp(j*x)
Fc = exp(j * X);
[Fxc, Fyc] = gradient(Fc, dx, dy);
print(Fxc(11, 11))     # ≈ 0 + j
```

**Step-by-step explanation:**

1. **`meshgrid(xs, ys)`** returns two 21×21 matrices. Row index `i` corresponds to `y = ys(i)`, column index `j` to `x = xs(j)`. All vector-calc builtins follow the same convention.

2. **`gradient(F, dx, dy)`** returns a tuple `[Fx, Fy]` of the same shape as `F`. Interior points use 2nd-order central differences; boundary points use 2nd-order one-sided stencils so the output keeps the input shape (NumPy convention). Quadratics like `x² + y²` are reproduced exactly even at the boundary.

3. **`divergence(Fx, Fy, dx, dy)`** computes `∂Fx/∂x + ∂Fy/∂y`. For the radial field `F = (x, y)` this is `1 + 1 = 2` everywhere.

4. **`curl(Fx, Fy, dx, dy)`** returns the z-component of `∇×F` as a scalar field: `∂Fy/∂x − ∂Fx/∂y`. The solid-rotation field `(-y, x)` has constant curl `2`. The radial field `(x, y)` is irrotational (curl `0`).

5. **Composition** — calling `divergence(gradient(V))` gives the Laplacian `∇²V`. For `V = x² + y²` the analytic answer is `4` and the numerical result matches.

6. **Complex inputs are first-class.** All three builtins operate on the complex matrix type that the script engine uses internally, so frequency-domain fields like `exp(j·x)` work without conversion. The numerical derivative of `exp(j·x)` at `x = 0` is `j` (with O(dx²) discretization error).

7. **`dx` and `dy` default to 1.0** if omitted: `gradient(F)` is shorthand for `gradient(F, 1.0, 1.0)`. Same for `divergence(Fx, Fy)` and `curl(Fx, Fy)`.

8. **Each axis must have length ≥ 3** so the 2nd-order one-sided boundary stencil has enough samples. A clear error is raised otherwise.

**Run it:**

```sh
rustlab run examples/vector_calc.rlab
```

---

## `examples/tensor3/tensor3.rlab`

**Full script:**

```
# ── Construction ─────────────────────────────────────────────────
A = zeros3(2, 3, 4);
B = ones3(2, 3, 4);
C = rand3(2, 3, 4);
D = randn3(2, 3, 4);

# Build a known tensor with column-major walk: T(:, 1, 1) = [1; 2], etc.
T = reshape(1:24, 2, 3, 4);

# ── Indexing ─────────────────────────────────────────────────────
print(T(1, 1, 1))               # → 1
print(T(2, 3, 4))               # → 24

page2 = T(:, :, 2);             # Matrix(2, 3) — trailing singleton dropped
row1  = T(1, :, :);             # Matrix(3, 4)
chunk = T(:, :, 1:2);           # Tensor3(2, 3, 2) — range slice keeps rank

# ── Page assignment ──────────────────────────────────────────────
U = zeros3(2, 2, 3);
U(:, :, 2) = [1, 2; 3, 4];
U(1, 1, 1) = 99;

# ── Arithmetic ───────────────────────────────────────────────────
E = T * 2;                      # scalar broadcast
H = B + T;                      # element-wise (same shape)
J = B .* T;                     # element-wise multiply
# T1 * T2 errors — use .* for element-wise
# Matrix + Tensor3 also errors — no broadcasting between ranks

# ── Reshape / permute / squeeze ──────────────────────────────────
flat = reshape(T, 24, 1);       # Vector of length 24 (column-major walk)
back = reshape(flat, 2, 3, 4);  # equals T

P = permute(T, [2, 1, 3]);      # swap rows ↔ cols
print(size(P))                   # → [3, 2, 4]

M1 = squeeze(reshape(1:6, 2, 3, 1));   # → Matrix(2, 3)
V1 = squeeze(reshape(1:5, 1, 1, 5));   # → Vector(5)

# ── cat along page axis ──────────────────────────────────────────
stacked = cat(3, [1, 2; 3, 4], [5, 6; 7, 8]);   # Tensor3(2, 2, 2)
more    = cat(3, stacked, [9, 10; 11, 12]);     # Tensor3(2, 2, 3)

# ── I/O round-trip ───────────────────────────────────────────────
save("/tmp/rustlab_demo_tensor3.npy", T);
T_loaded = load("/tmp/rustlab_demo_tensor3.npy");
print(ndims(T_loaded))           # → 3
```

**Step-by-step explanation:**

1. **Constructors.** `zeros3 / ones3 / rand3 / randn3` mirror their Matrix counterparts. The bracket form `zeros3([m, n, p])` accepts the output of `size()` directly, which is handy when copying the shape of another tensor.

2. **`reshape(1:24, 2, 3, 4)`** uses a **column-major** walk (the Octave convention): `T(:, 1, 1) = [1; 2]`, `T(:, 2, 1) = [3; 4]`, etc. The 4-argument `reshape` accepts vectors, matrices, or tensors as input and produces a Tensor3 when the last argument is supplied.

3. **Indexing is 1-based** on every axis, including the page axis. Slicing a single trailing axis with `:` keeps the page dimension if you pass a range (`T(:, :, 1:2)` → Tensor3(2, 3, 2)) but drops it for a singleton index (`T(:, :, 2)` → Matrix(2, 3)). Slicing internal singletons keeps them — `T(1, :, :)` returns a Matrix(3, 4) because the row axis collapses, while the page axis is fully retained.

4. **Page assignment** — `U(:, :, 2) = [1, 2; 3, 4]` writes a whole 2×2 page. Element assignment works the same as for matrices.

5. **No broadcasting between Matrix and Tensor3.** Adding or multiplying a Matrix into a Tensor3 raises an error. Likewise, `*` and `/` between two Tensor3s are not defined — use `.*` and `./` for element-wise. Scalar broadcasting (`T * 2`, `T + 10`, `T .^ 2`) is fine.

6. **`permute(A, [d1, d2, d3])`** rearranges the axes; the order must be a permutation of `[1, 2, 3]`. `permute(T, [2, 1, 3])` swaps the row and column axes.

7. **`squeeze(A)`** drops any singleton dimensions and returns a result of the appropriate rank: a singleton-page tensor becomes a Matrix; a tensor with two singletons becomes a Vector; an all-singleton tensor becomes a Scalar. Non-Tensor3 inputs pass through unchanged.

8. **`cat(3, A, B, ...)`** stacks matrices (or tensors) along the page axis. The first argument selects the dimension: `cat(1, ...)` is vertical concatenation (rows), `cat(2, ...)` is horizontal (columns), and `cat(3, ...)` is the page-axis form that produces Tensor3.

9. **NPY I/O preserves the rank-3 shape.** `save("...npy", T)` and `load(...)` round-trip the tensor without reshaping; this is the main reason to use `.npy` over `.csv` or `.toml` for multi-dimensional arrays.

**Run it:**

```sh
rustlab run examples/tensor3/tensor3.rlab
```

---

## `examples/contour.rlab`

**Full script:**

```
[X, Y] = meshgrid(linspace(-2, 2, 41), linspace(-2, 2, 41));
Z = X .^ 2 + Y .^ 2;

# 1. Default line contours (10 auto-spaced round-number levels)
figure();
contour(X, Y, Z);
savefig("/tmp/rustlab_contour_lines.svg");
savefig("/tmp/rustlab_contour_lines.html");

# 2. Explicit levels + line colour
figure();
contour(X, Y, Z, [0.5, 1, 2, 4], "k");
savefig("/tmp/rustlab_contour_explicit.svg");

# 3. Filled contours
figure();
contourf(X, Y, Z, 12);
savefig("/tmp/rustlab_contour_fill.html");

# 4. Overlay heatmap + contours under hold on (canonical EM diagram)
figure();
hold on;
imagesc(Z);
contour(X, Y, Z, 8, "k");
hold off;
savefig("/tmp/rustlab_contour_overlay.html");
```

**Step-by-step explanation:**

1. **`meshgrid(linspace(-2, 2, 41), linspace(-2, 2, 41))`** builds 41×41 coordinate matrices spanning `[-2, 2] × [-2, 2]`. Row index `i` corresponds to `y = Y(i, 1)`, column index `j` to `x = X(1, j)` — same convention as `gradient` and `imagesc`.

2. **`Z = X .^ 2 + Y .^ 2`** is a radial paraboloid. Its level sets `{(x, y) : Z(x, y) = c}` are concentric circles of radius `sqrt(c)`, so the contour plot should show clean nested rings — easy to eyeball for correctness.

3. **`contour(X, Y, Z)`** draws line contours at 10 auto-spaced round-number levels (here roughly `0.5, 1.0, 1.5, …`). Default colour is black. Algorithm: marching squares per level, with NaN cells skipped and saddle points resolved by the cell-centre value.

4. **`contour(X, Y, Z, [0.5, 1, 2, 4], "k")`** uses an explicit level vector and a single-letter colour code (`"k"` = black, `"r"` = red, …). Trailing modifiers can appear in any order — `contour(X, Y, Z, "title", 12)` is equivalent to `contour(X, Y, Z, 12, "title")`.

5. **`contourf(X, Y, Z, 12)`** draws filled contours with 12 colour bands. The HTML backend uses Plotly's exact polygon-fill renderer; the SVG backend uses a per-cell discrete-band approximation (each grid cell painted with the colour of its centre-value's band). For publication-quality fills use `.html` output.

6. **`hold on; imagesc(Z); contour(X, Y, Z, 8, "k"); hold off;`** is the canonical EM equipotentials-on-field-magnitude pattern. Under `hold on`, contour calls **append** to the subplot's contour list rather than replacing it; the heatmap state is independent and unaffected. When both are present, the chart bounds come from the contour's `(X, Y)` and the heatmap rectangles auto-rescale to fit, so the overlay aligns visually.

7. **Per-backend output.** Open the saved files:

   - `.html` files render exact Plotly contour traces — interactive, with hover and zoom.
   - `.svg` files contain marching-squares line segments (line contours) or per-cell colour rectangles (filled contours).
   - `.png` files are rasterised versions of the same SVG content.
   - The terminal does **not** render contour overlays — the script issues a one-time warning instead. Use `savefig` to view.

8. **One contour layer per call.** With `hold on`, multiple `contour` / `contourf` calls stack on the same subplot (and on top of any heatmap). With `hold off` (default), each call clears the subplot's contour list before adding the new one.

**Run it:**

```sh
rustlab run examples/contour.rlab
# Then open the generated files in /tmp/, e.g.:
open /tmp/rustlab_contour_overlay.html
```

## `examples/quiver.rlab`

Arrow plots (`quiver`) and streamlines (`streamplot`) for 2-D vector fields, including uniform flow, vortex, saddle, custom seeds, and the canonical overlay patterns (|E|² heatmap with E arrows; equipotentials with field lines).

```rustlab
# Vortex field: (U, V) = (-Y, X)
[X, Y] = meshgrid(linspace(-2, 2, 16), linspace(-2, 2, 16));
U = -Y;  V = X;

# 1. Arrows at every grid point
figure();
quiver(X, Y, U, V, "Vortex field (-y, x)");
savefig("/tmp/rustlab_quiver_vortex.html");

# 2. Streamlines through the same field
figure();
streamplot(X, Y, U, V, "Vortex streamlines");
savefig("/tmp/rustlab_stream_vortex.html");

# 3. Custom seeds — start streamlines at exactly these points
figure();
seeds = [[-1.5, 0.5]; [0.0, 1.8]; [1.5, -1.2]];
streamplot(X, Y, U, V, seeds);
savefig("/tmp/rustlab_stream_seeds.html");

# 4. Canonical EM overlay: |E|² heatmap with E = -grad(V) arrows
[X, Y] = meshgrid(linspace(-2, 2, 25), linspace(-2, 2, 25));
Vpot = X .^ 2 - Y .^ 2;             % saddle-shaped potential
[Ex, Ey] = gradient(Vpot);
Ex = -Ex;  Ey = -Ey;
Emag = Ex .* Ex + Ey .* Ey;

figure();
hold on;
imagesc(Emag);
quiver(X, Y, Ex, Ey, "|E|² with E arrows");
hold off;
savefig("/tmp/rustlab_heatmap_quiver.html");
```

### What's going on

1. **`quiver(X, Y, U, V)`** places one arrow per grid point. Arrows auto-scale so the longest one equals the nearest-neighbour cell distance — dense fields never overlap. An optional scalar (`quiver(X, Y, U, V, 0.5)`) multiplies the auto-scale for finer control.

2. **`quiver(U, V)`** is a shortcut: `X` and `Y` default to `1..ncols`, `1..nrows`. Handy for quick inspections before building a `meshgrid`.

3. **`streamplot(X, Y, U, V)`** integrates streamlines by RK4 forward and backward from each seed, clipping at the domain boundary, terminating on NaN samples, zero-magnitude field, or closed-orbit return (so vortex streamlines draw once rather than looping). A midpoint arrowhead indicates direction.

4. **Seed control.** A trailing scalar sets density (`density = 1.0` → 10×10 seed grid, ≈ 100 seeds). An explicit Nx2 matrix of `(x, y)` points replaces the default grid — useful when highlighting trajectories through known points of interest.

5. **Overlays under `hold on`.** `quiver` and `streamplot` append to the subplot rather than replacing it, and they stack with `imagesc` heatmaps and `contour` layers. Chart bounds come from the first vector-field or contour layer's `(X, Y)`; the heatmap cells rescale to fit. This is the canonical pattern for EM diagrams: a magnitude heatmap with the field arrows or equipotentials overlaid.

6. **NaN masking.** NaN entries in `U` or `V` skip that cell in `quiver` and terminate a streamline locally, so you can mask out regions of a field without corrupting neighbouring arrows or traces.

7. **Backends.** `.html` emits interactive Plotly line traces with null-separated arrow / streamline polylines; `.svg` uses plotters line + polygon strokes per cell. Terminal does **not** render either (one-time warning) — use `savefig` to view.

### Running

```sh
rustlab run examples/quiver.rlab
# Then open the generated files in /tmp/, e.g.:
open /tmp/rustlab_heatmap_quiver.html
open /tmp/rustlab_contour_stream.html
```

## `examples/laplacian.rlab`

The canonical Poisson demo: build the 5-point sparse Laplacian on a uniform grid, solve `∇²V = ρ` with a known analytic source, verify the residual against the exact solution, and visualise the result.

```rustlab
# Grid and stencil
nx = 33; ny = 25;
dx = 0.1;  dy = 0.1;
L = laplacian_2d(nx, ny, dx, dy);

# Analytic solution: V(x, y) = sin(pi*x/Lx) * sin(pi*y/Ly) — zero on the
# boundary, eigenfunction of the continuous Laplacian with eigenvalue
# -pi^2*(1/Lx^2 + 1/Ly^2).
Lx = (nx + 1) * dx;
Ly = (ny + 1) * dy;
V_exact = zeros(ny, nx);
for jj = 1:nx
  for ii = 1:ny
    V_exact(ii, jj) = sin(pi*ii*dx/Lx) * sin(pi*jj*dy/Ly);
  end
end
v_exact = V_exact(:)';   % column vector, column-major

# Build the RHS by applying L to V_exact, then solve — the solution should
# match within numerical precision.
rhs = full(L) * v_exact;
v_solved = spsolve(L, rhs);
V_solved = reshape(v_solved, ny, nx);

# Plot the solution
imagesc(V_solved);
savefig("/tmp/rustlab_laplacian_solution.html");

# Residual check
err = norm(v_solved' - v_exact) / norm(v_exact);
print(err)       # → very small (< 1e-10)
```

### What's going on

1. **`laplacian_2d(nx, ny, dx, dy)`** returns an `(nx·ny) × (nx·ny)` sparse matrix. Sign convention: `L` approximates `+∇²`, so Poisson `∇²V = -ρ/ε₀` solves as `V = spsolve(L, -rho/eps0)`.

2. **Column-major ordering** is the single most important detail. `V(i, j) → k = (j-1)*ny + i` matches rustlab's `reshape(V_flat, ny, nx)` and `V_grid(:)` convention (modulo the `'` to make it a column vector for `spsolve`). Use `ij2k(i, j, ny)` and `k2ij(k, ny)` when you need the index sugar — note the third argument is **`ny`**, not `nx`.

3. **Boundary conditions.** Homogeneous-Dirichlet: the stencil skips cross-boundary off-diagonal entries and leaves the diagonal at `-2/dx² - 2/dy²`, which is equivalent to assuming `V = 0` outside the grid. For non-zero Dirichlet values, encode them in the right-hand side.

4. **Self-consistency check.** Computing `rhs = full(L) * v_exact` and solving gives back the original vector to numerical precision — the residual norm is < 1e-10 for this analytic eigenfunction.

### Running

```sh
rustlab run examples/laplacian.rlab
open /tmp/rustlab_laplacian_solution.html
```

---

## `examples/masks.rlab`

Shape rasterization on a meshgrid: `rect_mask`, `disk_mask`, `polygon_mask`, plus boolean composition for building material-map-style geometry.

```rustlab
N = 200;
[X, Y] = meshgrid(linspace(-1.5, 1.5, N), linspace(-1.5, 1.5, N));

# Three primitives
R = rect_mask(X, Y, -0.8, -0.4, 1.2, 0.8);          # axis-aligned rectangle
D = disk_mask(X, Y, 0.0, 0.0, 0.9);                  # closed disk, radius 0.9
T = polygon_mask(X, Y, [-1.0,-1.0; 1.0,-1.0; 0.0,1.0]);   # right triangle

# Boolean composition on 0/1 masks
both   = R .* D;                 # intersection
r_only = R .* (1 - D);           # set difference R \ D
d_only = D .* (1 - R);           # set difference D \ R
union  = R + D - R .* D;         # inclusion-exclusion (avoids double-counting)

# A four-region "device" geometry built by stacking masks with material IDs.
# Each layer overwrites earlier layers via the (1 - mask) gating idiom.
substrate = rect_mask(X, Y, -1.2, -0.6, 2.4, 1.2);
via       = disk_mask(X, Y, -0.4, 0.0, 0.25);
contact   = polygon_mask(X, Y, [0.2,-0.3; 0.9,-0.3; 0.9,0.3; 0.2,0.3]);
trace     = rect_mask(X, Y, 0.2, -0.05, 0.7, 0.1);

device =        1 * substrate;
device = device .* (1 - via)     + 2 * via;
device = device .* (1 - contact) + 3 * contact;
device = device .* (1 - trace)   + 4 * trace;

imagesc(device);
savefig("/tmp/rustlab_mask_device.html");

# Numerical sanity: a unit disk integrates to π·r² = π.
unit_disk = disk_mask(X, Y, 0.0, 0.0, 1.0);
step = 3.0 / (N - 1);
print(sum(sum(unit_disk)) * step * step)    # → 3.135 (0.2% off π)
```

### What's going on

1. **All three masks return real-valued matrices the same shape as `X` and `Y`** — `1.0` inside the shape, `0.0` outside. Output is binary, not anti-aliased; users compose area-weighting on top if they need it.

2. **Boolean operators on 0/1 masks are just element-wise math.** Intersection is `M1 .* M2`; complement is `1 - M`; union via inclusion-exclusion is `M1 + M2 - M1 .* M2` (matrix `max` is for scalar arguments only). The set-difference and material-stacking idioms `M1 .* (1 - M2)` and `device .* (1 - layer) + id * layer` are the standard way to build layered material maps.

3. **`polygon_mask` uses even-odd ray casting (PNPOLY).** `verts` is `N × 2` with each row `[x, y]`. The polygon is implicitly closed — an edge runs from vertex `N-1` back to vertex `0`. Degenerate inputs (fewer than 3 vertices, or all vertices collinear) return an all-zero mask without panicking. Behaviour at points exactly on a polygon edge is implementation-defined.

4. **`rect_mask` is inclusive on all four sides** — `M(i,j) = 1.0` iff `x0 ≤ X(i,j) ≤ x0+w` and `y0 ≤ Y(i,j) ≤ y0+h`. A zero-extent rectangle (`w = 0` or `h = 0`) matches only the line / point on its boundary.

5. **`disk_mask` is closed** — boundary cells are included. `r = 0` matches the cells closest to the centre. The π integration overshoots slightly because the closed inequality picks up boundary cells that a half-open mask would omit.

### Running

```sh
rustlab run examples/masks.rlab
open /tmp/rustlab_mask_device.html
```
