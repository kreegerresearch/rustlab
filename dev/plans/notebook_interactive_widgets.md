# Interactive notebook widgets — sliders, option buttons, number inputs

## Agent handoff — read this first

**Where we are:** **COMPLETE — all phases 0–4 done (2026-06-07)** on
branch `feature/notebook-interactive-widgets`. All three v1 widget types
(`slider`, `number`, `option`) render as live controls; `widget("name")`
reads numeric or string values in code; drags/selections re-render over
the WS; the server validates incoming values against declarations and
carries live values over across `.md` reloads; a change triggers a
**scoped re-render** (only the first block that reads the changed widget
and everything downstream re-run); and the feature is documented
(`docs/notebooks.md`), demoed (`examples/notebooks/widgets_demo.md`), and
in `rustlab docs widget`. Nothing outstanding except the optional Phase 5
polish backlog (checkbox/text/dropdown/color, URL-state permalinks,
animation). Locked-in decisions and
phase task lists below are the agreed scope; the locked-ins are
*not* up for renegotiation without explicit user approval. The
four Phase-0 decisions are resolved in Open Questions §1, §2, §4
and the Phase 0 checklist below.

**This plan is unblocked as of 2026-05-30.** The server plan's
Phase 2 (live re-render over WebSocket) shipped, so widgets can
now ride the existing WS channel. The widget extension touchpoint
is already documented inline in
`crates/rustlab-notebook/src/server/ws.rs` at the inbound
`Message::Text` match arm — that's where the
`{"kind":"widget_update",…}` parsing lands. The render coordinator
in `render_loop.rs` will need an additional input channel (or a
small refactor to its signature) for widget-value overrides — see
Open Question §1 below.

**Phase progress at a glance:**

| Phase | State | Headline deliverable | Blocked on |
|-------|-------|----------------------|------------|
| 0 — Design + coordination | **complete (2026-06-07)** | parser = `toml 0.8`; `widget()` value table on `Evaluator` (mirrors `cancel`); `widget("typo")` hard-errors; batch returns defaults | — |
| 1 — Slider only, full re-render | **complete (2026-06-07)** | `rustlab-widget` fence parse, `widget()` builtin, slider HTML, `widget_update` WS | — |
| 2 — All three widget types | **complete (2026-06-07)** | `option` + `number`, validation, value carry-over on `.md` reload | Phase 1 |
| 3 — Scoped re-render | **complete (2026-06-07)** | per-block `widget()` read-set instrumentation, narrow cache invalidation | Phase 2 |
| 4 — Docs + REPL help | **complete (2026-06-07)** | `docs/notebooks.md` section, `examples/notebooks/widgets_demo.md`, AGENTS.md close-out | Phase 2 |
| 5 — Polish (optional) | not started | `checkbox`/`text`/`dropdown`/`color`, URL-state permalinks, animation | Phase 4 |

**Next concrete action:** start Phase 0. Deliver in order:

1. Resolve Open Question §1 — where the `widget(name)` value
   lives inside the evaluator (proposed:
   `&BTreeMap<String, WidgetValue>` threaded through
   `execute_notebook_with_cache`, no globals).
2. Resolve Open Question §2 — `widget("typo")` is a hard error,
   not a silent default. Confirm with user.
3. Coordinate with the server plan: get its Phase 1 to reserve
   the `widget_update` inbound WS message kind and the
   render-with-overrides entry point on the render loop.
4. Confirm `toml = "0.8"` is workspace-available (likely yes).

Bring all four back to the user for sign-off before opening
Phase 1.

**Required reading before touching code:**

- `dev/plans/notebook_interactive_server.md` — the channel
  widgets ride on; do not start without understanding its Phase
  2 design.
- `crates/rustlab-notebook/src/parse.rs` — where the new
  `rustlab-widget` fence info-string is recognised (alongside
  existing `rustlab` and `mermaid` fences).
