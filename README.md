# rustlab

A matrix algebra and DSP toolkit written in Rust — scriptable from the command line or embedded in your own applications.

**[Browse the rendered notebook gallery →](gallery/)** — twenty worked
notebooks (DSP filter design, spectral estimation, vector calculus,
contour and surface plots, sparse direct solves with Cholesky and LU,
multi-source electrostatics, dielectric Poisson, and more), all rendered
with plots inline. GitHub displays them directly — no install needed
to read.

---

## Install (macOS and Linux)

### Option 1 — into `~/.local/bin` (recommended)

```sh
make install
```

Builds release binaries and installs three tools to `~/.local/bin`:

- **`rustlab`** — REPL, script runner, and CLI toolkit
- **`rustlab-notebook`** — render Markdown notebooks to HTML, LaTeX, or PDF
- **`rustlab-viewer`** — standalone interactive plot viewer (egui)

No `sudo` required. On macOS, `codesign` is run automatically.

To install to a different location, override `INSTALL_DIR`:

```sh
make install INSTALL_DIR=/usr/local/bin
```

If `~/.local/bin` is not already on your `PATH`, add it once:

```sh
# bash (~/.bash_profile / ~/.bashrc) or zsh (~/.zshrc)
export PATH="$HOME/.local/bin:$PATH"
```

Then reload your shell or run `source ~/.zshrc`.

### Option 2 — into Cargo's bin directory

```sh
cargo install --path crates/rustlab-cli
```

The binary lands in `~/.cargo/bin/rustlab`, which is already on `PATH` if you installed Rust via `rustup`.

---

> **No system libraries required on any platform.** All plotting dependencies are
> pure Rust — no `libfreetype`, `libfontconfig`, or other C libraries needed.

After either option, verify with:

```sh
rustlab --version
```

---

## Quick Start

### Run a script

```sh
rustlab run examples/lowpass.r
```

### Interactive REPL

Run `rustlab` with no arguments to enter the interactive REPL. Readline history and editing are supported.

```sh
rustlab
```

```
rustlab 0.1.5 — type 'exit' or press Ctrl+D to quit
Tip: end a line with ; to suppress output

>> a = 1:5
a = [1.000000, 2.000000, 3.000000, 4.000000, 5.000000]
>> b = a .* a;
>> print(b)
[1.000000, 4.000000, 9.000000, 16.000000, 25.000000]
>> exit
bye
```

### Design an FIR filter

```sh
rustlab filter fir --type low --cutoff 1000 --sr 44100 --taps 64 --window hann
```

### Generate and plot a window function

```sh
rustlab window --type kaiser --length 64 --beta 8.6 --plot
```

---

## Scripting Language Reference

rustlab scripts use the `.r` extension. The interpreter is line-oriented: each statement is one line.

### Variables and Assignment

```
x = 3.14
name = "hello"
```

Variable names are lowercase identifiers. Values are dynamically typed (scalar, vector, matrix, or string).

### Suppress Output

End any statement with `;` to suppress its output:

```
h = fir_lowpass(64, 1000.0, 44100.0, "hann");   # silent — no output
plot(h)                                            # no semicolon — runs immediately
```

This is especially useful in scripts when you don't want intermediate results printed.

### Complex Numbers

`j` is predefined as the imaginary unit `Complex(0.0, 1.0)`. Compose complex literals with ordinary arithmetic:

```
a = 1.0 + j*2.0        # 1 + 2j
b = 3.0 * j            # 0 + 3j
c = a * b              # (-6) + 3j
```

Real and imaginary parts are `f64`. Magnitude is `abs(z)`, phase is `angle(z)`.

### Vectors and Matrices

**Literal syntax** — comma-separated elements in square brackets:

```
v = [1.0, 2.0, 3.0]
M = [1, 2; 3, 4]          # 2×2 matrix — semicolon separates rows
```

**Range / colon operator** — `start:stop` and `start:step:stop`:

```
v = 1:5                    # [1, 2, 3, 4, 5]
w = 0:0.5:2                # [0, 0.5, 1.0, 1.5, 2.0]  (start:step:stop)
r = 10:-1:1                # [10, 9, 8, ..., 1]
t = 0:1/44100:1            # time axis at 44.1 kHz
```

**1-based indexing**:

```
v = 1:10
x = v(3)                   # 3  (1-based)
s = v(2:5)                 # [2, 3, 4, 5]
last = v(end)              # 10
tail = v(end-2:end)        # [8, 9, 10]
```

