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
# each element, visited by hand
S = zeros(size(M));
for i = 1:size(M, 1)
  for k = 1:size(M, 2)
    S(i, k) = M(i, k)^2 + 2 * M(i, k);
  end
end
```

```rustlab
# the same arithmetic, as an array
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

Keep a `for` or `while` when step `k` has to read a value that step
`k - 1` wrote: a recurrence, a streaming filter, an animation frame
that carries state. Say why in a one-line comment. There is no `break`
and no `continue`. Put the stop test in the `while` header.

Those six practices, each with a short bad example and a good one, are
[Script style for agents](agent-guide.md#script-style-for-agents).

### Make the script easy to check

- Comments in a `.rlab` file are `#`. Both `#` and `%` start a comment;
  `%` is the notebook convention, below.
- Name intermediate arrays. A subscript stacked on a subscript is hard
  to check, and it evaluates the slice twice. Name the slice, then the
  next operation.
- Call only builtins that exist. [`docs/quickref.md`](quickref.md) is
  the list. `rustlab docs <name>` and [`docs/functions.md`](functions.md)
  have the signature.
- Write files with `savefig` (a plot) and `save` (data). Agents and CI
  run `rustlab run script.rlab --plot none` so a terminal plot cannot
  block the job. `savefig` still writes the file.

## Notebooks

Light conventions only. One idea per cell. Variables persist from one
rustlab code fence to the next in the same notebook, so define a grid,
a sample rate, or a loaded array once and reuse it below.

Comments inside a notebook code fence are `%`:

```rustlab
sr = 44100;   % sample rate
```

The same line in a `.rlab` file uses `#`.

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

## See also

- [`AGENTS.md`](../AGENTS.md) — working on the Rust crates.
- [`docs/agent-guide.md`](agent-guide.md) — running rustlab, language
  rules, and the worked
  [Script style for agents](agent-guide.md#script-style-for-agents)
  examples.
- [`docs/notebooks.md`](notebooks.md) — notebook format.
- [`docs/lesson-site-pattern.md`](lesson-site-pattern.md) — course
  layout, including the notebook rules this page summarizes.