- `crates/rustlab-notebook/src/execute.rs` —
  `execute_notebook_with_cache` is the call that needs to accept
  the widget value table.
- `crates/rustlab-notebook/src/cache.rs` — `NotebookCache` and
  `hash_block_source`; widget values must participate in the
  cache key for blocks that read them.
- `crates/rustlab-notebook/src/render.rs` and
  `render_markdown.rs` — where `rustlab-widget` fences render as
  HTML `<form>` elements.
- `crates/rustlab-script/src/builtins/...` — pattern for adding
  the `widget(name)` builtin; mirror the registration of an
  existing single-argument string-keyed builtin.

**Workflow rules** (per `AGENTS.md` and user memory):

- Plan-first. If anything below needs to change, **update this
  plan and get user approval** before coding.
- Feature branch only; never push to main. Suggested name:
  `feature/notebook-interactive-widgets`.
- Stage freely (`git add`) but do not commit or push without
  explicit user approval. No `Co-Authored-By: Claude …` lines
  in commit messages.
- Keep the rustlab binary small — all new code lands in
  `rustlab-notebook` and `rustlab-script` (for the `widget`
  builtin), not in the main `rustlab` CLI.
- Update on every meaningful change: (1) the Phase checkboxes
  in this plan, (2) the AGENTS.md "Active Plans" row, (3) the
  Status log at the bottom of this file (one dated line). These
  three views must stay in sync — that's what lets the next
  agent pick up cleanly.
- When a Phase ships: also update `docs/notebooks.md` and the
  REPL help for the `widget` builtin.

## Motivation

Once the interactive server is live, the obvious next ask is
"can I drag a slider and see the plot update?" Today the only way
to sweep a parameter is to edit the source `.md`, save, and watch
the page re-render. That's fine for one-shot exploration but kills
the "what does this filter look like at every cutoff between 0.1
and 0.9?" workflow.

Widgets give the notebook author a way to embed live controls
(slider, option button, number input) whose values feed into code
blocks. The page sends value changes over the existing WS channel;
the server re-runs the affected blocks and pushes new output back.

Everything lives in `rustlab-notebook`. The main `rustlab` binary
stays out of this entirely (per
[`feedback_rustlab_binary_size`](../../../../.claude/projects/-Users-mike-projects-2026-rustlab/memory/feedback_rustlab_binary_size.md)).

## Locked-in design decisions

1. **Widget state is server-side and ephemeral.** Each running
   server keeps a `HashMap<WidgetId, Value>` for the active
   notebook. State is not persisted to the source `.md` (preserves
   the no-source-modification rule from the server plan) and not
   persisted across server restarts. Reload of the source `.md`
   resets values to their declared defaults *unless* the same
   widget name+type still exists, in which case the current value
   carries over.
2. **Widgets feed values into rlab code via a `widget(name)`
   builtin.** Returns the current numeric or string value. The
   alternative — auto-injecting a variable named after the widget
   — was rejected because it pollutes the evaluator scope and
   makes it ambiguous where the value came from when reading the
   notebook.
3. **A widget change re-runs every block that calls
   `widget(<that name>)` and every block downstream of those.**
   Same prefix-cache machinery as a source edit, except the
   invalidation point is "first block that reads this widget"
   instead of "first block whose source changed."
4. **Browser is the source of truth for widget UI; server is the
   source of truth for widget value.** The page renders the
   declared widget from HTML emitted by the server, and emits a
   `widget_update` WS message on every interaction (debounced 50 ms
   for sliders).
5. **Per-server-process state, not per-tab.** Two tabs viewing
   the same notebook see and drive the same widget values. Two
   different notebooks have independent widget state. Multi-tab
   conflict resolution is "last write wins" — acceptable because
   the headline use case is one author at one machine.
6. **No widget syntax in the source `.md` means no widgets.** The
   feature is opt-in per notebook; existing notebooks render
   identically.

## Non-goals (v1)

