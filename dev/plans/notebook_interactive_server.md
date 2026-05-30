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
| 2 — Live re-render | **complete** (2026-05-30) | notify fs watcher → debounced render coordinator → WS broadcast (`{"kind":"full",…}`) → page swaps body + re-runs Plotly + KaTeX. WS-client script auto-injected. Reconnect with exponential backoff + visible "disconnected" banner. 21 unit + 5 + 1 integration tests. |
| 3 — Block-level diffing | **complete** (2026-05-30) | renderer wraps every block in `<section class="rl-block" id="b-<hash>">…</section>`; new server::diff module splits a rendered doc and computes per-position changes; coordinator picks `partial`/`full`/`None` per render; WS client gains `applyPartial` for in-place outerHTML swap + script re-exec + KaTeX re-render. 47 unit + 5 + 2 integration tests. |
| 4 — Docs + CLI help | **complete** (2026-05-30) | `docs/notebooks.md` § "Live preview" extended through Phase 2/3 (live reload + partial diffs + scroll preservation); `watch --help` long_about rewritten to lead with the interactive server (default) and demote re-render-on-save to secondary; `examples/notebooks/README.md` gained a "Live-edit one example" section. |
| 5 — Polish (optional) | **items 1–4 done** (2026-05-30) | directory mode (5a), source pane (5b), in-browser editor (5c), real render preemption (5d) all landed; removal-aware partial diffs (item 5) deferred for discussion |

**Next concrete action:** Phase 5 is underway (user asked for items
1–4). 5a (directory mode) is done; 5b (source pane), 5c (in-browser
editor), 5d (preemption) are next, then a discussion of item 5
(removal-aware partial diffs).

Phases 0–4 are complete:
- **Phase 0** — design, deps, vendored assets (offline-capable).
- **Phase 1** — server skeleton (one-shot render over HTTP).
- **Phase 2** — live re-render on save via WebSocket
  (full-document refresh).
- **Phase 3** — block-level partial diffs (scroll-preserving
  in-place swap).
- **Phase 4** — docs + CLI help close-out.

Phase 5 polish items:
- ~~**Real render preemption** (locked-in #9)~~ — **done (5d,
  2026-05-30)**: `Arc<AtomicBool>` cancel flag on the
  `rustlab-script` Evaluator.
- **`--watch-dir <DIR>`** — directory mode + index page.
- **Source-pane / split view** — raw .md alongside rendered.
- **Optional in-browser editor** (Monaco / CodeMirror) — opt-in
  via `--editable` because it would violate the
  "only `--obsidian` modifies" rule.
- **Removal-aware partial diffs** — today a block-count change
  forces full; with an `ops:["remove","insert"]` payload we
  could keep scroll position for structural edits too.

If the user is ready to land, open a PR; otherwise the next
agent can pick any Phase 5 item from the list above.

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

### Phase 2 — Live re-render  **Status:** complete (2026-05-30)

- [x] fs watcher on the input file using `notify`
      (`std::sync::mpsc` bridged to `tokio::sync::mpsc`); watches
      the parent directory non-recursively + filters by file name
      so atomic-rename editor saves still trigger us after the
      inode swap.
      [crates/rustlab-notebook/src/server/render_loop.rs](../../crates/rustlab-notebook/src/server/render_loop.rs)
- [x] WebSocket endpoint `/ws` with `axum::extract::ws`; accepts
      many concurrent connections (each connection is a
      `tokio::sync::broadcast::Receiver` subscriber on
      `ServerState.broadcast`).
      [crates/rustlab-notebook/src/server/ws.rs](../../crates/rustlab-notebook/src/server/ws.rs)
