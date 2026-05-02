# toml_filter_chain.r — Load a multi-filter config and design all filters
#
# Demonstrates:
#   TOML [[array_of_tables]] → Tuple of Structs
#   Indexing into tuples:  filters(k)
#   Looping over a config list

cfg = load("toml_filter_chain.toml");
sr = cfg.sample_rate;
filters = cfg.filters;

fprintf("=== %s ===\n", cfg.title)
fprintf("Sample rate: %d Hz\n", sr)
fprintf("Filters:     %d\n\n", length(filters))

for k = 1:length(filters)
    f = filters(k);
    switch f.type
        case "lowpass"
            h = fir_lowpass(f.taps, f.cutoff_hz, sr, "hann");
        case "highpass"
            h = fir_highpass(f.taps, f.cutoff_hz, sr, "hann");
        case "notch"
            h = fir_notch(f.cutoff_hz, 5.0, sr, f.taps, "hann");
    end
    fprintf("  [%d] %-12s  %5d Hz  %d taps  peak=%.4f\n", ...
        k, f.name, f.cutoff_hz, length(h), max(abs(h)))
end
