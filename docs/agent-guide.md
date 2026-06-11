# rustlab — AI Agent Usage Guide

This guide teaches an AI agent (or any new power user) how to **use** rustlab effectively: run code, discover capabilities, write correct `.rlab` scripts, produce plots and files, and author notebooks. It is the recommended starting point when you are pointed at this project and asked to *do something with rustlab*.

> Working **on** the rustlab codebase itself (Rust crates, builtins, tests)? Read [`AGENTS.md`](../AGENTS.md) instead. This guide is about using the tool, not modifying it.

**Reading order for an agent:**

1. This file — execution model, semantics, pitfalls, workflows.
2. [`docs/quickref.md`](quickref.md) — the canonical capability index. If a function is not listed there, it is not implemented.
3. [`docs/functions.md`](functions.md) — full signatures and per-function examples (consult on demand).
4. [`docs/notebooks.md`](notebooks.md) — complete notebook authoring and rendering reference.
5. [`docs/examples.md`](examples.md) + [`examples/`](../examples/) + [`gallery/`](../gallery/) — 81 runnable scripts and 38 rendered notebooks to copy patterns from.

---

## 1. What rustlab is

rustlab is a Rust-native toolkit for matrix algebra and digital signal processing with:

- A scripting language (`.rlab` files) — 1-based indexing, complex-by-default numerics, `:` ranges, element-wise `.`-operators.
- An interactive REPL (`rustlab` with no arguments).
- Direct CLI subcommands (`filter`, `convolve`, `window`, `plot`, `docs`, `cache`, `info`).
- A notebook renderer (`rustlab-notebook`) that executes ` ```rustlab ` fenced blocks inside Markdown and renders HTML / Markdown / LaTeX / PDF / JSON with inline plots.
- A standalone plot viewer (`rustlab-viewer`, egui-based).

Toolboxes: elementary math, linear algebra (dense + sparse), statistics, FFT/spectral analysis (Welch, STFT, CWT), FIR/IIR filter design (windowed-sinc, Kaiser, Parks-McClellan), fixed-point quantization, control systems, RF S-parameters with Smith charts, ML activations, vector calculus / PDE helpers, streaming DSP and audio I/O, terminal + file + live plotting, and animation export.

---

## 2. Executing code — the three entry points

### 2.1 Run a script

```sh
rustlab run script.rlab                # execute a .rlab file
rustlab run script.rlab --plot none    # suppress TUI plots (recommended for agents/CI)
rustlab run script.rlab --profile      # print per-function timing report to stderr
```

Facts an agent must know (all verified against the current binary):

- **Exit code is 0 even when the script hits a runtime error.** The error is printed to **stderr** as `error: line N: runtime error: <message>` and execution halts, but the process still exits 0. (Lex/parse errors *do* exit non-zero.) To detect failure reliably, capture stderr and check for a line starting with `error:` — do not rely on the exit code alone.
- **Plots do not block in non-TTY runs.** The interactive TUI pager is skipped automatically when stdout is not a terminal, and `--plot none` suppresses it explicitly. `savefig()` still writes files in both cases. An agent piping output can safely run scripts containing `plot(...)`.
- **Relative paths in `savefig`, `save`, and `load` resolve against the script's directory**, not the process working directory.
- A bare expression or assignment without `;` echoes its value to stdout; a trailing `;` suppresses the echo.

### 2.2 REPL

`rustlab` with no arguments starts the REPL. Inside it: `help` lists builtins by category, `help <name>` shows detail, `whos` lists variables, `run file.rlab` executes a script into the current scope, `clear` resets. The REPL is for humans; agents should prefer `rustlab run` on a temp file so output is reproducible.

### 2.3 Notebooks

`rustlab-notebook render note.md` executes every ` ```rustlab ` block in a Markdown file and emits HTML (default), `-f markdown` (GitHub-friendly with SVG plots), `-f latex`, `-f pdf`, or `-f json`. See §7.

---

## 3. Discovering capabilities

The authoritative, machine-readable source is the binary itself:

```sh
rustlab docs                  # all ~327 builtins grouped by category
rustlab docs eig              # full detail for one builtin (signature + example)
rustlab docs Plotting         # one category
rustlab docs --search welch   # substring match over names and briefs
rustlab docs --json           # complete JSON index — best single call for an agent
```

`rustlab docs --json` returns an array of `{name, toolbox, subcategory, brief, detail}` objects. The `detail` field contains the signature and a usage example. Parse this once and you know the entire callable surface.

In source form, [`docs/quickref.md`](quickref.md) is the canonical capability index, kept in sync with the registered builtins by project policy. **If a function is not in quickref.md, do not generate code that calls it.**

---

## 4. The language in 20 rules

These rules cover ~95% of what you need to write correct `.rlab` code. Full grammar: `docs/functions.md` § Language.

