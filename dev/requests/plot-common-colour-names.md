# Request: accept common colour names (`orange`, `purple`, …) in `"color"`

Affects rustlab **0.3.7** (and earlier).

## Symptom

`"color"` accepts single-letter and full-word forms of eight base colours plus
`gray`/`grey` and `#RRGGBB`. Anything outside that set warns and falls back to a palette
colour:

```
plot: warning: unrecognized color 'orange' — using a palette color.
Known: r/red, g/green, b/blue, c/cyan, m/magenta, y/yellow, k/black, w/white, gray/grey, #RRGGBB
```

`orange` and `purple` are the two that come up constantly, because they are the natural
5th and 6th series colours once r/g/b/c/m/y are spoken for — and they are the next two
entries in the default palette itself (`#FF7F0E`, `#9467BD`).

## Reproduction

```rustlab
plot(x, y, "label", "J_3", "color", "orange")    # warns, silently substitutes
plot(x, y, "label", "J_3", "color", "#FF7F0E")   # works, no warning
```

## Why this matters

The fallback is not neutral. Asking for `orange` currently yields `#2CA02C` — **green**:

```rustlab
plot(x, x * 0.6, "label", "named orange", "color", "orange")
# rendered stroke: #2CA02C
```

So a figure whose author picked distinguishable colours by hand can end up with two series
the same hue, and the only signal is a render-time warning that scrolls past in a build
log. On a multi-trace figure that is a correctness problem, not a cosmetic one: the reader
cannot tell the series apart.

## Encountered in

`quantum_lab`, four sites across two lessons:

- `notebooks/09-hyperfine-qubit-selection.md:76,80` — the Breit-Rabi diagram plots **eight**
  ground-state sublevels of ¹³⁷Ba⁺ against magnetic field. Eight simultaneous traces need
  eight distinguishable colours; the supported palette runs out at six.
- `notebooks/11-bessel-quantum-control.md:56,57` — `J_3` and `J_4` in a five-curve
  Bessel-function family.

Both were written with `orange`/`purple` as the obvious next colours and have been
silently rendering with substituted palette colours ever since.

## Workaround

Hex works and is warning-free — verified:

```rustlab
plot(x, y, "color", "#FF7F0E")   # orange
plot(x, y, "color", "#9467BD")   # purple
```

It costs the readability of the source, which is the main reason to prefer names in
teaching material where the plotting call is itself the thing being read.

## Proposed fix

In rough order of value:

1. **Add `orange` and `purple`** mapped to the existing palette entries `#FF7F0E` and
   `#9467BD`. Two names, covers the overwhelming majority of real use.
2. **Accept the CSS/SVG named-colour set** more broadly — it is a fixed, well-known table,
   and matplotlib/Octave users will reach for those names by habit.
3. **Make the fallback less dangerous** regardless of the above: prefer a palette slot not
   already in use on the current axes, so a bad name cannot collide with an existing
   series. Substituting green for a requested orange is the part that actually breaks
   figures.
