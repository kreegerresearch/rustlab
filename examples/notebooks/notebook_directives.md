---
title: Notebook Directives
order: 50
---

# Notebook Directives

`rustlab-notebook` understands a small set of directives — HTML-comment
tags that control how a code block or section is rendered. They use
HTML-comment syntax so the source `.md` file stays portable: any plain
CommonMark viewer (GitHub, VS Code preview) treats them as invisible
comments. Only `rustlab-notebook render` interprets them.

This notebook documents each directive with a working example.

## `<!-- hide -->`

Place `<!-- hide -->` on the line immediately before a code block to
suppress the *source* in the rendered output. The block still executes —
variables, text output, and plots flow through normally — only the code
listing is removed. Useful for setup that would distract from the
narrative: data loading, constant definitions, RNG seeding.

The block below is hidden — it sets up a noisy sinusoid:

<!-- hide -->
```rustlab
seed(42)
N = 200;
t = linspace(0, 2, N);
x = sin(2*pi*3*t) + 0.3*randn(N);
```

The signal `x` and its time axis `t` are now in scope. The hidden block
produced no visible code listing, but the variables it defined are
available to subsequent blocks:

```rustlab
plot(t, x)
title("Noisy 3 Hz sinusoid (signal defined in hidden setup block)")
xlabel("Time (s)")
ylabel("Amplitude")
grid on
```

## `<!-- details: Title -->`

Wraps a code block's *output* (text, errors, plots) in a collapsible
disclosure widget with the given summary label. The source remains
visible above. Useful when output would otherwise dominate the page —
long parameter sweeps, diagnostic dumps, large plot galleries.

<!-- details: Show all magnitude responses -->
```rustlab
fs = 16000;
figure()
hold("on")
for n_taps = [16, 32, 64, 128]
    h = fir_lowpass(n_taps, 3000, fs, "hamming");
    Hw = freqz(h, 512, fs);
    plot(Hw(1,:), 20*log10(abs(Hw(2,:))), "label", sprintf("N=%d", n_taps))
end
title("Lowpass magnitude response — tap-count sweep")
xlabel("Frequency (Hz)")
ylabel("Magnitude (dB)")
legend()
grid on
```

In Markdown output the section uses native `<details>`; in LaTeX/PDF it
becomes a labelled box; in HTML it's an animated disclosure widget.

## `<!-- grid: N -->`

Tiles the block's captured figures into an `N`-column responsive grid
instead of the default single-column stack. Each `savefig()` call
produces one snapshot — and one grid cell.

In notebook mode the path passed to `savefig()` is not used on disk;
the renderer is just looking for the snapshot. Use any non-`.html`
extension so the figure is included in the grid (Plotly HTML figures
render full-width by design and are excluded from grid tiling).

<!-- grid: 3 -->
```rustlab
[X, Y] = meshgrid(linspace(-2, 2, 60), linspace(-2, 2, 60));
G1 = exp(-(X.^2 + Y.^2));
G2 = exp(-((X - 1).^2 + Y.^2));
G3 = G1 - G2;

figure(); imagesc(G1, "viridis"); title("Centered");      savefig("g1.svg")
figure(); imagesc(G2, "viridis"); title("Offset");        savefig("g2.svg")
figure(); imagesc(G3, "viridis"); title("Difference");    savefig("g3.svg")
```

`N` must be a positive integer. Text output (`disp`, `print`) appears
above the grid full-width — only the plot zone is tiled.

## Callouts: `<!-- note -->`, `<!-- tip -->`, `<!-- warning -->`

Place one of the three tags on its own line, then write the body
underneath. The callout ends at the next blank line, the next heading,
or an explicit closing tag (`<!-- /note -->`, `<!-- /tip -->`,
`<!-- /warning -->`). Markdown inside the body — including inline math —
renders normally.

<!-- note -->
The DFT of a length-$N$ signal produces $N$ complex bins covering
frequencies $[0, f_s)$. Bins above $f_s/2$ correspond to negative
frequencies for real input — fold the spectrum at $f_s/2$ for the
one-sided view.

<!-- tip -->
For peak frequency resolution at fixed $N$, choose a window with a
narrow main lobe (rectangular is narrowest; Hann is a common
compromise). Increase $N$ — not the window — when you need finer bins.
<!-- /tip -->

<!-- warning -->
`fft(x)` does **not** apply any normalization. To match time-domain
amplitude, divide the magnitude spectrum by $N$ for a one-sided view,
or by $N/2$ for two-sided.

In HTML output, each callout renders as a coloured box (info / success
/ danger). In LaTeX/PDF output, callouts become labelled paragraphs.

## Exercises and solutions: `<!-- exercise -->`, `<!-- solution -->`

`<!-- exercise -->` on its own line begins an auto-numbered exercise.
An optional `<!-- solution -->` tag inside the exercise begins a
collapsible "Show solution" section. Blocks auto-close on the next
`<!-- exercise -->` or at end of document — no explicit closing tag is
required.

<!-- exercise -->

Design a 32-tap Hann-windowed FIR lowpass with cutoff at 2 kHz at a
sample rate of 8 kHz, and plot its magnitude response in dB.

<!-- solution -->

```rustlab
h = fir_lowpass(32, 2000, 8000, "hann");
Hw = freqz(h, 512, 8000);
plot(Hw(1,:), 20*log10(abs(Hw(2,:))))
title("32-tap Hann lowpass — 2 kHz / 8 kHz")
xlabel("Frequency (Hz)")
ylabel("Magnitude (dB)")
grid on
```

<!-- exercise -->

Plot the time-domain shapes of a length-32 Hann window and a length-32
Hamming window on the same axes.

<!-- solution -->

```rustlab
M = 32;
wh = window("hann", M);
wm = window("hamming", M);
figure()
hold("on")
plot(wh, "label", "hann")
plot(wm, "label", "hamming")
title("Hann vs Hamming — length 32")
xlabel("Sample")
ylabel("Amplitude")
legend()
grid on
```

Solutions render collapsed by default — readers can attempt the
exercise before clicking to reveal the answer.

## Stacking directives

`<!-- hide -->`, `<!-- details: ... -->`, and `<!-- grid: N -->` can be
stacked on consecutive lines immediately before a `rustlab` fence.
Order within the stack does not matter. The block below combines
`<!-- hide -->` and `<!-- grid: 2 -->` to produce a hidden-source,
two-column gallery:

<!-- hide -->
<!-- grid: 2 -->
```rustlab
seed(99)
A = rand(32, 32);
B = A * A';
figure(); imagesc(A, "viridis"); title("Random A");           savefig("a.svg")
figure(); imagesc(B, "viridis"); title("A * A' (symmetric)"); savefig("b.svg")
```

The output is a tiled gallery without the surrounding boilerplate code —
useful for "look at these results, the code is uninteresting" sections.

## Summary

| Directive | Purpose |
|-----------|---------|
| `<!-- hide -->` | Suppress source code listing; block still executes |
| `<!-- details: TITLE -->` | Collapsible output with summary label |
| `<!-- grid: N -->` | Tile captured figures into N-column grid |
| `<!-- note -->`, `<!-- tip -->`, `<!-- warning -->` | Inline callout boxes |
| `<!-- exercise -->`, `<!-- solution -->` | Auto-numbered exercises with hidden solutions |

For the canonical reference and edge cases, see `docs/notebooks.md`.
