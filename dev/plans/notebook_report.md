# Development Plan: rustlab-notebook

**Binary:** `rustlab-notebook`  
**Input:** Standard `.md` files with ` ```rustlab ` fenced code blocks  
**Output:** Self-contained HTML reports (and eventually more)  
**Current phase:** complete through Phase 6  
**Status:** Phases 1–6 complete; Future phases (live server, editable notebooks) not started  

---

## Overview

A separate binary (`rustlab-notebook`) that takes Markdown files, executes
any ` ```rustlab ` fenced code blocks through the evaluator, captures text
output and plots, and renders everything into a self-contained HTML report.

**Why a separate binary:** The main `rustlab` binary is targeting real-time
DSP operations. Rendering concerns (markdown parsing, HTML templating,
formula rendering, syntax highlighting) should not add dependencies or code
to that binary. Both binaries share the evaluator (`rustlab-script`) and
plot infrastructure (`rustlab-plot`) as library crates.

**Why `.md` files:** Standard Markdown is already rendered by Obsidian,
VS Code, GitHub, and every other documentation tool. Users get formula
support ($...$), syntax-highlighted code blocks, and readable prose without
installing anything. The `.md` is the source of truth; the HTML is the
build artifact.

### Vision

A lightweight notebook environment for DSP/engineering work. A project
directory might look like:

```
my-analysis/
  config.toml          — parameters, filter specs, thresholds
  data/
    measurements.csv   — raw data files
    reference.npy
  notebooks/
    overview.md        — project summary, links to other notebooks
    filter_design.md   — filter analysis with interactive plots
    validation.md      — test results against reference data
  scripts/
    preprocess.rlab       — standalone rustlab scripts
```

`rustlab-notebook` renders the `.md` files into interactive HTML reports.
Code blocks can load the TOML config, read data files, and produce plots —
all from standard rustlab code. The notebooks, config, data, and scripts
all live together as a normal project directory.

### Example usage

```
rustlab-notebook render filter_design.md              → filter_design.html
rustlab-notebook render filter_design.md -o report.html
rustlab-notebook render notebooks/                    → render all .md files
```

### Example `.md` input

````markdown
# Filter Analysis Report

## Lowpass FIR Design

We design a 64-tap lowpass filter with cutoff at $0.3\pi$:

```rustlab
h = fir1(64, 0.3);
[H, w] = freqz(h, 1, 512);
clf
plot(w/pi, 20*log10(abs(H)))
title("Magnitude Response (dB)")
xlabel("Normalized Frequency")
grid on
```

The passband ripple is well within spec.
````

---

## Architecture

### New crate: `crates/rustlab-notebook`

```
crates/rustlab-notebook/
  Cargo.toml
  src/
    main.rs       — CLI entry point (argument parsing, orchestration)
    parse.rs      — Markdown → Vec<Block> parser
    execute.rs    — Run code blocks through evaluator, capture output + figures
    render.rs     — Assemble HTML from executed blocks
```

**Dependencies:**
- `rustlab-script` — evaluator for code block execution
- `rustlab-plot` — `render_figure_plotly_div` for plot output
- `pulldown-cmark` — Markdown → HTML (pure Rust, no transitive deps)

**Not a dependency of `rustlab-cli`.** The two binaries are siblings, not
parent-child. They share library crates but neither depends on the other.

### Evaluator setup

`rustlab-notebook` instantiates the evaluator the same way `rustlab run`
does: create an `Evaluator`, which automatically registers all builtins
(math, DSP, plotting, I/O). No special setup needed — the evaluator's
`new()` gives a fully functional environment. Code blocks execute via
`ev.exec_stmt()` with state persisting across blocks within a notebook.

### Data flow

```
.md file
  → parse: Vec<Block>        (Markdown | Code)
  → execute: Vec<Rendered>   (Html | CodeResult { source, output, plot_html })
  → render: String           (single HTML page)
  → write to disk
```

