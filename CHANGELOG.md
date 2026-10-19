# Changelog

Notable user-facing changes to rustlab, newest first. Versions are the
workspace version in `Cargo.toml`; entries under a version may land across
several PRs while that version is current. **Breaking / behavior changes**
get their own subsection with migration guidance — downstream script owners
should re-validate against those entries when upgrading.

## 0.3.11

Perf flywheel iteration 02 (requests from `rustlab_llm`,
`perf/iterations/02-requests.md`). Plan: `dev/plans/closed/perf_flywheel_iter02.md`.

### Breaking / behavior changes
- **`--profile=ops` no longer shows `[transpose]` rows for `A' * B`,
  `A * B'` or `A' * B'` on matrices.** The product reads the transposed
  operand directly, so no transpose is formed; the time is in `[matmul]`.
  A transpose of a vector (`v' * M`) or a standalone `A'` still has its
  row. Results are unchanged, bit for bit.
- **Array division by a real number matches scalar division.** `./` (and
  `/` by a scalar) on arrays ran complex division, `x·conj(y) / |y|²`,
  even when `y` was real: about a third of `x ./ y` results differed from
  `x / y` in the last digit, and `[1e200] ./ [1e200]` was `NaN` because
  `|y|²` overflowed. A real divisor now divides each part directly, so
  `x ./ y` equals `x / y` bit for bit, and `(a + bj) ./ y` is
  `a/y + (b/y)j`. A complex divisor is unchanged. Results that depended
  on the old last digit move by one rounding step.
- **Negating a real value keeps it real.** `-v` left `-0.0` in every
  imaginary part, so `angle(-[1, 2])` was `[-π, -π]` and the complex
  branch of `sqrt`, `log` and `.^` put the result on the wrong side of the
  cut (`sqrt(-[4, 1j])` started with `-2j`). Negation no longer makes a
  negative zero, and `angle`, `.^` and the elementwise math builtins read a
  zero imaginary part as real whatever its sign: `angle(-1)` is `π`.
- **`save` keeps small imaginary parts.** NPY wrote real `<f8` data, and
  CSV a real cell, whenever the imaginary part was below `1e-12`, dropping
  it silently. Both now write real data only when the imaginary part is
  exactly zero; real arrays still save as `<f8`. TOML has no complex
  numbers and used to drop parts below `1e-15`, or refuse larger ones. A
  complex scalar, vector, matrix or rank-3 tensor now saves as a table of
  its parts (`__complex_re`, `__complex_im`, each the same shape as the
  value) and loads back exactly. Real values are written as plain numbers,
  as before. Those two field names are reserved, like the `Tensor3` ones.

### Fixed
- **`angle` accepts matrices and rank-3 tensors.** It returned an
  "unsupported type" error for anything but a scalar or vector. It now works
  element-wise and returns a real array of the same shape.

### Performance
- **Products with a transposed matrix operand skip the transpose.**
  `X * W'` and `H' * H` hand the matrix multiply a transposed view
  (conjugating inside the product for `'` on complex data), so no copy of
  the operand is made.
- **Arrays are copy-on-write.** A vector, matrix or rank-3 tensor is shared,
  not copied, when it is assigned to another name, passed to a function or
  builtin, read from a struct field, captured by a lambda, or handed to a
  `parmap` task. The first write to one of the names copies it, so values
  behave exactly as before. Passing a 13 MB struct to a user function went
  from 0.21 ms to 2 µs; the `rustlab_llm` training step is about 45 %
  faster.
- **`reshape` and one-row `[…]` are single passes.** `reshape` of a vector
  or matrix is one tiled transpose (parallel when large), and none when the
  order does not change: a vector to a vector or to the same shape is
  shared. `[a, b, …]` with one row of scalars, vectors and 1×N matrices is
  sized once and filled in parallel; `[v]` shares `v`. Results are
  unchanged. On 811 k elements, `reshape` went from 3.7 to 0.14 ms, a
  14-piece flatten from 2.4 to 0.4 ms, and the unflatten from 4.0 to
  1.5 ms. Large transposes (`A'`) are parallel too.
- **Less interpreter work per statement.** Region writes such as
  `M(r1:r2, :) = A` copy one row segment at a time when the columns are a
  consecutive run (`prim.region_write_128x384` 0.076 → 0.009 ms). The
  variable table uses a small built-in hash in place of SipHash, and a
  reassignment, the loop variable and the `end` binding overwrite their
  entry instead of allocating a new name (`acc = acc + k` loop of 100 k
  iterations 12.7 → 7.9 ms). Element-wise math builtins (`tanh`, `exp`,
  `gelu`, `gelu_grad`, `sin`, …) run in parallel from 16,384 elements.
  Results are unchanged, bit for bit. The `rustlab_llm` forward pass went
  from 12.5 to 9.5 ms and the backward pass from 17.5 to 14.7 ms.
- **Real matrix products have less overhead around the multiply.** The
  realness check and the conversion to real values are one pass, which is
  noticeable at the 100–500 element sizes typical of scripts.

## 0.3.10

Perf flywheel iteration 01 (requests from `rustlab_llm`,
`perf/iterations/01-requests.md`). Plan: `dev/plans/closed/perf_flywheel_iter01.md`.

### Breaking / behavior changes
- **The profile report has a new `Self (ms)` column and new rows.** The table
  is now Function, Calls, Total, Self, Avg, In, Out, Mbit/s (eight columns,
  was seven), nested calls that used to be hidden now have rows, and
  `profile(f)` reports calls `f` makes to other listed names. Migration: a
  parser of the stderr table must expect the extra column after `Total`;
  `TOTAL` is now the sum of main-thread self times.
- **Real matrix products use a real multiply, so last digits can change.**
  When both operands of `A * B` (or a row vector times a matrix) have no
  imaginary parts, the product is formed from the real parts with a real
  GEMM. Sums run in a different order from the complex kernel, so results
  can differ by rounding. A real operand holding `Inf` no longer produces
  `NaN` imaginary parts (the complex kernel computed `Inf·0`), so such a
  product now stays real. Migration: none expected; a test comparing
  products bit-for-bit should compare with a tolerance.

### Added
- **The profiler sees inside user functions.** Every call is recorded where
  it happens, nested calls and `arrayfun` callbacks included, with inclusive
  (`Total`) and `Self` time. A `[top-level]` row holds time outside any
  call, so the Self column sums to wall time (`TOTAL`). `[matmul]` rows time
  matrix products. `rustlab run --profile=ops` adds one row per operator plus
  `[transpose]`, `[index]` and `[index-assign]`. Calls inside `parmap` merge
  as `parmap:<name>` rows (CPU time across workers, not in `TOTAL`).

- **`gelu_grad(x)`** — derivative of `gelu` (same tanh approximation),
  element-wise, for backpropagating through a `gelu` layer.

### Fixed
- **In-script `profile()` now prints its report when `rustlab run` ends**, as
  its help always said. Before, only an explicit `profile_report()` printed.

- **`parmap` no longer copies the caller's variables into every task.**
  Workers start from the built-in constants and the user-defined functions;
  a lambda brings its captured values, and a user function already runs in
  its own scope, so results are unchanged. One worker is built per rayon
  split and reused across its tasks, and vector results are copied into the
  output matrix in parallel. A trivial 8-task `parmap` beside a 104 MB
  variable drops from 6.0 ms to 0.13 ms. `parmap(@feval, …)` still sees the
  caller's variables, because `feval` resolves names there.
