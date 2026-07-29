# Bug: `scatter()` ignores `"label"`, so scatter series cannot appear in a legend

Affects rustlab / rustlab-notebook **0.3.7**.

Two defects compound here. The first is long-standing and silent; the second is new in
0.3.7 and is what makes the first one visible.

1. **`scatter()` accepts a `"label"` argument and silently discards it.** Neither the
   keyword form nor the positional form has any effect. There is no spelling that gives a
   scatter series a legend entry.
2. **Auto-label suppression is unconditional.** 0.3.7 stopped emitting type-name
   auto-labels (`scatter`, `stem`, `bar`, `value`). For single-series plots that is a
   clear improvement. But those auto-labels were the *only* mechanism that ever put a
   scatter into a legend, so suppressing them everywhere means a scatter overlaid on a
   labelled curve now vanishes from the legend entirely.

## Reproduction

```rustlab
clf;
x = linspace(0.0, 10.0, 50)

# Case A — scatter with a keyword label
hold on
plot(x, x, "label", "the line", "color", "blue")
scatter([5.0], [5.0], "label", "the point")
hold off
```
Legend entries: `['the line']` — the label is dropped, no warning.

```rustlab
# Case B — scatter with a positional label
scatter([5.0], [5.0], "the point")
```
Legend entries: `['the line']` — also dropped.

```rustlab
# Case C — plot() with a marker, same visual result
plot([5.0], [5.0], "label", "the point", "marker", "o")
```
Legend entries: `['the line', 'the point']` — correct.

So `plot(..., "marker", "o")` honours `"label"` and `scatter(...)` does not, though the two
produce the same mark on the page.

## Why this matters

The failure mode is a plot that looks finished and is quietly incomplete. A reader who
trusts the legend concludes the extra mark is decoration, or misattributes it to the one
labelled series. Nothing warns, and the rendered SVG is the artefact that ships.

The broken idiom is a common one: draw a curve with a descriptive label, then overlay a
single `scatter` marker for the operating point, measured value, or intersection. The
overlay is exactly the thing a reader most needs identified.

Silent argument-dropping is the sharper half of this. An author writes
`scatter(x, y, "label", "operating point")`, sees no error, and reasonably assumes it
worked. Rejecting the argument loudly would be better than ignoring it.

## Encountered in

`quantum_lab`, Lesson 18 (Penning trap), stability-boundary figure —
`notebooks/18-penning-trap.md`:

```rustlab
plot(fz_s / 1.0e3, Bmin_s, "label", "B_min = sqrt(2) m w_z / q", "color", "blue")
scatter([f_z / 1.0e3], [B_min])
```

The scatter marks the trap's actual operating point (5 T at f_z = 150 kHz) against the
computed boundary — the whole point of the figure. Under 0.3.6 it at least appeared as
`scatter`; under 0.3.7 it is an unexplained dot. Worked around by switching to
`plot(..., "marker", "o", "color", "red")`.

Related, from this repo's `AGENTS.md` notes against 0.3.6: *"`legend("off")` does not hide
the legend … the renderer also always draws a legend for multi-series figures (unlabeled
series show as 'value')."* That note and this report are two ends of the same design
question — 0.3.7 fixed the noise, but the underlying inability to label a scatter was
never addressed, so the fix exposed it.

## Please do not revert the 0.3.7 legend change

Re-rendering an 18-lesson course across the 0.3.6 → 0.3.7 boundary showed the change is
overwhelmingly correct, and this report is not an argument against it:

- **32 plots correctly lost a noise legend** — all single-series, sole entry an
  uninformative `bar` / `stem` / `value`.
- **95 plots kept their legends**, entries unchanged.
- **Colorbar ticks were fixed** — degenerate scales that printed the same number five
  times (`['0.707', '0.707', '0.707', '0.707', '0.707']`) now render a real symmetric
  range (`['-0.707', '-0.354', '0.000', '0.354', '0.707']`).
- **1 plot regressed** — the case above.

## Proposed fix

Primary, and sufficient on its own:

- **Make `scatter()` honour `"label"`**, matching `plot()`. This restores a legend entry
  for the overlay idiom without reintroducing any noise legends.

Secondary, defensive:

- **Scope the auto-label suppression by series count.** One unlabelled series → suppress
  the legend (0.3.7 behaviour, keep). More than one series with at least one labelled →
  give every series an entry, falling back to a type-name or positional label
  (`series 2`) so the legend stays a complete key.
- **Warn on dropped arguments.** If an option is not supported by a plotting call, say so
  rather than discarding it; the same class of silent drop would be caught much earlier.

## Minor, likely the same area

`10-sideband-cooling` plot 4 renders a colorbar tick as `-0.000` under 0.3.7 where 0.3.6
printed `0.000`. Cosmetic negative-zero formatting, noted here to keep it with related
context rather than as a separate report.