**Concatenation** — place vectors inside `[...]` to join them:

```
a = 1:3                    # [1, 2, 3]
b = 4:6                    # [4, 5, 6]
c = [a, b]                 # [1, 2, 3, 4, 5, 6]
```

**Transpose** with `'`:

```
v = [1, 2, 3]
t = v'                     # conjugate transpose (for real data, same values)
```

### Arithmetic Operators

| Operator | Description |
|----------|-------------|
| `+`      | Addition (scalar, vector, matrix) |
| `-`      | Subtraction |
| `*`      | Multiplication / scalar broadcast |
| `/`      | Division / scalar broadcast |
| `^`      | Power |
| `.*`     | Element-wise multiply |
| `./`     | Element-wise divide |
| `.^`     | Element-wise power |
| `'`      | Conjugate transpose |

All operators broadcast scalars onto vectors and matrices automatically.
`v .^ 2` squares every element; `2 .^ v` raises 2 to each element of v.

### Non-conjugate Transpose

Use `.'` for non-conjugate transpose (swaps rows and columns without conjugating):

```
A = [1 + j, 2; 3, 4 - j]
B = A.'     # transpose without conjugation
C = A'      # conjugate transpose (Hermitian)
```

### Destructuring Assignment

Functions that return multiple values can be unpacked:

```
[U, S, V] = svd(A)
[y, t] = step(G)
```

### Chained Call-and-Index

Index the return value of a function call directly:

```
v = linspace(0, 1, 10)(3)   # third element
```

### Control Flow

```
# For loop
for i = 1:10
  v(i) = i ^ 2
end

# While loop  (true / false are built-in boolean constants)
while x > 0
  x = x - 1
end

# If / elseif / else
if x > 0
  print("positive")
elseif x < 0
  print("negative")
else
  print("zero")
end

# User-defined function
function [y] = double(x)
  y = x * 2
end

# Early return
function [y] = safe_div(a, b)
  if b == 0
    y = 0
    return
  end
  y = a / b
end
```

### Anonymous Functions and Handles

```
# Lambda (anonymous function)
sq = @(x) x .^ 2
sq([1, 2, 3])              # [1, 4, 9]

# Function handle — reference to a builtin or user function
f = @sin
f(pi / 2)                  # 1.0

# Apply a function to each element of a vector
arrayfun(@(x) x^2 + 1, 1:5)
```

### Structs

```
s = struct("x", 1, "y", 2)
s.x                        # 1
s.z = 3                    # add field
fieldnames(s)              # prints: x, y, z
```

### String Arrays

Brace syntax creates ordered collections of strings:

```
labels = {"Jan", "Feb", "Mar"}
labels(2)                  # "Feb"
iscell(labels)             # true

bar(labels, [10, 20, 30])  # categorical bar chart
```

> **Full language reference:** See [`docs/quickref.md`](docs/quickref.md) for a concise cheat sheet of all syntax and functions.

### Builtin Functions (highlights)

rustlab ships with 180+ builtins. Here are the most commonly used; see [`docs/quickref.md`](docs/quickref.md) for the complete list and [`docs/functions.md`](docs/functions.md) for full signatures and examples.

