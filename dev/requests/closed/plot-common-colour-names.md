# Request: accept common colour names (`orange`, `purple`, …) in `"color"`

**Status**: Done — all three proposals landed, **additively**. Added `orange`,
`purple`, `brown`, `pink`, `olive`, `teal`, `navy`, `gold`; the cycle fallback now
skips a hue already on the axes. No existing colour changed value. Verified:
lesson 09's eight Breit-Rabi traces render eight distinct colours.
**Date**: 2026-07-28

## Correction to the report as filed

The fallback colour is index-dependent, not always green. On a fresh single-series
plot `orange` yielded cyan `#17BECF`; the reported `#2CA02C` is what the third
series gets. The collision risk is real either way, and proposal 3 (never reuse a
hue already on the axes) is implemented, so a rejected name can no longer collide.

## The blocker the request did not see

`purple` could not be mapped to `#9467BD` as asked, because **`magenta` already
rendered `#9467BD`** — matplotlib's purple. `yellow` likewise rendered `#BCBD22`
(olive). Both base names were misnomers, and lesson 09 plots `purple` *and*
`magenta` on the same axes, so the requested mapping would have produced two
identical traces — the exact bug this request is about.

The first attempt fixed this by giving the base names MATLAB's values
(`m` = `#FF00FF`, `y` = `#FFFF00`, …). **That was reverted.** Review found the
named variants are load-bearing in four separate colour tables — the SVG series
path, the SVG overlay path (`series_color_to_rgb`), the Plotly path
(`color_to_css`), and the live viewer's wire protocol (`wire_color_to_egui`) —
and only two had been updated, so `plot(…,"blue")` and `quiver(…,"blue")` rendered
`#0000FF` and `#1F77B4` in the *same* SVG. Rebasing the cycle on RGB literals also
silently downgraded all default terminal output from 16-colour ANSI to 24-bit
truecolor.

Resolved instead by keeping every base value exactly as it was and giving
`purple`/`olive` their **CSS** values (`#800080`, `#808000`) so they stay
distinguishable from `magenta`/`yellow` as those actually render. A test pins the
base values, another asserts no two names render the same RGB, and `to_rgb()` is
now the single table all backends read — a pure dedup with no behaviour change.
Zero gallery figures change colour. Renaming the misnamed base colours remains a
separate, four-backend change.

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
