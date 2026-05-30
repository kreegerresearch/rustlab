# Dependency trade-off — interactive notebook server

**Companion to:** [`notebook_interactive_server.md`](notebook_interactive_server.md)
**Phase 0 deliverable.** Status: **proposal — pending user sign-off.**

This doc justifies adding `axum` + `tokio` + `tower-http` to
`rustlab-notebook`. The licensing rule
([`feedback_licensing`](../../../../.claude/projects/-Users-mike-projects-2026-rustlab/memory/feedback_licensing.md))
mandates trade-off study only for libraries on **core numerics**;
the notebook server is UI/infra and exempt from the strict rule.
The doc still exists because the dep surface is large enough to
warrant the discussion.

## What the server needs from its HTTP/WS layer

- HTTP/1.1 server (HTTPS not required — locked-in non-goal).
- Loopback-only bind, single port.
- ~3 routes: redirect, HTML, plot SVG bytes.
- WebSocket endpoint for re-render push (Phase 2+).
- Static file serving for KaTeX assets.
- JSON request/response framing for WS messages.

Concurrency requirements are modest: a single user, a handful of
browser tabs at most, one render in flight at a time. There is no
hot path here.

## Candidates

### 1. axum 0.8 + tokio + tower-http  *(recommended)*

- **Pros:** active maintenance (tokio team), idiomatic async Rust,
  WebSocket via `axum::extract::ws`, simple routing, JSON-in-out
  via `serde` extractors, tower middleware ecosystem if needed
  later.
- **Cons:** large transitive dep tree (~40-60 crates), brings a
  full tokio runtime, pulls hyper, http, http-body, tower,
  tower-http, plus their supporting cast.
- **Binary size impact:** estimated +1.5–2 MB stripped on the
  `rustlab-notebook` binary. Needs measurement (see § Verification
  below) before Phase 1 lands.
- **Runtime cost:** current-thread tokio runtime is ~200 KB
  binary, near-zero runtime overhead at idle.
- **License:** MIT.

### 2. tiny_http 0.12

- **Pros:** minimal (≤10 deps), sync, no async runtime.
- **Cons:** no WebSocket support — would need `tungstenite`
  separately on top of a thread-per-connection model. Routing is
  hand-rolled (a `match` on path strings). Less idiomatic for new
  code; little ecosystem.
- **Binary size impact:** +200–300 KB.
- **License:** Apache-2.0 / MIT.
- **Verdict:** the WebSocket gap is the killer. Bringing
  `tungstenite` plus the thread-per-connection lifecycle code
  recovers most of the dep surface we were trying to avoid, but
  with worse code to own.

### 3. hyper directly (no framework)

- **Pros:** same runtime as axum, so no tokio savings; total
  control over the request loop.
- **Cons:** routing, body extraction, content-type negotiation,
  WS upgrade — all hand-rolled. We become the framework.
- **Binary size impact:** ~1.2–1.5 MB (saves axum + tower-http,
  keeps hyper + tokio + http).
- **Verdict:** small binary win, large maintenance loss. Not
  worth it.

### 4. Hand-rolled `std::net` + `tungstenite`

- **Pros:** zero async runtime; smallest possible dep delta.
- **Cons:** we own an HTTP/1.1 parser. Chunked encoding,
  content-length, persistent connections, mime sniffing — all
  bug-rich surface area. Multi-tab WS requires thread-per-conn
  bookkeeping. The user's "core in pure Rust" preference does
  *not* extend to "hand-roll an HTTP server"; that's the kind of
  infrastructure libraries exist for.
- **Binary size impact:** ≤200 KB delta.
- **Verdict:** rejected. The bug surface area on a hand-rolled
  HTTP/1.1 server is real and not a great use of project time.

### 5. actix-web 4

- **Pros:** mature, full-featured, fast.
- **Cons:** larger and less idiomatic than axum for new 2026 code;
  brings the actor system; tokio underneath anyway.
- **Verdict:** dominated by axum on every axis that matters to us.

## Recommendation: axum 0.8 + tokio (current-thread) + tower-http

The notebook server is not CPU-bound, not high-concurrency, and
not on a hot path. Maintenance cost dominates binary-size cost.
axum is the option where the code we write is small, idiomatic,
and easy for the next agent to pick up. The dep tree is large but
not unprecedented for this binary (mermaid-rs-renderer + the SVG
stack already ship).

### Configuration to minimize surface

Even with axum, we keep the dep surface as tight as we can:

```toml
# crates/rustlab-notebook/Cargo.toml
axum = { version = "0.8", default-features = false, features = ["http1", "tokio", "ws", "json", "query"] }
tokio = { version = "1", default-features = false, features = ["rt", "macros", "net", "io-util", "time", "sync", "signal"] }
tower-http = { version = "0.6", default-features = false, features = ["fs"] }
```

Notable choices:

- **Current-thread tokio runtime** (`rt`, not `rt-multi-thread`).
  The server is one-user; multi-thread costs ~200 KB and adds
  cross-thread sync concerns in the render bridge. We don't need
  it.