| Category | Key functions |
|----------|--------------|
| **Math** | `abs`, `angle`, `real`, `imag`, `conj`, `sin`, `cos`, `exp`, `log`, `sqrt`, `atan2`, `floor`, `round`, `mod` |
| **Array** | `zeros`, `ones`, `eye`, `linspace`, `logspace`, `rand`, `randn`, `randi`, `len`, `size`, `reshape`, `diag`, `meshgrid` |
| **Statistics** | `sum`, `prod`, `cumsum`, `mean`, `median`, `std`, `min`, `max`, `sort`, `trapz`, `histogram`, `all`, `any` |
| **Linear algebra** | `dot`, `cross`, `outer`, `kron`, `norm`, `inv`, `det`, `trace`, `eig`, `svd`, `linsolve`, `expm`, `rank`, `roots` |
| **DSP — FIR** | `fir_lowpass`, `fir_highpass`, `fir_bandpass`, `fir_lowpass_kaiser`, `firpm`, `firpmq`, `fir_notch`, `window` |
| **DSP — IIR** | `butterworth_lowpass`, `butterworth_highpass`, `filtfilt` |
| **DSP — FFT** | `fft`, `ifft`, `fftshift`, `fftfreq`, `spectrum`, `convolve`, `upfirdn`, `freqz` |
| **Fixed-point** | `qfmt`, `quantize`, `qadd`, `qmul`, `qconv`, `snr` |
| **Control systems** | `tf`, `ss`, `pole`, `zero`, `bode`, `step`, `margin`, `rlocus`, `lqr`, `ctrb`, `obsv`, `care`, `dare`, `place` |
| **ML / activation** | `softmax`, `relu`, `gelu`, `layernorm` |
| **Plotting** | `plot`, `stem`, `bar`, `scatter`, `hline`/`yline`, `plotdb`, `imagesc`, `subplot`, `hold`, `figure`, `legend`, `savefig` |
| **Live plotting** | `figure_live`, `plot_update`, `figure_draw`, `figure_close`, `mag2db` |
| **I/O** | `print`, `disp`, `fprintf`, `save`, `load`, `whos` |
| **Streaming** | `state_init`, `filter_stream`, `audio_in`, `audio_out`, `audio_read`, `audio_write` |
| **Structs** | `struct`, `isstruct`, `fieldnames`, `isfield`, `rmfield` |
| **String arrays** | `{"a","b","c"}`, `iscell`, 1-based indexing, categorical `bar()` labels |
| **Sparse** | `sparse`, `sparsevec`, `speye`, `spzeros`, `spdiags`, `sprand`, `spsolve`, `laplacian_1d`, `laplacian_2d`, `laplacian_3d`, `laplacian_eps_2d`, `ij2k`, `k2ij`, `ijk2k`, `k2ijk`, `full`, `nnz`, `issparse`, `find`, `nonzeros` |
| **Geometry** | `rect_mask`, `disk_mask`, `polygon_mask` (rasterization on a meshgrid) |
| **Higher-order** | `arrayfun`, `feval`, `@(x) expr` (lambdas), `@name` (function handles) |
| **Special** | `laguerre`, `legendre`, `factor`, `rk4`, `lyap`, `gram`, `freqresp` |
| **Profiling** | `profile`, `profile_report` |

Window type strings: `"hann"`, `"hamming"`, `"blackman"`, `"rectangular"`, `"kaiser"`.

#### Kaiser filter design parameters

`fir_lowpass_kaiser`, `fir_highpass_kaiser`, and `fir_bandpass_kaiser` accept:

| Parameter | Description |
|-----------|-------------|
| `cutoff_hz` | Passband edge frequency in Hz |
| `tbw_hz` | One-sided transition bandwidth in Hz (controls tap count) |
| `attn_db` | Desired stopband attenuation in dB (controls window beta) |
| `sr` | Sample rate in Hz |

The required beta and number of taps are computed automatically using the Harris (1978) formulas. You no longer need to specify tap count manually.

#### Frequency response with `freqz`

`freqz` returns a 2×n_points matrix. Row 1 is the frequency axis in Hz, row 2 is the complex frequency response H(f):

```
h  = fir_lowpass_kaiser(1000.0, 200.0, 60.0, 8000.0)
H  = freqz(h, 512, 8000.0)
f  = H(1)      # frequency axis (row 1)
Hf = H(2)      # complex response (row 2)
plot(abs(Hf), "Magnitude response")
```

#### Figure-based plotting (multi-series, subplots, file output)

Use `figure()` + `hold()` to compose multi-series plots and save them:

```
t  = linspace(0, 1, 200);
y1 = sin(2 * 3.14159 * 3 * t);
y2 = cos(2 * 3.14159 * 3 * t);

figure()
hold("on")
plot(y1, "color", "blue", "label", "sin")
plot(y2, "color", "red",  "label", "cos")
title("Sine and cosine")
xlabel("Sample")
ylabel("Amplitude")
legend()
savefig("output.svg")
```

Use `subplot(rows, cols, idx)` for multi-panel figures:

```
figure()
subplot(2, 1, 1)
title("Signal")
plot(y1)
subplot(2, 1, 2)
title("Spectrum")
plot(abs(fft(y1)))
savefig("two_panel.svg")
```

Colormaps for `imagesc`: `"viridis"` (default), `"jet"`, `"hot"`, `"gray"`.

```
M = rand(32, 32)
imagesc(M, "jet")
savefig("heatmap.svg")
```

---

## CLI Reference

### `rustlab run <script>`

Execute a `.r` script file.

