# Time-Frequency Analysis

Where `pwelch` (see [`spectral_estimation.md`](spectral_estimation.md))
estimates how power is distributed across frequencies for a
*stationary* signal, time-frequency analysis tracks how the spectral
content evolves over time. This notebook introduces the **Short-Time
Fourier Transform** (STFT) and its visualisation, the **spectrogram**.

## Test signal — a linear chirp

A linear chirp sweeps its instantaneous frequency from $f_0$ at $t = 0$
to $f_1$ at $t = T$, with phase

$$\phi(t) = 2\pi\left[f_0 t + \frac{f_1 - f_0}{2T}\,t^2\right]$$

The signal $x(t) = \sin\phi(t)$ then has instantaneous frequency
$f(t) = f_0 + (f_1 - f_0)\,t/T$. A periodogram of this signal will
show a smear across $[f_0, f_1]$ with no information about *when*
each frequency was active — the time-frequency picture is what we
actually want.

```rustlab
seed(42)
fs  = 10000;
dur = 2.0;
n   = round(fs * dur);
t   = (0:(n-1)) / fs;
f0  = 100;
f1  = 5000;
phase = 2*pi*(f0*t + 0.5*(f1 - f0)*t.*t / dur);
x = sin(phase) + 0.05*randn(n);
plot(t(1:500), x(1:500))
title("First 50 ms of the chirp")
xlabel("Time (s)"); ylabel("Amplitude")
grid on
```

## Short-Time Fourier Transform

`stft(x, fs)` segments the signal into overlapping windowed pieces,
takes the FFT of each, and stacks the spectra as columns of a complex
matrix:

$$S[k, m] = \sum_{n=0}^{L-1} x[n + m\cdot H]\,w[n]\,e^{-j2\pi kn/N}$$

where $L$ is the window length, $H$ the hop, $N$ the FFT size, and
$m$ the frame index. The captured form `[S, f, t] = stft(...)` gives
the complex spectrogram plus the frequency and time axes; the bare
form auto-renders the magnitude spectrogram.

```rustlab
[S, f, tt] = stft(x, fs, window("hann", 512), 384, 1024);
size(S)
```

For a 20 000-sample chirp with a 512-sample Hann window and a hop of
128, this produces a $513 \times 153$ matrix — 513 one-sided frequency
bins (`nfft/2 + 1`) by 153 time frames.

## Spectrogram visualisation

`spectrogram(x, fs)` is a thin plot-only wrapper around `stft`. It
runs the transform, converts the magnitudes to dB via the shared
`db_clip` helper (clipped 80 dB below the global peak, which is what
keeps wide-dynamic-range signals readable), and renders the result as
an `imagesc` heatmap with the `viridis` colormap and `axis("xy")` so
frequency increases upward.

```rustlab
spectrogram(x, fs, window("hann", 512), 384, 1024)
```

The diagonal ramp from $f_0 = 100$ Hz at $t = 0$ to $f_1 = 5000$ Hz
at $t = 2$ s is the chirp signature — and unlike a periodogram, you
can read off the instantaneous frequency at any time.

## Time-frequency resolution trade-off

The Heisenberg–Gabor uncertainty principle has a discrete-signal twin:
**a shorter window gives finer time resolution but coarser frequency
resolution**. With a 128-sample window the time axis looks crisp but
the diagonal blurs vertically:

```rustlab
spectrogram(x, fs, window("hann", 128), 96, 256)
```

A 2048-sample window gives the opposite — sharp frequency resolution,
chunky time resolution:

```rustlab
spectrogram(x, fs, window("hann", 2048), 1536, 4096)
```

The 512-sample setting we started with is a typical sweet spot for
audio-rate signals.

## Continuous Wavelet Transform

The STFT uses one *fixed* window for all frequencies — and the
time/frequency resolution trade-off you set with that window is the
trade-off you live with everywhere on the plot. The **Continuous
Wavelet Transform** (CWT) does something cleverer: it scales the
analysis window with the analysed frequency, so high-frequency rows
get short windows (fine time resolution) and low-frequency rows get
long windows (fine frequency resolution).

The mother wavelet we use is the **analytic Morlet** with $\omega_0 = 6$:

$$\psi(t) = \pi^{-1/4}\,e^{j\omega_0 t}\,e^{-t^2/2}$$

