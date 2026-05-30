# Interactive `notebook watch` — local web server + live page

## Agent handoff — read this first

**Where we are:** Phase 0 (Design & scoping). No code has landed.
Locked-in decisions and phase task lists below are the agreed
scope; the locked-ins are *not* up for renegotiation without
explicit user approval.

**Phase progress at a glance:**

| Phase | State | Headline deliverable |
|-------|-------|----------------------|
| 0 — Design + dep trade-off | **complete** (2026-05-30) | trade-off doc, port 8042 + auto-bump, WS full→partial, embed Plotly + KaTeX (vendored), pdflatex external, license audit clean, deps added, vendored assets in tree |
| 1 — Server skeleton | **complete** (2026-05-30) | `server::start`, one-shot render over HTTP, embedded `/assets/`, plot tempdir, browser auto-open, 15 unit + 5 integration tests |
| 2 — Live re-render | not started | fs watcher → cancellable re-render → WS push |
| 3 — Block-level diffing | not started | content-hash block IDs, partial DOM updates |
| 4 — Docs + REPL help | not started | `docs/notebooks.md` section, AGENTS.md close-out |
| 5 — Polish (optional) | not started | `--watch-dir`, source pane, opt-in in-browser editor |

**Next concrete action:** start Phase 2 (live re-render on save).

Phase 0 and Phase 1 are complete. The server skeleton at
`crates/rustlab-notebook/src/server/` serves the rendered HTML +
embedded assets + animation artefacts once at startup, but does
not re-render when the source `.md` is saved. Phase 2 task list
(below) adds:

1. `notify`-based fs watcher on the input file (reuse the
   debouncer pattern from `watch.rs`).
2. WebSocket endpoint `/ws` with the discriminated message shape
   `{"kind": "full", "html": "…"}` (partial diffs land in Phase
   3).
3. Cancellable render task so prose edits don't stall behind a
   slow `cache enable`-d function from the previous render.
4. LRU bound on the in-memory plot table.
5. Page JS: connect WS, swap document body on receive, reconnect
   with exponential backoff 500 ms → 5 s capped + visible
   "disconnected" banner.
