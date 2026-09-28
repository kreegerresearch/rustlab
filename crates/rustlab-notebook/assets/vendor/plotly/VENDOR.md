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
  embeds and serves at `/assets/plotly.min.js`. This file is not
  patched. It already contains MapLibre GL JS and, at load time,
  injects `maplibre-gl.css` from unpkg. The stylesheet (MapLibre GL
  JS 4.5.2, the version this bundle depended on) and the Maki 2.1.0
  `*-15` icons it may request live in the sibling `maplibre/` and
  `maki/` directories. The notebook renderer installs a guard script
  that retargets those URLs; see those directories' `VENDOR.md`.

This is the open-source Plotly.js library only. Plotly's other
products (Dash, Chart Studio, Plotly Enterprise) are separately
licensed and are *not* included.

## Why vendored

The interactive `notebook watch` server embeds this file via
`include_bytes!` and serves it locally so plot interactivity
works fully offline (e.g. on a tablet over an SSH tunnel with no
WiFi). See `dev/plans/notebook_interactive_server.md` § locked-in
#15 and the trade-off doc for the full rationale.

## Size note

The Plotly bundle is ~4.3 MB on disk — it dominates the binary
size growth of the standalone `rustlab-notebook` binary. The
trade-off was accepted because offline interactive viewing is
the headline value of the embedded-asset choice.
