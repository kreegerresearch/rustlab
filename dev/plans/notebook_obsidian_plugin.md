# Plan: Obsidian community plugin (`rustlab-notebook`)

**Goal.** A community plugin that turns any note containing a
` ```rustlab ` fence into a live notebook, with **zero infrastructure
beyond two installs**: the user installs the Obsidian plugin and has
the `rustlab` binary on PATH. No background server, no Docker, no
configuration that isn't auto-discoverable.

**Non-goals.** This plan is the plugin itself. The sibling plan
`notebook_obsidian_vault.md` covers the no-plugin path (vault render
mode + watcher). Both can coexist; users pick the one that fits.

---

## User experience contract

The plugin is judged against this single sentence:

> Open Obsidian → install plugin → switch a note to Reading view →
> rustlab code blocks render with their plots and text inline,
> within ~1 s, with nothing else running.

Anything that violates this — a daemon to start, a setting that must
be configured, a dialog that must be dismissed — is a defect.

### What the user sees in Reading view

Each ` ```rustlab ` fenced block renders as:

```
┌─────────────────────────────────────────┐
│ ```rustlab                              │  ← source (syntax-highlighted, collapsible)
│ x = 0:1023;                             │
│ X = fft(x);                             │
│ plot(abs(X(1:512)))                     │
│ ```                                     │
├─────────────────────────────────────────┤
│ [SVG plot — magnitude spectrum]         │  ← rendered output
│                                         │
│ Result: 512 samples, peak at bin 0      │  ← text output (if any)
└─────────────────────────────────────────┘
```

Errors show inline in red, same shape as the rustlab HTML renderer.

Ribbon icon: a single "Run all rustlab blocks" command (force re-run,
ignoring cache). Command palette additions: "Run cell under cursor",
"Clear cached output for this note".

---

## Architecture — one CLI call per render

The simplest design that meets the contract:

1. Plugin registers a markdown post-processor for the entire note.
2. When Obsidian asks the plugin to render a note in Reading view,
   the plugin spawns `rustlab notebook render --format json --stdin`
   once for the note, piping the source into stdin.
