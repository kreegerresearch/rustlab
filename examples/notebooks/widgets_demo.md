# Interactive Widgets

This notebook embeds live controls — a **slider**, a **number input**, and
an **option** group — that drive a single plot. Open it with the
interactive server to make them live:

```sh
rustlab-notebook watch examples/notebooks/widgets_demo.md
```

Drag a control and only the blocks that read it (and those downstream)
re-run. Rendered statically (`notebook render`), each control shows at its
declared default and `widget(...)` returns that default — so this page is a
faithful snapshot either way.

See `docs/notebooks.md` → *Interactive widgets* for the full reference.

## Controls

```rustlab-widget
name = "amp"
type = "slider"
min = 0.1
max = 2.0
step = 0.1
default = 1.0
label = "Amplitude"
```

```rustlab-widget
name = "freq"
type = "number"
min = 1
max = 20
step = 1
default = 3
label = "Frequency (Hz)"
```

```rustlab-widget
name = "shape"
type = "option"
choices = ["sine", "cosine"]
default = "sine"
label = "Waveform"
```

## The plot

A one-second waveform whose amplitude, frequency, and shape come straight
from the controls above. Move a control (under `watch`) and this block
re-runs:

```rustlab
amp  = widget("amp");
freq = widget("freq");
t = linspace(0, 1, 500);
if widget("shape") == "cosine"
  y = amp * cos(2 * pi * freq * t);
else
  y = amp * sin(2 * pi * freq * t);
end
clf
plot(t, y)
title("Waveform")
xlabel("t (s)")
ylabel("amplitude")
```

The current settings are **amplitude ${amp:%.1f}**, **${freq} Hz**, shape
**${widget("shape")}** — the prose updates with the controls too, via
[template interpolation](../../docs/notebooks.md#template-interpolation).