- **Real matrix multiply is ~4–6× faster.** A 512×512×512 product went from
  8.4 to 50.8 GFLOP/s; the model shapes in `rustlab_llm` from ~8.6 to
  25–33 GFLOP/s. Complex operands keep the complex kernel. Shared helper:
  `rustlab_core::matmul`.
- **Elementwise operators no longer copy their operands.** A variable used
  in `+ - .* ./ .^` (or with a scalar in `* / ^`) is read in place, a
  temporary operand is overwritten with the result, and arrays of 131,072
  elements or more are processed on all cores. On 811k-element vectors,
  `theta - 0.001 * g` went from 1.9 ms to 0.27 ms and a full AdamW update
  from 19.5 ms to 4.7 ms. Results are bit-identical, including the rule
  that real inputs give real outputs.
- **Indexing reads the variable in place.** `v(a:b)` and `M(:, a:b)` no
  longer copy the whole array before selecting, a run of consecutive
  subscripts is copied as one block (in parallel when large), and integer
  ranges such as `1:n` are filled directly. `sum(Gm(:, 1:n), 1) / 8` on an
  8 × 811k matrix went from 29 ms to 2.7 ms.
- **`sum(M, 1)`** adds whole rows (in parallel when large) instead of
  gathering each column into a new buffer; every column is still summed top
  to bottom, so results are unchanged.
- **User functions move their outputs to the caller** instead of copying them.
- **Transpose is about 2× faster and returns a row-major matrix.** `A'` and
  `A.'` on a variable no longer copy `A` first, and the result is written in
  standard row-major layout with a tiled loop (129 × 512: 53 → 26 µs).
  Before, the result kept a column-major layout, so elementwise operations on
  a transposed matrix missed the fast paths above. Values are unchanged.
- **Real scalar arithmetic is faster.** `+ - * .* / ./` on two real scalars
  skip the general operator dispatch: a 100,000-iteration `acc = acc + k`
  loop went from 16.5 ms to 13.4 ms. Results are unchanged.
- **`S.field` reads copy only the field.** Reading a field of a struct
  variable (or a chain `S.a.b`) used to copy the whole struct first. For a
  model's 13 MB parameter struct read ~30 times per forward and backward
  pass, that was a measurable share of each pass.

## 0.3.9

### Breaking / behavior changes
- **HTML output no longer loads Plotly or KaTeX from a CDN.** Notebook
  HTML (`rustlab-notebook render -f html`), `savefig("….html")`, and
  `saveanim("….html")` reference a sibling `assets/` directory written
  next to the file. A directory render writes that tree once, at the
  output root, and nested pages use `../assets/…`. `notebook watch` is
  unchanged: it still serves the same vendored bytes at `/assets/`.
  Inlining was rejected — Plotly is about 4.4 MB and KaTeX (CSS, JS,
  woff2 fonts) about 0.6 MB, so a directory of notebooks would repeat
  ~5 MB on every page. Copy the HTML and its `assets/` directory
  together; opening the HTML from `file://` needs no network. A page
  that previously depended on `cdn.plot.ly` or `cdn.jsdelivr.net` being
  reachable now depends on that sibling directory instead.
- **`README.md` is a notebook.** Directory `render`, `watch`, `check`,
  `validate`, `clean`, and `strip` include `README.md` in any ASCII case
  (`readme.md`, `Readme.md`, a nested `ch/README.md`), the same way they
  include any other `.md` file. It is listed in the index and the file
  browser and served at `/n/README` (or `/n/<folder>/README`). Inside one
  folder, a README with no frontmatter `order:` sorts ahead of its
  unordered siblings; an explicit `order:` still wins. `index.md`,
  dotfiles, and `_` partials are unchanged. Migration: a directory render
  into a folder that already has a hand-written `README.md` overwrites
  that file when the source tree contains `README.md` (`make notebooks`
  will replace the hand-written `gallery/README.md` toolbox index with
  the render of `examples/notebooks/README.md`). Rename a notes file, or
  put the landing-page prose in the source README, if that overwrite is
  not what you want. A hand-written output README is left alone when the
  source tree has no `README.md`.
- **Directory `notebook watch` identifies a notebook by its collection path.**
  `ch2/intro.md` is `/n/ch2/intro` for the page, `/raw/ch2/intro`,
  `/save/ch2/intro`, `/annotate/ch2/intro`, `/plots/ch2/intro/`, and the
  live-reload socket `/n/ch2/intro/ws`. An old `/n/<stem>` URL redirects
  only when exactly one notebook has that file name (the name as written,
  or the previous lowercased form, so `/n/my-notes` still finds a lone
  `My Notes.md`). Two files named `filters.md` no longer send
  `/n/filters` to one of them: that URL is a 404 listing both paths.
  `/n/filters-2` is not an alias. A notebook at the collection root
  still serves `/n/intro` directly, even when a nested `intro.md`
  exists. A single-file `watch` stays at `/n/<stem>`. Migration:
  replace bookmarks of `/n/<stem>` and `/n/<stem>-N` for a nested
  notebook with `/n/<folder>/<stem>`.
- **`@name` calls the function, not a variable of the same name.**
  `sin = [1, 2, 3]; h = @sin; h(0)` is the builtin `sin(0)` (0). It
  used to be looked up again as a normal call, so a vector or lambda
  stored under that name was what the handle indexed or called. A user
  function shadowed by a variable is still what the handle calls; the
  direct call `sq(1)` still sees the variable. `@f` while `f` holds a
  lambda still copies that lambda, so a later write to `f` does not
  change the handle. Migration: call the variable (`sin(i)` or `f(x)`)
  when that value is what you want. A handle is the function.
- **`fprintf` / `sprintf` honor format flags and more specifiers.**
  `+`, a leading space, `0` (zero-pad between the sign and the digits),
  and `#` are applied. `*` and `.*` consume the next argument as the
  width or precision. Precision limits `%s`, sets a minimum digit count
  for integers (`%.0d` of 0 is empty), and is significant digits for
  `%g`/`%G`. New specifiers: `%u %o %x %X %c %E %G`. New escapes:
  `\r \b \f`. Non-finite values print as `Inf`, `-Inf`, and `NaN`.
  The `,` thousands-separator flag is unchanged. A vector or matrix is
  still an error — values are not recycled across elements. `%l` / `%ld`
  stay unknown. Migration: a script that passed `+`, `0`, `#`, or a
  space flag and expected them to be ignored will now see the sign,
  padding, or alternate form. `%u` of a negative number wraps as a
  64-bit unsigned value instead of erroring as an unknown specifier.
- **A matrix single index treats `end` as the element count.** `M(end)`
  and `(M)(end)` read and write the last element in column-major order,
  including a 1×N row (`zeros(1, n)`). Two indexes still bind `end` per
  axis, so `M(end, end)` is the corner. A 1×N or N×1 matrix honors
  `dim` the way a vector does: reducing the length-1 axis returns the
  matrix unchanged (`sum(r, 1)` on a row); the default and the long
  axis still reduce to a scalar. This applies to `sum`, `mean`, `prod`,
  `median`, `std`, `cumsum`, `max`/`min` with a dimension, and
  `argmin`/`argmax`. A vector now compares elementwise with a 1×N
  matrix of the same length (`==`, `!=`, `<`, `<=`, `>`, `>=`); the
  result is a 0/1 matrix. `all`/`any` accept a 1×N or N×1 matrix.
  `for i = r` walks the elements of a 1×N matrix. An N×1 column still
  cannot be iterated, and `fft` of a 1×N matrix is unchanged.
  Migration: `r(end)` on a 1×N matrix used to mean `r(1)`. `sum(r, 1)`
  on that row used to be the scalar total; it is now the row itself.
  Use `sum(r)` or `sum(r, 2)` for the total.
