# RustLab MVP — Plan

## Context
Build a Rust-based matrix algebra / DSP toolkit (`rustlab`) — a CLI + scripting tool targeting signal processing workflows. The user wants complex-number-native operations, FIR/IIR/convolution/windowing for MVP, a custom `.rlab` script format, and ratatui terminal plotting. Matrix decompositions (LU, SVD, etc.) are deferred but the architecture must accommodate them easily.

---

## Workspace Layout

```
rustlab/
├── Cargo.toml                   # workspace root
├── PLAN.md                      # this file
├── crates/
│   ├── rustlab-core/            # types, traits, no DSP
│   ├── rustlab-dsp/             # FIR, IIR, conv, windowing
│   ├── rustlab-plot/            # ratatui terminal plots
│   ├── rustlab-script/          # lexer → parser → evaluator for .rlab files
│   └── rustlab-cli/             # clap binary; assembles all crates
├── examples/
│   ├── lowpass.rlab
│   ├── bandpass.rlab
│   └── complex_basics.rlab
└── docs/
    └── examples.md
```

Dependency order (no cycles): `core ← dsp ← script ← cli`, `core ← plot ← script ← cli`

---

## Crate Responsibilities

### `rustlab-core`
- Type aliases: `C64 = Complex<f64>`, `CVector = Array1<C64>`, `CMatrix = Array2<C64>`, `RVector`, `RMatrix`
- Traits:
  - `Filter` — `apply(&CVector) → CVector`, `frequency_response(n) → CVector`
  - `Transform` — `forward/inverse`
  - `Decomposable<Output>` + marker traits `LuDecomposable`, `SvdDecomposable`, `CholeskyDecomposable`, `EigenDecomposable` (no implementors yet)
- `CoreError` via `thiserror`
- Optional feature `linalg` gates `ndarray-linalg` dep for future decompositions

### `rustlab-dsp`
- `window/`: `WindowFunction` enum (Rectangular, Hann, Hamming, Blackman, Kaiser{beta}) with `generate(length) → RVector`
- `fir/design.rs`: `fir_lowpass/highpass/bandpass(taps, cutoff_hz, sr, window) → FirFilter` (implements `Filter`)
- `iir/butterworth.rs`: `butterworth_lowpass/highpass(order, cutoff, sr) → IirFilter` (implements `Filter`); uses bilinear transform
- `convolution.rs`: `convolve(x, h)` (direct O(nm)) + `overlap_add(x, h, block_size)` (FFT-based O(n log n))

### `rustlab-plot`
- Built on `ratatui` + `crossterm` (braille-pixel charts, richer than ASCII)
- Functions: `plot_real(v, title)`, `plot_complex(v, title)`, `stem_real(v, title)`
- Handles real (magnitude) and complex (magnitude + real part overlaid)
- Non-interactive mode: enter alternate screen → render → "press any key" → exit
- Leaves no artifacts in terminal scrollback

### `rustlab-script`
- **Lexer** → standard tokens; `j` is a predefined constant (`Complex(0,1)`)
  - `a = 123.2 + j*123` parses naturally — `j` is a builtin constant, `*` and `+` are normal ops
- **Parser** → AST (`Expr`, `Stmt` enums); LL(1) grammar
- **`Value` enum**: `Scalar(f64)`, `Complex(C64)`, `Vector(CVector)`, `Matrix(CMatrix)`, `Bool(bool)`, `Str(String)`, `None`
- **`BuiltinRegistry`**: `HashMap<String, BuiltinFn>` — add DSP functions via `register(name, fn)`, no parser changes needed
- Default builtins: `fir_lowpass`, `fir_highpass`, `fir_bandpass`, `butterworth_lowpass`, `butterworth_highpass`, `convolve`, `window`, `plot`, `print`, `zeros`, `ones`, `linspace`, `abs`, `angle`, `real`, `imag`, `len`, `cos`, `sin`

### `rustlab-cli`
- Binary named `rustlab`
- Subcommands via clap derive:
  - `run <script.rlab>` — execute a `.rlab` script
  - `filter fir --taps N --cutoff F --sr F [--type low|high|band] [--window hann]`
  - `filter iir --order N --cutoff F --sr F [--type low|high]`
  - `convolve --signal <file> --kernel <file> [--method direct|overlap-add]`
  - `window --type hann|... --length N [--beta B]`
  - `plot --input <file> [--title STR]`
  - `info`

---

## Scripting Language Grammar (`.rlab` files)

```
program     ::= stmt*
stmt        ::= IDENT "=" expr NEWLINE   # assignment
              | expr NEWLINE             # expression (print result)
              | "#" <rest-of-line>       # comment (stripped by lexer)

expr        ::= expr ("+" | "-") term | term
term        ::= term ("*" | "/") factor | factor
factor      ::= primary ("^" factor)?   # right-assoc exponentiation
primary     ::= NUMBER | STRING | IDENT
              | IDENT "(" arg_list? ")" # function call
              | "[" row (";" row)* "]"  # matrix/vector literal  ; = row sep
              | "(" expr ")"
              | "-" primary
```

`j` is a predefined constant (`Complex(0.0, 1.0)`), so complex arithmetic works naturally:

```
a = 123.2 + j*123     # complex number
b = 2.0 * j           # pure imaginary
c = a * b             # complex multiply
```

---

## Key Crate Dependencies

| Crate | Version | Used in |
|---|---|---|
| `num-complex` | 0.4 | core, dsp, script |
| `ndarray` | 0.16 | core, dsp, plot (rayon feature) |
| `ndarray-linalg` | 0.17 | core (optional, `linalg` feature) |
| `thiserror` | 2 | core, dsp, plot, script |
| `anyhow` | 1 | script, cli |
| `clap` | 4 | cli (derive feature) |
| `ratatui` | 0.29 | plot |
| `crossterm` | 0.28 | plot (terminal backend for ratatui) |

---

## Implementation Order

1. **Workspace scaffold** — root `Cargo.toml`, all five crate skeletons
2. **`rustlab-core`** — type aliases, `Filter`/`Transform`/`Decomposable` traits, `CoreError`
3. **`rustlab-dsp`** — windowing → FIR → IIR → convolution
4. **`rustlab-plot`** — ratatui charts (real, complex, stem)
5. **`rustlab-script`** — lexer → parser → `Value` enum → `BuiltinRegistry` → evaluator
6. **`rustlab-cli`** — wire all subcommands; `run` delegates to script engine
7. **Example scripts** — `examples/lowpass.rlab`, `examples/bandpass.rlab`, `examples/complex_basics.rlab`
8. **Documentation** — `README.md` (quickstart + syntax reference) + `docs/examples.md`

---

## Verification

```sh
cargo build --workspace
cargo test --workspace
rustlab run examples/lowpass.rlab
rustlab filter fir --taps 32 --cutoff 1000 --sr 44100 --type low --window hann
rustlab window --type hann --length 64
```