- **No HTTP/2** (`http1` only). Loopback localhost; HTTP/1.1 is
  fine.
- **`tower-http` features pruned to `fs`** for serving KaTeX
  assets. No compression, no CORS, no auth — none of which apply
  to a loopback server.
- **axum WS feature on** (`ws`), since that's the whole point.
- **axum JSON feature on** (`json`) for typed WS messages.

### Risks we accept by picking axum

- **Binary growth.** Estimated +1.5–2 MB on `rustlab-notebook`.
  Acceptable: this is the standalone notebook binary, not the
  main `rustlab` CLI (which doesn't depend on `rustlab-notebook`
  per the existing rule). The main `rustlab` binary is unaffected.
- **Tokio runtime in another crate.** `rustlab-notebook` is the
  only place tokio appears. No leakage into `rustlab-script` or
  the EM/DSP crates.
- **Future axum breaking changes.** Axum is on 0.8 (pre-1.0); the
  jump from 0.7 → 0.8 in 2024 was a meaningful refactor. We
  accept a periodic upgrade cost; the server module is small
  enough that this is contained.

## Verification before Phase 1 lands

Two measurements required:

1. **Binary size delta:** build `rustlab-notebook` on main and on
   this branch with the deps added but no server code; record the
   stripped size delta. If it's worse than +2.5 MB, revisit.
2. **Clean `cargo build` time delta:** dep tree adds compile
   cost; record it. If it's worse than +30 s on a cold build,
   note it (probably still acceptable, but worth knowing).

Both go in the Status log of the main plan when measured.

## Default port + collision behaviour

**Decision (user, 2026-05-30): default port 8042, auto-increment
on collision with a clear log line.**

Behaviour:

