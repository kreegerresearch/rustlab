# Function call profiling example
#
# rustlab can track how many times each function is called, how long it takes,
# and how much data flows through it (IO throughput in Mbit/s).
#
# ── Two ways to activate profiling ───────────────────────────────────────────
#
#   1. In-script (selective):
#        profile(fn1, fn2)   — track only the named functions
#        profile()           — track all function calls
#      Use profile_report() to print a snapshot at any point in the script.
#      The report is also printed automatically to stderr at script exit.
#
#   2. CLI flag (no source changes needed):
#        rustlab run --profile script.r
#      Equivalent to calling profile() at the very top of the script.
#
# The report always goes to stderr so it does not mix with normal stdout output.
#
# ─────────────────────────────────────────────────────────────────────────────

N  = 1024;
fs = 44100.0;
t  = linspace(0, (N-1)/fs, N);
x  = sin(2 * pi * 1000 .* t) + 0.5 * sin(2 * pi * 3000 .* t);

# ── Example 1: selective profiling ───────────────────────────────────────────
# Track only fft and abs — everything else (linspace, sin, +, .*) is ignored.
profile(fft, abs)

X   = fft(x);
mag = abs(X);

disp("Peak magnitude (should be ~512 for a 1-sample-per-cycle sinusoid):")
max(mag)

disp("Selective profile — only fft and abs appear:")
profile_report()

# ── Example 2: track everything for a more expensive section ─────────────────
# Calling profile() again expands tracking to all functions.
# Stats accumulate across calls — the previous fft/abs entries are retained.
profile()

h    = fir_lowpass(63, 2000, fs, "hann");
y    = filtfilt(h, [1], x);
Y    = fft(y);
mag2 = abs(Y);

disp("Filtered peak magnitude:")
max(mag2)

disp("Full profile after second section:")
profile_report()

# ── Example 3: lambda profiling ───────────────────────────────────────────────
# Lambdas appear in the report under their variable name, not "<lambda>".
# Callbacks passed to arrayfun are suppressed — arrayfun's entry already
# captures their total execution time.
profile(mag_db, arrayfun)

mag_db = @(sig) 20 .* log10(abs(fft(sig)) ./ N + 1e-12);

db_x = mag_db(x);
db_y = mag_db(y);

disp("DC bin dB (raw):")
db_x(1)
disp("DC bin dB (filtered):")
db_y(1)

# arrayfun maps the pipeline over three gain levels.
# The report shows one "arrayfun" entry and one "mag_db" entry (from the
# two direct calls above). The three inner mag_db calls inside arrayfun
# are not tracked separately.
gains   = [0.5, 1.0, 2.0];
spectra = arrayfun(@(g) mag_db(x .* g), gains);

disp("Spectra matrix size (3 gains × N frequency bins):")
size(spectra)

disp("Lambda + arrayfun profile:")
profile_report()

# The final auto-printed report (to stderr) covers any calls made after the
# last profile_report(), so it will show only what ran after this point.
disp("")
disp("Script done — final report will follow on stderr.")
