# Per-block cell execution + inline cell editing — `notebook watch`

## Agent handoff — read this first

| Phase | What | Status |
|---|---|---|
| 1 | Force-run backend + `data-code-idx` stamping | ✅ complete |
| 2 | WS run path, ▶ Run buttons, running/done status | ✅ complete |
| 3 | Cell save backend (splice + CAS + `save_lock`) | ✅ complete |
| 4 | Inline cell editor UI (CodeMirror, veto hooks) | ✅ complete |
| 5 | Docs (`docs/notebooks.md`, AGENTS.md, `--help`) | ✅ complete |

All phases landed together on `feature/notebook-cell-execution`.
Required reading before touching this area:
`src/server/{ws,cell,render_loop,diff}.rs`, `src/execute.rs` (the
`force_from` clamp), `src/parse.rs::replace_code_block_source`, and the
parent plan `notebook_interactive_server.md` (its 16 locked-in
decisions still bind). Workflow rules: tests in the same commit, keep
AGENTS.md + `docs/notebooks.md` in sync, feature branches + PR.

## Motivation

The interactive server re-executes on *file saves* and *widget
changes* only, and an unchanged block is always a cache hit — there
was no way to re-run a block from the browser, and no Jupyter-style
edit-a-cell loop. This plan adds both without giving up the server's
core invariant: **the page is a consistent projection of the `.md` on
disk**.

## Locked-in design decisions

1. **Run semantics = "run from here."** Running executable block N
   force-re-executes N and everything downstream; upstream replays
   from the prefix cache. No out-of-order kernels, no execution
   counters, no stale-output tracking — the page never shows blocks
   computed against different states.
2. **Force is a one-line cache clamp.** `execute_core` computes
   `valid_k = min(valid_prefix_widget_aware(..), force_from)`. The
   forced tail re-executes on the *identical* snapshot-restore replay
   path a source edit takes, so `hold`, RNG snapshots, and widget
   refresh behave exactly as they do for edits. `NotebookCache`'s API
   is unchanged.
3. **Block addressing = executable ordinal** (`data-code-idx`,
   counting Code + Mermaid in document order — the cache's slot
   numbering, hidden Mermaid included). Stamped by
   `render::finalize_block` *after* content-hashing, so block ids are
   unaffected; inert metadata in batch `render` output (accepted).
   The ordinal is advisory: the executor clamps it, so a stale index
   can widen/narrow the re-run scope but never corrupt state.
4. **Transport = WebSocket for both `run_block` and
   `save_run_block`.** Run must be WS anyway (fire an action, watch
   the broadcast); making save-and-run one WS message gives a single
   ordered channel (write file → request forced render, no
   POST-vs-watcher race) and lets the per-request verdict
   (`cell_saved`) return on the requesting socket, which
   HTTP+broadcast can't express. `POST /save/{slug}` stays as-is for
   the whole-doc editor; both writers serialise on a new
   `Notebook.save_lock`.
5. **Inline editing requires `--editable` AND an embed-free
   notebook** (`embed::has_markdown_embeds`, checked on the host
   source every render *and* re-checked server-side at save time). A
   cell edit is spliced back by fence ordinal, which is only sound
   when every rendered block comes from the host file. ▶ Run ships
   unconditionally — it never writes.
6. **Write-back = positional splice + two guards.**
   `parse::replace_code_block_source` mirrors `parse_notebook`'s
   exact fence rules (bug-compatible — e.g. no generic-fence
   tracking) and touches only the target fence's body bytes. Guard 1:
   CAS — the on-disk block must still equal the editor's seed text.
   Guard 2: post-splice re-parse must round-trip the new source at
   the same ordinal with the same block count, else the save is
   rejected *before* writing. Splice-scanner divergence can therefore
   reject a save but never corrupt a file.
7. **One inline editor at a time; mutual exclusion with the
   whole-doc pane.** A dirty cell vetoes partial patches for its own
   section (`rl-cell-stale` marks it), defers structural swaps with a
   banner, and blocks the reconnect hard-reload (`__rlCellDirty`).
   The doc pane refuses to open over dirty cell edits and vice versa.