- **A notebook cell emits a plot only when it changes the figure.**
  `hold on` still keeps the axes, so a later `plot` overlays. A cell
  that only computes does not repeat the held figure. `savefig`
  snapshots are unchanged. Each notebook render starts from a reset
  figure, so `hold on` does not leak into the next notebook or the
  next `notebook watch` render. Migration: a cell that relied on a
  previous cell's plot appearing again without drawing or calling
  `savefig` will no longer show that plot. Draw in the cell that
  should display it, or call `savefig`.

- **`notebook watch` will not save a newly added SVG prose image, and it writes only the notebook `.md`.**
  `POST /save`, an inline cell save, and `POST /annotate` (including
  undo) reject a body that introduces `![…](….svg)`, `![[….svg]]`, a
  reference-style image, or a raw `<img src="….svg">` that the file on
  disk does not already reference. A highlight or comment is checked
  on the document that would be written (`==![x](eval.svg)==` and a
  `%%` note that contains an image both count). Nothing is written. A
  notebook that already references that SVG can still be saved or
  annotated, and an SVG mentioned only inside a code fence or inline
  code is not a prose image. All three write paths write only that
  notebook's `.md`, and only when the resolved path (symlink followed)
  is a `.md` file inside the jail — for annotate, even when
  `--jail-root` is unset. Migration: to add an SVG diagram,
  put the `.svg` in the notebook directory and add the image line in
  the `.md` on disk. The next in-browser save that keeps that reference
  is accepted. PNG, JPEG, GIF, and WebP prose images are unchanged.

### Fixed
- **A notebook widget change no longer flashes plots on the next save.**
  The watch page used to set a flag when it sent `widget_update` and
  outline plots on whichever reload arrived next. A widget render that
  was preempted never published, so the flag stayed set and a later
  file save flashed. The page now sends a request id. The server echoes
  it only on the envelope that render publishes. The page outlines plots
  when the id matches, and drops the id when the envelope has none or a
  different one.
- **`svd` no longer panics on a wide matrix, and `pinv` is available.**
  The real SVD built `U` with one column per row, then wrote a column for
  every eigenvalue of `A'A` above `1e-12`. On a matrix with more columns
  than rows that count can exceed the row count, and the write walked
  off `U`. Complex SVD always formed the larger Gram and paired the
  doubled eigenvalues by taking every other one. Both paths now factor
  the smaller Gram (`A Aᴴ` when there are fewer rows), drop singular
  values below `max(m, n) * eps * σ_max` when building the other side,
  and fill the null space with Gram-Schmidt. `svd(A, "econ")` and
  `svd(A, 0)` return the thin factors. `s` is still a vector.
  `pinv(A)` / `pinv(A, tol)` is the Moore-Penrose inverse of those
  factors. `norm(A, 2)` uses the same singular values, including for a
  complex matrix (it used to take the real parts only).
- **`g(idx) = c` works when `g` is the matrix from `zeros(size(v))`.**
  A vector or logical index was applied only when the target was a
  vector. `size` of a vector is `[1, n]`, so `zeros(size(v))` is a
  matrix and the write failed. A single index that is `:`, a numeric
  vector, or a 0/1 mask now writes in column-major order. A scalar
  broadcasts. A vector or matrix on the right must have one value per
  selected element. A `1×N` or `N×1` matrix grows; a general matrix
  does not. `M(i) = row` is unchanged.
- **`plot(x1, y1, x2, y2)` draws both pairs.** Extra numeric arguments
  used to be ignored after a warning. Each `(x, y)` pair is a series,
  and a line spec (`r`, `b--`, `ko`) may follow a pair. `:` and `-.`
  draw dashed. `plot(y1, y2)` is still one pair. A dangling `x`, or a
  non-spec string with more data after it, is an error. The terminal,
  notebook HTML, and the viewer all read the same series list.
- **Commenting on a rendered notebook in `notebook watch` no longer reports that the file changed.**
  The page hashed the notebook after the generated header and output
  sentinels were removed, then `POST /annotate` compared that hash to
  the file on disk. A rendered `.md` (in-place markdown, or any file
  that still has `<!-- Generated by rustlab-notebook … -->`) answered
  every highlight, comment, and delete with "This notebook changed.
  Reload and try again." and wrote nothing. The page hash and the
  comment offsets are now the bytes annotate reads and writes. A
  second comment after a delete lands. Code fences are still not
  modified. The SVG-image check on that write also no longer panics
  on the em dash in the generated-header comment.
- **`rustlab-notebook strip` on a terminal says the file was not modified.**
  With neither `--in-place` nor `-o`, cleaned Markdown is written to
  stdout and the input file is left unchanged. That is the same as
  before. A terminal now prints that fact on stderr, because `clean`
  (in place, rendered artifacts only) was the command the docs called
  "strip", and a `strip` run looked like it had ignored `%%` comments.
  `strip note.md --in-place` rewrites the file. `clean` still does not
  remove comments. There is no dry-run flag.
- **Same-named notebooks in different folders stay separate in `notebook watch`.**
  The file browser lists every `intro.md`. Saving or annotating one
  does not write the other. Plot files and live reload follow the
  notebook you have open. Previously the internal id was the file
  stem (`intro`, then `intro-2`), so a short URL, a case-only
  difference (`Intro.md` / `intro.md`), and every map keyed by that
  id could address the wrong file.
- **A lambda copies only the names its body uses.** `f = @(x) x + a`
  used to clone the entire workspace into the closure. The next lambda
  then cloned that closure, so lambdas created in a loop, nested, or
  kept side by side copied a large unreferenced matrix again and again
  (eight closures beside a 128×128 matrix held 255 copies of it). The
  closure now keeps the free variables of the body, by value, at the
  moment it is created. A later write to one of those names still does
  not change the lambda. `v(end)` inside a lambda is still the last
  element at the call. `feval` of a name that is not a string literal,
  and a call through a handle whose target is `feval`, still copy the
  whole workspace, because that name is not in the syntax. A name the
  body actually mentions is still copied in full — there is no
  copy-on-write — so mentioning a large matrix still copies that
  matrix once per lambda. `@name` does not also copy a variable of
  the same name; the handle calls the function (see the breaking note
  above).
- **Two-index matrix assignment honors `end` on each axis.**
  `M(end, end) = v`, `M(end, :) = row`, and `M(:, end) = col` (and the
  same forms on a sparse matrix) used to bind `end` to 0 and reject the
  write. Reads already bound `end` per axis. A single index is unchanged:
  `M(end)` is still the last column-major element.
