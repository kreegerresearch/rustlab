# Log-scale axes + legend/labeling fixes

Issue write-up and fix plan. Branch: `fix/log-axis-and-legend`.

## Issues (all reproduced against `main`, SVG output inspected)

### B1 — `semilogx` / `semilogy` / `loglog` render a log-*transformed* linear axis, not a log-*scaled* axis (HIGH)

**Symptom.** `semilogx([1,10,100,1000],[1,2,3,4]); savefig(...)` produces an
x-axis whose tick labels are `0.0, 0.5, 1.0 … 3.0` — the *log10 values* — and an
axis descriptor literally reading `log10(x)`. The tick marks are therefore
"indexed to log10" instead of showing the real numbers `1, 10, 100, 1000`.

**Root cause.** `builtin_loglog` / `builtin_semilogx` / `builtin_semilogy`
(`eval/builtins.rs`) implement the log plot as a *pre-transform shim*: they
replace the data with `log10(data)`, call `plot`, and set the axis label to
`"log10(x)"`. The data is thus plotted on an ordinary **linear** axis whose
values happen to be log10 of the input, so the auto-generated ticks are
`0,1,2,3` and the label is `log10(x)`.

**MATLAB behaviour (target).** `semilogx` keeps the *real* data and renders the
x-axis on a **logarithmic scale**: major ticks sit at decades (`1, 10, 100,
1000`) positioned logarithmically and labelled with the real values; no
`log10(...)` descriptor is added.

**Fix.**
1. Add an axis-scale mode to the subplot: `x_scale` / `y_scale : AxisScale
   { Linear, Log }` on `SubplotState` (rustlab-plot `figure.rs`).
2. `semilogx` / `semilogy` / `loglog` stop pre-transforming — they plot the
   **real** data and set the corresponding scale flag(s). No `log10(x)` label.
   (Positive-data assertion stays.)
3. Renderers honour the scale:
   - **SVG** (`file.rs`): for a log axis, map coordinates through `log10` for
     positioning, place major ticks at integer decades, and format each tick
     label as the real value `10^k`.
   - **HTML/Plotly** (`html.rs`): set the axis `type: "log"` (Plotly renders
     real-valued decade ticks natively).
   - **Terminal** (`ascii.rs`): map through `log10` for positioning; drop the
     `log10(...)` descriptor. (Decade tick labels are best-effort in ASCII.)

### B2 — a colour/line-spec string passed to `plot` is mis-used as the chart title (MED)

**Symptom.** `plot([1,2,3],[1,2,3],"r")` renders a chart **titled `r`**. In
MATLAB `"r"` only colours the line red; there is no title.

**Root cause.** `builtin_plot`'s title logic takes the lone trailing string arg
as the title unconditionally — even when `parse_plot_opts` has already
consumed it as a colour/line-spec.

**Fix.** Only treat a lone trailing string as the title when it is **not** a
recognised colour / line-spec (i.e. `SeriesColor::parse` returns `None`).

### B3 — a plain `plot(x, y)` draws a spurious "value" legend (MED)

**Symptom.** `plot([1,2,3],[1,2,3])` with no `legend(...)` call still draws a
legend box containing `value`. MATLAB shows no legend unless `legend(...)` is
called.

**Root cause.** `push_xy_line` / `push_line_series` give every plain series the
default label `"value"`; the SVG legend is drawn whenever *any* series has a
non-empty label, so the default label triggers a legend on every plot.

**Fix.** Default series label is empty (`""`); a legend is drawn only when the
user sets labels via `legend(...)`. The `"value"` string is not needed for
identification.

## Test cases

- `semilogx` SVG contains real-value x ticks (`10`, `100`, `1000`) and **not**
  a `log10(x)` descriptor; `semilogy` / `loglog` analogues.
- `AxisScale` round-trips through the subplot; log builtins set it and keep the
  real data.
- `plot(x,y,"r")` sets no title.
- `plot(x,y)` draws no legend; `legend("a","b")` does.

## Status — complete (2026-07-24)

- [x] **B1** — `AxisScale { Linear, Log }` on `SubplotState`; `semilogx`/
  `semilogy`/`loglog` keep real data and flag the scale (no `log10` label).
  SVG renders decade ticks with real values (`1, 10, 100, 1000`); Plotly uses
  `type: "log"`; terminal maps by log10 for the shape. `rfplot`'s axis label
  changed `log10(freq)` → `freq (Hz)`.
- [x] **B2** — a lone colour/line-spec string (`"r"`) is applied as the colour,
  not the title.
- [x] **B3** — plain `plot(x, y)` sets no default label, so no legend appears
  without `legend(...)`.

Tests: 8 new (`log_axis_and_legend_tests` ×7 in rustlab-script;
`log_x_axis_renders_real_decade_ticks_not_log10` in rustlab-plot). Two existing
tests updated to the new behaviour. Full suite green (35 suites); 76 examples
pass.
