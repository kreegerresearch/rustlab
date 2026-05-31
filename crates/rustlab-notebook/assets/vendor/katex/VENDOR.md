# KaTeX (vendored)

| Field | Value |
|---|---|
| Upstream | https://katex.org/ |
| Repo | https://github.com/KaTeX/KaTeX |
| Vendored version | 0.16.21 |
| Source URL | https://github.com/KaTeX/KaTeX/releases/download/v0.16.21/katex.tar.gz |
| License | MIT (see `LICENSE`) |
| Refresh command | `dev/scripts/vendor-notebook-assets.sh` |
| Per-file SHA256 | `crates/rustlab-notebook/assets/vendor/SHA256SUMS` |

## What's here

- `katex.min.css` — main stylesheet, references the fonts under `fonts/`
- `katex.min.js` — main math renderer
- `contrib/auto-render.min.js` — finds `$…$` / `$$…$$` spans in HTML and renders them
- `fonts/KaTeX_*.{woff2,woff,ttf}` — the bundled font files

The KaTeX upstream tarball also ships `.map` source maps, `.mjs`
ESM alternates, additional `contrib/` extensions, and a `README`
we don't need at runtime. Those are deliberately *not* copied
into the vendor dir to keep the binary embed tight.

## Why vendored

The interactive `notebook watch` server embeds these files via
`include_bytes!` and serves them at `/assets/katex/…` so the
rendered page works fully offline (e.g. on a tablet over an SSH
tunnel with no WiFi). See
`dev/plans/notebook_interactive_server.md` § locked-in #15 and
the trade-off doc for the full rationale.

## License (font note)

The single `LICENSE` file in this directory is KaTeX's MIT
license and covers all the vendored files — including the
woff2/woff/ttf fonts under `fonts/`. The KaTeX fonts are
generated from Metafont sources under the KaTeX project's own
MIT license (they are Computer Modern derivatives, tracing back
to Knuth's permissive TeX license); they are **not** under SIL
Open Font License despite a common misconception. No separate
OFL notice is required.