- **A `%%` note cannot inject TeX into a PDF, and comment writes do not follow a swapped path.**
  A note that contains `$...$` is paired, so the math is part of the
  note. With comments off (the PDF default) the note is deleted. With
  `--comments` the body is escaped text, not math. `pdflatex` is passed
  `-no-shell-escape`; that flag is not treated as enabling shell-escape.
  A `javascript:`, `data:`, `vbscript:`, or `blob:` link is plain text
  in the PDF, not an `\href`. `strip --in-place` does not follow
  symlinks. `POST /save`, cell save, and `POST /annotate` (including
  undo) write a temporary file in the canonical target's directory and
  `rename` it over that path. Annotate rejects a comment body that
  contains `\` or `$`, a notebook larger than 1 MiB, and a file that
  would have more than 400 comments.
- **An SVG prose image served by `notebook watch` cannot script the watch origin.**
  `GET /plots/…` (including `image/svg+xml`) and `GET /raw/{slug}` send
  `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; sandbox`
  and `X-Content-Type-Options: nosniff`. `sandbox` does not allow
  scripts, so the response has an opaque origin. Opening the SVG URL,
  or loading it with `<object>`, `<embed>`, or `<iframe>`, cannot
  `fetch` `POST /save` as the watch page. The HTML page keeps its nonce
  policy. Vendored `/assets` get `nosniff` only. Prose SVGs are still
  drawn with `<img>`, which does not run SVG script;
  `Content-Disposition: attachment` is not set.
- **Viewer `surf` stays opaque while you rotate it.** The surface is one
  sheet. The viewer used to drop every triangle whose screen winding was
  not positive, and egui has no depth buffer, so the far side of a curve
  (and the entire sheet when looking from below, including straight up
  through the bottom pole) became a hole onto the background. Azimuth
  just moved that hole. Both sides are now rasterized into a depth
  buffer: nearer depth wins, NaN cells stay holes, and the orbit is not
  clamped short of ±90° elevation. Ctrl/Cmd+left-drag rolls about the
  view axis. SVG/PNG surfaces are still the static isometric wireframe.

### Added
- **Notebook widgets can sit beside the plot they drive.** A
  `rustlab-widget` fence immediately above a rustlab cell (blank lines
  and cell directives only) becomes that cell's control panel: one card,
  controls in a 16rem column, the plot on the right, stacked above the
  plot under 800px. A heading, paragraph, or callout between them keeps
  the full-width bar, which gains a `drives: Figure N` link to each plot
  the cell that read the widget produced. Re-run scope is unchanged.
  Batch HTML shows the same panel or bar with the controls disabled and
  the note `default values — open with notebook watch to interact`.
  Markdown, LaTeX, PDF, and JSON still show label = value. A slider may
  set `run = "release"` to send its value when the thumb is released;
  `run` on a number or option, and any unknown key, is a widget error.
  Example: `examples/notebooks/widget_panel.md`. `widgets_demo.md` stays
  bars.
- **`pinv`, `cumtrapz`, `fzero`, `fliplr`, `nextpow2`, `diff`, 1-D `gradient`, and `rng`.**
  `gradient` of a vector (or a `1×N` / `N×1` matrix) is the 1-D MATLAB
  gradient and keeps that orientation. A true matrix is still the 2-D
  `[Fx, Fy]` tuple. `rng` is the same generator as `seed`; `rng(s)`
  restarts at the seed and does not resume mid-stream. `fzero` takes a
  lambda or a function handle.
- **`table` — named columns that share a row count.**
  `T = table(a, b)` names each column from the variable (`Var1`, `Var2`,
  … for an expression). `'VariableNames', {"x","y"}` renames them; the
  option name has to be a string literal. A matrix is one numeric column
  per matrix column (`A` → `A1`, `A2` when it has more than one column).
  A rustlab vector stays one column. A string or string array is one
  text column. A scalar `true`/`false` is a one-row logical column; a
  0/1 vector stays numeric, because there is no logical array.
  `T.x` reads a column. `T.x = v` replaces or appends one, and the
  length must match the height (an empty table takes the new length).
  `T(rows, vars)` returns a sub-table. `T{rows, vars}` returns the cells
  of exactly one variable. `T.Properties.VariableNames`, `height`,
  `width`, `size`, `head`, and `tail` are the size helpers. The REPL
  prints an aligned text table. A notebook HTML page inserts a
  `<table class="rl-table">` colored with the page theme; LaTeX,
  Markdown, and JSON keep the text form.
  Not included: row names, writing `Properties`, `T(rows, vars) =`,
  a multi-column `T{...}` (that is a comma-separated list), joins,
  `sortrows`, `summary`, `addvars` / `removevars` / `renamevars`,
  nested tables, categoricals, datetimes, `readtable` / `writetable`
  / `table2array`, and `T(rows)` with one subscript (`T(rows, :)`).
- **The notebook comment column is narrower, and the reader can resize it.**
  HTML and `notebook watch` reserve 14rem (`--rl-cm-col`) beside the
  prose. A handle on the column's left edge drags the width with the
  pointer or the arrow keys, from 10rem to 40% of the viewport.
  Double-click restores 14rem. The width is kept in `localStorage`
  for that browser. Collapse leaves a strip of the numbered markers.
  Under 800px the cards are still the drawer at the bottom of the
  section, and turning Comments off still drops the column.
- **Notebook highlights and comments.** Prose `==highlight==` and
  `%%comment%%` (optional `#cN`, `re #cN`, `@name`, and a date) render
  as `<mark>` and margin notes in HTML, JSON, and `notebook watch`.
  A whole-line `%%` immediately before a fence comments on that cell.
  Marks inside code fences, inline code, and math stay literal.
  `--comments` / `--no-comments` (and `~/.rustlabrc` `[notebook] comments`)
  control display. HTML and watch default to on, with an in-page
  Comments checkbox. PDF and LaTeX default to off; `--comments` uses
  `\hl` and a margin note. `notebook check` warns W006–W012, including
  a missing `#cN` (W012). `watch --annotate` (implied by `--editable`)
  mounts `POST /annotate/{slug}`: loopback Host and Origin, the file
  jail, and `If-Match` against the page's source hash (409, no write,
  when the file changed). Design: `dev/plans/notebook_comments.md`.
  A per-comment reviewed/accepted flag is reserved and not implemented.
  Replies render inside the parent card. Comment ids stay on
  `data-cm-id` and the card tooltip, not in the header. Annotating
  stamps source offsets on parsed text, so headings and other block
  syntax match a page that is not annotating. Every comment card
  (inline, block, cell, and replies) sits in a right margin column
  aligned with its numbered marker. The column stays in a directory
  page with the file browser and the contents sidebar; the prose
  shrinks. Turning Comments off, or `--no-comments` on static HTML,
  drops the column. Under 800px the cards are a drawer at the bottom
  of the section. PDF `--comments` puts block and cell notes in
  `\marginpar` as well (a footnote inside a table). The annotate
  popover is a card (Name, comment, Cancel / Highlight only / Save
  comment). Ctrl or Cmd+Enter saves; Esc cancels. `render -f markdown
  --comments-style=footnotes` turns notes into GitHub footnotes and
  highlights into `<mark>`. `--comments-style=callouts` turns notes
  into `> [!note]` and leaves `==` (Obsidian's preview hides a raw
  `%%`; the callout style is how those notes stay visible there).
  `rustlab-notebook strip` writes the `--no-comments` markdown
  cleanup and leaves every code fence unchanged. `POST /annotate` returns the new source hash and rejects
  any write whose offsets fall inside a code fence (400, file
  unchanged). A cell comment is only a `%%` line above the fence.
  Highlight is not offered on a code cell. The page offers Undo for
  the last annotate write in this `watch` process; git history is
  the archive. Cards share one full-width column. Markers on the
  same line stack, the first of them level with the line. A note
  taller than about six lines clamps until **more**. A card pushed
  below its marker draws a connector. The source pane narrows so it
  does not cover that column. PDF `--comments` numbers each note to
  match the HTML marker, highlights with a light yellow `\hl`, indents
  replies, and keeps the note column on the page (`marginfix`, right
  margin 2.15in, note column 1.7in).