- Persisting widget state to disk or to URL params (deferred to a
  later "shareable permalink" phase).
- Authoring widgets from the browser. Widgets are declared in the
  source `.md`.
- Two-way binding into Obsidian or any other editor.
- Animation / playback controls (the "scrub through a parameter
  sweep" UX). Out of scope for v1; revisit after widgets land.
- Layout primitives (rows, columns, tabs). One widget per
  declaration block, stacked vertically in source order.

## Widget syntax

Declared with a fenced code block whose info string is
`rustlab-widget` (parallel to the existing `rustlab` and `mermaid`
fences). Body is TOML — small, well-known, no ambiguity around
trailing commas or quoted strings.

```rustlab-widget
name = "cutoff"
type = "slider"
min = 0.1
max = 10.0
step = 0.05
default = 1.0
label = "Cutoff (Hz)"
```

```rustlab-widget
name = "window"
type = "option"
choices = ["hamming", "hann", "blackman"]
default = "hamming"
label = "Window"
```

```rustlab-widget
name = "order"
type = "number"
min = 1
max = 64
default = 8
label = "Filter order"
```

Then in any code block downstream:

```rustlab
fc = widget("cutoff");
n  = widget("order");
w  = widget("window");
[b, a] = butter(n, fc / (fs/2));
plot(freqresp(b, a, w));
```

TOML parsing failures render as an inline `[!CAUTION]` callout
(same pattern as embed errors) and the widget is skipped — the
page still renders, the downstream `widget("name")` calls error
cleanly.

## Widget types v1

| Type | Required keys | Optional keys | Value |
|------|---------------|---------------|-------|
| `slider` | `name`, `min`, `max`, `default` | `step`, `label` | f64 |
| `number` | `name`, `default` | `min`, `max`, `step`, `label` | f64 |
| `option` | `name`, `choices`, `default` | `label` | string |

Reserved for v2: `checkbox` (bool), `text` (string),
`dropdown` (string from a long list), `color` (hex string).

## Server changes

Builds on the interactive server plan; touches:

- **`crates/rustlab-notebook/src/server/widgets.rs` (new)** —
  parse `rustlab-widget` fences, hold the value table, validate
  incoming `widget_update` messages against declared types/ranges.
- **`crates/rustlab-notebook/src/server/ws.rs`** — accept a new
  inbound message `{"kind":"widget_update","name":"cutoff",
  "value":1.5}`; trigger a re-render scoped to dependent blocks.
- **`crates/rustlab-notebook/src/server/render_loop.rs`** — pass
  the current widget value table into execution so `widget(name)`
  resolves. Treat a widget change as a render trigger, same
  pipeline as an fs event but with a narrower invalidation set.
- **`crates/rustlab-notebook/src/render.rs`** — render
  `rustlab-widget` fences as `<form data-widget-name=…>` HTML
  elements with type-specific inputs. The page JS attaches
  listeners and emits `widget_update`.
- **`crates/rustlab-script/src/builtins/...` (new builtin)** —
  `widget(name)` looks up the current value from a thread-local
  / evaluator-scoped context populated by the server before each
  execution. Outside the interactive server (e.g. under
  `notebook render`), `widget(name)` returns the declared default.

The widget value table is *additional input* to the prefix cache
key, so a widget change correctly busts cached output for blocks
that read it without busting blocks that don't.

## Phases

### Phase 0 — Design & coordination  **Status:** complete (2026-06-07)

- [x] Land this plan
- [x] Confirm with the server plan that Phase 1/2 reserve the
      `widget_update` WS message kind and the
      "render-with-overrides" entry point on the render loop —
      the `ws.rs` inbound `Message::Text` arm already carries the
      documented reservation (`ws.rs:44–62`); no render-loop
      signature change needed for Phase 1's full re-render (the
      widget table is installed on the evaluator, see §1 below).
      The scoped-invalidation render-loop change moves to Phase 3.