### Frontmatter

Notebooks may optionally begin with YAML frontmatter delimited by `---`:

```markdown
---
title: Filter Analysis
---

# Filter Analysis Report
...
```

The parser strips frontmatter from Phase 1 onward (so it never breaks
rendering), but the contents are ignored until a later phase needs them.
This reserves the frontmatter space for future use (config paths,
parameters, output settings) without breaking existing notebooks.

---

## Phase 1 — Batch Render (one file in, one HTML out)

**Goal:** `rustlab-notebook render analysis.md` produces a working HTML report.

This is the foundation everything else builds on and the largest single
phase — it stands up the crate, parser, evaluator integration, and HTML
renderer all at once. No server, no fancy rendering — just parse, execute,
and produce HTML.

- Create `crates/rustlab-notebook` with binary target
- CLI argument parsing (clap): `rustlab-notebook render <file.md> [-o output.html]`
  - Default output: `<input_stem>.html` in current directory
- Parser: split `.md` on ` ```rustlab ` / ` ``` ` fences
  - Strip optional `---` frontmatter block (discard contents)
  - Everything outside fences → `Block::Markdown`
  - Everything inside ` ```rustlab ``` ` → `Block::Code`
  - Other fenced blocks (```python, etc.) → treated as markdown
- Instantiate `Evaluator` from `rustlab-script`
- Execute code blocks in sequence (variables persist across blocks)
- After each code block, snapshot figure state if it has series data
- Render HTML:
  - Markdown prose → HTML via `pulldown-cmark`
  - Code blocks shown in `<pre><code>` (source only — no output capture yet)
  - Plotly divs for captured figures
  - Dark theme (catppuccin)
  - Plotly JS from CDN
- Errors in code blocks: render inline in red, continue with remaining blocks

**Deliverable:** Can render all three example notebooks in `examples/notebooks/`.

**Files:**
- New: `crates/rustlab-notebook/` (entire crate)
- Edit: `Cargo.toml` (add to workspace members + workspace dependency)

---

## Phase 2 — Output Capture + Code Display

**Goal:** Show computed text output alongside code and plots.

Each code block renders as up to three zones:

1. **Source** — the rustlab code (monospace, styled)
2. **Text output** — `ans =`, `disp()`, printed values (muted `<pre>`)
3. **Plot** — interactive Plotly chart (if figure has data)

Implementation:
- Add a thread-local capture buffer in `rustlab-script` (same pattern as
  `HTML_FIGURE_PATH` in `rustlab-plot`) to intercept evaluator print output
  during block execution. **This is a cross-crate change** — the buffer
  lives in `rustlab-script`, with `start_capture()` / `stop_capture()`
  public API that `rustlab-notebook` calls around each block.
- Blocks with no output or no plot just omit those zones

**Files:**
- Edit: `crates/rustlab-script/src/eval/mod.rs` (add capture buffer)
- Edit: `crates/rustlab-notebook/src/execute.rs` (use capture API)
- Edit: `crates/rustlab-notebook/src/render.rs` (render output zone)

---

## Phase 3 — Formulas (KaTeX)

**Goal:** `$...$` and `$$...$$` render as math in the HTML output.

- Include KaTeX CSS + JS from CDN in `<head>` (same pattern as Plotly)
- Auto-render extension processes `$...$` and `$$...$$` on page load
- No build-time processing — KaTeX renders client-side
- Works for inline ($0.3\pi$) and display ($$H(z) = \sum_{k} h[k] z^{-k}$$)

---

## Phase 4 — Export Formats

**Goal:** Support output formats beyond HTML.

```
rustlab-notebook render analysis.md                   → HTML (default)
rustlab-notebook render analysis.md --format latex     → LaTeX
rustlab-notebook render analysis.md --format pdf       → PDF
```

- **LaTeX export:** New renderer that emits a `.tex` document. Prose maps
  to LaTeX sections/paragraphs. Code blocks to `lstlisting` or `minted`
  environments. Formulas pass through natively (already LaTeX syntax in
  the `.md` source). Plots are the hard part — see below.
- **Static plot images:** Export figures as PNG files alongside the
  document. Options to evaluate:
  - Shell out to a bundled Plotly exporter (requires Node/Kaleido)
  - Render server-side via headless browser
  - Emit SVG from the plot data directly (most self-contained, most work)
  - Decision deferred to implementation time — pick the simplest that works
- **PDF export:** Generate via the LaTeX pipeline (`pdflatex`/`tectonic`)
  or via headless browser rendering the HTML. LaTeX path produces better
  typesetting; browser path is simpler to implement.

This phase is larger than it appears and may need to be split into
sub-phases (e.g., LaTeX-without-plots first, then static images, then PDF).

---

## Phase 5 — Polish

**Goal:** Make reports look great.

- Syntax highlighting in code blocks (rustlab keyword coloring)
- Navigation sidebar auto-generated from `#` headings
- Table of contents
- Responsive layout
- Block directives: `<!-- hide -->` to suppress code display for setup blocks

---

## Phase 6 — Multi-Notebook Projects ✓ COMPLETE

**Goal:** Render a directory of notebooks into a linked report site.

```
rustlab-notebook render notebooks/           → notebooks/*.html + index.html
```

- ✓ Render all `.md` files in a directory
- ✓ Generate an index page with links to each notebook (catppuccin dark theme)
- ✓ Cross-notebook links (`[see filter design](filter_design.md)`) resolve
  to the rendered HTML equivalents (`.md` → `.html` rewriting)
- ✓ Each notebook has its own independent evaluator state
- A `<!-- include: setup.rlab -->` directive could run shared setup code
  before the notebook's own blocks (deferred to future)

---

## Future — Live Server + Editable Notebooks

These features are out of scope for the initial development phases but
captured here for future consideration.

**Live server:** `rustlab-notebook serve analysis.md` — render, start a
local HTTP server, watch for file changes, auto-reload browser via SSE.
Deps: `tiny_http`, `notify`.

**Editable server:** Browser UI with editable code/markdown cells that
save back to the `.md` file on disk. Lightweight Jupyter-style workflow
where the `.md` remains the source of truth.

---

## Open Questions

- **Data loading:** Code blocks can already use `load("data.csv")` etc.
  via the evaluator. Is that sufficient, or do we need a declarative
  `<!-- data: measurements.csv -->` mechanism?
- **TOML config:** Same story — `cfg = toml_read("config.toml")` works
  today. Frontmatter could eventually support `config: params.toml` but
  that's additive — no need to decide now.
- **Parameterized notebooks:** `rustlab-notebook render analysis.md --set N=1024`
  to inject variables before execution. Useful but not before Phase 4.
- **Caching:** For large notebooks, cache block outputs and only re-execute
  changed blocks + their dependents. Complex — save for much later.

---

## Files Summary

| File | Action | Phase |
|------|--------|-------|
| `Cargo.toml` (workspace) | Add `crates/rustlab-notebook` to members + dep | 1 |
| `crates/rustlab-notebook/` | New crate — entire notebook binary | 1 |
| `crates/rustlab-script/src/eval/mod.rs` | Add stdout capture buffer | 2 |
| `crates/rustlab-plot/src/html.rs` | No changes — `render_figure_plotly_div` already public | — |
| `examples/notebooks/*.md` | Example notebooks (already created) | 1 |

---

## Verification (Phase 1)

1. `rustlab-notebook render examples/notebooks/filter_analysis.md`
2. Open generated HTML in browser
3. Verify: prose rendered, code shown, plots interactive, variables persist
4. Test: `quick_look.md` (minimal), `spectral_estimation.md` (tables)
5. `cargo test` passes across workspace
