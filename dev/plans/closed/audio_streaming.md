# Development Plan: Real-Time Audio Streaming & Explicit Filter State

**Target example:** `examples/audio/realtime_fir.rlab`
**Current phase:** complete
**Status:** All phases complete (implemented in commit d37a630)

---

## Overview

Add real-time audio streaming and stateful DSP to rustlab using **stdin/stdout
raw PCM** as the I/O transport. No audio libraries are linked into rustlab —
hardware I/O is delegated to external tools (`sox`, `ffmpeg`, `arecord/aplay`,
or a custom bridge). This keeps the rustlab binary lean and dependency-free,
and makes the audio pipeline testable without any hardware.

```
sox -d -t raw -r 44100 -e float -b 32 -c 1 - \
  | rustlab run filter.rlab \
  | sox -t raw -r 44100 -e float -b 32 -c 1 - -d
```

The scripting API:

```r
h   = firpm(64, [0, 900.0/22050.0, 1000.0/22050.0, 1.0], [1, 1, 0, 0]);
st  = state_init(length(h) - 1);

adc = audio_in(44100.0, 256);
dac = audio_out(44100.0, 256);

while true
    frame          = audio_read(adc);
    [out, st]      = filter_stream(frame, h, st);
    audio_write(dac, out);
end
```

`audio_in` / `audio_out` are **pure metadata handles** — they record sample
rate and frame size but open no file descriptors. `audio_read` calls
`stdin().read_exact()` and `audio_write` calls `stdout().write_all()`. Both
are backed entirely by `std::io`.

**Zero new dependencies.** The dependency graph does not change.

Work is split into three phases ordered by dependency. Each phase must be
fully implemented, tested, and committed before the next begins.

---

## Phase 1 — Language: `while` loop

**Status: complete**

The streaming loop requires `while cond ... end`. The existing `for` over a
range is not a substitute for an infinite hardware-clocked loop.

### 1a. Lexer token

- **File:** `crates/rustlab-script/src/lexer.rs`
- **Change:** Recognize `"while"` in the identifier arm and emit a new
  `Token::While` (alongside the existing `Token::For`, `Token::If`, etc.).
- **Test:** `"while"` tokenizes as `Token::While`, not `Token::Ident("while")`.

### 1b. AST node

- **File:** `crates/rustlab-script/src/ast.rs`
- **Change:** Add to `Stmt`:
  ```rust
  /// `while cond ... end`
  While {
      cond: Expr,
      body: Vec<Stmt>,
  },
  ```

### 1c. Parser

- **File:** `crates/rustlab-script/src/parser.rs`
- **Change:** In `parse_stmts_until_end`, add a `Token::While` arm:
  1. Consume `Token::While`.
  2. Parse `cond` as an expression (up to the next newline).
  3. Recursively call `parse_stmts_until_end` terminated by `Token::End`.
  4. Return `Stmt::While { cond, body }`.
- **Test:** `while x < 10 \n x = x + 1 \n end` produces the correct AST.

### 1d. Evaluator

- **File:** `crates/rustlab-script/src/eval/mod.rs`
- **Change:** Add `Stmt::While { cond, body }` arm in `run()`:
  ```rust
  Stmt::While { cond, body } => {
      loop {
          let v = self.eval_expr(cond)?;
          if !v.is_truthy() { break; }
          self.run(body)?;
      }
  }
  ```
  Reuse `Value::is_truthy()` (nonzero real part → true). The loop exits on
  Ctrl-C via the normal OS signal — no special handling needed.
- **Test:** `x = 0; while x < 5 \n x = x + 1 \n end` → `x == 5`.
- **Test:** `while false \n x = 99 \n end` → body never executes.

---

## Phase 2 — Explicit Filter State

**Status: complete**

Add the `Value::FirState` handle and two builtins — `state_init` and
`filter_stream`. DSP functions remain internally stateless; all sample history
lives in the caller-held state handle.

### 2a. `Value::FirState`

- **File:** `crates/rustlab-script/src/eval/value.rs`
- **Change:** Add variant:
  ```rust
  /// Opaque history buffer for stateful FIR streaming.
  /// Arc<Mutex<...>> allows cheap Clone (ref-counted handle) while
  /// still providing &mut access inside filter_stream with no
  /// heap reallocation per frame.
  FirState(Arc<Mutex<Vec<C64>>>),
  ```