- [x] Pick TOML parser — `toml = "0.8"` confirmed present as a
      workspace dependency (`Cargo.toml:38`).

**Sign-off (2026-06-07):** all four Phase-0 decisions approved by
the user. Resolutions recorded in Open Questions §1, §2, §4 below.
Decision §1 refines the original "thread a `&BTreeMap` through
`execute_notebook_with_cache`" proposal: the value table is instead
installed on the `Evaluator` (mirroring the existing `cancel` flag),
so the execute signature is unchanged. Approved.

### Phase 1 — Slider only, full re-render  **Status:** complete (2026-06-07)

- [x] Parse `rustlab-widget` fences, slider type only
      (`crates/rustlab-notebook/src/widget.rs` + `parse.rs`
      `Block::Widget`; malformed TOML → Caution callout)
- [x] `widget(name)` builtin returning the current f64
      (`rustlab-script`: `WidgetValue` enum, `Evaluator.widget_values`
      installed via `with_widgets`, intercepted in the `Expr::Call`
      arm alongside `parmap`/`feval`; unknown name / no-notebook are
      hard errors; added to `IMPURE_BUILTINS`)
- [x] Render slider as `<input type="range">` + numeric readout
      (`render.rs::render_widget_html`; static emitters show
      label+value; `JsonBlock::Widget` in JSON)
