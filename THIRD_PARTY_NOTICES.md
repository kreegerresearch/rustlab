# Third-party notices

`rustlab` is licensed under `MIT OR Apache-2.0` (see `LICENSE-MIT`
and `LICENSE-APACHE`). This file enumerates third-party assets
that ship inside the build outputs (vendored binaries embedded
via `include_bytes!`, distinct from regular Cargo dependencies
which are tracked by `Cargo.lock` and auditable with
`cargo tree`).

## Vendored web assets — `rustlab-notebook` interactive server

The interactive `rustlab-notebook watch` server (see
`dev/plans/notebook_interactive_server.md`) embeds the following
JavaScript/CSS/font assets into the `rustlab-notebook` binary
and serves them locally at `/assets/…` so the rendered page
works fully offline. Per-file SHA256 checksums live alongside
the assets in
`crates/rustlab-notebook/assets/vendor/SHA256SUMS`; the fetch is
reproducible via `dev/scripts/vendor-notebook-assets.sh`.

| Asset | Version | Upstream | License | Vendored at |
|---|---|---|---|---|
| KaTeX (CSS + JS + auto-render + fonts) | 0.16.21 | https://github.com/KaTeX/KaTeX | MIT — see `crates/rustlab-notebook/assets/vendor/katex/LICENSE` | `crates/rustlab-notebook/assets/vendor/katex/` |
| Plotly.js | 2.35.0 | https://github.com/plotly/plotly.js | MIT — see `crates/rustlab-notebook/assets/vendor/plotly/LICENSE` | `crates/rustlab-notebook/assets/vendor/plotly/` |

The KaTeX fonts ship under the same MIT license as the rest of
KaTeX — they are Computer Modern derivatives generated from
Metafont sources under the KaTeX project's own license, not SIL
Open Font License (a common misconception). The single KaTeX
`LICENSE` covers them.

Each vendored directory has a `VENDOR.md` documenting upstream
URL, version, and contents.

## Cargo dependencies

Regular Cargo dependencies (the Rust crates pulled in via
`Cargo.toml`) are auditable via `cargo tree -p <crate>`. As of
the interactive-server work landing on this branch, all new
transitive crates added to `rustlab-notebook` are permissive
(MIT, MIT/Apache-2.0, or Apache-2.0-only) and compatible with
the workspace's `MIT OR Apache-2.0` license. The interactive
server deliberately avoids the TLS stack (`rustls`/`ring`/
`webpki`) — it binds loopback only — which keeps the license
audit short.
