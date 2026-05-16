# Notebook sources

This directory holds the rustlab-notebook **sources** — markdown files with
` ```rustlab ` fenced code blocks that the notebook runner executes.

**Looking for the rendered notebooks?** They live at the repo-root
[`gallery/`](../../gallery/) directory and are browseable directly on
GitHub with plots inline.

## Layout

```
rustlab/
├── examples/notebooks/      # ← you are here (sources only — no generated files)
│   ├── contour_plots.md
│   ├── laplacian.md
│   ├── vector_fields.md
│   ├── ...
│   └── README.md            # this file (skipped by the renderer)
└── gallery/                 # all rendered output (top-level for discoverability)
    ├── README.md            # gallery index page (committed)
    ├── contour_plots.md     # rendered Markdown — committed
    ├── contour_plots.html   # rendered HTML       — gitignored
    ├── ...
    ├── index.html           # generated HTML index — gitignored
    └── plots/
        ├── contour_plots/plot-1.svg ...
        └── ...
```

Sources and generated files never mix. Generated files all live in
`gallery/`; sources live here in `examples/notebooks/`.

## Regenerating

```sh
make notebooks                                          # rebuild gallery/
open gallery/index.html                                 # browse interactively
```

`make notebooks` runs `dev/build-notebooks.sh`, which invokes
`rustlab-notebook render examples/notebooks/` twice — once for markdown
and once for HTML — both writing into `gallery/`. The `.gitignore`
splits visibility: `*.md` and `plots/**/*.svg` are committed; `*.html`
files are local-only.

## Output formats

The `rustlab-notebook render` command supports four formats:

| Format     | Output                                       | Use case                                  |
|------------|----------------------------------------------|-------------------------------------------|
| `markdown` | `<name>.md` + `plots/<name>/*.svg`           | Static, GitHub-renderable, diff-friendly  |
| `html`     | `<name>.html` (self-contained)               | Interactive Plotly, KaTeX, prev/next nav  |
| `latex`    | `<name>.tex` + `plots/<name>/*.svg`          | Camera-ready typesetting / paper inclusion |
| `pdf`      | `<name>.pdf` (self-contained)                | Single-file deliverable                   |

Markdown and LaTeX share the same on-disk shape: one document file plus a
`plots/<stem>/` subdirectory. HTML and PDF are self-contained — no
sidecar files. See `docs/notebooks.md` for the full design rationale.

## Adding a new notebook

1. Create `examples/notebooks/<name>.md`. Use ` ```rustlab ` fenced
   blocks for code; everything else is regular markdown.
2. Optional YAML frontmatter sets the title and sort order on the
   generated index page:
   ```yaml
   ---
   title: My Analysis
   order: 5
   ---
   ```
3. Run `make notebooks` to render and verify.
4. Commit the source `.md` plus the regenerated `gallery/<name>.md`
   and any new files under `gallery/plots/<name>/`. Add a row for the
   notebook to `gallery/README.md`. (`gallery/<name>.html` stays
   gitignored.)

## Why this split

- **Sources are diffable.** No execution-result churn in the source
  files — reviewers see only what changed in the prose or code.
- **Generated Markdown is committed.** GitHub renders `gallery/<name>.md`
  with inline SVG plots cleanly and with no infrastructure, so a visitor
  browsing the repo sees the executed notebook (text output, plots,
  tables) without cloning. SVG diffs are noisier than source diffs but
  stay readable.
- **Generated HTML is gitignored.** Self-contained Plotly bundles are
  large and contain `<script>` blocks GitHub won't execute anyway. Run
  `make notebooks` and open `gallery/index.html` in a browser for the
  interactive view (zoom, pan, hover, KaTeX-rendered math).

If you'd rather not commit any rendered output, ignore `gallery/*.md`
and `gallery/plots/` too — the pipeline keeps working with everything
local-only.