6. Documentation pointers at the two future widget extension
   sites (per locked-in #14).

**Required reading before touching code:**

- `crates/rustlab-notebook/src/watch.rs` — current `cmd_watch`
  entry point and `notify` + debouncer pattern this plan reuses.
- `crates/rustlab-notebook/src/cache.rs` — `NotebookCache` (the
  prefix cache the server re-uses verbatim).
- `crates/rustlab-notebook/src/execute.rs` —
  `execute_notebook_with_cache`, the entry point the render loop
  calls.
- `crates/rustlab-notebook/src/render.rs` and
  `render_markdown.rs` — where block-ID injection and the new
  "served" plot-path mode go (see Crate layout below).
- `crates/rustlab-notebook/src/lib.rs` — how existing notebook
  commands (`cmd_render`, `cmd_check`) wire CLI to the render
  pipeline.

**Workflow rules** (per `AGENTS.md` and user memory):

- Plan-first. If anything below needs to change, **update this
  plan and get user approval** before coding.
- Feature branch only; never push to main. Suggested name:
  `feature/notebook-interactive-server`.
- Stage freely (`git add`) but do not commit or push without
  explicit user approval. No `Co-Authored-By: Claude …` lines
  in commit messages.
- Keep the rustlab binary small — all new code lands in
  `rustlab-notebook`, not in the main `rustlab` CLI.
- Update on every meaningful change: (1) the Phase checkboxes
  in this plan, (2) the AGENTS.md "Active Plans" row, (3) the
  Status log at the bottom of this file (one dated line). These
  three views must stay in sync — that's what lets the next
  agent pick up cleanly.
- When a Phase ships: also update `docs/notebooks.md` and any
  REPL help text touched.

**Companion plan:** `notebook_interactive_widgets.md` (sliders,
option buttons, number inputs) depends on Phase 2 of this plan.
The widgets plan adds a `widget_update` inbound WS message kind
and a render-with-overrides entry point on the render loop — keep
both reserved when designing Phase 2, or you'll retrofit.

## Motivation

Today the only good way to view a rustlab notebook live is to point
the watcher at an Obsidian vault, edit in Editing view, switch to
Reading view. That works, but it's:

- Obsidian-specific (and you're stuck on whatever vault layout
  Obsidian wants — frontmatter rewrites, `_attachments/`, wikilinks).
- A modification of your source `.md` (per the "only `--obsidian`
  modifies" rule landed alongside this plan).
- Locally-bound: there's no easy way to share the live notebook
  with a colleague or open it on a tablet next to your laptop.

The interactive default solves all three: zero source modification,
no Obsidian dependency, opens in any browser at
`http://localhost:<port>/<notebook>`. Edit the `.md`, save, page
reflects the new output via WebSocket push.

## Locked-in design decisions

Captured up front so review can focus on the open questions:

1. **Default action**: `notebook watch <input>` (no other flags)
   spins up the server. The user has to explicitly choose
   `--obsidian` or `--output` for the existing in-place / two-dir
   render modes.
2. **No source modification.** Ever. The server reads the `.md`,
   renders to HTML in memory, serves it. Plot SVGs and other
   binary outputs are served from the in-memory render or a temp
   directory that's torn down on exit.
3. **WebSocket push on save.** Re-render happens on every save and
   pushes a diff or a full HTML refresh to connected pages. Same
   debouncer as `notebook watch` already uses for fs events
   (250 ms by default).
4. **Block-level prefix cache** (the existing
   `crates/rustlab-notebook/src/cache.rs::NotebookCache`) is reused
   directly. Edit a prose-only block, every code block returns
   cached output instantly.
5. **Persistent function cache is shared** with other rustlab
   processes through `.rustlab/cache.db`, exactly like
   `notebook render`. The server does not scope the cache to its
   own session (sharing is the whole point of the persistent layer).
6. **Single notebook per server instance** for v1. Pointing at a
   directory in interactive mode is a follow-up.
7. **Loopback-only bind.** The server listens on `127.0.0.1`
   exclusively; any user-supplied bind that isn't loopback is
   rejected at startup with a clear error pointing at SSH tunnels.
8. **Auto-open the browser** when stdout is a TTY and `CI` is
   unset; never otherwise. `--no-browser` forces off.
9. **Cancel in-flight render on new fs event.** A prose edit must
   never block waiting for a slow code block from the previous
   render to finish. The render task is cancellable; the next
   debounced fs event preempts it.
10. **LRU bound on served plots.** In-memory plot table is an LRU
    keyed on `(block_id, source_hash)` capped at 256 entries.
    Evicted entries fall back to re-render on next request.
11. **HTTP/WS stack: axum 0.8 + current-thread tokio + tower-http**
    with the trimmed feature set in the trade-off doc. Tokio
    stays contained inside `rustlab-notebook`; main `rustlab` CLI
    is unaffected.
12. **Default port 8042; auto-increment up to 10 attempts** on
    collision when the port is the default. An explicit
    `--port <N>` fails loud on collision (explicit means
    explicit). Each retry and the final bound URL are logged
    prominently so SSH-tunnel users see what port actually bound.
13. **WS protocol shape: full-document refresh only in Phase 2;
    partial added in Phase 3.** The `{"kind": …}` discriminator
    ships from day one so Phase 3 is additive (new case, not a
    schema change).
14. **Widget integration boundary is *not* reserved up front.**
    Phase 2 documents the touchpoints (a future
    `widget_update` inbound WS kind and a render-overrides
    channel on the render loop) but ships no scaffolding. The
    widgets plan's Phase 1 wires both when it lands; cost of any
    resulting refactor is accepted.
15. **Server-mode page is fully offline-capable; assets are
    embedded in the binary.** KaTeX (CSS + JS + fonts) and the
    Plotly bundle are vendored under
    `crates/rustlab-notebook/assets/vendor/{katex,plotly}/` and
    embedded via `include_bytes!`. The server serves them from
    `/assets/katex/…` and `/assets/plotly.min.js`; emitted page
    HTML references those local paths instead of the
    `cdn.jsdelivr.net` / `cdn.plot.ly` URLs the renderer uses
    today. Net binary growth ≈ 4.5 MB on the standalone
    `rustlab-notebook` binary; main `rustlab` CLI is unaffected.
    `pdflatex`/`tectonic` remain external (PDF output only); the
    existing graceful "neither found in PATH" error is correct.
    `notebook render --output foo.html` keeps emitting CDN
    `<script src=…>` tags by default (a future
    `--self-contained-html` flag can opt into inlining for
    emailable offline HTML — out of scope here).
16. **License hygiene: every addition is compatible with the
    workspace's `MIT OR Apache-2.0` dual license.**
    - Rust deps (`axum`, `tokio`, `tower-http`, transitives) are
      MIT or MIT/Apache-2.0. Two notables: `matchit 0.8.4` is
      `MIT AND BSD-3-Clause` (both notices required, trivially
      satisfied); `sync_wrapper 1.0.2` is Apache-2.0-only (the
      workspace dual license permits the Apache redistribution
      path).
    - Vendored Plotly.js, KaTeX JS, KaTeX CSS, **and KaTeX
      fonts** are all under the upstream KaTeX/Plotly MIT
      LICENSE — KaTeX's bundled woff2/ttf/woff fonts are
      generated from Metafont under the KaTeX project's MIT
      license (Computer Modern derivatives, not OFL). No
      separate OFL notice is required.
    - No TLS stack pulled in (`ring`/`rustls` avoided), keeping
      the license audit short.
    - A repo-root `THIRD_PARTY_NOTICES.md` enumerates each
      vendored asset + upstream URL + version + license. SHA256
      checksums for every vendored file live in
      `crates/rustlab-notebook/assets/vendor/SHA256SUMS`,
      regenerated by `dev/scripts/vendor-notebook-assets.sh`.

## Non-goals (out of scope for v1)

- Editing the notebook from the browser. The browser is read-only;
  the file is edited in your editor of choice. (Future polish:
  optional in-browser editor backed by the same file.)
- Multi-user collaboration / authentication. The server binds
  `127.0.0.1` only; remote access requires SSH tunnelling or
  similar, which is the user's call.
- HTTPS / TLS. localhost-only; not needed.
- Stateful sessions. The page is a pure projection of the current
  `.md` on disk. No server-side per-user state.
- Replacing `notebook render` for batch / CI use. Render stays the
  shipping format for `make notebooks`, CI artifacts, and email-able
  HTML.

## Crate layout

Most of the work fits in `crates/rustlab-notebook`:

- `crates/rustlab-notebook/src/server/mod.rs` — public entry point,
  CLI wiring.
- `crates/rustlab-notebook/src/server/http.rs` — HTTP routes
  (`/`, `/notebook.html`, `/assets/*`, `/plots/*`).
- `crates/rustlab-notebook/src/server/ws.rs` — WebSocket endpoint
  (`/ws`) for re-render push.
- `crates/rustlab-notebook/src/server/render_loop.rs` — fs watcher
  + debouncer + re-render bridge, reuses
  `execute::execute_notebook_with_cache` and the in-memory
  `NotebookCache`.

Cross-cutting touches outside `server/`:

- `crates/rustlab-notebook/src/render.rs` and
  `render_markdown.rs` — emit a stable `id="b-<short-hash>"`
  attribute on every rendered block so Phase 3 partial diffs can
  identify what changed even when blocks are reordered. Hash is
  blake3-truncated over the block source.
- `crates/rustlab-notebook/src/render.rs` — new "served" plot path
  mode that rewrites `<img src>` to `/plots/<index>.svg` for the
  server, alongside the existing file-path and `_attachments/`
  modes.

Dependencies: `axum = "0.8"` (well-maintained, small surface),
`tokio` (net-new — `notify` uses `std::sync::mpsc`, not the
tokio-backed debouncer, so the runtime arrives with this work),
`tower-http` for static-file handling. Trade-off doc is
best-practice here rather than policy-mandated (notebook server is
infrastructure, not core numerics — see `feedback_licensing`), but
the axum+tokio surface is large enough to warrant one anyway:
`dev/plans/notebook_interactive_server-tradeoff.md` (to be written
before Phase 1).

## CLI surface

```
rustlab-notebook watch <input>                          (default — interactive server)
rustlab-notebook watch <input> --port 8765              (override default port; loopback always)
rustlab-notebook watch <input> --no-browser             (don't auto-open the browser)
```

There is no `--bind` flag: the server is loopback-only by
design (see locked-in #7). Users who need remote access set up
an SSH tunnel.

The existing `--obsidian` and `--output` flags retain their current
meanings and continue to suppress interactive mode (since the user
has explicitly chosen a render destination).

## Wire format / endpoints

| Path | Purpose |
|---|---|
| `GET /` | Redirect to `/notebook.html` (the index for the active notebook) |
| `GET /notebook.html` | The current rendered HTML, with the WebSocket connect snippet injected at the bottom |
| `GET /plots/<index>.svg` | Served from the in-memory render. Index is the per-notebook figure number the existing renderer already assigns; no new hashing scheme. |
| `GET /assets/<name>` | Static CSS/JS (the same KaTeX bundle that the render uses) |
| `WS /ws` | Re-render push channel. Server sends one of: `{"kind":"full","html":"…"}` (initial / large change), or `{"kind":"partial","blocks":[{"id":"b3","html":"…"}]}` (only the blocks that re-executed) |

The partial-update format makes the prefix cache user-visible: a
prose-only edit pushes a tiny payload, the page does `morphdom`-style
DOM replacement on the changed blocks, no scroll position loss.

## Phases

### Phase 0 — Design & dependency trade-off  **Status:** library / port / WS / widget questions signed off; asset-embedding still pending

- [x] Write `dev/plans/notebook_interactive_server-tradeoff.md`.
- [x] HTTP/WS library: axum 0.8 + current-thread tokio +
      tower-http (locked-in #11).
- [x] Default port + collision: 8042, auto-increment up to 10
      attempts; explicit `--port` fails loud (locked-in #12).
      Signed off 2026-05-30.
- [x] WebSocket protocol shape: full-document only in Phase 2;
      partial added in Phase 3; discriminated union from day one
      (locked-in #13). Signed off 2026-05-30.
- [x] Widget integration boundary: not reserved up front;
      documented in Phase 2 as a forward-compat note
      (locked-in #14). Signed off 2026-05-30.
- [x] Self-contained-binary asset embedding: embed Plotly +
      KaTeX, leave pdflatex/tectonic external (locked-in #15).
      Signed off 2026-05-30.
- [x] License hygiene: verified `MIT OR Apache-2.0` compatibility
      of all proposed deps and vendored assets (locked-in #16).
      Signed off 2026-05-30.

**Phase 0 closing tasks** (Phase 1 unblocks when all green):

- [x] Add `axum`, `tokio`, `tower-http` deps to
      `crates/rustlab-notebook/Cargo.toml` with the trimmed
      feature set from the trade-off doc; verifies it compiles
      (release build green in 30.6 s incremental).
- [x] `cargo tree -p rustlab-notebook` audit — all 29 new
      transitives are permissive (MIT / MIT-or-Apache-2.0 / one
      Apache-2.0-only and one MIT-AND-BSD-3-Clause — both
      compatible). See Status log entry 2026-05-30.
- [x] Measure release binary-size delta: **+16 KB** with deps
      added but unused (dead-code elimination is doing its
      work). Real growth lands when Phase 1 wires the deps; the
      +1.5-2 MB estimate from the trade-off doc still applies
      at that point. Recorded in Status log.
- [x] Vendor KaTeX 0.16.21 + Plotly 2.35.0 under
      `crates/rustlab-notebook/assets/vendor/{katex,plotly}/`
      via `dev/scripts/vendor-notebook-assets.sh`. Each dir has
      `LICENSE` (both MIT — KaTeX fonts also MIT, the OFL
      assumption was incorrect), `VENDOR.md`, and the repo-wide
      `SHA256SUMS` manifest. Total vendored size 5.7 MB
      (KaTeX 1.4 MB + Plotly 4.3 MB).
- [x] Create `THIRD_PARTY_NOTICES.md` at repo root enumerating
      the new vendored assets.

**Phase 0 complete.** Phase 1 unblocked as of 2026-05-30.

### Phase 1 — Server skeleton  **Status:** complete (2026-05-30)

- [x] `server::start(input, opts)` — current-thread tokio runtime,
      binds on `127.0.0.1:8042` (default) with auto-increment up
      to 10 attempts per locked-in #12, logs the bound URL, blocks
      on `tokio::signal::ctrl_c` via `axum`'s graceful shutdown.
      Explicit `--port <N>` fails loud on collision.
      [crates/rustlab-notebook/src/server/mod.rs](../../crates/rustlab-notebook/src/server/mod.rs)
- [x] Browser auto-open (TTY-gated via `std::io::IsTerminal`,
      `CI`-aware) per locked-in #8. Shells out to `open` /
      `xdg-open` / `cmd /c start` per platform; failure logs a
      hint without crashing the server.
- [x] `GET /notebook.html` returns the one-shot render of the
      input, with all CDN URLs swapped to local `/assets/…` paths.
- [x] `GET /plots/<file>` serves animation artefacts from the
      per-server tempdir (static plots are inline Plotly so no
      `<index>.svg` endpoint was needed — the route shape can
      carry SVGs once Phase 3 / scoped re-render produces them).
      Includes traversal protection.
- [x] `GET /assets/<path>` serves the embedded KaTeX (CSS + JS +
      auto-render + 20 woff2 fonts) and Plotly bundle via
      `include_bytes!` per locked-in #15.
      [crates/rustlab-notebook/src/server/assets.rs](../../crates/rustlab-notebook/src/server/assets.rs)
- [x] Render uses local `/assets/…` paths instead of CDN URLs via
      `assets::rewrite_cdn_urls` post-processing — no
      modification to `render.rs` itself; existing render modes
      (`notebook render`, `--obsidian`) keep CDN refs.
- [x] Initial render runs once at startup, on the calling thread,
      before the tokio runtime spins up.
- [x] CLI: `--port` and `--no-browser` added to the `watch`
      subcommand; bare-input mode now dispatches to
      `server::start` (replacing the old `cmd_check` fallback).
      [crates/rustlab-notebook/src/main.rs](../../crates/rustlab-notebook/src/main.rs)
- [x] Tests: 15 unit tests in
      `server::{assets,http,…}::tests` + 5 integration tests in
      [tests/server_smoke.rs](../../crates/rustlab-notebook/tests/server_smoke.rs)
      covering route shape, asset embedding, CDN-rewrite, and
      end-to-end render of a fixture notebook. All 490 notebook
      crate tests pass with no regressions.
- [x] Real-world smoke: `target/debug/rustlab-notebook watch
      examples/notebooks/contour_plots.md --no-browser --port
      18042` serves `/notebook.html` referencing only `/assets/`,
      `/assets/katex/katex.min.css` returns 23 KB of CSS, and
      `/assets/plotly.min.js` returns 4.5 MB of JS.
- [x] Docs: `docs/notebooks.md` § "Live preview" rewritten to
      describe the interactive default + the existing re-render
      mode as the two `watch` paths.

### Phase 2 — Live re-render  **Status:** not started

- [ ] fs watcher on the input file (reuse `notify` + `watch.rs`'s
      debouncer pattern)
- [ ] WebSocket endpoint `/ws` accepting many concurrent
      connections (one per page-load — laptop tab + tablet is the
      headline use case)
- [ ] Cancellable render task: a new debounced fs event preempts
      the in-flight render (see locked-in #9) so prose edits don't
      stall behind slow code blocks
- [ ] LRU eviction on the in-memory plot table per locked-in #10
- [ ] Re-render on save → push `{"kind":"full","html":"…"}` to every
      connected client
- [ ] Page JS: connect WS, replace document on receive, reconnect
      with exponential backoff 500 ms → 5 s capped (~10 tries),
      then surface a visible "disconnected" banner until the next
      successful connect
- [ ] **Document widget integration touchpoints** (locked-in #14)
      in code comments at the two future-extension sites: the WS
      inbound-message `match` (where `widget_update` will land as
      a new arm) and the `render_loop` entry signature (where the
      optional `&BTreeMap<String, WidgetValue>` override will be
      added). Goal: when the widgets plan's Phase 1 starts, the
      author finds the two extension points by grep without
      reading the WS plumbing end-to-end. Add a `// see:
      dev/plans/notebook_interactive_widgets.md` pointer at each
      site.

### Phase 3 — Block-level diffing  **Status:** not started

- [ ] Tag each rendered block with `id="b-<short-hash>"` where the
      hash is blake3 over the block source, truncated to 8 hex
      chars. Content-addressed so a moved-but-unchanged block keeps
      its ID; positional `b<n>` is rejected because inserting a
      block at the top would shift every subsequent ID and degrade
      the partial diff into a full refresh. Position is the
      tiebreaker for duplicate-source blocks.
- [ ] After a re-render, diff against the previous render's per-block
      HTML keyed on the stable ID; emit only changed blocks
- [ ] Page JS: replace only the changed `<section id="b-…">`
      elements; preserve scroll position
- [ ] Note: when the *content* of a block genuinely changes, its
      ID changes too, so the page sees a remove+insert pair rather
      than an in-place swap. Document the scroll-position
      implications.

### Phase 4 — Docs + REPL help  **Status:** not started

- [ ] `docs/notebooks.md`: new section "Interactive watch
      (`--interactive` / default)"
- [ ] `examples/notebooks/README.md`: how to point the server at a
      shipped example
- [ ] AGENTS.md Active Plans row → mark this plan complete on
      landing

### Phase 5 — Polish / optional  **Status:** not started

- [ ] `--watch-dir <DIR>` — watch a whole directory, index page
      lists notebooks
- [ ] Source-pane mode: split view with the rendered output on
      one side and the raw `.md` on the other
- [ ] Optional in-browser editor (Monaco / CodeMirror) writing
      back to the same `.md` — explicitly opt-in via
      `--editable`, because it violates the "only `--obsidian`
      modifies" rule

## Open questions

(All Phase 0 design questions are resolved. Asset embedding
signed off 2026-05-30 and moved into Locked-in design decisions
#15; license hygiene moved into #16. Default port, WS protocol
shape, and widget integration boundary moved into #12, #13, #14.
Phase 1 unblocks once the Phase 0 closing tasks — `cargo tree`
licence audit, binary-size measurement, vendored asset
acquisition — complete.)

## Risks

- **Tokio dependency growth.** Axum pulls in tokio + tower + a handful
  of supporting crates. Trade-off doc covers; ~1.5 MB binary growth
  estimate.
- **WebSocket lifecycle bugs.** Connection survives `tokio` worker
  panic? Page reconnect on server restart? Both need tests.
- **Cross-platform fs watching.** `notify` already handles macOS /
  Linux / Windows for the existing watcher; the server just reuses
  that layer.

## What lands first

The smallest useful slice is Phase 1: skeleton + one-shot render +
plot serving. That's enough to demo *"I can browse my notebook in a
real browser without modifying anything"*. Phase 2 (live re-render)
follows immediately because that's the actual reason users want this.

Phases 3-5 are polish that can ship independently.

## Status log

One dated line per meaningful change. Newest at the top. Keep
this in sync with the Phase checkboxes and the AGENTS.md row.

- 2026-05-30 — Agent-handoff section + status log added; the
  plan is now self-describing for any agent picking it up.
- 2026-05-30 — Review pass tightened locked-ins: loopback-only
  bind (dropped `--bind`), TTY-gated auto-open, cancel-in-flight
  render, LRU plot table (256 entries), content-hash block IDs,
  shared persistent cache; corrected the tokio dependency claim
  (`notify` runs on std-mpsc, axum brings net-new tokio);
  switched plot endpoint to `<index>.svg`.
- 2026-05-30 — **Phase 1 complete.** Server skeleton landed:
  `crates/rustlab-notebook/src/server/{mod.rs, http.rs,
  assets.rs}` (~530 LOC + ~360 LOC of tests). One-shot render at
  startup; current-thread tokio + axum 0.8 serves
  `/notebook.html`, `/assets/*` (embedded KaTeX + Plotly), and
  `/plots/*` (per-server tempdir for animations). Bare-input
  `notebook watch <file.md>` dispatches to `server::start`,
  replacing the prior `cmd_check` fallback. New CLI flags
  `--port` and `--no-browser`. Port 8042 with auto-increment up
  to 10 attempts; explicit `--port` fails loud. Browser
  auto-open is TTY-gated + CI-aware. CDN URLs in the rendered
  HTML are rewritten to local `/assets/…` paths via
  `assets::rewrite_cdn_urls` so no `render.rs` change was
  needed; existing render modes keep CDN refs. Tests: 15 unit
  + 5 integration (490 total in the crate, no regressions).
  End-to-end smoke: ran against
  `examples/notebooks/contour_plots.md` — page loads with no
  CDN references, KaTeX CSS (23 KB) and Plotly JS (4.5 MB)
  serve from `/assets/`. Docs updated. Next: Phase 2 (live
  re-render on save via fs watcher + WebSocket).
- 2026-05-30 — **Phase 0 complete.** Vendored KaTeX 0.16.21
  (1.4 MB) + Plotly 2.35.0 (4.3 MB) under
  `crates/rustlab-notebook/assets/vendor/{katex,plotly}/` via
  the new `dev/scripts/vendor-notebook-assets.sh` (reproducible
  fetch). Each dir has upstream `LICENSE` (both MIT — the
  earlier OFL claim for KaTeX fonts was wrong; KaTeX's bundled
  woff2/ttf/woff are Computer Modern derivatives generated from
  Metafont under KaTeX's project MIT license). Per-file SHA256
  in `assets/vendor/SHA256SUMS`. Repo-root
  `THIRD_PARTY_NOTICES.md` enumerates the new vendored assets.
  Locked-in #16 and trade-off doc § Licensing corrected for
  the KaTeX font-license issue. Phase 1 unblocked.
- 2026-05-30 — Phase 0 closing tasks partial: deps added to
  `crates/rustlab-notebook/Cargo.toml` (axum 0.8 + tokio 1 +
  tower-http 0.6, all with trimmed feature sets). `cargo build
  --release -p rustlab-notebook --features mermaid` succeeded in
  30.6 s incremental. **Binary size delta: +16 KB** (10,473,376
  → 10,490,272 bytes) — much smaller than the +1.5-2 MB estimate
  because rustc's dead-code elimination strips nearly all the
  new crate code; nothing references axum/tokio yet. Real
  binary-size growth lands when Phase 1 code actually uses these
  deps; estimate stands at +1.5-2 MB for that point. License
  audit on the 29 new transitives via `cargo metadata`: all
  permissive, all compatible with `MIT OR Apache-2.0`. Two
  noteworthy: `matchit 0.8.4` is `MIT AND BSD-3-Clause` (both
  notices required; trivially satisfied) and `sync_wrapper 1.0.2`
  is Apache-2.0-only (workspace dual license lets downstream
  pick the Apache path). Still open: vendor KaTeX + Plotly
  bundles; create `THIRD_PARTY_NOTICES.md`.
- 2026-05-30 — Phase 0 design fully signed off. Locked-in
  decisions #11–#16 capture: axum 0.8 + current-thread tokio +
  tower-http; default port **8042** with auto-increment up to 10
  attempts (explicit `--port` still fails loud); WS full-only in
  Phase 2 with partial added in Phase 3; widget integration
  documented in Phase 2 rather than reserved; KaTeX + Plotly
  embedded via `include_bytes!`, pdflatex/tectonic remain
  external; license audit confirms `MIT OR Apache-2.0`
  compatibility for all additions (KaTeX fonts are OFL 1.1,
  shipping their notice; no TLS stack). Phase 0 closing tasks
  remaining: add deps to `Cargo.toml`, run `cargo tree` license
  audit, measure binary-size + clean-build-time deltas, vendor
  KaTeX + Plotly bundles under
  `crates/rustlab-notebook/assets/vendor/`, create
  `THIRD_PARTY_NOTICES.md`.
- 2026-05-30 — Phase 0 work started on branch
  `feature/notebook-interactive-server`. Trade-off doc drafted at
  `dev/plans/notebook_interactive_server-tradeoff.md`.
- 2026-05-30 — Initial design + scoping doc landed.
