# Maki icons (vendored)

| Field | Value |
|---|---|
| Upstream | https://github.com/mapbox/maki |
| Vendored version | 2.1.0 |
| Source URL | https://registry.npmjs.org/maki/-/maki-2.1.0.tgz |
| License | CC0 1.0 (see `LICENSE`) |
| Refresh command | `dev/scripts/vendor-notebook-assets.sh` |
| Per-file SHA256 | `crates/rustlab-notebook/assets/vendor/SHA256SUMS` |

## What's here

- `icons/*-15.svg` — 124 files, 148451 bytes. Only the `-15` size
  Plotly requests. The full Maki icon directory is not shipped.

Plotly's map `styleimagemissing` handler, when the image id contains
`-15`, sets `Image.src` to
`https://unpkg.com/maki@2.1.0/icons/<name>.svg`. That path runs for
vector styles that are missing a sprite. The notebook guard rewrites
it to `/assets/maki/<name>.svg`. An unknown name is a same-origin
404. Static HTML clears the prefix, so the image becomes a 1×1
data-GIF and no request is made.

Raster styles (`white-bg`, `open-street-map`, Carto raster) do not
need these icons. `white-bg` also has no tile sources, so it is the
offline map style. Tile hosts stay outside `connect-src`.