- `type_name()` → `"fir_state"`.
- `Display` → `"<fir_state N>"` where N is the buffer length.
- `Clone` is derived (Arc clone — O(1), not a deep copy).
- No arithmetic operations on `FirState` — it is a pure opaque handle.

### 2b. `state_init(n)` builtin

- **File:** `crates/rustlab-script/src/eval/builtins.rs`

```rust
fn builtin_state_init(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("state_init", &args, 1)?;
    let n = args[0].to_usize()?;
    let buf = vec![C64::new(0.0, 0.0); n];
    Ok(Value::FirState(Arc::new(Mutex::new(buf))))
}
```

- **Note on sizing:** A filter with M taps requires M-1 history samples.
  The caller must pass `length(h) - 1`, not `length(h)`:
  ```r
  st = state_init(length(h) - 1);
  ```
  `filter_stream` validates the size and returns a runtime error if it
  does not match `h.len() - 1`.
- **Test:** `s = state_init(63)` → type is `"fir_state"`, length 63.

### 2c. `filter_stream(frame, h, state)` builtin

- **File:** `crates/rustlab-script/src/eval/builtins.rs`
- **Returns:** `Tuple([output_frame: Vector, state: FirState])`

**Algorithm (overlap-save):**

1. Lock `state` to obtain `history: &mut Vec<C64>` (length M-1).
2. Validate `history.len() == h.len() - 1`; return `ScriptError::Runtime` if not.
3. Build `extended: Vec<C64> = [history..., frame...]` (length M-1 + N).
4. Compute N output samples:
   `output[i] = Σ_{k=0}^{M-1} h[k] * extended[i + M - 1 - k]`
   This is the valid inner product — output length equals frame length exactly.
5. Update history in place: copy `extended[N .. N+M-1]` back into `*history`
   (the last M-1 samples of the input frame become the new history).
6. Release lock.
7. Return `Value::Tuple(vec![Value::Vector(output), Value::FirState(arc.clone())])`.
   The returned `FirState` is the same `Arc` — a pointer copy, not a new buffer.

**Properties:**
- **Zero-copy:** the history buffer is mutated in place; no reallocation per frame.
- **Multi-channel:** two independent `state_init` handles with the same `h`
  process two channels with no shared state.
- **Test:** Process a 440 Hz sine at 44100 Hz through a firpm lowpass
  (cutoff 500 Hz) in 256-sample frames. After 4 frames, concatenate outputs
  and compare against a single `convolve(full_signal, h)` — must match to
  within 1e-9.
- **Test:** A frame of silence through a settled filter produces a silent
  output (all real parts < 1e-12).

### 2d. REPL help entries

```
"state_init"    → "state_init(n) — allocate FIR history buffer of n zeros; n = length(h)-1"
"filter_stream" → "filter_stream(frame, h, state) — [out, state] overlap-save FIR frame processing"
```

---

## Phase 3 — stdin/stdout Audio Builtins

**Status: complete**

Four builtins backed entirely by `std::io`. No new crates, no new Cargo
dependencies, no audio library required to build or run.

### 3a. `Value::AudioIn` and `Value::AudioOut`

- **File:** `crates/rustlab-script/src/eval/value.rs`
- **Change:** Add two variants:
  ```rust
  /// Metadata handle for stdin audio input. Opens no file descriptors.
  AudioIn  { sample_rate: f64, frame_size: usize },
  /// Metadata handle for stdout audio output. Opens no file descriptors.
  AudioOut { sample_rate: f64, frame_size: usize },
  ```
  These are plain data — no `Arc`, no `Mutex`. They carry only the parameters
  needed by `audio_read` and `audio_write` to know how many bytes to transfer.
- `type_name()` → `"audio_in"` / `"audio_out"`.
- `Display` → `"<audio_in 44100 Hz / 256>"` / `"<audio_out 44100 Hz / 256>"`.

### 3b. `audio_in(sr, frame_size)` builtin

```rust
fn builtin_audio_in(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("audio_in", &args, 2)?;
    let sample_rate = args[0].to_scalar()?;
    let frame_size  = args[1].to_usize()?;
    Ok(Value::AudioIn { sample_rate, frame_size })
}
```

### 3c. `audio_out(sr, frame_size)` builtin

```rust
fn builtin_audio_out(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("audio_out", &args, 2)?;
    let sample_rate = args[0].to_scalar()?;
    let frame_size  = args[1].to_usize()?;
    Ok(Value::AudioOut { sample_rate, frame_size })
}
```

