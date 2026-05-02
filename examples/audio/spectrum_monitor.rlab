# Real-time audio spectrum monitor
#
# Displays a live ratatui plot of the Hann-windowed FFT magnitude spectrum
# in dB (DC to Nyquist), updated roughly once per second.
# Set show_time = 1 below to add a time-domain waveform subplot on top.
# Y-axis limits expand to fit the data and stabilize over time.
#
# Run with sox (macOS):
#   sox -d -t raw -r 44100 -e float -b 32 -c 1 - \
#     | rustlab run examples/audio/spectrum_monitor.r
#
# Run with arecord (Linux):
#   arecord -f FLOAT_LE -r 44100 -c 1 -t raw \
#     | rustlab run examples/audio/spectrum_monitor.r
#
# Or use the wrapper:
#   ./examples/audio/spectrum_monitor.sh
#   ./examples/audio/spectrum_monitor.sh --test

# ── Options ──
show_time   = 0;     # set to 1 to show time-domain waveform subplot
update_sec  = 0.5;   # display update interval in seconds

sr       = 44100.0;
frame    = 256;
fft_size = 4096;
half     = fft_size / 2;
frames_per_fft = fft_size / frame;                          # 16
update_every   = max(frames_per_fft, round(update_sec * sr / frame));  # frames between redraws
slots    = frames_per_fft;           # number of frame slots in buffer

# Hann window and frequency axis
win  = window("hann", fft_size);
freqs = fftfreq(fft_size, sr);
f_hz  = freqs(1:half);

# Panel layout: 1 row (spectrum only) or 2 rows (time + spectrum)
if show_time
    t_ms = linspace(0, fft_size / sr * 1000, fft_size);
    fig = figure_live(2, 1);
    spec_panel = 2;
else
    fig = figure_live(1, 1);
    spec_panel = 1;
end

adc = audio_in(sr, frame);

# Set panel labels
if show_time
    plot_labels(fig, 1, "Waveform", "Time (ms)", "Amplitude")
    plot_labels(fig, 2, "Spectrum", "Frequency (Hz)", "Magnitude (dB)")
else
    plot_labels(fig, 1, "Spectrum", "Frequency (Hz)", "Magnitude (dB)")
end

# Ring buffer: store each frame in its own slot for O(frame) writes
# On read, concatenate tail + head to get contiguous time-domain data.
ring  = zeros(fft_size);
wpos  = 0;        # next slot index (0-based, wraps at slots)
count = 0;

# Running axis limits — expand to fit data, rounded to 10 dB steps
db_lo = -120.0;
db_hi =  0.0;

# Running amplitude limits for time-domain plot
amp_lo =  0.0;
amp_hi =  0.0;

while true
    samples = audio_read(adc);

    # Write frame into the current ring slot
    base = wpos * frame;
    for k = 1:frame
        ring(base + k) = real(samples(k));
    end
    wpos  = mod(wpos + 1, slots);
    count = count + 1;

    # Update plot once per full buffer cycle, skip until buffer is full
    if count >= frames_per_fft
        if mod(count, update_every) == 0
            # Linearize ring buffer: data after write-pointer is oldest,
            # data before it is newest → [tail, head] is time-contiguous.
            head_end = wpos * frame;
            if head_end == 0
                buf = ring;
            else
                buf = [ring(head_end + 1:fft_size), ring(1:head_end)];
            end
            # --- Time-domain waveform (panel 1, if enabled) ---
            if show_time
                cur_amp_min = min(buf);
                cur_amp_max = max(buf);
                if cur_amp_min < amp_lo
                    amp_lo = cur_amp_min;
                end
                if cur_amp_max > amp_hi
                    amp_hi = cur_amp_max;
                end
                plot_limits(fig, 1, [0, fft_size / sr * 1000], [amp_lo, amp_hi]);
                plot_update(fig, 1, t_ms, buf);
            end

            # --- Frequency-domain spectrum ---
            X  = fft(buf .* win);
            Xd = mag2db(X(1:half));

            # Expand running limits to fit this frame
            cur_min = min(Xd);
            cur_max = max(Xd);
            if cur_min < db_lo
                db_lo = floor(cur_min / 10) * 10;
            end
            if cur_max > db_hi
                db_hi = ceil(cur_max / 10) * 10;
            end
            plot_limits(fig, spec_panel, [0, sr / 2], [db_lo, db_hi]);
            plot_update(fig, spec_panel, f_hz, Xd);

            figure_draw(fig);
        end
    end
end