- Try `--port` (defaulting to **8042** — `42 = "the answer"` per
  Hitchhiker's Guide, with `80` HTTP-flavour prefix).
- If busy, try `8043`, `8044`, … up to `8042 + 9` (i.e. cap at 10
  attempts). Each retry logs a `[watch] port 8042 busy, trying
  8043…` line.
- On the successful bind, log the actual bound URL prominently:
  `[watch] listening on http://127.0.0.1:8043 (port 8042 was busy)`.
- If all 10 attempts fail, exit with an error pointing at
  `--port <N>` for a manual override.
- When `--port <N>` is explicit (user-supplied, not the default),
  do *not* auto-increment — explicit means explicit. Fail loud
  with the same error.

Trade-offs accepted:

- **SSH-tunnel users** must read the log line to know what port
  actually bound, then re-establish the tunnel if the port
  shifted. Documented in `docs/notebooks.md`.
- **Multi-window users** rely on the log line to tell windows
  apart. The auto-open browser hits the right URL because the
  open is issued *after* the bind succeeds.

Known collision worth mentioning: Hadoop YARN NodeManager web UI
also runs on 8042. Practically irrelevant on a dev laptop, and
the auto-increment handles it transparently if it ever happens.

## WebSocket protocol shape

**Proposal: full-document refresh only in Phase 2; partial added
in Phase 3.** Ship the discriminator (`{"kind": "full", ...}`)
from day one so Phase 3 is additive.

Rationale:

- **Partial diffs need stable block IDs**, which Phase 3 adds.
  Designing the partial message format before block IDs exist
  means guessing the right key shape — likely wrong.
- **Full-refresh page JS is a one-liner:** receive message,
  `document.body.innerHTML = msg.html`, done. No DOM
  reconciliation in Phase 2 means no scroll preservation in
  Phase 2 either, which is fine — Phase 3 is where scroll
  preservation becomes a real concern.
- **Schema is forward-compatible.** Phase 2 ships:
  ```json
  {"kind": "full", "html": "<…rendered body…>"}
  ```
  Phase 3 adds:
  ```json
  {"kind": "partial", "blocks": [{"id": "b-a1b2c3d4", "html": "…"}]}
  ```
  Page JS gains a `switch (msg.kind)` — additive, not a refactor.

## Self-contained binary

The current renderer pulls these assets from CDNs at page-load
time (see `crates/rustlab-notebook/src/render.rs:308-311`):

| Asset | Source today | Approx size |
|---|---|---|
| Plotly JS bundle | `cdn.plot.ly/plotly-2.35.0.min.js` | ~3.5 MB |
| KaTeX CSS | `cdn.jsdelivr.net/npm/katex@0.16.21/dist/katex.min.css` | ~70 KB |
| KaTeX JS | `cdn.jsdelivr.net/npm/katex@0.16.21/dist/katex.min.js` | ~280 KB |
| KaTeX auto-render | `cdn.jsdelivr.net/npm/katex@0.16.21/dist/contrib/auto-render.min.js` | ~10 KB |
| KaTeX fonts (woff2) | loaded via KaTeX CSS `@font-face` | ~600 KB |
| `pdflatex` / `tectonic` | system PATH (`lib.rs:1485-1511`) | external 500+ MB install |
| mermaid-rs-renderer | pure-Rust crate (already bundled) | — |

**Decision (user, 2026-05-30):** embed Plotly + KaTeX into the
binary; serve from `/assets/`. Leave `pdflatex`/`tectonic`
external (PDF output is an explicit user action; existing
graceful error when missing is correct).

### Why this is the right shape

- **Server mode becomes truly offline.** Open a tab on a tablet
  over SSH tunnel on a plane with no WiFi — math still renders,
  plots still interact. The whole point of the local server is
  defeated if it depends on `cdn.plot.ly` being reachable.
- **Version-pinned, reproducible.** No more "the CDN updated and
  broke KaTeX rendering" failure mode. The bundle you tested
  against is the bundle that ships.
- **Render-mode HTML stays small.** `notebook render --output
  foo.html` keeps writing CDN `<script src="…">` tags by default.
  Embedding into every exported HTML would balloon each file to
  ~5 MB, which is wrong for email/share use cases. A future
  `--self-contained-html` render flag could opt-in to inlining
  (out of scope here).
- **PDF stays external.** `pdflatex`/`tectonic` are 500+ MB
  installs nobody wants embedded. Existing PATH detection +
  graceful error remains correct.

### Implementation

- New dir `crates/rustlab-notebook/assets/vendor/{katex,plotly}/`
  tracked in git, containing the vendored files plus `LICENSE`,
  `LICENSE.OFL` (KaTeX fonts only), and a `VENDOR.md` recording
  upstream URL + version + SHA256 per file.
- New module `crates/rustlab-notebook/src/server/assets.rs` with
  `include_bytes!` constants for each file and an
  `axum::Router` builder that mounts the routes.
- Render emits page HTML pointing at `/assets/katex/…` and
  `/assets/plotly.min.js` instead of the CDN URLs — new "served"
  mode on the existing render layer.
- Repo-root `THIRD_PARTY_NOTICES.md` (created if absent)
  enumerates the new vendored assets per § "Licensing" below.

### Licensing

Workspace is `MIT OR Apache-2.0` (root `Cargo.toml:19`, both
`LICENSE-MIT` and `LICENSE-APACHE` present). Every addition is
compatible:

| Addition | License | Compatible? |
|---|---|---|
| `axum` 0.8 | MIT | ✓ |
| `tokio` 1.x | MIT | ✓ |
| `tower-http` 0.6 | MIT | ✓ |
| Transitives (`hyper`, `http`, `tower`, `bytes`, `mio` …) | MIT or MIT/Apache-2.0 | ✓ |
| Plotly.js (vendored) | MIT | ✓ |
| KaTeX JS + CSS + fonts (vendored) | MIT (single upstream LICENSE) | ✓ |

Note: an earlier draft of this doc asserted KaTeX fonts were
SIL OFL 1.1 — that was wrong. KaTeX's bundled woff2/ttf/woff
fonts are generated from Metafont sources under the KaTeX
project's own MIT LICENSE (they are Computer Modern derivatives,
ultimately tracing back to Knuth's permissive TeX license). The
single `LICENSE` file in the upstream KaTeX release covers
everything we vendor, including the fonts. No separate OFL
notice is required.

Deliberately avoided for license-audit cleanliness: TLS stack
(`rustls`/`ring`/`webpki`). `ring` ships with a non-standard
custom license that some compliance teams flag. We're loopback-
only — no TLS — so this never enters the tree.

A `cargo tree -p rustlab-notebook` audit is a Phase 0 closing
task; the goal is "every line MIT or MIT/Apache-2.0", with any
surprise transitive flagged in the Status log.

## Decisions captured

1. **Default port + collision:** 8042, auto-increment up to 10
   attempts on collision (default port only — explicit `--port`
   fails loud). User sign-off 2026-05-30.
2. **WS protocol shape:** full-only in Phase 2, partial in
   Phase 3, discriminated union from day one.
3. **HTTP/WS library:** axum 0.8 + current-thread tokio +
   tower-http with the trimmed feature set above.
4. **Widget integration boundary:** *not* reserved in Phase 1/2;
   documented in Phase 2 as a forward-compatibility note pointing
   at `notebook_interactive_widgets.md`. The widgets plan's Phase
   1 will introduce the `widget_update` WS kind and the
   render-overrides channel when it lands. Risk accepted: the
   widgets plan may require a minor refactor of the WS handler
   or the render loop signature; cost is small. User decision
   2026-05-30.
5. **Self-contained binary asset embedding:** embed Plotly +
   KaTeX via `include_bytes!`, serve at `/assets/`; leave
   pdflatex/tectonic external. ~4.5 MB binary growth on the
   standalone `rustlab-notebook` binary. User sign-off
   2026-05-30. See § "Self-contained binary" above.
6. **License hygiene:** all additions (Rust deps + vendored
   Plotly/KaTeX bundles) are MIT or MIT/Apache-2.0; KaTeX fonts
   are OFL 1.1 (permissive, non-viral); no TLS stack pulled in.
   User confirmed 2026-05-30. See § "Licensing" above.

All decisions signed off. Items move into the main plan's
"Locked-in design decisions" as #11–#16. Phase 0 closing tasks
(dep add, `cargo tree` audit, binary-size measurement, vendored
asset acquisition) gate Phase 1.