- **Viewer windows expand.** The window opens at about 75% of the
  monitor (at least 800×600 when the screen is larger, and never larger
  than the monitor), is resizable, has a minimum size, and has a
  maximize button. The plot uses the space the window gives it, including
  on a HiDPI display. `F11` toggles fullscreen. Double-click on a 3D
  surface toggles fullscreen; double-click on a 2D plot still resets
  that subplot. A figure with more than one panel has an **Expand**
  button (double-click the panel title does the same) that shows that
  subplot alone; `Esc` returns to the grid. When several figures are
  open, **Fill window** expands one of them to the viewer window and
  `Esc` restores the floating window.
- **Directory file-browser folders start collapsed.** Every folder
  disclosure is closed until you open it. The folders that contain the
  notebook on the page stay open, so the current file stays visible.
  The index has no current notebook, so every folder starts closed
  there. The root disclosure stays open. Clicking a file no longer
  reloads the page with every folder expanded; clicking a folder
  summary still opens or closes only that folder. Static HTML and
  `notebook watch` share this markup.

## 0.3.8

### Breaking / behavior changes
- **Directory `notebook watch` pages use the collection path.**
  `ch2/filters.md` is `http://127.0.0.1:<port>/n/ch2/filters` (the `.md`
  stripped). An old `/n/<stem>` URL, including a `-N` collision suffix,
  redirects to that path. Two files named `filters.md` are
  `/n/ch2/filters` and `/n/ch3/filters`. A single-file `watch` stays at
  `/n/<stem>`. Static HTML links are unchanged. WebSocket, `/raw/`, and
  `/save/` still use an internal id; the page stamps it in
  `<meta name="rl-slug">`. Migration: bookmarks of `/n/<stem>` for a
  nested notebook follow the redirect.
- **The directory file browser shows file names.** Each row is the file
  name (`filters.md`) inside its folder. The notebook title and the full
  collection-relative path are no longer stacked on the row. Listing
  rules, order, the current-page marker, and native `<details>` collapse
  are unchanged.
