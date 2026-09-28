# MapLibre GL JS stylesheet (vendored)

| Field | Value |
|---|---|
| Upstream | https://maplibre.org/ |
| Repo | https://github.com/maplibre/maplibre-gl-js |
| Vendored version | 4.5.2 |
| Source URL | https://unpkg.com/maplibre-gl@4.5.2/dist/maplibre-gl.css |
| License | BSD-3-Clause (see `LICENSE`, from the v4.5.2 tag's `LICENSE.txt`) |
| Refresh command | `dev/scripts/vendor-notebook-assets.sh` |
| Per-file SHA256 | `crates/rustlab-notebook/assets/vendor/SHA256SUMS` |

## What's here

- `maplibre-gl.css` — 65534 bytes. Self-contained: `url()`s are
  data-URI SVGs. No remote font or image hosts.

MapLibre GL **JS** is not copied here. Plotly.js 2.35.0 already
bundles it (`maplibre-gl` ^4.5.2, lockfile 4.5.2). That bundle is
not modified. At load time it still appends a `<link>` whose href
is the unpkg range `maplibre-gl@^4.3.2`. The notebook guard script
retargets that link at `/assets/maplibre/maplibre-gl.css` (watch)
or drops it after this file is inlined (static HTML). 4.5.2 is the
copy whose class names match the JS inside Plotly 2.35.0.

## Why this version

Plotly 2.35.0 started fetching this stylesheet from unpkg while the
bundle evaluates, including on pages that never draw a map (plotly.js
issue 7139). Upstream inlined the CSS in 2.35.2. rustlab stays on
2.35.0 and does not patch `plotly.min.js`.