and its scaled / shifted family
$\psi_{s,\tau}(t) = (1/\sqrt{s})\,\psi((t-\tau)/s)$. The CWT is the
inner product of the signal with every such wavelet:

$$W(s, \tau) = \int_{-\infty}^{\infty} x(t)\,\psi^*_{s,\tau}(t)\,dt$$

`cwt(x, fs)` computes this for 64 log-spaced scales by default.

```rustlab
[W, freqs, tt] = cwt(x, fs);
size(W)
```

64 rows (one per scale) by 20 000 columns (one per sample). Each row's
centre frequency is $f = \omega_0\,f_s / (2\pi\,s)$.

## Scalogram visualisation

`scalogram(x, fs)` is the CWT's `spectrogram` analogue — same
`db_clip` → `imagesc` pipeline, same 80 dB dynamic range, same
`axis("xy")`. Because the default scales are log-spaced, the
row-index y-axis is effectively a logarithmic frequency axis.

```rustlab
scalogram(x, fs);
```

Compare the upper-left of this image with the upper-left of the
spectrogram above: the CWT's chirp ridge is *sharper in time* at the
high-frequency end (short wavelets, fine time localisation) and
*sharper in frequency* at the low-frequency end (long wavelets, fine
frequency localisation). The STFT can't do both — pick a window and
you've picked a single point on the resolution trade-off curve.

## Streaming time-frequency

`pwelch`, `stft`, and `cwt` all have *frame-by-frame* counterparts —
`pwelch_stream`, `stft_stream`, `cwt_stream` — that mirror the
`state_init` / `filter_stream` streaming pattern. Audio analyzers,
SDR dashboards, and any "feed me one chunk at a time" workflow use
these. The state struct owns the ring buffer plus any running
accumulator and threads through each call:

```
state = pwelch_stream_init(fs, window, noverlap, nfft);
[Pxx, state] = pwelch_stream(frame, state);
```

To stay self-contained (notebooks render offline, no `audio_in`), we
simulate streaming by chopping our synthetic chirp into fixed-size
chunks and feeding them to `stft_stream` one at a time. The output
columns we accumulate manually; in a live program they would feed
straight into `plot_update_heatmap` for a scrolling spectrogram.

```rustlab
state = stft_stream_init(fs, window("hann", 512), 384, 1024);
n_freqs = 513;        % nfft/2 + 1
n_cols_total = 0;
% Run the chirp through stft_stream in 1024-sample chunks.
chunk = 1024;
for i = 0:floor(n/chunk) - 1
    frame = x(i*chunk + 1 : (i+1)*chunk);
    [S_new, state] = stft_stream(frame, state);
    n_cols_total = n_cols_total + size(S_new, 2);
end
n_cols_total
```

The accumulated column count matches what the batch `stft` produced
above (153). For a live display, the typical pattern is:

```rustlab
% Pseudo-code (won't run in a notebook — see examples/audio/spectrogram_monitor.sh):
% fig = figure_live(1, 1);
% state = stft_stream_init(sr, window("hann", 1024), 512, 1024);
% while true
%     samples = audio_read(adc);
%     [S, state] = stft_stream(samples, state);
%     if size(S, 2) > 0
%         S_db = 20*log10(abs(S) + 1e-12);
%         plot_update_heatmap(fig, 1, S_db, "viridis", -80, 0);
%         figure_draw(fig);
%     end
% end
```

The live demo lives at
[`examples/audio/spectrogram_monitor.{sh,rlab}`](../examples/audio/spectrogram_monitor.sh)
— wire it up to `sox` (macOS) or `arecord` (Linux) and watch a real
spectrogram scroll past in `rustlab-viewer` or the ratatui fallback.

## Method summary

| Method                          | What it estimates                  | When to reach for it          |
|---------------------------------|------------------------------------|-------------------------------|
| `pwelch`                        | Time-averaged PSD                  | Stationary signals; one peak per tone |
| `stft`                          | Complex time-frequency matrix      | When you need the phase or want to manipulate $S$ |
| `spectrogram`                   | dB magnitude heatmap, fixed window | Visualising non-stationary signals at one resolution |
| `cwt`                           | Complex time-scale matrix          | When you need wavelet coefficients for further analysis |
| `scalogram`                     | dB magnitude heatmap, log-freq y   | Wide-band signals where one window can't capture both ends |
| `*_stream` + `plot_update_heatmap` | Frame-by-frame analysis + live heatmap | Realtime audio / SDR / sensor monitoring |
