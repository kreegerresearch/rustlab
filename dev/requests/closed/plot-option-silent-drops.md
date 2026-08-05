# Bug: plotting options were discarded silently (found while fixing the quantum_lab audit)

**Status**: Fixed — all five below. Found by probing the option surface around the
two filed reports, not reported by quantum_lab.
**Date**: 2026-07-28

Affects rustlab **0.3.7** and earlier. Every one of these had the same shape: an
argument the author wrote was thrown away, the figure still rendered, and nothing
said a word. That is worse than an error, because the artefact ships looking finished.

## 1. An unknown option key discarded every argument after it

`parse_plot_opts` stopped at the first key it did not recognize (`_ => break`),
abandoning the rest of the list.

```rustlab
plot(x, y, "marker", "o", "color", "red")   # renders a PALETTE colour, no warning
```

`"marker"` was never a plot option — it is a Smith-chart builtin — so `"color", "red"`
went with it. This is what quantum_lab's Penning-trap workaround hit: their operating
point was meant to be red and rendered cyan.

**Fixed**: unknown keys warn and parsing continues.

```
plot: warning: unknown option 'linewidth' — ignored. Known options: color/colour, label, style, marker
```

## 2. A single-point series drew nothing

With no marker support, `plot([5], [5], "label", "…")` created a one-point *line*
series. A polyline of one point renders no pixels, so the figure got a legend entry
for a mark that was not on the page — verified: 0 `<circle>` elements in the plot area,
only the legend swatch.

**Fixed**: `"marker"` is now a real option. `o` / `.` / `circle` / `point` draw point
marks (MATLAB `plot(x, y, 'o')` semantics — marks, no connecting line); `none` keeps
the line. Only circle spellings are accepted, because a filled circle is the only mark
the renderers draw — taking `"s"` and quietly drawing a circle would repeat the bug.

## 3. A title after any option was dropped

The title was only read when it was the *sole* trailing argument, so putting an
option in front of it lost it.

```rustlab
plot(x, y, "color", "red", "My Title")   # title silently missing
```

**Fixed**: `parse_plot_opts_for` returns the number of arguments it consumed and
the title is whatever genuinely remains. The first attempt inferred this from the
remainder's *parity*, which review caught as wrong — when parsing stops early the
leftover is not where parity says it is, and `plot(x, y, "r", "label", "L")`
promoted the label value to the chart title while dropping both the colour and the
label. The parser also now understands MATLAB's `plot(x, y, LineSpec, Name, Value,
…)` grammar, where a bare colour spec *leads* the pairs.

All three of `plot()`, `stem()` and `scatter()` go through one entry point
(`parse_trailing_plot_args`) so they cannot disagree again — they previously each
had their own copy of the rule.

## 3b. A non-string option value discarded everything after it

```rustlab
plot(x, y, "linewidth", 3, "color", "red")   # renders a palette colour, no warning
```

The same silent-drop bug via the other exit from the loop: `to_str()` failed on the
numeric `3` and the parser gave up. **Fixed**: a non-string *value* is a typo'd
option, so it warns and steps over the pair; a non-string *key* means we are no
longer looking at options and the caller reports the leftover.

## 3c. Trailing arguments that are not options are now reported

`scatter(x, y, sz, c)` — MATLAB's per-point size/colour form — used to raise an
arity error. During the fix that guard was removed and the call silently drew a
plain default scatter, ignoring both vectors. **Fixed**: unconsumed trailing
arguments produce
`scatter: warning: 2 trailing argument(s) ignored — expected "key", "value" pairs
optionally followed by a title`.

## 4. An unrecognized `"style"` value fell through to solid

Anything that was not `"dashed"` became `LineStyle::Solid`, so `"style", "dotted"`
drew a plain line with no diagnostic.

**Fixed**: `solid` and `dashed` are accepted; anything else warns and leaves solid.

## 5. Colorbar midpoint printed `-0.000`

Noted as a minor item at the end of the legend report (`10-sideband-cooling` plot 4).
`format_cbar_value` used `{:.3}`, so a midpoint that was a hair below zero from
floating-point drift printed a signed zero — which reads as a real negative bin
sitting next to the positive one on a symmetric scale.

**Fixed**: a value that rounds to zero prints unsigned. Genuine negatives keep their
sign. Three gallery heatmaps re-rendered; the only change in each is `-0.000` → `0.000`.

## 6. Four hand-maintained copies of one colour table

`SeriesColor` → RGB was written out longhand in four places: the SVG series path
(`to_plotters`), the SVG overlay path (`series_color_to_rgb`, used by quiver,
streamlines and contours), the Plotly path (`color_to_css`), and the live viewer's
wire protocol (`wire_color_to_egui`). Nothing compared them.

This is what made the first attempt at renaming the base colours unsafe — two of
the four were updated and the other two were not, so one SVG contained `#0000FF`
and `#1F77B4` for the same requested `"blue"`. **Fixed** by making
`SeriesColor::to_rgb()` the single source and pointing the backends at it. With
the values left unchanged this is a pure dedup: no figure moves a pixel.

## 7. Extended colour names silently ate one-word titles

Every builtin that treats a lone trailing string as "colour spec, else title" got
wider the moment new names were added, so `plot(y, "Gold")` became an untitled gold
line. **Fixed**: `SeriesColor::is_base_spec()` gates that rule to the historical
spellings, so adding names can never reclassify a title again. Extended names still
work via the explicit `"color", "gold"` form.

## Not fixed — left for review

- **`plot(M, "label", "s")` gives every column of the matrix the same legend label.**
  Arguably correct (one label was supplied for N series), but the legend ends up with
  N identical rows and no way to tell the columns apart. A positional fallback
  (`s 1`, `s 2`, …) or a warning would both be defensible; it needs a product call
  rather than a bug fix.
- **The repo is not `rustfmt`-clean.** `cargo fmt --all` rewrites 88 files that no
  one touched (749 lines in `tests.rs` alone), so it cannot be run as part of a normal
  change without swamping the diff. Worth either fixing once in a dedicated commit and
  adding `cargo fmt --check` to CI, or documenting that fmt is not enforced.