```sh
rustlab run examples/bandpass.r
```

### `rustlab filter fir [OPTIONS]`

Design an FIR filter and print coefficients to stdout (one per line).

| Option | Default | Description |
|--------|---------|-------------|
| `--taps N` | 32 | Number of filter taps |
| `--cutoff HZ` | required | Cutoff frequency (low cutoff for bandpass) |
| `--cutoff-high HZ` | — | High cutoff frequency (bandpass only) |
| `--sr HZ` | 44100 | Sample rate in Hz |
| `--type TYPE` | `low` | Filter type: `low`, `high`, `band` |
| `--window WIN` | `hann` | Window function: `hann`, `hamming`, `blackman`, `rectangular`, `kaiser` |
| `--beta B` | — | Kaiser beta parameter |

Examples:

```sh
# 64-tap lowpass at 2 kHz
rustlab filter fir --taps 64 --cutoff 2000 --sr 44100

# Bandpass 500–2000 Hz with Hamming window
rustlab filter fir --type band --cutoff 500 --cutoff-high 2000 --sr 44100 --window hamming

# Highpass with Kaiser window
rustlab filter fir --type high --cutoff 4000 --window kaiser --beta 8.6
```

Output format (one coefficient per line):
```
h[  0] = +0.00012345 + +0.00000000j
h[  1] = +0.00234567 + +0.00000000j
...
```

### `rustlab filter iir [OPTIONS]`

Design an IIR Butterworth filter and print `b` (numerator) and `a` (denominator) coefficients.

| Option | Default | Description |
|--------|---------|-------------|
| `--order N` | 2 | Filter order |
| `--cutoff HZ` | required | Cutoff frequency in Hz |
| `--sr HZ` | 44100 | Sample rate in Hz |
| `--type TYPE` | `low` | Filter type: `low`, `high` |

```sh
rustlab filter iir --order 4 --cutoff 1000 --sr 44100
rustlab filter iir --type high --order 2 --cutoff 8000 --sr 44100
```

### `rustlab convolve [OPTIONS]`

Convolve two signals from CSV files. Prints the result to stdout (one `re,im` per line).

| Option | Default | Description |
|--------|---------|-------------|
| `--signal FILE` | required | Input signal CSV |
| `--kernel FILE` | required | Kernel CSV |
| `--method METHOD` | `direct` | `direct` or `overlap-add` |

Each CSV line is either `re` (real-only) or `re,im`.

```sh
rustlab convolve --signal input.csv --kernel filter.csv
rustlab convolve --signal input.csv --kernel filter.csv --method overlap-add
```

### `rustlab window [OPTIONS]`

Generate a window function and print values to stdout.

| Option | Default | Description |
|--------|---------|-------------|
| `--type TYPE` | required | Window type: `hann`, `hamming`, `blackman`, `rectangular`, `kaiser` |
| `--length N` | required | Number of samples |
| `--beta B` | — | Kaiser beta parameter |
| `--plot` | false | Show an ASCII stem plot |

```sh
rustlab window --type hann --length 32
rustlab window --type kaiser --length 64 --beta 8.6 --plot
```

### `rustlab plot [OPTIONS]`

Plot a CSV signal file (ASCII art rendered to the terminal).

| Option | Default | Description |
|--------|---------|-------------|
| `--input FILE` | required | CSV file (one `re` or `re,im` per line) |
| `--title TITLE` | `Signal` | Plot title |
| `--type TYPE` | `line` | `line` or `stem` |

```sh
rustlab plot --input output.csv --title "Filtered signal"
rustlab plot --input coeffs.csv --type stem --title "Filter coefficients"
```

### `rustlab info`

Print version and usage hints.

```sh
rustlab info
```

---

## Notebooks

