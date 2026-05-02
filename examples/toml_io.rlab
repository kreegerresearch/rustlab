# toml_io.r — Load config from TOML, design a filter, save results back
#
# Demonstrates:
#   load("file.toml")  — returns a struct
#   save("file.toml", s) — writes a struct to TOML
#   Nested field access:  cfg.audio.sample_rate

# ── Load pipeline settings from TOML ──────────────────────────────────────

cfg = load("toml_config.toml")
fprintf("Project: %s  (v%d)\n", cfg.project, cfg.version)
fprintf("Audio:   %d Hz, %d-sample frames\n", cfg.audio.sample_rate, cfg.audio.frame_size)
fprintf("Filter:  %s @ %d Hz, %d taps, %s window\n", ...
    cfg.filter.type, cfg.filter.cutoff_hz, cfg.filter.taps, cfg.filter.window)

# ── Design the filter from config values ──────────────────────────────────

h = fir_lowpass(cfg.filter.taps, cfg.filter.cutoff_hz, cfg.audio.sample_rate, cfg.filter.window)
fprintf("\nDesigned %d-tap FIR filter\n", length(h))

# ── Quantize using config ─────────────────────────────────────────────────

fmt = qfmt(cfg.quantization.word_bits, cfg.quantization.frac_bits, ...
    cfg.quantization.round, cfg.quantization.overflow)
hq = quantize(h, fmt);
fprintf("Quantized to Q%d.%d\n", cfg.quantization.word_bits, cfg.quantization.frac_bits)
fprintf("SNR: %.1f dB\n", snr(h, hq))

# ── Save a results summary back to TOML ───────────────────────────────────

results = struct( ...
    "filter_length", length(h), ...
    "snr_db", snr(h, hq), ...
    "peak_tap", max(abs(h)))
output = struct("project", cfg.project, "results", results)
save("toml_results.toml", output)
fprintf("\nResults saved to toml_results.toml\n")