1. **Everything numeric is `Complex<f64>` underneath.** `j` and `i` are the imaginary unit: `z = 3.0 + j*4.0`. Element-wise ops on purely real inputs return exactly-real results. `real(v)`, `imag(v)`, `conj(v)`, `abs(v)`, `angle(v)` decompose complex values.
2. **Indexing is 1-based, with `end` for the last element:** `v(1)`, `v(end)`, `v(2:4)`, `M(i, j)`, `M(i, :)`, `M(:, j)`. Slices return new values.
3. **Single-index matrix access is column-major linear:** `M(k)` is the k-th element of `M(:)`; it round-trips with `find(M)`. `reshape` also walks column-major.
4. **Ranges:** `1:5`, `0:0.5:2`, `10:-1:1` (`start:stop` or `start:step:stop`).
5. **Literals:** `[1, 2, 3]` row, `[1; 2; 3]` column, `[a, b; c, d]` matrix. `[A, B]` / `[A; B]` concatenate. `{"a", "b"}` is a string array. `1_000_000` underscores are allowed in numbers.
6. **`*` is matrix multiply; `.*`, `./`, `.^` are element-wise.** Mixing them up is the most common generated-code bug.
7. **`'` is conjugate transpose; `.'` is plain transpose.**
8. **Statements ending in `;` are silent; without `;` they print.** Comments are `#` or `%`. `...` continues a statement onto the next line.
9. **Control flow:** `for k = 1:n ... end`, `while cond ... end`, `if / elseif / else / end`, `switch / case / otherwise / end`. **There is no `break` and no `continue`** — design the exit condition into the loop header (`while` with a flag variable if needed).
10. **Functions:** `function [a, b] = name(args) ... end` (multi-output), `return` for early exit. Callers destructure with `[p, q] = name(x)`; a bare `v = name(x)` takes the first output only.
11. **Lambdas and handles:** `f = @(x) x.^2` (captures the environment by snapshot at definition time); `@sin` references a builtin or user function; `feval("name", args)` calls by string name; `arrayfun(f, v)` maps; `parmap(f, xs)` maps in parallel (lambda must be pure — no plotting/IO/seed inside).
12. **Multi-output builtins use the same destructuring:** `[V, D] = eig(A)`, `[m, idx] = max(v)`, `[Pxx, f] = pwelch(x, fs)`, `[I, J, V] = find(M)`. There is **no `~` placeholder** to skip an output — bind it to a dummy variable, or use a single-output alternative (`argmax(v)` instead of the index from `max`).
13. **Indexed assignment grows vectors automatically:** `v(i) = val` extends `v` as needed. Matrix region writes `M(rows, cols) = mat` require exact shape match.
14. **Structs:** `s.field = val` auto-creates the struct; `struct("x", 1, "y", 2)` constructs directly; `fieldnames`, `isfield`, `rmfield` introspect.
15. **Booleans:** `true` / `false` are constants; `&&` / `||` short-circuit with scalar operands (non-zero is truthy). Comparison results behave as 0/1 numerics.
16. **Compound assignment:** `+=`, `-=`, `*=`, `/=`.
17. **Constants always in scope:** `pi`, `e`, `Inf`, `NaN`, `i`, `j`.
18. **`run file.rlab`** (bare statement, no quotes needed) executes another script and merges its variables/functions into the current scope — the script equivalent of an include.
19. **Tensor3** is the rank-3 type: build with `zeros3(m, n, p)` / `reshape(v, m, n, p)` / `cat(3, A, B)`; slice pages with `T(:, :, k)`. No broadcasting between Matrix and Tensor3, and no `*` between two Tensor3s — use `.*`.
20. **Sparse matrices** come from `sparse(I, J, V, m, n)`, `speye`, `spdiags`, `laplacian_1d/2d/3d`; solve with `spsolve(A, b)` or factor once with `F = chol(A)` / `F = lu(A)` then `solve(F, b)` many times. Mixed sparse+dense arithmetic promotes to dense.

A minimal end-to-end script:

```rustlab
sr = 44100.0;
t  = (0:1/sr:0.05);                       # 50 ms time axis
x  = sin(2*pi*440*t) + 0.5*sin(2*pi*1000*t);

h  = fir_lowpass_kaiser(600.0, 100.0, 60.0, sr);   # auto-designed Kaiser lowpass
y  = convolve(x, h);

H  = freqz(h, 512, sr);
plotdb(H, "Kaiser lowpass response"); savefig("response.svg")
fprintf("designed %d taps\n", length(h))
```

---

## 5. Pitfalls that bite generated code

Ordered roughly by how often they matter:

| Pitfall | Correct handling |
|---|---|
| Runtime errors do **not** change the exit code of `rustlab run` | Capture stderr; treat any `error: line N:` line as failure |
| No `break`/`continue` in loops | Restructure with a flag in the `while` condition or bounded `for` |
| No `~` placeholder in destructuring (`[~, i] = ...` is a lex error) | Bind unwanted outputs to a dummy variable, or use `argmin`/`argmax` |
| `*` vs `.*` on same-shaped matrices | `*` is matrix product; use `.*` for element-wise |
| `'` conjugates | On complex data use `.'` if you only want a transpose |
| `fft(v)` zero-pads to the next power of 2 | Output length may exceed `length(v)`; use `fftfreq(n, sr)` or `spectrum(x, sr)` for matching axes |
| `min`/`max`/`sort` on complex data compare by magnitude (real part for `sort`/`median`) | Take `abs()`/`real()` explicitly if you mean it |
| All-NaN input to `min`/`max`/`argmin`/`argmax` is an error, not NaN | Filter NaNs first |
| `length(M)` is `max(size(M))`, not the element count | Use `numel(M)` for total elements |
| `butterworth_lowpass`/`_highpass` return numerator (`b`) coefficients only | Apply with `filtfilt(b, [1], x)`; these are FIR-style approximations, not full IIR `[b, a]` pairs |
| `contour`, `quiver`, `streamplot` are not rendered in the terminal TUI | Always pair with `savefig("file.svg")` (or `.html`) to see them |
| `loglog`/`semilogx`/`semilogy` require strictly positive data on the log axes | Clamp or shift data first |
| `savefig`/`save`/`load` paths are script-relative | Generate output paths relative to the script, or use absolute paths |
| Figure state is global and stateful (`figure`, `subplot`, `hold on`) | Call `figure()` (or `clf`) before building a new multi-panel plot; `frame()`/`saveanim` for animations |
| `parmap` lambdas must be pure | No plotting, file I/O, `seed`, or audio calls inside the lambda body |
| String arrays `{"a","b"}` only hold strings | There is no general cell-array type |
| `mod(v, m)` needs a real scalar `m`; `sign` of complex is `z/|z|` | Stay real where the math expects real |
| Live plotting (`figure_live`) and the TUI pager require a real TTY | In headless contexts use `savefig`/`saveanim` instead |

---

## 6. Producing output

### Plots to files

Call any plotting function, then `savefig(path)` — the extension picks the backend:

```rustlab
plot(x, y, "title");      savefig("out.svg")    # static SVG
imagesc(M, "viridis");    savefig("out.png")    # static PNG
surf(X, Y, Z);            savefig("out.html")   # interactive Plotly
```

Multi-panel: `figure(); subplot(2,1,1); plot(...); subplot(2,1,2); plotdb(...); savefig("both.svg")`. Styling: `title`, `xlabel`, `ylabel`, `xlim`, `ylim`, `legend`, `grid on`, `hold on` (overlay), `axis("equal")`.

### Animations

```rustlab
figure()
for k = 1:60
  imagesc(state_at(k), "viridis"); title(sprintf("frame %d", k))
  frame()                          # snapshot current figure
end
saveanim("wave.gif", 30)           # .gif or .html (Plotly with play/slider)
```

### Data files

`save("x.npy", x)` / `save("d.npz", "a", a, "b", b)` / `save("x.csv", x)` / `save("s.toml", st)` and the matching `load(...)`. `whos("d.npz")` inspects an archive. NPY/NPZ round-trip with NumPy, including complex and rank-3 arrays.

### Formatted text

`fprintf("fmt", args...)` / `sprintf(...)` with `%d %f %g %e %s`, flags `- + 0 # ,`; `print(...)` and `disp(x)` for quick output; `commas(x)` for thousands separators.

---

## 7. Notebooks — executable Markdown

