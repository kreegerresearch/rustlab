# Interactive `notebook watch` — local web server + live page

**Current phase:** Phase 0 — Design & scoping
**Status:** Not started. Watch's bare-input default is currently a
read-only `cmd_check` fallback (see
`crates/rustlab-notebook/src/watch.rs::cmd_watch`); this plan
replaces that fallback with the real interactive experience.

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
5. **Composes with the persistent function cache** at zero extra
   work. A notebook starting with `cache enable` populates
   `.rustlab/cache.db` the same way it does under `notebook render`.
6. **Single notebook per server instance** for v1. Pointing at a
   directory in interactive mode is a follow-up.

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

Dependencies: `axum = "0.8"` (well-maintained, small surface),
`tokio` (already a transitive dep via `notify`), `tower-http` for
static-file handling. Trade-off doc required because these are
infrastructure libraries; see
`dev/plans/notebook_interactive_server-tradeoff.md` (to be
written before Phase 1).

## CLI surface

```
rustlab-notebook watch <input>                          (default — interactive server)
rustlab-notebook watch <input> --port 8765              (override default port)
rustlab-notebook watch <input> --no-browser             (don't auto-open the browser)
rustlab-notebook watch <input> --bind 127.0.0.1:8765    (full bind spec, takes precedence over --port)
```

The existing `--obsidian` and `--output` flags retain their current
meanings and continue to suppress interactive mode (since the user
has explicitly chosen a render destination).

## Wire format / endpoints

| Path | Purpose |
|---|---|
| `GET /` | Redirect to `/notebook.html` (the index for the active notebook) |
| `GET /notebook.html` | The current rendered HTML, with the WebSocket connect snippet injected at the bottom |
| `GET /plots/<hash>.svg` | Served from the in-memory render |
| `GET /assets/<name>` | Static CSS/JS (the same KaTeX bundle that the render uses) |
| `WS /ws` | Re-render push channel. Server sends one of: `{"kind":"full","html":"…"}` (initial / large change), or `{"kind":"partial","blocks":[{"id":"b3","html":"…"}]}` (only the blocks that re-executed) |

The partial-update format makes the prefix cache user-visible: a
prose-only edit pushes a tiny payload, the page does `morphdom`-style
DOM replacement on the changed blocks, no scroll position loss.

## Phases

### Phase 0 — Design & dependency trade-off  **Status:** not started

- [ ] Write `dev/plans/notebook_interactive_server-tradeoff.md`
      (axum vs tiny-http vs hand-rolled, tokio dep surface)
- [ ] Pick the WebSocket protocol shape (full vs partial; see § wire format above)
- [ ] Decide default port + collision behaviour (auto-bump? fail?)
- [ ] Decide auto-browser-open behaviour (`xdg-open` / `open` /
      `start` — refuse on headless CI)

### Phase 1 — Server skeleton  **Status:** not started

- [ ] `server::start(input, opts)` — bind, log the URL, block until
      Ctrl-C
- [ ] `GET /notebook.html` returns a one-shot render of the input
- [ ] `GET /plots/<hash>.svg` serves figures from the render's
      in-memory plot table
- [ ] Initial render runs once at startup
- [ ] Integration test: spawn the server in-process against a fixture
      `.md`, fetch `/notebook.html`, assert the rendered output
      contains the expected text

### Phase 2 — Live re-render  **Status:** not started

- [ ] fs watcher on the input file (reuse `notify` + `watch.rs`'s
      debouncer pattern)
- [ ] WebSocket endpoint `/ws` accepting one connection per page
- [ ] Re-render on save → push `{"kind":"full","html":"…"}` to every
      connected client
- [ ] Page JS: connect WS, replace document on receive, retry on
      disconnect

### Phase 3 — Block-level diffing  **Status:** not started

- [ ] Tag each rendered block with a stable `id="b<n>"` attribute
- [ ] After a re-render, diff against the previous render's per-block
      HTML; emit only changed blocks
- [ ] Page JS: replace only the changed `<section id="b…">`
      elements; preserve scroll position

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

1. **Re-render cost on a save during heavy compute.** If a notebook
   contains a slow `cache enable`-d function and a user edits prose
   while the slow function is running, what does the server do?
   Naive: re-renders are serialised, prose edit waits for compute.
   Better: cancel the in-flight render when a new fs event arrives.
2. **Memory bound for served figures.** Each re-render produces a
   fresh set of plot SVGs; without LRU eviction the in-memory table
   grows unbounded across a long session. Decision needed by Phase
   2.
3. **Browser auto-open vs CI safety.** Phase 0 question (above).
4. **Should the persistent function cache scope to the server's
   own session, or share `.rustlab/cache.db` with other processes?**
   Probably share (the whole point of persistent cache); flag if any
   reason to isolate.

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
