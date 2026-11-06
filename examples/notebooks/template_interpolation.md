# Template Interpolation

Notebook markdown cells can embed computed values using `\${expr}` syntax.
The expressions are evaluated against the shared notebook environment.

## Basic Usage

```rustlab
n_samples = 1024;
fs = 16000;
duration = n_samples / fs;
```

This analysis uses **${n_samples}** samples at a sample rate of
${fs} Hz, giving a total duration of ${duration:%,.3f} seconds.

## Expressions in Templates

Templates can contain any valid rustlab expression, not just variables:

```rustlab
seed(42)
x = randn(1, n_samples);
signal_power = sum(x .* x) / length(x);
```

The signal has ${length(x)} samples with mean power ${signal_power:%.4f}.
The Nyquist frequency is ${fs / 2} Hz.

## Format Specifiers

Use `\${expr:format}` to control formatting. The format spec uses
`sprintf` syntax:

```rustlab
big_number = 1234567.89;
ratio = 1/3;
```

| Format | Syntax | Result |
|--------|--------|--------|
| Commas | `\${big_number:%,.2f}` | ${big_number:%,.2f} |
| Scientific | `\${big_number:%.3e}` | ${big_number:%.3e} |
| Percentage | `\${ratio:%.1f%%}` | ${ratio:%.1f%%} |
| Integer | `\${n_samples:%d}` | ${n_samples:%d} |
| Signed | `\${ratio:%+.3f}` | ${ratio:%+.3f} |
| Zero-pad | `\${n_samples:%05d}` | ${n_samples:%05d} |

`+`, a leading space, `0`, and `#` are real flags (a leading `+` forces
the sign; `0` pads between the sign and the digits). Width and
precision may also come from the next argument via `*` and `.*`.

## Escaping

Use `\${...}` for literal dollar-brace in the output: \${not_evaluated}.

## Filter Design Summary

```rustlab
n_taps = 64;
fc = 3000;
h = fir_lowpass(n_taps, fc, fs, "hamming");
Hw = freqz(h, 512, fs);
w = Hw(1,:);
H = Hw(2,:);
plot(w, 20*log10(abs(H)))
title("Lowpass Filter Response")
xlabel("Frequency (Hz)")
ylabel("dB")
grid on
```

Designed a **${n_taps}-tap** FIR lowpass filter with cutoff at
${fc} Hz. The filter has ${length(h)} coefficients and its DC gain
is ${sum(h):%.6f}.
