# Interactive notebook widgets — sliders, option buttons, number inputs

## Agent handoff — read this first

**Where we are:** Phase 0 (Design & scoping). No code has landed.
Locked-in decisions and phase task lists below are the agreed
scope; the locked-ins are *not* up for renegotiation without
explicit user approval.

**This plan is blocked on the companion plan.** Widgets require
[`notebook_interactive_server.md`](notebook_interactive_server.md)
to reach **Phase 2** (live re-render over WebSocket) before any
widget code can be wired up. Phase 0 work below — the design
coordination — can and should happen in parallel with the server
plan's Phase 0/1 so the server reserves the `widget_update` WS
message kind and the render-with-overrides entry point.

**Phase progress at a glance:**

| Phase | State | Headline deliverable | Blocked on |
|-------|-------|----------------------|------------|
| 0 — Design + coordination | not started | parser choice, server-plan coordination, builtin context shape | — |
| 1 — Slider only, full re-render | not started | `rustlab-widget` fence parse, `widget()` builtin, slider HTML, `widget_update` WS | server Phase 2 |
| 2 — All three widget types | not started | `option` + `number`, validation, value carry-over on `.md` reload | Phase 1 |
| 3 — Scoped re-render | not started | per-block `widget()` read-set instrumentation, narrow cache invalidation | Phase 2 |
| 4 — Docs + REPL help | not started | `docs/notebooks.md` section, `examples/notebooks/widgets_demo.md`, AGENTS.md close-out | Phase 2 |
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

### Phase 0 — Design & coordination  **Status:** not started

- [ ] Land this plan
- [ ] Confirm with the server plan that Phase 1/2 reserve the
      `widget_update` WS message kind and the
      "render-with-overrides" entry point on the render loop
- [ ] Pick TOML parser (likely `toml = "0.8"`, already in the
      workspace — confirm)

### Phase 1 — Slider only, full re-render  **Status:** not started

- [ ] Parse `rustlab-widget` fences, slider type only
- [ ] `widget(name)` builtin returning the current f64
- [ ] Render slider as `<input type="range">` + numeric readout
- [ ] WS `widget_update` → full notebook re-render → existing full
      WS push
- [ ] Integration test: fixture notebook with one slider, drive a
      `widget_update` over a test WS client, assert the re-rendered
      HTML contains the new plot

### Phase 2 — All three widget types  **Status:** not started

- [ ] `option` (radio group / segmented control) and `number`
      (text input with min/max) widget types
- [ ] String-valued `widget(name)` for `option`
- [ ] Server-side validation: reject out-of-range / unknown-choice
      updates, log + ignore (don't crash the render loop)
- [ ] Widget value table carries over on source `.md` reload when
      the widget declaration is unchanged

### Phase 3 — Scoped re-render  **Status:** not started

- [ ] Track which blocks call `widget(name)` during execution
      (instrument the builtin to record reads against the current
      block ID)
- [ ] On widget change, invalidate only the prefix cache entries
      from the first reading block onward, not from block zero
- [ ] Integration test: prove that a slider drag doesn't re-run
      blocks upstream of the first `widget()` call

### Phase 4 — Docs + REPL help  **Status:** not started

- [ ] `docs/notebooks.md`: new section "Interactive widgets"
- [ ] `examples/notebooks/widgets_demo.md` — one slider, one
      option, one number, all driving a single plot
- [ ] REPL help for the `widget` builtin
- [ ] AGENTS.md Active Plans row → mark complete on landing

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
   evaluator?** Thread-local feels brittle; passing a value table
   through `execute_notebook_with_cache` is cleaner but touches
   the signature. Decide before Phase 1.
2. **What happens when a code block calls `widget("typo")`?**
   Hard error (the block fails and shows in the output, same as
   any other rlab error) is the most honest answer; a silent
   default would hide bugs. Lock in before Phase 1.
3. **Debounce window for slider drags.** 50 ms feels right for a
   slider on a fast notebook; too tight if every drag triggers
   a 200 ms render. Make it configurable per widget
   (`debounce_ms = 100`)? Phase 2 decision.
4. **Does `widget()` work under `notebook render` (batch)?**
   Locked-in #5 of the server plan says the persistent cache
   shares with batch render; symmetric question here. Proposed:
   `widget(name)` returns the declared default in non-interactive
   mode, so the rendered HTML is a snapshot at defaults. Confirm.
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

- 2026-05-30 — Agent-handoff section + status log added; the
  plan is now self-describing for any agent picking it up.
- 2026-05-30 — Initial design + scoping doc landed alongside
  `notebook_interactive_server.md`.
