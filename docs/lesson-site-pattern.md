# The Rustlab Lesson + Site Pattern

A drop-in layout for course-style projects built around
[rustlab](https://github.com/kreegerresearch/rustlab) notebooks. This is
the pattern `rustlab_em` uses; copy it into any sibling course project
and the workflow transfers exactly.

If you want to skip the rationale and just copy files, jump to
[Bootstrap a new project](#bootstrap-a-new-project).

## What you get

- **GitHub renders the course inline.** Visitors land on a markdown
  index, click a lesson, see executed code with text output and SVG
  plots — no clone, no build step.
- **Local interactive view.** `make html` produces a Plotly + KaTeX site
  with prev/next navigation between notebooks.
- **One source of truth.** Every lesson is one editable Markdown file
  with ` ```rustlab ` fenced code blocks. No duplicated theory + code.
- **Diff-friendly sources.** Execution output never lives in the source
  files — reviewers see only what changed in prose or code.
- **CI drift guard.** A single `make notebooks-check` ensures the
  committed rendered output matches the sources it was generated from.
- **Standalone scripts on the side.** `.r` files exist parallel to
  notebook code blocks for shell-based tinkering, separate from the
  rendered pipeline.

## On-disk layout

```
your-course/
├── notebooks/                   # editable source notebooks (committed)
│   ├── 01-topic-slug.md
│   ├── 02-topic-slug.md
│   ├── ...
│   └── README.md                # editor-facing notes (skipped by the renderer)
│
├── lessons/                     # standalone .r scripts (per-lesson, optional)
│   ├── 01-topic-slug/
│   │   ├── some_script.r
│   │   ├── another_script.r
│   │   └── *.svg|html|png       # script artefacts (gitignored)
│   └── README.md                # explains the .r-script convention
│
├── site/                        # rendered output for GitHub display (committed)
│   ├── README.md                # hand-written index — GitHub landing
│   ├── 01-topic-slug.md         # rendered notebook with inline ![](...) SVGs
│   ├── 02-topic-slug.md
│   ├── plots/
│   │   └── 01-topic-slug/plot-1.svg ...
│   ├── index.html               # auto-generated entry (gitignored, browser landing)
│   └── 01-topic-slug.html ...   # interactive Plotly per-notebook (gitignored)
│
├── docs/
│   └── lesson-site-pattern.md   # this file (optional)
├── Makefile
├── .gitignore
└── README.md
```

### Three top-level dirs, three jobs

- **`notebooks/`** — what humans edit. Flat, one source `.md` per
  lesson. The filename is the slug; that's what propagates downstream.
- **`lessons/`** — where standalone `.r` scripts live, organized by
  slug. Optional; skip the dir entirely if a lesson has no side
  scripts. Created on demand as scripts are authored.
- **`site/`** — what GitHub displays and what `make notebooks`
  rebuilds. Two landing pages coexist here, one per context: the
  hand-written `README.md` is what GitHub shows when someone clicks the
  dir; the auto-generated `index.html` (from `make html`, gitignored)
  is what a local browser opens. They don't conflict — GitHub doesn't
  render `.html`, and you point your browser at `index.html` directly.
  Everything else under `site/` is generated.

### Naming rules

- Slug: `NN-topic-slug` with two-digit zero-padded numbering. Sorts
  correctly in directory listings and `wildcard` globs.
- Source notebook: `notebooks/<slug>.md`. The filename stem becomes the
  slug at every later stage (`site/<slug>.md`, `site/plots/<slug>/`,
  `site/<slug>.html`).
- Standalone scripts: `lessons/<slug>/<descriptive-name>.r`. Comments
  inside `.r` files use `#`.

## Editing workflow

```sh
# 1. Edit the source notebook.
$EDITOR notebooks/01-topic-slug.md

# 2. Regenerate the site.
make notebooks

# 3. Review the diff. Commit source + regenerated files together.
git add notebooks/01-topic-slug.md site/01-topic-slug.md site/plots/01-topic-slug/
git commit
```

The "commit source + site together" rule keeps the tree internally
consistent. The CI drift check (below) enforces it.

## Makefile

Drop this into the project root. It works unchanged for any number of
notebooks; rename `SITE` if your output dir is named differently.

```make
SITE := site

.DEFAULT_GOAL := help
.PHONY: help all notebooks notebooks-check html clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
	@echo "  lesson-NN          Run .r scripts for one lesson (e.g. make lesson-01)"

all: notebooks html ## Regenerate the rendered site/ and the interactive HTML build

notebooks: ## Render site/<slug>.md from notebooks/<slug>.md
	rustlab notebook render notebooks --format markdown --output $(abspath $(SITE))

html: ## Build interactive HTML at site/index.html (auto-generated entry + per-notebook html)
	rustlab notebook render notebooks --format html --output $(abspath $(SITE)) --title "your-course"

# Drift guard: re-render, then fail if site/ has uncommitted changes.
# Wire this into CI to enforce the source-and-site-together rule.
notebooks-check: notebooks
	@if [ -n "$$(git status --porcelain -- $(SITE)/)" ]; then \
		echo "site/ drifted from sources. Run 'make notebooks' and commit." >&2; \
		git status --short -- $(SITE)/ >&2; exit 1; \
	fi

# Per-lesson script runner: `make lesson-01` runs lessons/01-*/*.r.
lesson-%:
	@for f in lessons/$*-*/*.r; do echo "=== $$f ==="; rustlab run "$$f" || true; done

clean: ## Delete the interactive HTML build and .r script artefacts
	rm -f $(SITE)/*.html
	rm -f lessons/*/*.svg lessons/*/*.html lessons/*/*.png
```

### Why this Makefile is so short

`rustlab notebook render` accepts a directory of source notebooks and
renders the whole batch in one call, with two niceties that fall out of
directory mode and let us avoid per-file pattern rules:

1. **The hand-written `site/README.md` is preserved.** The renderer
   skips files named `README.md` on input and never writes one on
   output, so the markdown landing page survives every rebuild.
2. **HTML output gets a free `index.html`** with prev/next navigation
   between notebooks. Single-file mode would emit one `.html` per
   source with no entry page.

Because the source filenames (`<slug>.md`) are unique at the top of
`notebooks/`, directory mode works directly — no staging, no
intermediate copies, no temp dirs. This is the whole reason sources are
flat in `notebooks/<slug>.md` instead of nested as
`lessons/<slug>/lesson.md`: nested same-named files would collide as
stems and force a staging step.

Trade-off: every `make notebooks` rebuilds all lessons (no Make
incrementality). Release-mode rendering is fast enough that this
doesn't matter for course-sized projects (tens of lessons). If you have
hundreds of notebooks, switch to per-file pattern rules — but you'll
lose the hand-written-README behaviour and the auto-generated
`index.html`, and pay for those with extra Makefile complexity.

## .gitignore

```
# Standalone .r script artefacts. Scripts call savefig("foo.svg")
# relative to their own dir (no subdirectory), and the canonical
# rendered plots live under site/, so we ignore the artefacts in
# place rather than committing them.
lessons/*/*.svg
lessons/*/*.html
lessons/*/*.png

# Interactive HTML notebook build — `make html` writes index.html and
# one *.html per notebook into site/, alongside the committed *.md.
site/*.html
```

There is deliberately no `outputs/` subdirectory per lesson. Scripts
call `savefig("foo.svg")` (no path prefix); the file lands next to the
`.r` and is gitignored. The committed site is the single canonical
location for rendered plots — `.r` artefacts are throwaway tinkering
output.

## CI drift guard

Wire `make notebooks-check` into your CI pipeline. It re-renders the
site, then asks `git status` whether anything under `site/` changed.

A failure means the committed site is out of sync with its sources —
the contributor forgot to run `make notebooks` and stage the result.
Failure mode is loud and the fix is mechanical: run the command
locally, commit, push.

## What `<slug>.md` should contain

This is conventional, not enforced by the pattern, but works well for
course-style notebooks:

1. `# Lesson NN: Title` — H1, no suffix.
2. Brief motivation paragraph.
3. `## Learning Objectives` — 3–5 bullets.
4. `## Background` — prerequisite knowledge assumed.
5. Theory sections: prose interleaved with ` ```rustlab ` blocks. One
   concept per block; long blocks become unreadable when rendered.
6. `## Standalone Scripts` — short table referencing the parallel `.r`
   files under `lessons/<slug>/`.
7. `## Expected Numerical Outputs Summary` — Markdown table of every
   `print()` value students should see.
8. `## Exercises` — 3–5 follow-up questions or script modifications.
9. `## What's next` — one-paragraph forward link to the next lesson.

Notebook authoring constraints (renderer-specific):

- Variables persist across ` ```rustlab ` blocks within one notebook.
  Define `[X, Y]`, `dx`, `dy` etc. once, reuse below.
- The renderer captures the active figure automatically. **Don't** call
  `figure()` or `savefig()` inside notebook code blocks — those belong
  in standalone `.r` scripts.
- Use `clf;` at the start of each plot block to clear before drawing.
- Comments in notebook code blocks: `%`. Comments in `.r` files: `#`.

## Bootstrap a new project

From an empty repo:

```sh
mkdir -p notebooks lessons site docs

# Stub a first lesson source so make doesn't render an empty dir.
cat > notebooks/01-getting-started.md <<'EOF'
# Lesson 01: Getting Started

Replace this stub with prose, ```rustlab``` blocks, and exercises.

```rustlab
print("hello, rustlab")
```
EOF

# Editor-facing source notes (skipped by the renderer).
cat > notebooks/README.md <<'EOF'
# Source notebooks

Edit `<slug>.md` files here. Run `make notebooks` from the repo root
to regenerate the rendered site/ tree.
EOF

# Hand-written landing page that GitHub displays at site/.
cat > site/README.md <<'EOF'
# your-course

Rendered lessons. Sources at `../notebooks/`.

| # | Lesson |
|---|--------|
| 01 | [Getting Started](01-getting-started.md) |
EOF

# .gitignore — see the section above.
cat > .gitignore <<'EOF'
lessons/*/*.svg
lessons/*/*.html
lessons/*/*.png
site/*.html
EOF

# Drop in the Makefile from the section above (replace "your-course"
# with your project name in the --title flag).

# Sanity check.
make notebooks
ls site/
```

That's the whole bootstrap. Add lessons by dropping more `<slug>.md`
files into `notebooks/`, optionally creating `lessons/<slug>/` for side
scripts, and updating the table in `site/README.md`.

## Migrating from older layouts

### From `lessons/<slug>/lesson.md` (per-lesson source dir)

If your project has source notebooks colocated with `.r` scripts under
each `lessons/<slug>/`, hoist them to a flat `notebooks/`:

```sh
mkdir -p notebooks
for d in lessons/*/; do
    slug=$(basename "$d")
    if [ -f "$d/lesson.md" ]; then
        mv "$d/lesson.md" "notebooks/$slug.md"
        # If the dir is now empty (no .r scripts), drop it.
        rmdir "$d" 2>/dev/null
    fi
done
```

Then replace your Makefile with the recipe above and update any docs
that referenced the old path.

### From `lessons/<slug>/src/lesson.md` (with rendered output committed alongside)

A two-step migration: first hoist out of `src/`, then flatten to
`notebooks/`:

```sh
# 1. Drop the src/ indirection per lesson.
for d in lessons/*/; do
    if [ -f "$d/src/lesson.md" ]; then
        rm -f "$d/lesson.md"               # old rendered output
        mv "$d/src/lesson.md" "$d/lesson.md"
        rmdir "$d/src"
        rm -rf "$d/lesson_plots"           # old per-lesson plot dir
    fi
done

# 2. Then run the migration above to move sources into notebooks/.
```

In both cases, after the file moves: `make notebooks` to populate
`site/`, then commit source moves + new site files in one changeset.

## Why this layout works on GitHub

GitHub renders Markdown prose, fenced code blocks, and inline images
(`![alt](relative/path.svg)`). It does *not* execute `<script>` tags or
run code blocks. The pattern leans into that:

- **Sources stay diffable.** No SVG bytes or executed-output churn in
  `notebooks/`. Reviewers see exactly what changed in prose or code.
- **`site/` is committed and self-rendering.** A reader visiting the
  repo sees executed notebooks with text output and inline plots, with
  no infrastructure to set up.
- **Interactive HTML build is gitignored.** `make html` writes
  `site/index.html` and one `<slug>.html` per notebook *alongside* the
  committed `<slug>.md` files. Self-contained Plotly bundles are large
  and contain `<script>` GitHub won't run anyway, so they're locked
  behind `site/*.html` in the gitignore. Local readers who want
  interactive plots run `make html` and open `site/index.html`.

## Best practices

### Highlight the rendered gallery from the top-level README

Visitors landing on your repo's GitHub page should learn within a few
lines that they can read the course without cloning. Put a prominent
link to `site/` near the top of the project root's `README.md`:

````markdown
**[Browse the rendered lessons →](site/)** — N worked notebooks with
plots inline. GitHub displays them directly, no install needed.
````

GitHub auto-renders `site/README.md` when the link resolves to the
directory, so readers land on your hand-written index without any
extra wiring. Skip this lead-in and the rendered output — one of the
best things about the layout — is easy to miss; visitors assume they
need to clone and build something to see executed code with plots.

### Hide setup blocks with `<!-- hide -->`

Lessons often need imports, data loading, or boilerplate that a reader
doesn't benefit from seeing. Mark those blocks with the `<!-- hide -->`
directive immediately before the fence so the renderer evaluates them
but omits them from the rendered output:

````markdown
<!-- hide -->
```rustlab
sr = 44100;
data = load("samples.npy");
```
````

Variables defined in a hidden block persist into later blocks like any
other code — only the source and output are suppressed in the render.
Use sparingly: if a setup step is conceptually load-bearing for the
lesson, leave it visible.

## See also

- Upstream rustlab uses the same pattern at a larger scale (sources
  flat in `examples/notebooks/`, rendered output committed to a
  top-level dir they call `gallery/` rather than `site/`, no
  `lessons/` layer for side scripts):
  <https://github.com/kreegerresearch/rustlab/tree/main/gallery>.
- `rustlab/docs/notebooks.md` — full notebook format reference,
  including directives, frontmatter, and output format details.