8. **Force is one-shot.** A forced render preempted by a newer save
   drops the force (the user clicks ▶ again). Requests landing in the
   same 250 ms debounce window merge: min-of-forces, force beats
   plain.
9. **Terminal status is render-scoped, not block-scoped.**
   `{"kind":"cell_status","state":"done"}` broadcasts after every
   completed latest-generation render (including no-change and
   render-error outcomes; never on preempt/stale) — renders are
   whole-document, so one `done` clears every spinner, exactly once
   per accepted run.

## Non-goals

- Jupyter kernel semantics (out-of-order execution, execution counts,
  stale badges).
- Adding/removing/reordering cells from the cell UI — the whole-doc
  editor covers structure.
- Editing embedded/transcluded blocks, or per-cell editing in
  embed-bearing notebooks (run-only there).
- A rustlab CodeMirror mode (cells edit as plain text).
- Debounce bypass for ▶ Run (shares the 250 ms widget path; revisit
  only if it bites).
- New CLI flags (rides `--editable`).

## Wire format

Browser → server (defensively parsed; garbage logged and dropped):

```json
{"kind":"run_block","idx":3}
{"kind":"save_run_block","idx":3,"source":"…","prev_source":"…"}
```

Server → browser:

```json
{"kind":"cell_status","state":"running","idx":3}      // broadcast on accept
{"kind":"cell_status","state":"done"}                 // broadcast per completed render
{"kind":"cell_saved","idx":3,"ok":true}               // requesting socket only
{"kind":"cell_saved","idx":3,"ok":false,"error":"…"}
```

Reconcile envelopes additionally carry `"codeIdx"` per block so reused
DOM nodes get their ordinal re-stamped after structural edits.

## Touched files

- `src/execute.rs` — `force_from` through `execute_notebook_scoped` /
  `execute_core`.
- `src/server/render_loop.rs` — `RenderRequest { slug, force_from }`
  channel payload, debounce merge, `done` broadcast.
- `src/render.rs` — `finalize_block` extra-attrs + ordinal stamping.
- `src/server/diff.rs` — attr-tolerant id parse, `code_idx` on
  reconcile items.
- `src/server/ws.rs` — `run_block`/`save_run_block` parsing +
  handling, status envelopes, client-script hooks
  (`__rlSend`, `__rlCellMessage`, veto call sites).
- `src/server/cell.rs` (new) — cell UI script/CSS + injection.
- `src/parse.rs` — `replace_code_block_source`.
- `src/embed.rs` — `has_markdown_embeds`.
- `src/server/http.rs` — `Notebook.save_lock`, `render_tx` type.
- `src/server/page.rs` — doc-pane mutual exclusion.
- Tests: unit suites in each module + `tests/server_ws_smoke.rs`
  (`ws_run_block_forces_reexecution_of_unchanged_block`,
  `ws_save_run_block_writes_file_and_rerenders`,
  `ws_save_run_block_rejected_without_editable`).

## Risks / accepted trade-offs

- **RNG determinism surprise**: re-running an unchanged `rand()`
  block reproduces identical values (snapshot restore). Documented in
  `docs/notebooks.md`; honest under run-from-here semantics.
- **`data-code-idx` in batch HTML**: inert; gating it would thread a
  server flag through the shared renderer for zero functional gain.
- **Whole-doc `POST /save` stays last-writer-wins** (unchanged); the
  cell path is CAS-guarded, so a doc-save clobber surfaces as a cell
  CAS rejection, never a torn file.
- **Notebooks with baked render artifacts** are refused by the cell
  saver (fence ordinals of raw vs stripped source could diverge);
  `notebook clean` or the doc editor handles those.
- **Lost force on preemption** (locked-in #8): acceptable; add a
  `pending_force` on `Notebook` only if it bites in practice.

## Status log

- 2026-07-09 — plan written, all phases implemented and tested on
  `feature/notebook-cell-execution`; end-to-end verified against the
  real binary (editable / read-only / embed-bearing / static render).