3. rustlab returns a single JSON document: per-block source + outputs.
4. The plugin walks the rendered DOM, finds each ` ```rustlab ` block,
   and inserts the matching output beneath it.
5. Results are cached in-memory by `hash(noteText)` so re-opening the
   same note (or switching tabs back) is instant.

**No long-lived process.** Each render is a fresh `rustlab` invocation.
On a typical 10-block notebook the cold-start latency is dominated
by process spawn + JSON serialise, both well under 500 ms on modern
hardware. Notebooks with heavy computations are no worse than running
`rustlab notebook render` on the command line.

**Why not a persistent kernel.** A long-lived `rustlab serve` process
would lower latency for the "edit one block, re-run" loop, but it
breaks the contract: the user (or the plugin) must manage process
lifecycle, port allocation, restart on crash, vault-switch teardown,
etc. We accept the per-render cost in exchange for "it just works."
A future plan can add a server mode as an opt-in, but the default
must be stateless.

---

## Phasing

| Phase | Deliverable | Depends on |
|---|---|---|
| 1 | `rustlab notebook render --format json` (CLI side) | none |
| 2 | Plugin MVP — read-only Reading-view rendering | Phase 1 |
| 3 | Run-cell-under-cursor + cache controls | Phase 2 |
| 4 | Distribution (community catalog submission) | Phase 2 |

---

## Phase 1 — `rustlab notebook render --format json` ✓ SHIPPED 2026-05-17

Implemented in the standalone `rustlab-notebook` binary (not the main
`rustlab` CLI, per the "keep `rustlab` binary small" rule — `rustlab-cli`
does not depend on `rustlab-notebook`). New module
`crates/rustlab-notebook/src/render_json.rs` plus
`cmd_render_json` in `lib.rs`. CLI flags `--format json`, `--stdin`,
`--cwd <DIR>`, `--pretty`. Output goes to stdout only — JSON has no
`--output` path. Schema version 1 (see `Document` / `JsonBlock` /
`JsonPlot` in `render_json.rs`). Tests live in `render_json.rs::tests`;
9 unit tests cover the schema, code/markdown/callout/exercise/solution
shapes, source hashing, plot SVG inlining, and mermaid (with and
without the feature). End-to-end smoke-tested against
`examples/notebooks/quick_look.md` and `embeds_demo.md` (the latter
with `--stdin --cwd`).

**Deviations from the original sketch below.** No `Format::Json` was
added to the public `Format` enum — JSON has stdout-only IO semantics
(no plot-dir, no file path) so the existing file-based `render_output`
pipeline doesn't fit. The CLI dispatches `--format json` to
`cmd_render_json` directly. SVG plots only in v1; Plotly-HTML and
animation-GIF alternates were deferred and remain valid future
additions to the `JsonPlot` variant set without bumping the schema
version (the `format` field is open).

### Original spec (preserved below for reference)

A new output format on the existing CLI. The plugin's only contract
with rustlab.

### Invocation

```
rustlab notebook render --format json path/to/note.md
rustlab notebook render --format json --stdin < note.md   # for unsaved buffers
```

`--stdin` is essential: the plugin renders Obsidian's *current
buffer*, which may not be saved yet. The CLI reads the source from
stdin and treats CWD as the notebook's directory for relative-path
resolution (`![[embeds]]`, image references, frontmatter resolution).
The plugin passes `--cwd <notebook-dir>` so this is unambiguous.

### Output schema

A single JSON document on stdout:

```json
{
  "version": 1,
  "title": "Filter Analysis",
  "blocks": [
    {
      "kind": "markdown",
      "source": "# Filter Analysis\n\nDesign a 64-tap...",
      "html": "<h1>Filter Analysis</h1><p>Design a 64-tap...</p>"
    },
    {
      "kind": "code",
      "language": "rustlab",
      "source": "h = fir_lowpass(64, 3000, 16000);\nplot(...)",
      "source_hash": "blake3:abc123...",
      "text_output": "ans = ...",
      "error": null,
      "plots": [
        { "format": "svg", "data": "<svg>...</svg>" },
        { "format": "html", "data": "<div id='plot-...'>...</div>" }
      ],
      "hidden": false,
      "details": null
    },
    {
      "kind": "mermaid",
      "source": "flowchart LR\n  A --> B",
      "svg": "<svg>...</svg>",
      "caption": null
    },
    {
      "kind": "callout",
      "callout_type": "NOTE",
      "title": null,
      "html": "<div class='callout note'>...</div>"
    },
    { "kind": "exercise_start", "number": 1 },
    { "kind": "solution_start" }
  ],
  "diagnostics": [
    { "level": "warn", "message": "broken embed: ![[missing]]", "block_index": 3 }
  ]
}
```

Design notes:

- **`source_hash`** lets the plugin cache per-block. BLAKE3 because
  the rest of the codebase uses it (mermaid cache, ditto).
- **Two plot formats per block** — SVG (always present) and HTML
  (Plotly, present when interactive). The plugin defaults to SVG;
  users can opt into HTML for interactive Plotly via a setting (the
  HTML uses a CDN so it requires network; SVG is offline). Mirrors
  the existing markdown / HTML format split.
- **Pre-rendered HTML for prose / callouts / mermaid.** The plugin
  *could* re-render markdown itself, but then we'd have two
  renderers' subtle differences to reconcile. Letting the CLI emit
  the HTML guarantees the plugin and the standalone notebook output
  look identical. The plugin sanitises before injecting (no `<script>`,
  no event handlers).
- **`block_index` on diagnostics** lets the plugin show errors next
  to the relevant block in the editor.

### Stability

This becomes a public API — the plugin and any other downstream tool
depends on it. Conventions:

- `version: 1` on the top-level object. Bumped on breaking changes.
- New optional fields are non-breaking; the plugin tolerates unknown
  keys.
- The CLI prints schema version + git commit on `--format json --version`
  so the plugin can warn on mismatch (informational, never blocking).

### Files touched (Phase 1)

| File | Change |
|---|---|
| `crates/rustlab-notebook/src/lib.rs` | Add `Format::Json`. |
| `crates/rustlab-notebook/src/render_json.rs` (new) | Walk `Rendered` blocks, serialise to schema above. |
| `crates/rustlab-notebook/src/main.rs` | Wire `--format json` and `--stdin`. |
| `crates/rustlab-notebook/Cargo.toml` | Add `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`. (Already in workspace; just enable here.) |
| `docs/notebooks.md` | New "JSON output (for tooling)" section with the schema. |

### Tests

In `render_json.rs::tests`:

1. `json_minimal_notebook_has_required_fields` — fixture with one
   markdown + one code block; assert top-level shape.
2. `json_code_block_includes_source_hash`.
3. `json_plot_emitted_as_svg_string`.
4. `json_error_block_serialises_with_error_field_set`.
5. `json_callout_emits_html_pre_rendered`.
6. `json_mermaid_emits_inline_svg`.
7. `json_diagnostic_for_broken_embed_includes_block_index`.
8. `json_stdin_uses_cwd_for_relative_paths` — integration test via
   `assert_cmd`.
9. `json_schema_version_is_1`.

### Done criteria

- `rustlab notebook render --format json examples/notebooks/quick_look.md`
  produces JSON that round-trips through `serde_json::from_str`.
- The standalone HTML and the JSON-rendered HTML for the same source
  produce visually identical output (manual comparison; one
  pixel-diff smoke test).

### Effort

~1 day. The hard work (parsing, executing, capturing plots) is done
— this is a new emitter that walks the same `Rendered` tree.

---

## Phase 2 — Plugin MVP

A TypeScript plugin in a sibling repo (e.g., `obsidian-rustlab`).
Out-of-tree because Obsidian community plugins live under their own
GitHub repos for the catalog submission process. Linked from the main
rustlab README.

### Tech stack

- TypeScript, built with `esbuild` (Obsidian's standard plugin
  template).
- Zero runtime npm deps. The plugin spawns child processes via Node's
  `child_process` (which Obsidian exposes), parses JSON natively.
- Test runner: `vitest` for unit tests (parsers, cache logic);
  manual end-to-end testing via Obsidian itself.

### Settings (auto-detected, all optional)

| Setting | Default | How it's auto-detected |
|---|---|---|
| `rustlabPath` | `"rustlab"` | `which rustlab` / `where rustlab` on plugin load. If not found, plugin shows a single banner with install link; nothing else gates. |
| `theme` | `"match-obsidian"` | Reads `document.body.classList` for `.theme-dark` / `.theme-light`. |
| `plotFormat` | `"svg"` | `"svg"` (offline, default) or `"html"` (interactive Plotly via CDN). |
| `autoRun` | `true` | Render in Reading view automatically. Off → user must invoke "Run all" from the palette. |
| `cacheSize` | `50` notes | LRU cap on the in-memory result cache. |

The plugin does *not* expose vault-render-mode toggles, attachment
paths, frontmatter injection, etc. — none of that is relevant when
Obsidian itself is the renderer.

### Code-block processor

```ts
this.registerMarkdownPostProcessor(async (el, ctx) => {
  const source = ctx.getSectionInfo(el)?.text;
  if (!source || !source.includes("```rustlab")) return;

  const cached = cache.get(hash(source));
  const result = cached ?? await runRustlab(source, vaultPathOf(ctx));
  if (!cached) cache.set(hash(source), result);

  injectOutputs(el, result);
});
```

Key details:

- **Whole-note render, not per-block.** `ctx.getSectionInfo` gives the
  current section; we walk up to the note's full source so the
  rustlab evaluator sees prior blocks' state. Without this, a block
  using `fs` defined three blocks earlier would error out.
- **Vault path threading.** The plugin passes the note's absolute
  path as `--cwd` so file embeds and image references resolve
  identically to a CLI render.
- **Cache key** is `hash(noteSource)`, not per-block, so any change
  to any block invalidates the whole note. (Per-block-with-prefix
  caching is a Phase 3 optimisation.) For 10-block notebooks where
  one block runs in 50 ms and the others run in 5 ms, the whole
  notebook still re-runs in ~100 ms — well within the contract.

### Output injection

For each ` ```rustlab ` block in the rendered DOM:

1. Find the source from `el.querySelector("code.language-rustlab")`.
2. Match against `result.blocks[].source` (exact string compare —
   Obsidian and rustlab agree on the source).
3. Insert a `<div class="rustlab-output">` sibling immediately after
   the `<pre>` containing:
   - SVG plots (sanitised, scaled to container width).
   - Text output (in a `<pre class="rustlab-text">`).
   - Error (in a `<div class="rustlab-error">`, red border).

CSS lives in `styles.css` shipped with the plugin and uses Obsidian's
CSS variables (`--background-secondary`, `--text-error`, etc.) so it
matches the active theme automatically. No theme detection logic
needed beyond letting CSS variables do their job.

### Failure modes

| Failure | Plugin behaviour |
|---|---|
| `rustlab` not on PATH | Banner at top of any rustlab-containing note: "rustlab CLI not found. [Install instructions]". No execution attempted. |
| `rustlab` exits non-zero | Inline `<div class="rustlab-error">` with stderr text under the offending block. |
| `rustlab` JSON unparseable | Same as above — the unparseable text becomes the error message. |
| `rustlab` schema version > plugin's known max | One-time notice: "rustlab newer than plugin; some features may not render". Render proceeds; unknown block kinds skipped silently. |
| Cold spawn slower than 2 s | A spinner replaces the output area while in flight. (Obsidian's default `setIcon('refresh-cw')` with CSS spin.) |

No telemetry, no auto-update prompts, no nag dialogs.

### Files (in the new repo)

| File | Purpose |
|---|---|
| `manifest.json` | Obsidian plugin metadata. |
| `main.ts` | Plugin entry, code-block processor, settings tab. |
| `src/runner.ts` | Spawns `rustlab`, parses JSON, returns typed result. |
| `src/cache.ts` | LRU cache keyed by source hash. |
| `src/inject.ts` | DOM walker that maps rustlab JSON blocks → DOM nodes. |
| `src/types.ts` | TypeScript mirror of the JSON schema. |
| `styles.css` | Output styling using Obsidian CSS variables. |
| `tests/runner.test.ts`, `tests/cache.test.ts`, `tests/inject.test.ts` | Vitest. |

### Tests (plugin)

- `runner_invokes_rustlab_with_stdin_and_cwd` — mock `child_process`,
  assert spawn args.
- `runner_propagates_nonzero_exit_as_error_string`.
- `runner_handles_invalid_json_gracefully`.
- `cache_evicts_lru_at_capacity`.
- `inject_matches_block_by_source_string`.
- `inject_strips_script_tags_from_pre_rendered_html`.
- `inject_renders_svg_inline_not_as_img`.
- `theme_match_uses_obsidian_css_vars` (snapshot test of generated DOM
  classes).

### Done criteria (Phase 2)

- Plugin loads in Obsidian without errors on Mac, Windows, Linux.
- Opening any note in `examples/notebooks/` (copied into a vault) in
  Reading view renders the same plots and text the standalone HTML
  output produces, within 1.5 s for the largest notebook.
- Switching themes (light ↔ dark) re-styles the output without a
  re-render — proves the CSS-variable approach works.
- All eight vitest tests green.

### Effort

~3 days. TypeScript scaffolding, the post-processor, runner/cache,
DOM injection, manual testing across platforms.

---

## Phase 3 — Per-cell run + cache controls

Quality-of-life for power users. Not required for the MVP contract.

### Run cell under cursor

Command palette entry "Rustlab: Run cell under cursor" runs only the
block containing the cursor *plus all preceding rustlab blocks in the
note* (so state is correct). Result replaces only that block's cached
entry; other blocks' cached output is untouched.

Implementation: same `--stdin` invocation as Phase 2, but the plugin
truncates the source at the closing fence of the cursor block before
piping. The rustlab CLI never knows it's a partial render.

### Clear cache

Two commands:

- "Rustlab: Clear cached output for this note" — removes the LRU
  entry for the active note.
- "Rustlab: Clear all cached output" — full LRU flush.

Both are pure plugin work, no CLI changes.

### Per-block cache (optional optimisation)

If users complain about whole-notebook re-runs being slow, switch the
cache from `hash(wholeNote) → blocks[]` to
`hash(blocksUpToAndIncluding(i)) → block[i]`. The CLI invocation is
unchanged; the cache lookup just composes a per-block key by hashing
the cumulative prefix. Expect ~5–10× speedup on
single-block edits in long notebooks. Defer until measured pain.

### Effort

~1.5 days for the run-cell + cache commands. Optional per-block cache:
add ~half a day if/when needed.

---

## Phase 4 — Distribution

Submitting to the Obsidian community catalog. Procedural, not
technical, but documented here so the plan is complete.

1. Repo at `github.com/<user>/obsidian-rustlab` with `manifest.json`,
   `main.js` (built), `styles.css`, README, license.
2. Tag a release matching `manifest.json` version (Obsidian requires
   the tag to be the version with no `v` prefix).
3. Open a PR against
   `github.com/obsidianmd/obsidian-releases` adding an entry to
   `community-plugins.json`.
4. Address review feedback (typically: license, README quality,
   confirming no telemetry, sandboxing of injected HTML).
5. Once merged, the plugin appears in Obsidian's community catalog
   for one-click install.

The link from the rustlab README points users to:

> Install Obsidian → Settings → Community plugins → Browse →
> "Rustlab Notebook" → Install. Then ensure `rustlab` is on PATH.

That's the entire onboarding.

### Pre-distribution: BRAT install path

While the catalog PR is in review (typically 1–4 weeks), users can
install via [BRAT](https://github.com/TfTHacker/obsidian42-brat) by
pasting the GitHub repo URL. Document this in the plugin README as
the "early access" path.

---

## Risks and decisions

1. **Spawn-per-render latency.** Empirically: `rustlab notebook
   render --format json examples/notebooks/spectral_estimation.md`
   completes in ~250 ms cold on a 2023 MacBook. Acceptable. Worst
   case: the largest notebook (`controls_bootcamp.md`-style with
   30+ blocks and heavy computation) might hit 2–3 s. The spinner
   covers it; the cache makes re-opens instant. If real-world
   feedback shows otherwise, Phase 3's per-block cache helps before
   we'd need a server.

2. **Schema versioning discipline.** Once the plugin ships, breaking
   the JSON schema breaks installed plugins. Treat `version: 1` as
   load-bearing: additive changes only; bump to `2` only on a
   breaking change with a deprecation period.

3. **CSP / security.** Obsidian's HTML rendering allows raw HTML
   from markdown post-processors but the plugin must sanitise to
   avoid XSS from notebook content (a malicious `.md` with
   `<script>` in a callout). Use a small allowlist sanitiser
   (DOMPurify is overkill; a 50-line walker that strips
   `<script>`/`<iframe>`/`on*` attributes suffices). Already a
   plugin-best-practice; the catalog reviewers will check.

4. **Plotly via CDN.** The "interactive Plotly" plot format pulls
   `plotly.min.js` from a CDN, which requires network and may be
   blocked by strict CSP / vault security policies. SVG is the
   default for that reason; HTML is opt-in. Document.

5. **Variable persistence model.** This plan is "every render starts
   from scratch." Users coming from Jupyter may expect a kernel
   that survives between renders. The plugin's cache and the
   notebook's whole-note evaluation give *behavioural* persistence
   (block N always sees N-1's state), but a heavy initial block
   (load 1 GB CSV, train a model) re-runs every time the note is
   opened. If this becomes a complaint: Phase 3's per-block cache
   already solves the common case; a true persistent kernel would
   be a future server-mode plan.

6. **Windows path handling.** `child_process.spawn` and stdin
   plumbing differ subtly on Windows (notably, no inherited shell).
   Test on Windows from day one; do not rely on shell features in
   the spawn.

---

## Out of scope (deliberate)

- **Editing-view live render.** Obsidian's Editing view (formerly
  "Live Preview") has a different rendering pipeline than Reading
  view; supporting both doubles the complexity. MVP is Reading view
  only; users get the live experience by toggling.
- **Code-block "Run" buttons in Editing view.** Same reason. Defer.
- **Long-lived `rustlab serve` mode.** Violates the "no
  infrastructure" contract. A future opt-in plan can add it.
- **Vault-wide indexing, search, dataview integration.** Out of
  scope; plugin renders one note at a time.
- **Authoring tools (snippet inserter, plot picker UI).** Pure value
  add; defer.

---

## Sequencing and risk

| Phase | Effort | Risk | Why |
|---|---|---|---|
| 1. JSON output | ~1 day | low | Pure additive; reuses existing pipeline. |
| 2. Plugin MVP | ~3 days | medium | TypeScript + cross-platform process spawning + DOM injection — all well-trodden but several pieces. |
| 3. Per-cell run + cache | ~1.5 days | low | Pure plugin work. |
| 4. Catalog submission | ~0.5 day code, weeks calendar | low (procedural) | Out of our hands once submitted. |

Suggested order: **1 → 2 → (4 in parallel) → 3.** Phase 4 is
calendar-bound, so kicking off the catalog PR right after Phase 2 is
ready hides the review wait behind Phase 3 development.

---

## Open questions

1. **Plugin repo location.** Sibling under the same GitHub org as
   rustlab? Personal repo? Org membership probably matters more for
   discoverability than for code organisation. Recommend: same org,
   `obsidian-rustlab`.
2. **Plugin name displayed in Obsidian catalog.** "Rustlab
   Notebook" reads cleanly; the catalog allows spaces. Confirm with
   user.
3. **Should the plugin ship a sample vault?** A `sample-vault/`
   directory in the plugin repo with 3-4 starter notebooks gives
   users an immediate "try it" experience post-install. Cheap; do
   it.
4. **License match.** rustlab is dual MIT/Apache-2.0. Obsidian
   plugins overwhelmingly use MIT. Recommend MIT for the plugin
   repo to match catalog norms.
5. **Telemetry.** None. Don't add analytics, don't auto-check for
   updates, don't phone home. Obsidian community standards take this
   seriously and so do we.

---

## Files touched (consolidated)

### rustlab repo (Phase 1)

| File | Change |
|---|---|
| `crates/rustlab-notebook/src/lib.rs` | Add `Format::Json` variant. |
| `crates/rustlab-notebook/src/render_json.rs` (new) | Serialise `Rendered` to schema. |
| `crates/rustlab-notebook/src/main.rs` | `--format json`, `--stdin`, `--cwd`. |
| `crates/rustlab-notebook/Cargo.toml` | Enable `serde`, `serde_json`. |
| `docs/notebooks.md` | "JSON output (for tooling)" section + schema. |
| `README.md` | Link to the Obsidian plugin repo once it exists. |

### obsidian-rustlab repo (Phases 2–4)

| File | Purpose |
|---|---|
| `manifest.json` | Plugin metadata. |
| `main.ts` | Entry + post-processor + settings. |
| `src/runner.ts` | CLI invocation + JSON parse. |
| `src/cache.ts` | LRU. |
| `src/inject.ts` | DOM mapping. |
| `src/types.ts` | Schema types. |
| `styles.css` | Obsidian-variable styling. |
| `tests/*.test.ts` | Vitest. |
| `README.md`, `LICENSE` | MIT, install instructions. |
| `sample-vault/*.md` | 4-note starter vault. |

Total estimated diff:

- rustlab side: ~250 lines of production code, ~9 unit tests, no new
  deps (serde already in workspace).
- plugin side: ~600 lines TypeScript, ~8 unit tests, zero npm runtime
  deps.