### 3d. `audio_read(adc)` builtin

Reads exactly `frame_size * 4` bytes from stdin as little-endian `f32`,
converts to a real-valued `CVector`.

```rust
fn builtin_audio_read(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("audio_read", &args, 1)?;
    let Value::AudioIn { frame_size, .. } = args[0] else {
        return Err(ScriptError::type_error("audio_read", "audio_in", args[0].type_name()));
    };
    let mut buf = vec![0u8; frame_size * 4];
    std::io::stdin().lock()
        .read_exact(&mut buf)
        .map_err(|e| ScriptError::Runtime(format!("audio_read: {e}")))?;
    let cvec: CVector = buf.chunks_exact(4)
        .map(|b| C64::new(
            f32::from_le_bytes(b.try_into().unwrap()) as f64,
            0.0,
        ))
        .collect();
    Ok(Value::Vector(cvec))
}
```

`read_exact` blocks until the full frame is available — this is what rate-locks
the script loop to the upstream audio source. If stdin closes (source exits),
`read_exact` returns an error and the script terminates cleanly.

### 3e. `audio_write(dac, frame)` builtin

Converts the frame to little-endian `f32` and writes all bytes to stdout,
then flushes. Only the real part of each sample is used.

```rust
fn builtin_audio_write(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("audio_write", &args, 2)?;
    let Value::AudioOut { .. } = args[0] else {
        return Err(ScriptError::type_error("audio_write", "audio_out", args[0].type_name()));
    };
    let frame = args[1].to_cvector()?;
    let mut out = std::io::stdout().lock();
    for c in frame.iter() {
        out.write_all(&(c.re as f32).to_le_bytes())
           .map_err(|e| ScriptError::Runtime(format!("audio_write: {e}")))?;
    }
    out.flush()
       .map_err(|e| ScriptError::Runtime(format!("audio_write flush: {e}")))?;
    Ok(Value::None)
}
```

### 3f. Register all builtins

```rust
r.register("audio_in",    builtin_audio_in);
r.register("audio_out",   builtin_audio_out);
r.register("audio_read",  builtin_audio_read);
r.register("audio_write", builtin_audio_write);
```

### 3g. REPL help entries

```
"audio_in"    → "audio_in(sr, n) — stdin PCM handle: sample rate sr Hz, frame size n"
"audio_out"   → "audio_out(sr, n) — stdout PCM handle: sample rate sr Hz, frame size n"
"audio_read"  → "audio_read(adc) — read one frame of f32-LE PCM from stdin"
"audio_write" → "audio_write(dac, frame) — write one frame of f32-LE PCM to stdout"
```

### 3h. Testing without hardware

Because I/O is plain stdin/stdout, every test runs in CI with no audio
hardware and no audio libraries:

```rust
// In crates/rustlab-script/src/tests.rs

#[test]
fn test_audio_pipeline_via_script() {
    // Generate 512 samples of a 440 Hz sine as raw f32-LE bytes
    let sr = 44100.0f32;
    let input_bytes: Vec<u8> = (0..512)
        .flat_map(|i| {
            let s = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr).sin();
            s.to_le_bytes()
        })
        .collect();

    // Run the filter script with input piped through stdin
    // (use the existing script runner with injected stdio — see note below)
    let script = r#"
        h   = firpm(32, [0, 0.04, 0.05, 1.0], [1, 1, 0, 0]);
        st  = state_init(length(h) - 1);
        adc = audio_in(44100.0, 256);
        dac = audio_out(44100.0, 256);
        frame         = audio_read(adc);
        [out, st]     = filter_stream(frame, h, st);
        audio_write(dac, out);
        frame         = audio_read(adc);
        [out, st]     = filter_stream(frame, h, st);
        audio_write(dac, out);
    "#;

    let output_bytes = run_script_with_stdin(script, &input_bytes).unwrap();
    assert_eq!(output_bytes.len(), input_bytes.len());

    // 440 Hz is well within the passband — energy should be preserved
    let output_samples: Vec<f32> = output_bytes.chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let energy: f32 = output_samples.iter().map(|s| s * s).sum();
    assert!(energy > 0.1, "passband signal lost: energy={energy}");
}
```

**`run_script_with_stdin`** is a small test helper that redirects stdin/stdout
for the duration of a script run. Implementing it requires a way to inject
a `Read` source into the evaluator. The cleanest approach:

- Add an optional `stdin_override: Option<Box<dyn Read>>` field to `Evaluator`.
- `audio_read` checks this first; falls back to `std::io::stdin()` when `None`.
- Similarly `stdout_override: Option<Box<dyn Write>>` for `audio_write`.
- In production (normal script runs), both fields are `None`.
- In tests, inject `Cursor<Vec<u8>>` for stdin and `Vec<u8>` for stdout.

This is the only structural change to `Evaluator` in this entire plan.

### 3i. Example scripts

**`examples/audio/realtime_fir.rlab`**
```r
# Real-time FIR lowpass: Parks-McClellan, cutoff ~1 kHz at 44100 Hz
# Run with:
#   sox -d -t raw -r 44100 -e float -b 32 -c 1 - \
#     | rustlab run examples/audio/realtime_fir.rlab \
#     | sox -t raw -r 44100 -e float -b 32 -c 1 - -d

sr     = 44100.0;
n_taps = 64;
cutoff = 1000.0 / (sr / 2.0);   # normalise to [0, 1]

h  = firpm(n_taps, [0, cutoff * 0.9, cutoff, 1.0], [1, 1, 0, 0]);
st = state_init(length(h) - 1);

adc = audio_in(sr, 256);
dac = audio_out(sr, 256);

while true
    frame     = audio_read(adc);
    [out, st] = filter_stream(frame, h, st);
    audio_write(dac, out);
end
```

**`examples/audio/passthrough.rlab`**
```r
# Minimal passthrough — useful for testing the pipeline with no DSP
adc = audio_in(44100.0, 256);
dac = audio_out(44100.0, 256);
while true
    audio_write(dac, audio_read(adc));
end
```

### 3j. Documentation updates

- **`docs/functions.md`:** Add sections for `state_init`, `filter_stream`,
  `audio_in`, `audio_out`, `audio_read`, `audio_write`. Include the full
  pipeline command, the overlap-save explanation, and the testing recipe.
- **`docs/quickref.md`:** Add `Audio I/O` and `Streaming DSP` rows.
- **`AGENTS.md`:** Add all six new builtins to the builtins table. Add
  `Value::AudioIn`, `Value::AudioOut`, `Value::FirState` to the Value enum
  table. Note the `stdin_override`/`stdout_override` fields on `Evaluator`.

---

## Bridge programs (outside this codebase)

rustlab has no opinion on which bridge is used. Any program that produces
raw mono f32-LE PCM on stdout and/or consumes it on stdin works:

| Tool | Input command | Output command |
|------|--------------|----------------|
| sox (macOS/Linux) | `sox -d -t raw -r 44100 -e float -b 32 -c 1 -` | `sox -t raw -r 44100 -e float -b 32 -c 1 - -d` |
| ffmpeg | `ffmpeg -f avfoundation -i :0 -f f32le -ar 44100 -ac 1 -` | `ffmpeg -f f32le -ar 44100 -ac 1 -i - -f avfoundation :0` |
| arecord/aplay (Linux) | `arecord -f FLOAT_LE -r 44100 -c 1` | `aplay -f FLOAT_LE -r 44100 -c 1` |
| Python test signal | `python3 -c "import struct,math,sys; [sys.stdout.buffer.write(struct.pack('f', math.sin(2*math.pi*440*i/44100))) for i in range(44100*5)]"` | — |

A standalone `rustlab-bridge` binary (thin cpal wrapper) may be published
separately in the future for users who prefer a single-command setup, but it
is explicitly out of scope for this codebase.

---

## Architectural summary

| Property | How it is achieved |
|---|---|
| No audio library in rustlab | `audio_read`/`audio_write` use `std::io` only |
| Fast load time | No dynamic library linking for audio; startup unchanged |
| Testable in CI | stdin/stdout can be redirected to `Cursor<Vec<u8>>` in tests |
| Rate-locking to hardware | `read_exact` blocks until upstream delivers a full frame |
| Stateless DSP functions | All history lives in the caller-held `FirState` handle |
| Zero-copy filter state | `Arc<Mutex<Vec<C64>>>` mutated in place; Arc clone is O(1) |
| Multi-channel | Two independent `state_init` handles share no state |

## Dependency graph

Unchanged from before this feature:

```
rustlab-core
    ↑           ↑
rustlab-dsp   rustlab-plot
    ↑           ↑
    └─────┬─────┘
    rustlab-script
          ↑
    rustlab-cli
```