- [~] Render preemption (locked-in #9): Phase 2 ships
      "let-it-finish, then coalesce" — only one render runs at a
      time; once it ends, any pending event triggers exactly one
      fresh render. Real preemption requires rustlab-script to
      poll a cancellation token; deferred to Phase 5 polish.
      Documented in `render_loop.rs` and `docs/notebooks.md`.
- [~] LRU eviction on the in-memory plot table (locked-in #10):
      not needed in Phase 2 — only animations land on disk, the
      Plotly path keeps figures inline. Deferred until renders
      grow an on-disk plot table again.
- [x] Re-render on save → debounce 250 ms → broadcast
      `{"kind":"full","html":"…"}` to every WS subscriber. JSON
      framing handled by `ws::full_envelope`; broadcast payload
      is `Arc<str>` so each receiver clones cheaply.
- [x] Page JS auto-injected into `<head>` via
      `ws::inject_ws_client` (so body replacement on receive
      doesn't double-up the WS connection): connects WS, on
      `kind=full` swaps `document.body`, re-executes inline
      `<script>` tags (re-runs Plotly), re-invokes KaTeX
      `renderMathInElement`. Reconnect with exponential backoff
      500 ms → 5 s capped at 10 attempts; visible red banner on
      disconnect; on reconnect after a disconnect, hard-reloads
      to pick up any state missed during the gap.
- [x] **Documented widget integration touchpoints** (locked-in
      #14): inline `// ── Widget integration extension site ──`
      banner in `ws.rs` at the inbound `Message::Text` arm, with
      a `// see dev/plans/notebook_interactive_widgets.md`
      pointer. The render-overrides channel on the coordinator
      is a smaller extension (one extra parameter on the
      `coordinator` fn signature when widgets land) — the
      widgets plan's Phase 1 should add it then.
- [x] End-to-end test
      `tests/server_ws_smoke.rs::ws_receives_full_envelope_on_file_save`:
      binds an ephemeral port, opens a WS client via
      `tokio-tungstenite`, edits the fixture, asserts the
      `{"kind":"full","html":"…"}` envelope arrives with the new
      content and the local `/assets/` references.

### Phase 3 — Block-level diffing  **Status:** complete (2026-05-30)

- [x] Tag each rendered block with `id="b-<short-hash>"` where
      the hash is the low 32 bits of the chunk's `DefaultHasher`
      digest rendered as 8 hex chars (not blake3 — the existing
      `cache.rs` uses `DefaultHasher` and per-process stability
      is sufficient for a watcher session). Position is the
      tiebreaker for duplicate-source blocks via `-N` suffix.
      Hash is computed over the *rendered* HTML chunk, so the
      ID changes exactly when the rendered output changes (the
      property the diff needs).
      [crates/rustlab-notebook/src/render.rs `finalize_block`](../../crates/rustlab-notebook/src/render.rs)
- [x] After a re-render, diff against the previous render's
      per-block list. **Important deviation from the original
      plan:** the diff is keyed *by source-order position*, not
      by stable ID. Reason: a content edit changes the
      content-hash id, so an id-keyed diff would see "remove old
      + insert new" and force a full refresh on every prose
      tweak. Position-based addressing keeps the partial payload
      tight for the common edit-in-place case. The ID is still
      useful for debugging and DOM addressability.
      [crates/rustlab-notebook/src/server/diff.rs](../../crates/rustlab-notebook/src/server/diff.rs)
- [x] Coordinator classification (`Broadcast::{None, Full,
      Partial}`): block-count change → `Full`; >50% of blocks
      changed → `Full` (partial payload would exceed full
      refresh); zero changes → `None` (skip broadcast); else
      `Partial` with per-position replacements.
- [x] Page JS `applyPartial`: for each `{position, html}`,
      `document.querySelectorAll('section.rl-block')[position]
      .outerHTML = html`, then walk the refreshed section's
      `<script>` tags and re-clone them so they execute (Plotly
      re-init), and call `renderMathInElement` scoped to the
      affected section so KaTeX picks up new math.
      Preserves scroll position because untouched sections never
      move in the DOM.
- [x] Documented the scroll-position contract in
      `docs/notebooks.md`: partial swaps preserve scroll;
      structural edits (count change) and large rewrites fall
      back to `kind:"full"` which resets scroll to top.

### Phase 4 — Docs + CLI help  **Status:** complete (2026-05-30)

- [x] `docs/notebooks.md` § "Live preview" rewritten across
      Phases 2–3 to describe the interactive server default,
      re-render-on-save flow, live-reload via WS, block-level
      partial diffs, scroll preservation, and reconnect/banner
      UX.
- [x] `examples/notebooks/README.md` gained a "Live-edit one
      example with `notebook watch`" section pointing at the
      interactive server, with a concrete command against
      `contour_plots.md`.
- [x] `rustlab-notebook watch --help` long_about rewritten to
      lead with the interactive server (default for single .md
      files) and demote re-render-on-save to the secondary
      mode. New examples for `--port` and `--no-browser`.
      [crates/rustlab-notebook/src/main.rs](../../crates/rustlab-notebook/src/main.rs)
- [x] AGENTS.md Active Plans row reflects Phases 0–4 complete;
      Phase 5 is the only remaining (optional) work.

There is no REPL help to update — `watch` is a CLI-only command
on the standalone `rustlab-notebook` binary; the main `rustlab`
REPL does not expose notebook subcommands. Renaming this phase
"Docs + CLI help" in the table above to match what actually
shipped.

### Phase 5 — Polish / optional  **Status:** items 1–4 done (2026-05-30); item 5 (removal-aware partial diffs) deferred

- [x] **Real render preemption (5d, done 2026-05-30, locked-in #9)** —
      `Evaluator` gained an optional `cancel: Option<Arc<AtomicBool>>`
      (default `None` → REPL/CLI unaffected) polled in `exec_stmt` and
      at the top of While/For iterations, returning the new
      `ScriptError::Cancelled`. `execute::execute_notebook_cancellable`
      builds the evaluator with the flag and returns `None` when
      tripped; `server::render_for_server_cancellable` propagates that.
      The coordinator's `schedule_render` runs each render in its own
      task with a per-notebook generation counter + cancel slot; a new
      save sets the in-flight render's flag and schedules a fresh one,
      and a stale (superseded) render never publishes. Verified: a real
      `while true; end;` notebook stops on the next save with CPU back
      to ~0% (not leaked). The *initial* startup render is the one
      non-preemptible render (nothing to preempt yet). All capture
      state is thread-local so concurrent renders don't collide.
- [x] **Directory mode (5a, done 2026-05-30)** — bare
      `watch <dir>` serves every `.md` under it behind a generated
      index page (`/`), one URL per notebook at `/n/<slug>`.
      Implemented as bare-directory input rather than a separate
      `--watch-dir <DIR>` flag (the `watch` positional already
      accepts a dir; a separate flag would be redundant).
      `ServerState` generalised to `HashMap<slug, Arc<Notebook>>`;
      each notebook owns its own `html`/`prev_blocks`/`broadcast`.
      Per-notebook routes `/n/<slug>`, `/n/<slug>/ws`,
      `/raw/<slug>`, `/plots/<slug>/`. Also hardened browser
      auto-open for WSL/Linux (`wslview`→`xdg-open`→`gio`→
      `sensible-browser`).
- [x] **Source-pane / split view (5b, done 2026-05-30)** — new
      `server::page::inject_chrome` adds a top-right toolbar with a
      "Source" toggle and a slide-in pane showing the raw `.md`
      (fetched from `/raw/<slug>`). Chrome lives outside `<main>`;
      the WS client's `applyFull`/`applyPartial` were rescoped from
      `document.body` to `<main>` so the chrome (and a future open
      editor) survive re-renders. An open read-only pane refreshes
      via a `window.__rlAfterUpdate` hook the WS client calls.
- [x] **In-browser editor (5c, done 2026-05-30)** — opt-in
      `--editable` turns the source pane into a vendored CodeMirror 5
      editor (Markdown mode) whose Save / Ctrl-S `POST`s to
      `/save/<slug>`; the server writes the `.md`, the watcher
      re-renders, and the WS push updates only the rendered `<main>`
      so the editor buffer is preserved. The `/save` route is mounted
      only under `--editable`. CodeMirror vendored under
      `assets/vendor/codemirror/` (MIT), served from
      `/assets/codemirror/…` but referenced only in editable mode.
      Chose CodeMirror 5 over Monaco (single-file bundle, ~190 KB, no
      bundler/AMD loader).

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

- 2026-05-30 — **Phase 5d complete.** Real render preemption
  (locked-in #9). `rustlab-script`: `Evaluator` gained
  `cancel: Option<Arc<AtomicBool>>` (default `None`), builder
  `with_cancel` / `set_cancel`, and a `check_cancelled` polled at the
  top of `exec_stmt` and each While/For iteration → new
  `ScriptError::Cancelled`. Cost when unset: one `is_some` branch per
  statement; REPL/CLI unchanged (4 `tests/cancellation.rs` cases incl.
  interrupting `while true; end;` from another thread). Notebook:
  `execute::execute_notebook_cancellable(blocks, cancel) -> Option<…>`
  (None = abandoned); `server::render_for_server_cancellable` returns
  `Ok(None)` when preempted, and `render_for_server` delegates with a
  never-set flag. Coordinator rewritten: `schedule_render` spawns each
  render in its own task with a per-notebook `render_gen` counter +
  `cancel` slot on `Notebook`; a newer save sets the in-flight flag and
  schedules fresh, and a render whose gen is stale never publishes.
  All notebook capture state is thread-local, so concurrent
  cancel-unwinding + fresh render don't collide. Real-binary smoke: a
  `while true; end;` notebook, on the next save, logs `render preempted`
  then `re-rendered (full)` and CPU returns to ~0% (the runaway thread
  was cancelled, not leaked). 3 `execute` cancellable tests + 1
  `render_loop` preemption test. Full workspace suite 2403 passed, 0
  failed. Docs (`docs/notebooks.md` "Render preemption") +
  `render_loop` doc-comment updated. **Phase 5 items 1–4 all done;
  item 5 (removal-aware partial diffs) deferred for discussion.**
- 2026-05-30 — **Phase 5c complete.** In-browser editor
  (`--editable`). Vendored CodeMirror 5.65.19 (core + CSS + Markdown
  mode, MIT) under `assets/vendor/codemirror/` via the extended
  `vendor-notebook-assets.sh`; SHA256SUMS regenerated, `VENDOR.md` +
  `THIRD_PARTY_NOTICES.md` updated; served from `/assets/codemirror/…`
  (`assets::asset_for_path`). `page::inject_chrome` now branches on
  `PageOpts.editable`: editable mode swaps the read-only `<pre>` for a
  CodeMirror host, links the bundle in `<head>` (+ dark-theme
  overrides), and the controller script wires Edit/Save buttons +
  Ctrl/Cmd-S → `POST /save/<slug>`. The `/save/<slug>` route (added in
  5a) is mounted only under `--editable`. Edit loop: save → fs watch →
  re-render → WS updates only `<main>`, so the editor buffer is never
  clobbered (`window.__rlAfterUpdate` skips reload when editable).
  `--editable` CLI flag → `ServerOpts.editable` → `render_for_server`.
  Chose CodeMirror 5 over Monaco/CM6 (single-file, ~190 KB, no
  bundler). 5 `page` unit tests (readonly vs editable scaffold), http
  save-route gating tests from 5a; end-to-end smoke confirms save
  writes the file and triggers a re-render. Next: 5d preemption.
- 2026-05-30 — **Phase 5b complete.** Source pane / split view.
  New module `server/page.rs`: `inject_chrome(html, theme, opts)`
  injects a `<style>` into `<head>` and a toolbar + slide-in source
  pane before `</body>`. The "Source" button toggles a fixed
  right-side pane that fetches `/raw/<slug>` into a `<pre>` and
  shifts `<main>` left (responsive: full-width pane under 768px).
  `render_for_server` now ends with `page::inject_chrome` (threads a
  new `editable` param through from `build_state`/the coordinator).
  Critical WS-client refactor: `applyFull` no longer replaces
  `document.body` — it swaps only `<main>`'s `innerHTML` (and
  `document.title`), so the injected chrome survives re-renders;
  `applyPartial` unchanged in addressing (sections live in `<main>`)
  but now shares the `rerunScripts`/`rerunKaTeX(root)` helpers and
  fires a `window.__rlAfterUpdate` hook the pane uses to refresh.
  4 `page` unit tests + smoke confirming the toolbar sits after
  `</main>`. Next: 5c in-browser editor.
- 2026-05-30 — **Phase 5a complete.** Directory mode landed.
  `ServerState` generalised from one notebook to
  `HashMap<slug, Arc<Notebook>>` (each `Notebook` owns its
  `html: RwLock<String>`, `prev_blocks: Mutex<Vec<Block>>`, and a
  per-notebook `broadcast` channel). Bare `watch <dir>` serves
  every `.md` under the directory (reuses
  `list_md_files_recursive`, now `pub`) behind a generated index
  page at `/` (reuses `generate_index_html`); single-file mode is
  the one-entry case. URL scheme unified on `/n/<slug>` (slug =
  URL-safe stem via `http::slugify`, `-N` on collision); per-page
  WS at `/n/<slug>/ws`, raw source at `/raw/<slug>`, plots at
  `/plots/<slug>/`. `/notebook.html` kept as a redirect to `/`.
  WS-client script derives its slug from `location.pathname` so it
  stays a static const. `render_loop` watches the dir recursively
  (single file: parent non-recursive), maps each changed path back
  to its notebook by source path, and re-renders just that one.
  Also: hardened browser auto-open for WSL/Linux — try
  `wslview` (under WSL → Windows browser), then `xdg-open`, `gio
  open`, `sensible-browser`; first present-and-exit-0 wins, else
  the existing "open … manually" hint. Extracted `build_state`
  from `start` for testability. New/updated tests: `http` unit
  (slugify, routes, save-route gating), `mod` unit (dir index +
  serves each + per-notebook broadcast isolation), `render_loop`
  unit (slug matching, coordinator), integration smoke updated to
  the new URL scheme. All server tests green. Docs + AGENTS row +
  `--help` updated. Next: 5b source pane.
- 2026-05-30 — Agent-handoff section + status log added; the
  plan is now self-describing for any agent picking it up.
- 2026-05-30 — Review pass tightened locked-ins: loopback-only
  bind (dropped `--bind`), TTY-gated auto-open, cancel-in-flight
  render, LRU plot table (256 entries), content-hash block IDs,
  shared persistent cache; corrected the tokio dependency claim
  (`notify` runs on std-mpsc, axum brings net-new tokio);
  switched plot endpoint to `<index>.svg`.
- 2026-05-30 — **Phase 4 complete.** Documentation +
  CLI-help close-out:
  - `rustlab-notebook watch --help` long_about rewritten to
    lead with the interactive server (default for single .md
    files); demoted re-render-on-save to the secondary mode.
    New examples for `--port` and `--no-browser`.
  - `examples/notebooks/README.md` gained a "Live-edit one
    example with `notebook watch`" section with a concrete
    command against `contour_plots.md`.
  - `docs/notebooks.md` § "Live preview" was already extended
    through Phases 2/3 to cover live reload + block-level
    diffs + scroll preservation + reconnect/banner UX; no
    further changes needed.
  - AGENTS.md row updated to phases 0–4 complete.
  Renamed the phase in the at-a-glance table from "Docs + REPL
  help" to "Docs + CLI help" — `watch` is a CLI-only command
  on the standalone `rustlab-notebook` binary; the main
  `rustlab` REPL doesn't expose notebook subcommands. All
  required phases (0–4) now complete; Phase 5 polish items
  remain optional. Plan is ready to land.
- 2026-05-30 — **Phase 3 complete.** Block-level diffing
  landed. Render path now wraps every diffable block (Markdown,
  Code, Mermaid, Callout) in
  `<section class="rl-block" id="b-<hash>">…</section>` via the
  new `render::finalize_block` helper (additive — existing
  `.prose`/`.code-block` CSS intact). New module
  `crates/rustlab-notebook/src/server/diff.rs` splits a rendered
  doc by scanning section openers, then `compute_changes`
  compares the two block lists **by source-order position** —
  *not* by content-hash id, because a content edit changes the
  id and id-keyed diffing would force a full refresh on every
  prose tweak. `Broadcast::{None, Full, Partial}` classifier
  decides per render: block-count change → Full, >50% changed
  → Full, zero changes → None, else Partial. Coordinator caches
  the previous block list under `state.html.read()` on startup
  and updates per render. WS-client JS grew an `applyPartial`
  case: address by `querySelectorAll('section.rl-block')[position]`,
  `outerHTML =`, re-clone inline `<script>`s in the swapped
  region (Plotly re-init), `renderMathInElement(fresh, …)` for
  KaTeX. Untouched sections never move → scroll position
  preserved. New tests: 3 render unit tests for block-id
  stability/collision/wrapping, 11 diff unit tests covering
  split + classify + envelope, 1 integration test
  `ws_receives_partial_envelope_when_one_of_many_blocks_changes`
  proving end-to-end that editing one prose block (in a
  prose/code/prose/code/prose fixture) yields exactly one
  position-addressed entry in a `kind="partial"` envelope. Full
  crate suite: 512 tests, no regressions (was 497). Real-world
  smoke against `examples/notebooks/contour_plots.md`: appending
  a section logs `(partial: 1 block)`; replacing the whole file
  with a smaller notebook logs `(full)`. Docs updated. Next:
  Phase 4 close-out and/or Phase 5 polish.
- 2026-05-30 — **Phase 2 complete.** Live re-render on save
  landed. New modules:
  `crates/rustlab-notebook/src/server/{ws.rs, render_loop.rs}`
  (~400 LOC + ~140 LOC tests).
  `ServerState` gained `RwLock<String>` for the HTML and
  `broadcast::Sender<Arc<str>>` for re-render notifications.
  `render_loop::spawn` runs a `notify` watcher (parent dir,
  non-recursive, file-name filter) on a std thread, bridges to a
  tokio `mpsc`, and feeds a debounced coordinator that calls
  `render_for_server` via `spawn_blocking`, writes the new HTML
  into state, and broadcasts the JSON-framed envelope
  (`ws::full_envelope`). WS handler subscribes per connection
  via `tokio::select!` (no `futures-util` dep needed). Auto-
  injected `ws::WS_CLIENT_SCRIPT` (placed in `<head>` so body
  replacement doesn't double the WS connection) handles:
  `{"kind":"full",…}` → swap `document.body` → re-execute inline
  `<script>` tags (Plotly re-init) → re-call `renderMathInElement`
  (KaTeX). Reconnect with exponential backoff 500 ms → 5 s
  capped at 10 attempts; visible red banner on disconnect;
  hard-reload on reconnect-after-disconnect. Two adjacent
  locked-in items deferred: real render preemption (#9 — needs
  rustlab-script cancellation tokens; Phase 5) and LRU plot
  table (#10 — only animations write to disk today; defer until
  on-disk plot artefacts return). Widget integration touchpoint
  documented inline in `ws.rs`. End-to-end test
  `server_ws_smoke::ws_receives_full_envelope_on_file_save`
  binds an ephemeral port, opens a `tokio-tungstenite` WS
  client, edits the fixture, asserts the envelope arrives with
  the new content. Real-world smoke against
  `examples/notebooks/contour_plots.md` confirmed live reload
  works. Tests: 497 total in the crate (482 unit + 9 + 5 + 1
  integration), all pass. Docs updated. Next: Phase 3
  (content-hash block IDs + partial diffs for in-place updates
  that preserve scroll position).
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