`rustlab-notebook` renders Markdown files with `` ```rustlab `` code blocks
into HTML, LaTeX, PDF, or GitHub-friendly Markdown.

```sh
rustlab-notebook render analysis.md                  # → analysis.html
rustlab-notebook render analysis.md -f pdf           # → analysis.pdf (self-contained)
rustlab-notebook render analysis.md -f markdown      # → analysis.md + plots/analysis/*.svg
rustlab-notebook render analysis.md -f latex         # → analysis.tex + plots/analysis/*.svg
rustlab-notebook render notebooks/ -o build/         # render all → *.html + index.html
```

**Output formats** (all four are first-class):

| Format     | Output                                       |
|------------|----------------------------------------------|
| `html`     | `<name>.html` — self-contained, Plotly + KaTeX |
| `pdf`      | `<name>.pdf`  — self-contained, intermediates compile in tempdir |
| `markdown` | `<name>.md`   + `plots/<name>/*.svg`         |
| `latex`    | `<name>.tex`  + `plots/<name>/*.svg`         |

Markdown and LaTeX share the same on-disk shape: one document file plus a
`plots/<stem>/` subdirectory under the output folder. HTML and PDF embed
plots inline.

**Features:**
- Interactive Plotly charts (HTML) or static SVG (Markdown / LaTeX / PDF)
- KaTeX math rendering ($...$, $$...$$)
- Syntax-highlighted code blocks
- Template interpolation: `${expr}` embeds computed values in prose
- `<!-- hide -->` directive to suppress setup code
- Multi-notebook directory rendering with index generation (customise
  via `--title`, `index.md`, or frontmatter `order:`)
- YAML frontmatter (`title:`, `order:` / `weight:`) for index titling
  and sort order
- Cross-notebook `.md` → `.html` link rewriting
- Variables persist across code blocks within a notebook

This repo's notebooks live at `examples/notebooks/*.md`. `make notebooks`
writes everything into [`gallery/`](gallery/):

- `gallery/<name>.md` + `gallery/plots/<name>/*.svg` — **committed**, the
  entry point for browsing rendered notebooks on GitHub
- `gallery/<name>.html` + `gallery/index.html` — **gitignored**, for
  local interactive viewing with Plotly + KaTeX

See [`docs/notebooks.md`](docs/notebooks.md) for full documentation and
[`examples/notebooks/README.md`](examples/notebooks/README.md) for the
directory layout.

---

## Examples

The `examples/` directory contains annotated scripts demonstrating common workflows. See [`docs/examples.md`](docs/examples.md) for step-by-step walkthroughs.

### Core language and math

| File | Description |
|------|-------------|
| `examples/complex_basics.r` | Complex number arithmetic, magnitude, phase, complex vectors |
| `examples/vectors.r` | Range operator, indexing, concatenation, element-wise ops, transpose |
| `examples/trig_special.r` | Trig, hyperbolic, Laguerre, and Legendre functions |
| `examples/stats.r` | Statistics: mean, median, std, histogram, trapz |
| `examples/matrix_ops.r` | Linear algebra: inv, det, eig, svd, linsolve, kron, expm |
| `examples/random.r` | Random number generation: rand, randn, randi |
| `examples/functions.r` | User-defined functions with multiple return values |
| `examples/lambda.r` | Anonymous functions, function handles, arrayfun |
| `examples/lambda_pipeline.r` | Functional pipeline patterns with lambdas |
| `examples/save_load.r` | NPY, NPZ, and CSV round-trip save/load |
| `examples/profiling.r` | Call profiling with profile() and profile_report() |

### DSP and filter design

| File | Description |
|------|-------------|
| `examples/lowpass.r` | Design and inspect a 32-tap Hann-windowed FIR lowpass filter |
| `examples/bandpass.r` | Bandpass filter design and application to a synthetic dual-tone signal |
| `examples/fft.r` | Compute and plot the spectrum of a two-tone signal; round-trip FFT/IFFT |
| `examples/kaiser_fir.r` | Auto-designed Kaiser FIR lowpass, highpass, bandpass, and notch filters |
| `examples/firpm.r` | Parks-McClellan optimal equiripple FIR design |
| `examples/upfirdn.r` | Polyphase interpolation, decimation, and rational rate conversion |
| `examples/fixed_point.r` | Fixed-point quantization and SNR bitwidth study |
| `examples/ml_activations.r` | ML activation functions: softmax, relu, gelu, layernorm |

### Control systems (`examples/controls/`)

| File | Description |
|------|-------------|
| `examples/controls/transfer_function.r` | Transfer function creation and arithmetic |
| `examples/controls/pole_zero.r` | Pole-zero analysis |
| `examples/controls/step_response.r` | Unit step response |
| `examples/controls/bode_plot.r` | Bode magnitude and phase |
| `examples/controls/stability_margins.r` | Gain and phase margins |
| `examples/controls/state_space.r` | State-space conversion |
| `examples/controls/controllability.r` | Controllability and observability |
| `examples/controls/lqr_design.r` | LQR optimal control |
| `examples/controls/root_locus.r` | Root locus plotting |
| `examples/controls/linear_algebra.r` | Lyapunov, Gramians, SVD |
| `examples/controls/ode.r` | ODE integration with rk4 |
| `examples/controls/design.r` | CARE, DARE, pole placement |

Run any example with:

```sh
rustlab run examples/<name>.r
```

### Real-time audio examples (`examples/audio/`)

Rustlab can process raw PCM audio streams via stdin/stdout with no audio library dependencies. External bridge programs handle hardware I/O; rustlab is a pure filter node in the pipeline.

| File | Description |
|------|-------------|
| `examples/audio/filter.r` | Core FIR lowpass script — reads stdin, writes stdout, handles EOF cleanly |
| `examples/audio/passthrough.r` | Minimal stdin → stdout loopback (pipeline test) |
| `examples/audio/spectrum_monitor.r` | Live two-panel terminal plot: waveform + FFT spectrum in dB |
| `examples/audio/spectrum_monitor.sh` | Platform-aware launcher for spectrum monitor (macOS/Linux/synthetic) |
| `examples/audio/macos.sh` | Live microphone → lowpass filter → speakers (requires `sox`) |
| `examples/audio/linux.sh` | Same via ALSA `arecord`/`aplay` |
| `examples/audio/wsl.sh` | WSL2 via PulseAudio or `sox` |
| `examples/audio/tcp.sh` | Network DSP node using `socat`/`nc` — pipe audio over TCP |
| `examples/audio/test_filter.sh` | Hardware-free CI test — 440 Hz + 8 kHz synthetic signal, verifies ≥ 20 dB attenuation |

**Quickstart (macOS):**
```sh
sox -d -r 44100 -c 1 -b 32 -e float -t raw - \
  | rustlab run examples/audio/filter.r \
  | sox -r 44100 -c 1 -b 32 -e float -t raw - -d
```

**Hardware-free test:**
```sh
bash examples/audio/test_filter.sh
```

### Notebooks

Sources are at [`examples/notebooks/`](examples/notebooks/), rendered
output at [`gallery/`](gallery/) — see the gallery README for the full
list with descriptions, or jump straight to a notebook:

| Source | Rendered | Description |
|--------|----------|-------------|
| [quick_look.md](examples/notebooks/quick_look.md) | [view](gallery/quick_look.md) | Minimal one-block notebook (random signal + plot) |
| [filter_analysis.md](examples/notebooks/filter_analysis.md) | [view](gallery/filter_analysis.md) | FIR filter design with frequency response plots |
| [firpm_quantization.md](examples/notebooks/firpm_quantization.md) | [view](gallery/firpm_quantization.md) | `firpm` vs `firpmq` 10-bit quantized FIR design |
| [spectral_estimation.md](examples/notebooks/spectral_estimation.md) | [view](gallery/spectral_estimation.md) | Periodogram vs. windowed PSD, tables, display math |
| [contour_plots.md](examples/notebooks/contour_plots.md) | [view](gallery/contour_plots.md) | `contour` and `contourf` for 2-D scalar fields |
| [surface_plots.md](examples/notebooks/surface_plots.md) | [view](gallery/surface_plots.md) | 3-D `surf` with `meshgrid` and colormaps |
| [vector_fields.md](examples/notebooks/vector_fields.md) | [view](gallery/vector_fields.md) | Quiver and streamplot for 2-D vector fields |
| [vector_calculus.md](examples/notebooks/vector_calculus.md) | [view](gallery/vector_calculus.md) | Gradient / divergence / curl on uniform 2-D and 3-D grids |
| [laplacian.md](examples/notebooks/laplacian.md) | [view](gallery/laplacian.md) | Sparse 5-point Laplacian + Poisson solve |
| [string_arrays.md](examples/notebooks/string_arrays.md) | [view](gallery/string_arrays.md) | String arrays, categorical bar charts, `iscell()` |
| [template_interpolation.md](examples/notebooks/template_interpolation.md) | [view](gallery/template_interpolation.md) | `${expr}` interpolation with format specs |
| [multi_notebook.md](examples/notebooks/multi_notebook.md) | [view](gallery/multi_notebook.md) | Directory rendering and cross-notebook links |

Render everything locally:

```sh
make notebooks                                          # → gallery/ (md committed, html ignored)
open gallery/index.html                                 # interactive view
```

See `docs/examples.md` for annotated walkthroughs.
