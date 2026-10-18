# Plotly.js (vendored)

| Field | Value |
|---|---|
| Upstream | https://plotly.com/javascript/ |
| Repo | https://github.com/plotly/plotly.js |
| Vendored version | 2.35.0 |
| Source URL | https://cdn.plot.ly/plotly-2.35.0.min.js |
| License | MIT (see `LICENSE`) |
| Refresh command | `dev/scripts/vendor-notebook-assets.sh` |
| Per-file SHA256 | `crates/rustlab-notebook/assets/vendor/SHA256SUMS` |

## What's here

- `plotly.min.js` — the bundle the interactive notebook server
  serves at `/assets/plotly.min.js`, and that batch notebook HTML,
  `savefig` HTML, and `saveanim` HTML copy to a sibling
  `assets/plotly.min.js`. The bytes are compiled into `rustlab-plot`
  (`PLOTLY_JS`) so the CLI and the notebook binary share one copy.

This is the open-source Plotly.js library only. Plotly's other
products (Dash, Chart Studio, Plotly Enterprise) are separately
licensed and are *not* included.

## Why vendored

The interactive `notebook watch` server serves this file locally,
and batch HTML / `savefig` / `saveanim` copy it beside the output,
so plot interactivity works fully offline (e.g. on a tablet over an
SSH tunnel with no WiFi, or an HTML file opened from `file://`).
See `dev/plans/notebook_interactive_server.md` § locked-in #15 and
the trade-off doc for the full rationale. The bundle is not inlined
into each HTML file: at ~4.4 MB that would repeat on every page.

## Size note

The Plotly bundle is ~4.3 MB on disk — it dominates the binary
size growth of the standalone `rustlab-notebook` binary. The
trade-off was accepted because offline interactive viewing is
the headline value of the embedded-asset choice.