- **A fenced code block with no language tag renders as `text`.** It uses
  the same boxed, uncolored panel as an explicit ` ```text ` fence in
  HTML, `notebook watch`, and PDF, including the `text` label. A tag this
  renderer does not highlight (`javascript`, `sh`) is unchanged, and an
  indented code block is still not a fence.

## 0.3.7

All entries below shipped under no released version number before this
one. Note for 0.3.6 pinners: the 2026-07-11 binaries reported 0.3.6 but
already contained the `fft` change and others below; 0.3.7 is the first
version where the number and the behavior match again (see AGENTS.md
Workflow Rule 12).

### Breaking / behavior changes
- **`rustlab-notebook watch` always opens the browser** unless
  `--no-browser` is passed or `CI` is set. The old rule opened only when
  stderr was a TTY, so IDE, launcher, and piped launches never opened.
  There is no opener override; the OS default browser is used. Openers
  now cover macOS (`open`), Windows (`cmd /c start`, then PowerShell
  `Start-Process`), WSL (`wslview`, then `cmd.exe` / `explorer.exe`, then
  the Linux list), and Linux (`xdg-open`, `gio open`, `sensible-browser`,
  `x-www-browser`). Each is waited on for at most about a second, so a
  browser binary that keeps running no longer blocks the server from
  starting. Migration: scripts that run `watch` in the background and do
  not want a window pass `--no-browser`.
- **LaTeX and PDF are always Catppuccin Latte on white paper.** `-t` and
  `~/.rustlabrc` `[notebook] theme` still theme HTML and `notebook watch`.
  They no longer paint a dark PDF page. Body text is `#4c4f69`; headings
  are unnumbered and colored (H1 mauve, H2 blue, H3 teal); the title has
  no date. Source, printed output, and errors are breakable panels with
  a mauve left rule and a small `rustlab` label above the source (omitted
  when `<!-- hide -->` hides it). Callouts and exercises are cards; a
  solution is printed, not collapsed. `<!-- grid: N -->` places plots in
  a row. Mermaid figures stay with their heading. Migration: do not pass
  `-t dark` expecting a dark PDF; install a normal TeX Live (the new
  packages are `tcolorbox`, `fancyvrb`, `sectsty`, `float`, `lmodern`,
  and `xcolor`'s `table` option). Shell-escape is still not used.
- **PDF compile no longer enables TeX shell-escape.** Plot SVGs are
  converted to PDF via fixed-argv Inkscape before `pdflatex`/`tectonic`
  runs; `\includegraphics` replaces `\includesvg`/`svg.sty`. Inkscape is
  required whenever a notebook has SVG plots. Migration: install Inkscape;
  do not rely on TeX packages that need `-shell-escape`.
- **`notebook watch` checks loopback Origin and Host.** `POST /save`
  and WebSocket upgrades require an `Origin` of
  `http://127.0.0.1:<port>`, `http://localhost:<port>`, or
  `http://[::1]:<port>` (a missing `Origin` is 403). Every request
  must present a matching loopback `Host` or it is rejected. There is
  no session token. Other processes on the same machine can still
  reach the loopback port. See `docs/security.md`.
- **Notebook file I/O is jailed.** Embeds, `run` / `load` / `save` /
  `savefig` / `saveanim` / `figure("….html")` must resolve under the
  jail root or error with `path escapes notebook directory`. The root is
  the notebook's own directory for a single-file `render` / `watch`, the
  collection root for a directory (nested notebooks may read
  `../data/`), or `--jail-root <DIR>` on either command. `sub/../x` is
  fine when it stays inside; absolute paths outside the root (including
  `/tmp`) are rejected. REPL and `rustlab run` are unchanged.
- **Raw HTML in notebook prose is sanitised in HTML output.** Only
  attribute-free formatting tags (`<b>`, `<br>`, `<sub>`, `<kbd>`,
  `<details>`/`<summary>`, `<div>`, table tags, …) pass through; any tag
  with an attribute and every other tag (`<script>`, `<iframe>`,
  `<a>`, …) renders as escaped text, and HTML comments are dropped.
  One exception: an `<img>` whose only attributes are a quoted `src`
  and an optional quoted `alt` is embedded (see Fixed). Extra
  attributes, including `on*` handlers, stay escaped. `javascript:` /
  `data:` / `vbscript:` / `blob:` links are neutralised; images keep
  `data:image/*` only. Migration: write links in markdown; use
  `> [!NOTE]` callouts and the `<!-- details: -->` directive instead of
  attributed HTML. Markdown output (`-f markdown`) is unaffected.
- **Single-output `svd` returns the singular values.** `s = svd(A)`
  now binds the singular-value vector (descending) — previously it
  bound the entire `(U, σ, V)` tuple, which was unusable as a single
  value (`size(s)` errored). `[U, S, V] = svd(A)` is unchanged.
  Migration: code that relied on the tuple binding should destructure
  explicitly.
- **`fft(x)` is now length-preserving.** It returns exactly `length(x)`
  bins instead of silently zero-padding to the next power of two;
  non-power-of-two lengths use a hand-rolled Bluestein (chirp-z) transform
  over the existing radix-2 kernel, and `ifft(X)` now accepts any length
  (previously a hard error off powers of two). New optional size argument:
  `fft(x, n)` / `ifft(X, n)` zero-pad or truncate to exactly `n` first.
  Migration: the axis idiom `f = fftfreq(length(X), fs)` is now always
  correct; scripts that relied on the implicit padding must request it
  explicitly, e.g. `fft(x, 1024)`. Windowed-frame estimators (`pwelch`,
  `stft`, `spectrogram`, `waterfall`, and their streaming forms)
  intentionally keep rounding their explicit `nfft` argument up to a power
  of two, as documented.

### Added
- Notebook prose fences tagged `bash`, `python`, or `text` use the same
  panel as a rustlab cell in HTML, `notebook watch`, and PDF. `bash` and
  `python` are server-side highlighted (comments, strings, keywords,
  numbers) with the rustlab token colors and a small language label.
  `text` stays uncolored on the quieter printed-output background, which
  also covers cell stdout the markdown renderer already writes as a
  `text` fence. Other fence tags are unchanged.
- Directory `rustlab-notebook watch` and the matching static directory
  HTML show a collapsible file browser on the index and on every notebook
  page. Rows are grouped by folder and show the notebook title plus the
  collection-relative path. The in-page heading list, index breadcrumb,
  and prev/next stay. Single-file watch/render is unchanged. LaTeX and
  PDF do not get the browser. Collapse is native `<details>` (no inline
  event handlers).
- `rustlab remote <host>` (in `--features viewer` builds, which `make`
  install` produces): run rustlab on another machine with plots in the
  local `rustlab-viewer`. Checks a viewer is listening, then runs `ssh -t
  -o ExitOnForwardFailure=yes -R <remote>:<local>` with a fresh
  per-session socket name; the remote script exports `RUSTLAB_VIEWER_SOCK`
  and removes the socket when the command exits, so a stale file never
  blocks the next session and sessions never collide. No probe of the
  remote. `--print` shows the command instead of running it. `rustlab
  repl --viewer [--viewer-name NAME]` connects at startup and wins over
  `[viewer] auto_connect` / `name` in `~/.rustlabrc`; both go through one
  connect routine. The rc `[viewer] name` is ignored whenever
  `RUSTLAB_VIEWER_SOCK` is set (named sessions bypass that variable, so
  honouring it would defeat a forward); this applies to `run --plot
  viewer` too. Failed viewer connections now name the socket path they
  tried. Guide: `docs/remote-viewer.md`.
- Notebook themes: named Catppuccin builtins `mocha`, `macchiato`,
  `frappe`, `latte` via `-t` / `--theme` (aliases `dark`→mocha,
  `light`→latte) and via `~/.rustlabrc` `[notebook] theme` / `[plot]
  theme`, which accept the same names. HTML `color-scheme` follows
  background luminance so custom dark palettes work without matching the
  Mocha static; LaTeX/PDF stay Latte (see Breaking, above). Mapping
  documented in `docs/notebooks.md`.
- Notebook HTML defines `:root { --rl-*: … }` tokens from the resolved
  `ThemeColors` palette. Page CSS uses `var(--rl-…, <literal>)` so
  visuals match the pre-token colors when a variable is missing.
- Notebook `` ```rustlab `` cells are syntax-colored in HTML (including
  `notebook watch` live updates) and in LaTeX/PDF. Highlighting follows
  the rustlab lexer (`#` and `%` comments, keywords, numbers, strings,
  operators, call-like names). HTML uses the active Catppuccin theme;
  LaTeX/PDF always uses Latte (see Breaking, above).
  PDF color is `\textcolor` with escaped tokens (not `minted`). Markdown
  export still emits plain `` ```rustlab `` fences. Printed output is
  not highlighted. In HTML and `notebook watch` the source, printed
  output, and errors share one indented block with a theme-accent left
  rule. In LaTeX/PDF they are separate breakable panels; the accent
  rule is the panel's left edge and continues when a panel breaks
  across pages. In HTML and
  `notebook watch` the source alone is an open disclosure (a `rustlab`
  summary); collapsing it leaves output, errors, and plots visible.
  LaTeX/PDF always shows the source expanded. Plots and
  animations stay full width. The disclosure's initial state is open
  unless a cell `<!-- code: collapsed -->`, notebook frontmatter
  `code: collapsed`, or `~/.rustlabrc` `[notebook] code = "collapsed"`
  says otherwise (most specific wins; `<!-- code: open -->` forces
  open). `<!-- hide -->` still removes the source. An unrecognised
  value warns (`notebook check` W005, or once on stderr for the rc
  key) and falls back to the next level. `notebook watch` keeps a
  disclosure the reader has toggled; cells they have not touched pick
  up a changed directive or frontmatter on re-render. LaTeX/PDF ignore
  the setting.
- Optional user-global settings file. rustlab reads
  `$XDG_CONFIG_HOME/rustlab/config.toml` if it exists, else `~/.rustlabrc`,
  else built-in defaults. The file is declarative TOML (never executed).
  v1 keys: `[display] format`, `[plot] theme` / `default_axis`,
  `[notebook] theme` / `code`, `[repl] history_limit`, `[viewer] auto_connect` /
  `name`. Precedence: CLI flags > in-script / REPL commands > rc >
  defaults. Unknown keys warn once; invalid values abort with path + key,
  except `[notebook] code` (warns once and falls back to open).
  Example: `docs/rustlabrc.example.toml`. REPL: `help rustlabrc`.
- Security hardening for notebooks / watch / PDF / viewer (see
  `docs/security.md`): CSP on watch pages with the nonce stamped at
  render time on rustlab's own script tags (no inline event handlers
  remain in rendered pages), HTML-escaped math restore, raw-HTML
  allow-list and dangerous URL scheme stripping (index page included),
  viewer Unix socket mode `0600`, and a 32 MiB IPC frame size cap.
- `rustlab-notebook render` / `watch` gained `--jail-root <DIR>` to
  widen the notebook file-I/O jail. Inkscape conversion failures now
  report the tail of Inkscape's stderr.
- Plot color names now include `gray`/`grey` and hex `"#RRGGBB"`
  everywhere a color string is accepted (`plot(..., "color", c)`,
  `hline`/`yline`, contour/quiver/streamplot color args).
- Plot argument validation is no longer silent: an unrecognized color
  name in a dedicated color slot (`hline(y, "dashed")`,
  `plot(..., "color", "chartreuse")`) prints a one-line stderr warning
  naming the accepted colors, and `heatmap`/`imagesc`/`bar` warn once
  per call when NaN/Inf values flow into plot data (renderers already
  handled them defensively, but silently — broken figures shipped with
  no hint at render time).
- `tic` / `toc` wall-clock stopwatch (bare or with parentheses): `tic`
  starts/restarts, `toc` returns elapsed seconds without clearing so
  repeated calls take split times; `toc` before any `tic` is an error.
  Thread-local (a `tic` inside a `parmap` worker times that worker).
- `ellipke(m)` — complete elliptic integrals K(m) and E(m) via the
  arithmetic-geometric mean. Parameter convention `m = k²`; domain
  [0, 1] with the exact limits `ellipke(1) → (Inf, 1)`; elementwise
  over vectors/matrices; `[K, E] = ellipke(m)` returns both.
- `pin_dirichlet(A, b, mask_or_indices, values) → [A, b]` — enforce
  Dirichlet boundary values on a linear system: pinned rows of `A`
  become identity rows and `b` gets the pinned values, so
  `spsolve(A, b)` reproduces the boundary potential exactly. Accepts a
  grid mask (column-major, matching `ij2k`/`ijk2k` and the
  `laplacian_*` builders) or a 1-based index list; sparse or dense
  square `A` (the sparse ordering hint survives).
- `trapz(M)` / `trapz(x, M)` — trapezoidal integration now handles
  matrices per column, returning a 1×ncols row (1-D-shaped inputs keep
  returning a scalar). Double integrals are two calls:
  `trapz(ys, trapz(xs, F))`.
- `quiver(..., "normalized")` — direction-only arrow plots: every
  vector is drawn at unit length × scale.

### Changed (plotting)
- **quiver auto-scaling is outlier-robust.** The auto-scale now keys on
  the 95th percentile of the field's nonzero magnitudes (was: the single
  longest arrow) and clamps outliers to one grid cell at draw time — a
  near-singular sample (e.g. a Biot-Savart field evaluated on the wire)
  no longer shrinks every other arrow to invisibility. Uniform fields
  are unchanged. Decimate dense grids with stride indexing
  (`U(1:5:end, 1:5:end)`), documented in `docs/functions.md`.
- **"not rendered to the terminal" warnings are deferred and
  savefig-aware.** Scripted runs that render vector plots and then save
  them (`quiver(...); savefig(...)`) no longer emit stderr noise; a plot
  that never reaches a file warns once, at the end of the run (or REPL
  line), as one combined message naming the plot kinds.
- **The viewer zooms with the plain scroll wheel, and every subplot has
  a Home button.** In `rustlab-viewer`, rolling the wheel over a 2-D
  panel now zooms both axes about the pointer (it used to pan the view;
  zoom was ctrl+wheel only). Left-drag still pans. Each subplot carries
  its own **Home** button in its header — plus the `Home` key while the
  panel is hovered, and double-click — which restores the script's
  `xlim`/`ylim` when it set any and otherwise re-fits the data. 3-D
  `surf` panels get the same Home button and `Home` key as an alias for
  the existing `R` reset; their scroll zoom and shift+scroll Z-scale are
  unchanged.

### Fixed
- **Theme fonts stay readable on the surface they sit on.** Notebook
  HTML, `notebook watch` (page, sidebar, code and output panels, and
  the CodeMirror source and cell editors), and Latte-on-white PDF /
  LaTeX no longer pair a font with a background below WCAG AA 4.5:1.
  Backgrounds are unchanged. Roles that already cleared 4.5 are
  unchanged. What moved, and why:
  - Mocha and Macchiato: footer (`#585b70` → `#81859c`, `#5b6078` →
    `#898ea5`; surface2 on the page was ~2.4:1) and code comments
    (`#6c7086` → `#787c92`, `#6e738d` → `#7c8199`; overlay0 on the code
    panel was ~3.8:1).
  - Frappé: dim text (`#a5adce` → `#aab2d1`, 4.26:1 on the border),
    footer (`#626880` → `#979caf`), mauve and blue accents
    (`#ca9ee6` → `#cda4e7`, `#8caaee` → `#98b3f0`; they missed 4.5 on
    the lighter border), and comments (`#737994` → `#868ca3`). Keyword
    and function tokens stay the stock swatches — they only sit on the
    code panel, which those swatches already clear — so they no longer
    share a hex with the accents.
  - Latte, including white PDF paper: dim text (`#6c6f85` → `#56586a`),
    footer (`#9ca0b0` → `#686d82`), headings and links
    (`#8839ef` → `#7113ec`, `#1e66f5` → `#094dd3`, `#179299` → `#12747a`;
    the old link blue was ~4.3:1 on the page and ~3.2:1 on the border),
    error red (`#d20f39` → `#c60e36`; 4.46:1 on the sidebar and 4.10:1
    on the code panel), and every syntax token. Stock peach `#fe640b`
    was ~2.3:1 on the code panel and ~3.0:1 on white; it is now
    `#ad4001`. Code-panel tokens are a smaller darkening than the
    headings of the same hue, because the code panel is lighter than
    the border.
  - CodeMirror in `notebook watch` no longer uses its default dark blue
    (`#00f` / `#00c`, unreadable on a dark code panel) or yellow-gray
    brackets (`#997`, ~2.2:1 on the Latte code panel) or the yellow
    search wash (`#ffa`, which made light theme text unreadable). Those
    classes take the theme token colors; a search match is an underline
    on the code background.
  The plot viewer's egui chrome and 3D axis labels already cleared 4.5
  on their dark panels and are unchanged. Plot series colors are
  strokes, not theme fonts.
- **HTML and `notebook watch` prose figures.** Markdown images and a
  safe raw `<img src alt>` are copied into the plot directory after the
  path-jail check and referenced from that copy (`plots/<stem>/prose-N.ext`
  beside a static HTML file, `/plots/<slug>/prose-N.ext` on the watch
  server). A relative `src` used to be resolved against the page URL, so
  in watch the browser requested `/n/<slug>/dot.png` and the figure was a
  broken image. Percent-encoded names are decoded. A path outside the
  jail is a placeholder that does not repeat the outside path, and the
  file is not served. Unsafe `<img>` tags stay escaped. Watch CSP is
  unchanged (`img-src 'self' data: blob:`; script nonce and
  `'strict-dynamic'`). Remote URLs are not fetched.
- **PDF prose figures, wide tables, and in-page links.**
  Markdown images and a safe raw `<img src alt>` in prose are copied
  into the plot directory (path-jailed; png/jpg/jpeg/svg) and included
  in PDF / LaTeX, scaled to the line width. gif/webp, remote URLs, and
  missing files become a visible placeholder and a stderr warning
  instead of a broken `\includegraphics`. PDF tables use `tabularx`
  fitted to `\linewidth` so a wide grid wraps inside the margins.
  Same-page `#heading` links emit `\hyperref` only when that page has
  a matching `\hypertarget`; a fragment with no target is plain text.
  A link to another notebook's PDF still drops its fragment. Wikilink
  fragments use the same heading slug as those targets (the shared
  preprocessor, so HTML wikilink fragments change to that slug too).
- **PDF builds no longer fail on Unicode in prose.** When `pdflatex`
  rejects a character the preamble does not declare (`2ⁿ`, `Aᵀ`, `ħ`, an
  emoji), `render -f pdf` recompiles with a fallback per rejected
  character: sub/superscript and modifier letters, Greek, and common math
  and arrow symbols map to the LaTeX macro; anything else prints as
  `[U+XXXX]`. A stderr warning lists the characters and placeholders.
- **Directory-mode PDF renders continue past a failing notebook.** The
  PDF compile step used to call `exit(1)` from inside the library, so one
  bad notebook stopped the whole `render <dir> -f pdf` build with nothing
  after it produced. Each failure is now reported (with its `<stem>.log`),
  the remaining notebooks render, and the command exits 1 at the end.
- **`notebook watch` renders KaTeX math again.** The watch
  Content-Security-Policy (`script-src` nonce + `'strict-dynamic'`)
  blocks inline event handlers, so the auto-render `onload` and the
  sidebar `onclick` never ran and display math stayed as raw `\[…\]`.
  Both now run from nonce'd scripts. The invalid `connect-src` token
  `ws://[::1]:*` is gone.
- **Clicking a notebook link in `notebook watch` no longer sticks on
  "disconnected — reconnecting…".** The WebSocket client read
  `window.__RL_TOKEN` before that assignment ran, and cross-notebook
  links do not carry `?token=`, so the upgrade returned 401 and the
  retry used the same empty secret. The token is gone; loopback
  Origin and Host checks remain (see the behavior note above).
- **Zooming a viewer panel with script limits no longer snaps back.**
  The viewer re-applied a panel's `xlim`/`ylim` on every frame, so any
  zoom or pan of a panel whose script had called `plot_limits` (or
  `xlim`/`ylim`) was undone on the next repaint. Limits are now applied
  on the panel's first show, when *changed* limits arrive from the
  script, and on Home — so a live plot re-sending the same limits each
  redraw leaves the user's view alone.
- Dark-mode notebook HTML no longer leaves prose links unstyled. Body,
  callout, and exercise `<a>` tags inherit the browser default
  (`#0000EE` / visited `#551A8B`) which is invisible on Catppuccin
  Mocha (`#1e1e2e`). They now use the theme accents (`#89b4fa` /
  `#cba6f7` in dark, the Latte blues/purples in light), with
  `color-scheme` set on `<html>` so UA chrome matches the page. The
  index intro links and the `--editable` CodeMirror `.cm-link` token
  get the same treatment.
- `notebook watch` (and directory `render`) now resolve
  collection-root paths and unique basenames from nested notebooks.
  `[x](ch2/notes.md)` and `[[ch2/notes]]` written from `ch1/` used to
  stay as `.md` hrefs — the watch server has no `*.md` route, so
  clicks 404'd. Lookup is still page-relative first; `./` and `../`
  spellings do not take the fallback; two files sharing a basename
  stay unresolved rather than picking a winner. Static HTML emits the
  climbed path (`../ch2/notes.html`) for a fallback hit.
- Bare `figure()` and `histogram(v)` / `hist(v)` statements no longer
  echo their return values (a meaningless figure-handle integer above
  every plot in notebook output — churning with global figure count
  across a directory render — and a 2×n bin matrix, respectively).
  Statement-position builtin calls now carry `nargout = 0`; assigned
  forms (`h = figure()`, `b = histogram(v)`) still return their values.
  Other builtins are unaffected.
- `heatmap`, `imagesc`, `contour`/`contourf`, and `image` (grayscale /
  colormap modes) now color by **signed** value instead of magnitude.
  Previously complex-to-real collapse used |v| at ingest, so any signed
  matrix rendered wrong: `[-2, -1; 1, 3]` showed −2 at mid-scale and −1
  identical to +1, and large-magnitude negatives rendered *hot*. All
  static paths now match the live-viewer path (real part, `.re`). For
  genuinely complex inputs this means the real part is displayed —
  take `abs(Z)` explicitly to plot magnitudes. Spectrogram/scalogram dB
  displays are unchanged (magnitude there is intentional).
- Multi-panel `subplot` figures containing heatmaps (`heatmap`/`imagesc`)
  or 3-D surfaces (`surf`) now export **all** panels to SVG. Previously
  the file was finalized after the first heatmap/surface panel and every
  later panel was silently dropped (line-plot panels were unaffected;
  PNG was unaffected). Captured notebook figures had the same defect.
- `cache` is no longer a reserved word. The 0.3.6 cache statement had
  silently reserved the lowercase identifier `cache`, breaking scripts
  that use it as a variable (`cache = 5`, `function [y, cache] = f(x)`).
  It is now a soft keyword: `cache <subcommand>` / `cache "path"` /
  `cache path.rcache` still parse as cache statements, and every other
  use is an ordinary identifier. One corner changed: a bare `cache` line
  is now a variable reference (previously a parse error asking for a
  subcommand).
- `rustlab-viewer` no longer burns CPU while idle (~10% of a core on WSLg,
  where every frame goes through RDP compositing or software GL). The GUI
  previously repainted at ~60 fps around the clock to poll for socket
  messages; repaints are now event-driven — the socket listener wakes the
  GUI when a message arrives, so an idle viewer draws no frames and plots
  appear with lower latency than the old 16 ms poll.
- `T(:)` on a 3-D tensor no longer panics the interpreter — it flattens
  column-major (the `reshape`/`ijk2k` order); linear indexing `T(k)`,
  index vectors `T(I)`, and `T(end)` now work on tensors too.
- `rustlab run` exits with code 1 when the script fails to parse or
  dies on a runtime error (previously it printed the error but exited
  0, silently passing CI gates). `AudioEof`/`Interrupted` remain
  clean exits.
- `max(a, b)` / `min(a, b)` are now elementwise over any mix of scalars,
  vectors, and matrices, with the same implicit-expansion (broadcast) rules
  as `+`. Previously the two-argument form accepted only two scalars. Per
  element, a `NaN` loses to any non-`NaN` partner. `max(M1, M2)` is the
  canonical union of two 0/1 masks.
- Imaginary numeric literals: `2j`, `1.5i`, `3e8j` parse directly, so
  `z = 1 + 2j` works and printed complex values (`1+2j`) round-trip as
  input. The suffix binds to the literal, so it is immune to a variable
  named `i` or `j` shadowing the builtin constant.

### Changed
- The non-integer index error now suggests a fix: `index 2.5 is invalid
  (must be a positive integer; round a computed index explicitly with
  floor()/round(), or use integer arithmetic)`.
- `rustlab info` prints the version and pointers to `rustlab docs` /
  `rustlab docs --json` and `rustlab-notebook --help`. It no longer
  prints a short DSP feature list. `rustlab --help` stays a subcommand
  list and adds the same two pointers.

### Docs
- Agent-facing docs match the binary. `rustlab run` exits 1 on a lex,
  parse, or runtime failure, including `--profile`. `rustlab docs --json`
  records are `{name, toolbox, subcategory, brief, detail}` (349
  builtins). Toolbox names are the twelve lowercase ids (`rustlab docs
  dsp`); there is no `Plotting` toolbox. `window --plot` and `rustlab
  plot` use the plot library's terminal charts. The gallery has 40
  rendered notebooks and `examples/` has 82 scripts. `bash`, `python`,
  and `text` fences are highlighted. LaTeX and PDF stay Catppuccin Latte
  on white paper, with no `pagecolor`.
- Corrected the physical rationale for the harmonic-mean face coefficients
  in the `laplacian_eps_2d` reference (series composition of half-cell
  fluxes / O(h) accuracy, not flux-conservation).
- Added warnings near the `spsolve` and `transpose` entries: reshaping a
  complex right-hand side with postfix `'` silently conjugates it — use
  `.'` for pure reshaping.

## 0.3.6

### Breaking / behavior changes
- **Non-integer indices are now a hard error.** `v(2.5)` raises
  `index 2.5 is invalid (must be a positive integer)`; earlier versions
  silently floored fractional indices. Scripts that relied on implicit
  flooring must round explicitly — `v(floor(n/2))` — or use integer
  arithmetic, e.g. `(n + 1) / 2` for the midpoint of an odd-length `n`.
  Landed in PR #24 alongside logical-mask indexing (`v(v > 2)`) and
  element-wise comparison fixes.
