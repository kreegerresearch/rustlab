# CodeMirror 5 (vendored)

| Field | Value |
|---|---|
| Upstream | https://codemirror.net/5/ |
| Repo | https://github.com/codemirror/codemirror5 |
| Vendored version | 5.65.19 |
| Source URL | https://cdnjs.cloudflare.com/ajax/libs/codemirror/5.65.19 (JS/CSS); LICENSE from the GitHub repo at the same tag |
| License | MIT (see `LICENSE`) |
| Refresh command | `dev/scripts/vendor-notebook-assets.sh` |
| Per-file SHA256 | `crates/rustlab-notebook/assets/vendor/SHA256SUMS` |

## What's here

- `codemirror.min.js` — the core editor.
- `codemirror.min.css` — base editor styles.
- `mode/markdown/markdown.min.js` — the Markdown syntax mode.
- `LICENSE` — upstream MIT license.

Served at `/assets/codemirror/…` **only** when the interactive
server runs with `rustlab-notebook watch --editable`.

## Why vendored

The in-browser editor (`--editable`, Phase 5c of
`dev/plans/notebook_interactive_server.md`) embeds these files via
`include_bytes!` and serves them locally so the editor works fully
offline, consistent with the rest of the embedded-asset choice
(locked-in #15). CodeMirror 5 was chosen over CodeMirror 6 / Monaco
because it ships as a single self-contained minified bundle (~170 KB
JS + ~6 KB CSS + ~15 KB for the Markdown mode), trivial to vendor and
serve without a module bundler or AMD loader.

## Why CodeMirror 5 (not 6)

CodeMirror 6 is ESM-only and expects a bundler; Monaco is multi-MB
and ships as many AMD chunks with a loader. CodeMirror 5's single-file
build keeps the vendored footprint small and the serving logic a plain
static-file lookup.