- [x] WS `widget_update` → full notebook re-render → existing full
      WS push (`server/ws.rs` `parse_widget_update` + client-side
      delegated 50 ms-debounced sender; live values on
      `Notebook.widget_values`; `ServerState.render_tx` lets the WS
      handler ping the coordinator; `render_loop` snapshots the
      values and passes them as overrides to
      `execute_notebook_cancellable`. Server renders are already
      cache-free full re-executions, so a slider drag re-runs every
      block with the new value — Phase 1's "full re-render" for free)
- [x] Integration test: `crates/rustlab-notebook/tests/widgets.rs`
      (socket-free parse→execute→render round trip: override drives a
      new computed output + slider position) and a WS-transport test
      in `tests/server_ws_smoke.rs`
      (`ws_widget_update_triggers_rerender_with_new_value`) driving a
      `widget_update` over a real WS client. Plus `rustlab-script`
      `tests/widgets.rs` for the builtin and `widget.rs` unit tests
      for parsing/coercion.

**Phase 1 note:** server-side validation (clamping incoming values to
declared bounds) is wired in `WidgetDecl::coerce` but not yet enforced
in the WS handler — the render filters overrides to declared names, so
unknown names are harmlessly ignored, and the browser slider's
min/max/step keep values in range. Strict reject-out-of-range is the
Phase 2 task. Docs + `examples/notebooks/widgets_demo.md` are Phase 4.

### Phase 2 — All three widget types  **Status:** complete (2026-06-07)

- [x] `option` (radio group) and `number` (numeric input with
      optional min/max) widget types — `WidgetKind::{Number, Option}`
      in `widget.rs`; parsed/validated (option requires non-empty
      `choices` + default ∈ choices; number bounds optional).
- [x] String-valued `widget(name)` for `option` — `WidgetValue::Text`
      flows through the table; `render_widget_html` emits a radio
      group, `JsonBlock::Widget` carries `widget_type`.
- [x] Server-side validation: `WidgetDecl::coerce(&WidgetValue)`
      validates+clamps; the WS handler looks up the decl
      (`Notebook.widget_decls`, refreshed each render via the new
      `ServerRender { html, widget_decls }` return) and **logs +
      ignores** unknown-name / out-of-range / unknown-choice /
      wrong-type updates — the render loop never sees garbage.
- [x] Value carry-over on `.md` reload: live values persist on
      `Notebook.widget_values`; `build_widget_table` re-coerces every
      override against the *current* declaration each render, so an
      unchanged widget carries its value over (clamped if a numeric
      range narrowed) while a changed type / removed widget / now-
      invalid choice falls back to the declared default. No explicit
      reset pass needed — coercion is the reconciliation.

**Implementation note:** the WS message `value` is now a JSON number
*or* string (`parse_widget_update → WidgetValue`); the client JS sends
strings for `option` and listens on both `input` and `change` (radios
fire `change`). Tests: `widget.rs` unit (number/option parse + coerce),
`tests/widgets.rs` (option string round trip, number clamping,
invalid-choice / wrong-type fallback), `ws.rs` `parse_widget_update`
unit tests, and `server_ws_smoke.rs::ws_option_update_selects_choice…`.

### Phase 3 — Scoped re-render  **Status:** complete (2026-06-07)

- [x] Track which blocks call `widget(name)` during execution —
      `Evaluator` gained a `widget_reads: BTreeSet<String>` that the
      `widget()` builtin appends to; `take_widget_reads()` lets the
      executor harvest each code block's read-set.
- [x] On widget change, invalidate only the prefix cache from the
      first reading block onward — `CacheEntry.widget_reads` stores
      `{name: value}`; `NotebookCache::valid_prefix_widget_aware`
      breaks the prefix at the first block whose source hash *or*
      recorded reads no longer match. The interactive server now uses
      an in-memory prefix cache (`Notebook.render_cache`) via the new
      `execute_notebook_scoped`, so a drag re-runs only the
      widget-reading block and everything downstream (state flows
      forward), reusing earlier blocks from their snapshots.
- [x] Integration test:
      `tests/widgets.rs::widget_change_reruns_only_from_first_reading_block`
      asserts `cached_blocks == 1` (the pre-widget block is reused) on
      a value change, and `scoped_rerender_produces_correct_new_values`
      proves downstream blocks recompute while the upstream stays
      cached. Plus `cache.rs::valid_prefix_widget_aware_breaks_at_…`.

**Refactor note (DRY):** the three execution entry points now share
one `execute_core(blocks, cache, cancel, overrides)`;
`execute_notebook_with_cache` (batch), `execute_notebook_cancellable`
(cache-free server fallback), and `execute_notebook_scoped` (cached +
cancellable server path) are thin wrappers. As a bonus, server
*file-save* renders are now scoped by source hash too (they weren't
before — the server used to re-run every block on each save).
Snapshots restored from the cache get their widget table + cancel flag
re-installed for the current render so the executed tail never reads a
stale table or polls a dead cancel flag.

### Phase 4 — Docs + REPL help  **Status:** complete (2026-06-07)

- [x] `docs/notebooks.md`: new "Interactive widgets" subsection
      (fence syntax, the three v1 types, `widget()`, live/scoped
      re-render, validation, carry-over, batch-default behaviour).
- [x] `examples/notebooks/widgets_demo.md` — slider + number +
      option all driving one waveform plot; renders clean
      (`notebook render` + `notebook check` pass). Picked up
      automatically by `make notebooks` (the gallery is a
      `examples/notebooks/*.md` glob — no index to hand-edit).
- [x] REPL/`docs` help for the `widget` builtin —
      `crates/rustlab-cli/src/commands/repl.rs` `HELP` entry +
      a `language / Notebook` `CATEGORIES` row (`rustlab docs widget`).
- [x] AGENTS.md Active Plans row → marked complete.

### Phase 5 — Polish / optional  **Status:** not started

- [ ] `checkbox`, `text`, `dropdown`, `color` widget types
- [ ] URL-encoded widget state for shareable permalinks
       (`/notebook.html?w.cutoff=2.5`)
- [ ] Multi-tab semantics decision: keep "last write wins" or
      switch to per-tab state
- [ ] Animation: a `play` button on a slider that sweeps through
      its range at a chosen rate

## Open questions

1. **Where does the `widget(name)` value live inside the
   evaluator?** **RESOLVED (2026-06-07):** install an
   `Option<Arc<BTreeMap<String, WidgetValue>>>` field on
   `Evaluator` with a `with_widgets(...)` setter, exactly
   mirroring the existing `cancel: Option<Arc<AtomicBool>>`
   field (`eval/mod.rs:68`, installed by the server via
   `with_cancel`). `None` everywhere except under the interactive
   server. `execute_notebook_with_cache(blocks, cache)` keeps its
   2-arg signature; the server installs values on the evaluator it
   already builds. `WidgetValue` is an enum
   `{ Number(f64), Text(String) }` defined in `rustlab-script`.
   Composes with `deep_clone()` via cheap `Arc` clone, same as
   `cancel`. No thread-locals, no globals — satisfies the original
   intent while avoiding the signature churn.
2. **What happens when a code block calls `widget("typo")`?**
   **RESOLVED (2026-06-07):** hard error. The block fails with a
   `ScriptError` ("unknown widget '<name>'") that renders in that
   block's output like any other rlab error. A silent default
   would mask renamed/forgotten widget references. Locked in.
3. **Debounce window for slider drags.** 50 ms feels right for a
   slider on a fast notebook; too tight if every drag triggers
   a 200 ms render. Make it configurable per widget
   (`debounce_ms = 100`)? Phase 2 decision.
4. **Does `widget()` work under `notebook render` (batch)?**
   **RESOLVED (2026-06-07):** yes, as a defaults snapshot. With
   the §1 design the evaluator's `widget_values` is `None` outside
   the interactive server, so `widget(name)` returns the declared
   default and the rendered HTML is a snapshot at defaults.
   Symmetric with the server plan's batch/cache-sharing rule.
   Confirmed.
5. **Widget state and the prefix cache.** Widget values need to
   participate in the cache key for blocks that read them, but
   *not* for blocks that don't. Easiest implementation: the
   recorded read-set for a block (open question #1's instrumentation)
   is part of that block's cache key. Phase 3 work.

## Risks

- **Builtin-context plumbing.** `widget(name)` is the first
  builtin whose return value depends on out-of-band state from
  the server. The plumbing is small but novel; mis-designed it
  could leak server concerns into `rustlab-script`. Mitigation:
  the value table is a `&BTreeMap<String, WidgetValue>` passed
  through the execution call, no globals.
- **Source-of-truth drift between page UI and server state.**
  A dropped WS message could desync the slider position from the
  rendered output. Mitigation: every WS response includes the
  current widget value table; the page re-syncs its UI from
  server state on each render push.
- **Scope creep into "Jupyter ipywidgets but for rlab."** Plenty
  of room to overbuild here. v1 ships three widget types; Phase 5
  is the explicit valve for everything else.

## What lands first

Phase 1 is the smallest convincing slice: one slider type, full
re-render. That's enough to demo "drag a slider, see the plot
move" against a real notebook. Scoped re-render (Phase 3) is the
performance win that makes widgets feel snappy on large notebooks,
but Phase 1 is shippable without it.

## Status log

One dated line per meaningful change. Newest at the top. Keep
this in sync with the Phase checkboxes and the AGENTS.md row.

- 2026-06-07 — Phase 4 complete; **plan fully delivered (v1).**
  Added the "Interactive widgets" section to `docs/notebooks.md`, a
  `examples/notebooks/widgets_demo.md` (slider+number+option → one
  plot, renders clean), and a `widget` entry in the CLI `HELP` array
  under a new `language / Notebook` category (`rustlab docs widget`).
  Only the optional Phase 5 polish backlog remains.
- 2026-06-07 — Phase 3 complete (scoped re-render). `Evaluator`
  records per-block `widget()` reads (`widget_reads` +
  `take_widget_reads`); `CacheEntry.widget_reads` stores the
  name→value read-set; `NotebookCache::valid_prefix_widget_aware`
  breaks the cached prefix at the first block whose source hash or
  widget reads changed. The three execute entry points were unified
  behind `execute_core`; the server now renders through
  `execute_notebook_scoped` against a per-`Notebook` in-memory
  `render_cache`, so a slider drag re-runs only the first
  widget-reading block onward (earlier blocks restored from
  snapshots) — and file-save renders are now source-hash-scoped too
  (a bonus the server lacked before). Restored snapshots get the
  current widget table + cancel flag re-installed. Tests:
  `valid_prefix_widget_aware` unit, `widget_change_reruns_only_from_
  first_reading_block` (asserts cached_blocks==1) and a correctness
  test that downstream values recompute while upstream stays cached.
  Only Phase 4 (docs + demo) remains, optional.
- 2026-06-07 — Phase 2 complete. Added `number` and `option`
  widget types (`WidgetKind::{Number, Option}`); `widget()` now
  returns strings for options. `WidgetDecl::coerce(&WidgetValue)`
  validates+clamps; the WS handler validates against
  `Notebook.widget_decls` (refreshed each render via the new
  `ServerRender` return) and logs+ignores unknown-name /
  out-of-range / unknown-choice / wrong-type updates. Value
  carry-over on `.md` reload falls out of re-coercing persisted
  overrides against the current decls in `build_widget_table`
  (clamp if a range narrowed, reset to default if the type changed
  or a choice vanished). WS `value` is now number-or-string; client
  JS sends strings for options and listens on `input`+`change`.
  Tests: number/option parse+coerce units, option/number/invalid/
  wrong-type integration cases, `parse_widget_update` units, and a
  WS option round-trip. Verified by batch-rendering a number+option
  notebook. Deferred: scoped re-render (Phase 3), docs + demo
  (Phase 4).
- 2026-06-07 — Phase 1 complete. Slider widgets end-to-end:
  `rustlab-widget` TOML fences parse to `Block::Widget` (malformed →
  Caution callout); `WidgetValue` + `Evaluator::with_widgets` carry
  the value table (intercepted `widget()` builtin, added to
  `IMPURE_BUILTINS`); sliders render as `<input type="range">` with a
  live readout; the WS client sends 50 ms-debounced `widget_update`
  frames; `ServerState.render_tx` + `Notebook.widget_values` let the
  WS handler ping the coordinator, which feeds live values as overrides
  to the (cache-free, full) server render. Tests: `rustlab-script`
  builtin tests, `widget.rs` parse/coerce unit tests, notebook
  `tests/widgets.rs` round trip, and a WS-transport test in
  `server_ws_smoke.rs`. Verified by batch-rendering a slider notebook
  (control + computed default output both correct). Deferred to later
  phases: strict server-side validation (Phase 2), docs + demo
  notebook (Phase 4), scoped re-render (Phase 3).
- 2026-06-07 — Phase 0 complete + signed off. Four decisions
  resolved: (1) `widget()` value table installed on `Evaluator`
  as `Option<Arc<BTreeMap<String, WidgetValue>>>` mirroring the
  `cancel` flag — execute signature unchanged (refines the
  original `&BTreeMap`-through-signature proposal, user-approved);
  (2) `widget("typo")` is a hard `ScriptError`; (3) `ws.rs`
  reservation suffices for Phase 1, render-loop signature change
  deferred to Phase 3; (4) `toml 0.8` confirmed in workspace,
  batch `notebook render` returns declared defaults. Phase 1
  started on branch `feature/notebook-interactive-widgets`.
- 2026-05-30 — Agent-handoff section + status log added; the
  plan is now self-describing for any agent picking it up.
- 2026-05-30 — Server plan's Phase 2 shipped; widgets Phase 1
  is now unblocked. The WS extension touchpoint is documented
  inline in `crates/rustlab-notebook/src/server/ws.rs` at the
  inbound `Message::Text` arm; `render_loop` will need a small
  signature tweak when widget overrides arrive. No widget code
  yet.
- 2026-05-30 — Initial design + scoping doc landed alongside
  `notebook_interactive_server.md`.
