# Coding guidelines

How to write rustlab that a person can read. This page is for people
writing `.rlab` scripts and notebooks. An agent can follow the same
rules.

It is a style page. Builtins are listed in
[`docs/quickref.md`](quickref.md); a name missing from that list is
not implemented. Signatures and examples are in
[`docs/functions.md`](functions.md) and `rustlab docs <name>`.
Notebook format (directives, widgets, output formats) is
[`docs/notebooks.md`](notebooks.md). The worked `.rlab`
before-and-after examples for the array style are
[Script style for agents](agent-guide.md#script-style-for-agents),
checked by
[`examples/language/vectorized_style.rlab`](../examples/language/vectorized_style.rlab).

## Scripts (`.rlab`)

### Cut nested loops

A pair of `for` loops that visit every element is usually an array
expression. Write the indexing, the logical mask, or the reduction.
Readability comes first. That same form is what the array kernels run,
so the clearer script is also the fast one.

```rustlab
% each element, visited by hand
S = zeros(size(M));
for i = 1:size(M, 1)
  for k = 1:size(M, 2)
    S(i, k) = M(i, k)^2 + 2 * M(i, k);
  end
end
```

```rustlab
% the same arithmetic, as an array
S = M .^ 2 + 2 .* M;
positive = S > 0;
kept = S(positive);
idx = find(positive);
```

A colon slice is the same idea: `block = M(2:end, 1:3)` names the rows
and columns you mean.

When each element is its own scalar calculation, pass a lambda to
`arrayfun`. Use `parmap` when the array is long enough that the workers
earn their keep. A `parmap` lambda cannot plot, `fprintf`,
`savefig` / `saveanim`, or call `seed` / `rng`. If `sin`, `exp`, and
`.*` already say the math, call them on the whole array.

`sum`, `mean`, `cumsum`, `diff`, `min`, `max`, `all`, `any`, and
`trapz` are the reductions. On a matrix, `sum(M)` is one sum per
column.

Those six practices, each with a short bad example and a good one, are
[Script style for agents](agent-guide.md#script-style-for-agents).

### One loop, or none

The default is one loop, or no loop. A loop that walks every element
is a slice, a mask, `find`, a reduction, or an outer product.

A `for` with an `if` that keeps some elements is a mask:
`kept = M(M > 0)`. A `while` that should stop puts that test in the
header.

There is no `break` and no `continue`. Do not invent a stand-in for
them. The stop test belongs in the `while` header.

A nested loop that is the physics, the carried state, or the index
arithmetic stays. Say why in a one-line comment, or name the value the
loop is updating (`pressure`, `state`, `k`). A row or a column that is
the exception sits outside the loop. If the body is hard to follow,
move it into a small local function and call that.

A diagonal, a lookup, or a mapping is often clearer written as that
mapping than as a mask of the whole array. Do not rewrite a loop into
array form just to have no loop. A seeded stream, a self-term, or a
result that has to match a reference one step at a time is a nested
loop; the comment says which of those it is.

A seeded `rand` stream is that kind of loop.
[`examples/language/seeded_random_loop.rlab`](../examples/language/seeded_random_loop.rlab)
draws four `rand()` values per trial under `seed(322)`, in order, and
checks that a re-seed replays the first four results. The arithmetic
inside the loop is already array code. One `rand` array sliced by
component would change the draw order, and `parmap` reseeds each
worker, so either rewrite would move the printed numbers. The loop
stays so the stream stays.

### Shapes of `+`, `.*`, and comparisons

`+` and `.*` use the same pairing, and a comparison uses that pairing
too.

- A scalar meets every element.
- An `n×1` column and a length-`n` vector pair element by element, the
  same length as `v .* v`. `v = 1:3; v' == v` is a `1×3` logical, the
  same pairs as `v' .* v`.
- Two matrices repeat a dimension of length 1.
  `c = [1; 2; 3]; r = ones(1, 3); c == r` is `3×3`, the same shape as
  `c .* r`.
- Two vectors have to be the same length. A length-1 vector does not
  stretch across another vector. It does stretch across a matrix, as
  `.*` does.
- A shape that cannot pair is an error.

Real parts are compared. `NaN == x` is false and `NaN != x` is true.

### Make the script easy to check

- `#` and `%` both start a comment, in a `.rlab` file and in a
  notebook code fence. Prefer `%`, the marker used in `.rlab` and in
  the notebook body. `#` is accepted. It is not required.
- Name intermediate arrays. A subscript stacked on a subscript is hard
  to check, and it evaluates the slice twice. Name the slice, then the
  next operation.
- Call only builtins that exist. [`docs/quickref.md`](quickref.md) is
  the list. `rustlab docs <name>` and [`docs/functions.md`](functions.md)
  have the signature.
- An example that checks a number ends the intermediate assignments
  with `;`, so a REPL does not echo them. The check itself is a
  `print` or an `fprintf`. Leave those lines alone when you only change
  style. `--quiet-echo` is not a flag you can pass. It is an idea for
  later, not something the CLI does today.
- Write files with `savefig` (a plot) and `save` (data). Agents and CI
  run `rustlab run script.rlab --plot none` so a terminal plot cannot
  block the job. `savefig` still writes the file.

A `NaN` in `imagesc` marks a cell that is not data. The viewer raster
stores that pixel with alpha 0, so it is transparent. SVG and PNG paint
the cell neutral gray. HTML sends `null`, and Plotly leaves a gap.
rustlab does not print an "alpha premultiplication" line for this.
`rustlab-notebook render --strict` fails when a code cell errors. A
line on stderr, by itself, does not.

## Notebooks

Light conventions only. One idea per cell. Variables persist from one
rustlab code fence to the next in the same notebook, so define a grid,
a sample rate, or a loaded array once and reuse it below.

The notebook fence is the same language as a `.rlab` file. `#` and `%`
both start a comment. Prefer `%`:

```rustlab
sr = 44100;   % sample rate
```

Start a plot cell with `clf;` so the previous cell's figure does not
carry into this one. Leave `figure()` and `savefig()` to a side
`.rlab` script. The notebook renderer captures the active figure on
its own. Where those side scripts live in a course repo is
[`docs/lesson-site-pattern.md`](lesson-site-pattern.md).

`<!-- hide -->` on the line before a fence runs that cell and omits it
from the rendered page. Use it for setup a reader does not need to
see. Leave a step that the lesson depends on visible.

Directives, widgets, embeds, and the render formats are
[`docs/notebooks.md`](notebooks.md).

Before you call the page done:

```sh
rustlab-notebook render note.md
rustlab-notebook check note.md
```

For a page full of plots, `rustlab run side.rlab --plot none` (and the
notebook render, which does not open a terminal figure) keeps the job
from waiting on a keypress. `--plot none` does not change a `NaN`
mask. It only skips the terminal drawing.

## See also

- [`AGENTS.md`](../AGENTS.md) — working on the Rust crates.
- [`docs/agent-guide.md`](agent-guide.md) — running rustlab, language
  rules, and the worked
  [Script style for agents](agent-guide.md#script-style-for-agents)
  examples.
- [`docs/notebooks.md`](notebooks.md) — notebook format.
- [`docs/lesson-site-pattern.md`](lesson-site-pattern.md) — course
  layout, including the notebook rules this page summarizes.