A rustlab notebook is a plain `.md` file. Code in ` ```rustlab ` fences executes top-to-bottom with shared state; everything else is normal Markdown (KaTeX math, Mermaid diagrams, GFM callouts like `> [!NOTE]` all render).

```sh
rustlab-notebook render note.md                 # → note.html (self-contained, Plotly + KaTeX)
rustlab-notebook render note.md -f markdown -o out.md   # GitHub-friendly .md + plots/<stem>/*.svg
rustlab-notebook render dir/                    # render every .md + index.html
rustlab-notebook watch note.md                  # live-reload server at http://127.0.0.1:8042
rustlab-notebook check note.md                  # lint (exit 2 errors / 1 warnings / 0 clean)
rustlab-notebook clean note.md                  # strip generated output sentinels from source
```

Authoring features an agent should use (full reference: [`docs/notebooks.md`](notebooks.md)):

- **Template interpolation in prose:** `${expr}` evaluates against the notebook environment; `${expr:%.3f}` formats; `\${...}` escapes.
- **Directives** as HTML comments immediately before a fence (stackable): `<!-- hide -->` (run but hide source), `<!-- details: Title -->` (collapsible output), `<!-- grid: 2 -->` (plot grid). Also `<!-- exercise -->` / `<!-- solution -->` for teaching material.
- **Embeds:** `![[_setup.md]]` transcludes another file; its rustlab blocks execute in the host's evaluator (shared-variable setup across lessons). `[[Other Notebook]]` wikilinks cross-reference; directory renders get navigation automatically.
- **Interactive widgets** (under `watch` only): a ` ```rustlab-widget ` fence with TOML body (`name`, `type = "slider"|"number"|"option"`, bounds/choices, `default`) plus `val = widget("name")` in code. Batch `render` substitutes the declared default.
- **Frontmatter:** `title:` and `order:` control the directory index.
- **Caching for slow cells:** put `cache enable` in an early block — pure user functions get a persistent SQLite-backed result cache (`.rustlab/cache.db`) that survives re-renders. Functions that call `rand`, plotting, or I/O are excluded automatically.

Verification loop for notebook authoring: edit → `rustlab-notebook render note.md -f html` (watch stderr for block errors) → `rustlab-notebook check note.md`.

The [`gallery/`](../gallery/) directory holds 38 rendered example notebooks (committed Markdown output, browsable on GitHub) — the best source of real authoring patterns. Their sources live in `examples/notebooks/`.

---

## 8. Toolbox map — where to look for what

All function names below are in `docs/quickref.md` with one-line descriptions; full signatures in `docs/functions.md`; live detail via `rustlab docs <name>`.

| Task | Key functions | Example scripts |
|---|---|---|
| Elementary math | `exp sqrt abs log sin cos atan2 floor round mod real imag conj angle` | `examples/math/` |
| Arrays & grids | `linspace zeros ones eye rand randn seed reshape repmat meshgrid size numel` | `examples/language/` |
| Linear algebra | `inv det rank trace eig svd expm linsolve roots dot cross kron norm` | `examples/linalg/` |
| Sparse | `sparse speye spdiags spsolve chol lu solve eigs laplacian_1d/2d/3d full nnz find` | `examples/sparse/`, `examples/pde/` |
| Statistics | `sum mean median std min max argmax sort cumsum trapz hist all any` | `examples/stats/` |
| FFT & spectral | `fft ifft fftshift fftfreq spectrum pwelch stft spectrogram cwt waterfall` + `*_stream` variants | `examples/spectral/` |
| FIR/IIR filters | `fir_lowpass/_highpass/_bandpass` (+`_kaiser`), `fir_notch firpm firpmq freqz filtfilt convolve upfirdn window` | `examples/dsp/` |
| Fixed-point | `qfmt quantize qadd qmul qconv snr` | `examples/dsp/fixed_point.rlab` |
| Control systems | `tf ss pole zero bode nyquist step margin rlocus lqr care dare place lyap gram ctrb obsv rk4 freqresp` | `examples/controls/` |
| RF / S-parameters | `sparameters sij s2z/s2y/s2t/s2abcd cascade deembed newref smith rfplot vswr stabilityk gainmax` | `examples/rf/` |
| ML activations | `softmax relu gelu layernorm` | `examples/math/` |
| Vector calculus / PDE | `gradient divergence curl` (+ `*3` 3-D forms), `rect_mask disk_mask polygon_mask ij2k k2ij` | `examples/pde/` |
| Plotting | `plot stem bar scatter imagesc heatmap surf contour contourf quiver streamplot polar loglog semilogx/y plotdb smith savefig` | `examples/plot/` |
| Live / streaming | `figure_live plot_update figure_draw state_init filter_stream audio_in/out audio_read/write` | `examples/audio/` |
| I/O | `save load whos fprintf sprintf` (NPY/NPZ/CSV/TOML/Touchstone) | `examples/language/save_load.rlab` |

---

## 9. Recommended agent workflow

When asked to produce rustlab code:

1. **Check the capability index first** (`docs/quickref.md` or `rustlab docs --json`). Never invent function names.
2. **Write the script** to a `.rlab` file, ending file outputs with `savefig`/`save` rather than interactive plots.
3. **Run it:** `rustlab run script.rlab --plot none`, capturing stderr.
4. **Treat any stderr line matching `^error:` as failure** (the exit code stays 0); fix and re-run.
5. **Verify artifacts** (the SVG/CSV/NPZ files you expected) exist and are non-trivial in size.
6. For notebooks: `rustlab-notebook render note.md` then `rustlab-notebook check note.md`.
7. To mimic existing style, copy patterns from the closest script in `examples/` or notebook in `gallery/` — they are all maintained and runnable (`cargo test -p rustlab-cli` executes the example suite in CI).
