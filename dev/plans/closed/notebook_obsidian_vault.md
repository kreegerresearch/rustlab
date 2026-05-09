# Plan: Obsidian vault integration — render-mode upgrades + watcher

**Status:** Complete — both phases shipped.

**Goal.** Make a rustlab notebook directory a first-class Obsidian vault
without writing an Obsidian plugin. The user installs Obsidian, points a
vault at a folder, runs one rustlab command in the background, and gets:

- live-updating rendered notebooks as they edit source `.md` files,
- shared `_setup.md`-style transclusion across notebooks,
- vault-native cross-links and attachments so Obsidian's graph,
  backlinks, and image picker all light up.

**Scope.** Two composable phases (the originally-planned third phase —
file embeds — has shipped separately).

| Phase | Deliverable | Depends on | Status |
|---|---|---|---|
| ~~File embeds (`![[file]]`, `#heading`, `#^block-id`)~~ | parser + embed expander | — | **shipped** — see `dev/plans/closed/notebook_file_embeds.md` |
| A | Obsidian render-mode upgrades (under existing `--obsidian` flag) | none | **shipped** — wikilinks, `_attachments/`, frontmatter merge, iframe (with `--no-iframe`), auto `index.md`. CLI flags `--attachments-dir <DIR>`, `--no-iframe`. |
| B | Watch mode (`rustlab notebook watch <dir>`) | A | **shipped** — `notebook watch` subcommand, `notify` debouncer, embed dependency graph, plot-dir gc, failure isolation. |

References:
- `dev/plans/closed/notebook_obsidian_alignment.md` (callouts,
  wikilinks, footnotes — Phases A–E ✓ done; this plan extends them).
- `dev/plans/closed/notebook_file_embeds.md` (transclusion — done).
- `docs/notebooks.md` § "Obsidian integration (`--obsidian`)" — the
  flag this plan upgrades.

**Flag policy (decision 2026-05-09).** The existing `--obsidian` flag
is the only knob — there is no `--vault`. Today `--obsidian` does just
one thing (append a trailing iframe to the markdown output); this
plan grows it to do all the vault-friendly rewrites listed below.
Since nothing in the wild depends on the iframe-only behaviour, no
deprecation cycle is needed.

---

## Phase A — Upgrade `--obsidian` to render a vault-native directory

The current `--obsidian` flag (`--format markdown --obsidian`) appends
a single trailing `<iframe>` so an Obsidian Reading view can show the
interactive Plotly version inline. That's a useful trick, but it does
nothing to make the *vault* feel native — links still emit as
`[Foo](Foo.md)`, plots go under `plots/<stem>/`, and there's no
frontmatter for vault indexing. Phase A grows the flag's behaviour
to the full superset.

### What changes

1. **Cross-notebook links emit as wikilinks.**
   - Today: `[Filter Design](filter_design.md)` after rewrite.
   - With `--obsidian`: `[[filter_design|Filter Design]]`.
   - Anchored: `[Sec](file.md#section)` → `[[file#section|Sec]]`.
   - The reverse direction is already supported as input
     (`render.rs:1078` wikilink transform), so the round-trip is
     symmetric.

2. **Plots emit to a vault attachments folder.**
   - Today: `plots/<stem>/plot-N.svg` referenced as
     `plots/<stem>/plot-N.svg`.
   - With `--obsidian`: configurable attachment dir, default
     `_attachments/<stem>/plot-N.svg`. Single underscore prefix keeps
     them grouped at the top of Obsidian's file pane and out of the
     way of authored notes.
   - Reference syntax stays standard markdown image
     (`![](_attachments/foo/plot-1.svg)`) — Obsidian renders these
     identically to wikilink embeds and they survive on GitHub.

3. **Frontmatter injection.**
   - If the source has no frontmatter, emit a minimal block:
     ```yaml
     ---
     tags: [rustlab]
     cssclasses: [rustlab-notebook]
     ---
     ```
   - If the source already has frontmatter, *merge* — preserve every
     existing key, add `tags: [rustlab]` only if missing, append
     `cssclasses: [rustlab-notebook]` likewise. Never overwrite.
   - The `cssclasses:` value lets vault users theme rustlab notebooks
     with a CSS snippet (Obsidian's standard mechanism). Out of scope
     for this plan to ship a snippet; documented as a hook.

4. **Iframe behaviour preserved.**
   - The trailing `<iframe>` to the sibling `.html` continues to be
     appended so interactive Plotly works inside Obsidian's Reading
     view. Authors can opt out with `--no-iframe`.

5. **`index.md` becomes a vault home page.**
   - When rendering a directory, the auto-generated `index.html` logic
     already exists. With `--obsidian` we also write/update an
     `index.md` (if the source directory does not provide one) with a
     wikilink list of notebooks in `order:` sequence — Obsidian users
     land on a useful home note instead of an empty vault.

### Files touched

| File | Change |
|---|---|
| `crates/rustlab-notebook/src/lib.rs` | The `Format::Markdown { obsidian: bool }` variant grows new behaviour: when `obsidian` is true the markdown emitter does all five rewrites listed above (today it only appends the iframe). |
| `crates/rustlab-notebook/src/render_markdown.rs` | New `LinkStyle::Wiki` branch in cross-notebook link emission; new `attachments_dir` plumbing; frontmatter merge helper. |
| `crates/rustlab-notebook/src/main.rs` | Add `--attachments-dir <path>` and `--no-iframe`. Existing `--obsidian` flag unchanged at the CLI surface. |
| `docs/notebooks.md` | Replace the existing brief "Obsidian integration (`--obsidian`)" section with one that describes all the rewrites. |

### Tests

In `render_markdown.rs::tests` (names use the `obsidian_` prefix to
mirror the flag):

1. `obsidian_emits_wikilink_for_cross_notebook_link` —
   `[Foo](other.md)` → `[[other|Foo]]`.
2. `obsidian_anchored_link_uses_wikilink_anchor` —
   `[Sec](other.md#sec)` → `[[other#sec|Sec]]`.
3. `obsidian_external_link_unchanged` — `[GH](https://github.com)`
   passes through; obsidian mode never touches absolute URLs.
4. `obsidian_emits_plots_to_attachments_dir` — fixture with one plot
   lands at `_attachments/<stem>/plot-1.svg`, MD references it.
5. `obsidian_injects_minimal_frontmatter_when_absent`.
6. `obsidian_merges_frontmatter_preserving_existing_keys` — input has
   `title: X, tags: [foo]`; output has both plus `tags: [foo,
   rustlab]` and the original `title` untouched.
7. `obsidian_iframe_appended_by_default` and
   `obsidian_no_iframe_flag_suppresses_iframe`.
8. `obsidian_off_keeps_today_behaviour` — without the flag, output is
   byte-for-byte identical to current markdown rendering. Locks down
   the no-regression contract.

### Done criteria

- `rustlab notebook render examples/notebooks/ --format markdown
  --obsidian -o /tmp/vault` produces a directory openable as an
  Obsidian vault with: working backlinks (graph view shows them),
  images visible in Reading view from `_attachments/`, no broken-link
  warnings.
- `--format markdown` (without `--obsidian`) is byte-for-byte
  unchanged from today.
- Test suite green.

### Effort

~1 day. Mostly emitter plumbing; no parser work, no new deps.

---

## Phase B — `rustlab notebook watch <dir>`

A long-running process that re-renders changed notebooks the moment
they're saved. The user runs it once in a terminal; Obsidian then
shows updates in Reading view as they author.

### Behaviour

```
$ rustlab notebook watch examples/notebooks/ -f markdown --obsidian
[watch] watching examples/notebooks/ (3 notebooks)
[watch] filter_analysis.md changed → re-rendering... done in 312 ms
[watch] _setup.md changed → re-rendering 3 dependent notebooks... done in 480 ms
```

Rules:

1. **Debounce** filesystem events at 250 ms — editors emit several
   events per save; we want one render per visible change.
2. **Re-render only what changed.** A change to `lesson_3.md`
   re-renders only that file. A change to `_setup.md` re-renders
   every notebook that embeds it (uses the embed dependency graph
   from the shipped file-embeds work).
3. **Plot dirs are owned by the watcher.** Stale plot files for
   removed blocks are cleaned (compare freshly produced
   `plot-N.svg` set to what's on disk; delete extras). This stops
   `_attachments/` accumulating dead plots.
4. **Hot reload of frontmatter / index.** When `order:` changes or a
   notebook is added/removed, regenerate the directory `index.md`
   in the same render.
5. **Failure isolation.** A parse or execution error in one notebook
   logs to stderr and writes the error inline into the rendered
   output (already what `render` does); the watcher keeps running.

### Dependency graph

The file-embeds expander already tracks which sources each notebook
loads (via the per-render `HashMap<PathBuf, String>` cache in
`crates/rustlab-notebook/src/embed.rs`). For watch mode we extend
that to a **persistent** dependency graph: after each render, the
watcher writes a single JSON-line manifest noting the canonical
paths of every embedded source the notebook touched. On subsequent
filesystem events, the watcher loads the manifest, looks up which
notebooks depend on the changed file, and re-renders just that set.

If the manifest is missing (first run), the watcher uses the
conservative rule: every notebook in the directory re-renders.
Acceptable for ≤20-file vaults; the manifest builds itself after the
first cycle.

### Files touched

| File | Change |
|---|---|
| `crates/rustlab-notebook/Cargo.toml` | Add `notify = "7"` (cross-platform fs watcher; well-maintained, ~5 deps). |
| `crates/rustlab-notebook/src/watch.rs` (new) | Watcher loop, debouncer, dependency-graph cache, plot-dir gc. |
| `crates/rustlab-notebook/src/main.rs` | New `watch` subcommand. Mirrors `render` flags (theme, format, obsidian, attachments-dir). |
| `crates/rustlab-cli/src/commands/notebook.rs` | Wire `notebook watch` through the unified `rustlab` binary. |
| `docs/notebooks.md` | New "Live preview with `watch`" section. |

### Why `notify` and not a custom poll loop

`notify` wraps `inotify` (Linux), `FSEvents` (macOS), and
`ReadDirectoryChangesW` (Windows). Polling is acceptable as a fallback
but burns battery and adds latency; a real fs watcher is the standard
tool for this job and adds <300 KB compiled. Per the licensing memo
this is "infrastructure" not "core numerics" — library use is fine.

### Tests

The watcher itself is hard to unit-test cleanly (real filesystem
events are racy). Cover the tractable pieces:

1. `debouncer_collapses_burst_into_single_event` — feed N synthetic
   events within 250 ms; assert one downstream callback.
2. `dependency_graph_invalidates_dependents` — fixture with three
   notebooks embedding `_setup`; mutate `_setup` mtime; assert all
   three appear in the rerender set.
3. `plot_dir_gc_removes_orphans` — render with 3 plots, edit source
   to produce 2, assert `plot-3.svg` deleted.
4. End-to-end smoke (gated behind `#[ignore]` so CI doesn't run it)
   — spawn the watcher, write a file via `tempfile`, sleep 500 ms,
   assert the rendered output exists and has the new content.

### Done criteria

- `rustlab notebook watch examples/notebooks/ -f markdown --obsidian
  -o gallery/` reflects edits within ~500 ms of save.
- The user-visible loop is: edit in Obsidian Editing view → switch
  to Reading view → see updated plots and text inline. No manual
  rerun.
- Watcher survives parse errors, missing embeds, and renamed files
  without crashing.

### Effort

~1.5 days, including the dependency-graph manifest.

---

## Sequencing and risk

| Phase | Effort | Risk | Why |
|---|---|---|---|
| A | ~1 day | low | Pure emitter changes, mirrors existing `--obsidian` plumbing. |
| B | ~1.5 days | medium | `notify` is mature but cross-platform fs is always a source of edge cases (renames, symlinks). |

**Suggested order: A → B.** A is independently useful; B is the
capstone that makes vault editing feel live.

Per-phase PR. Do not bundle.

---

## Out of scope (deliberate)

- **No Obsidian plugin.** Covered by the sibling plan
  `notebook_obsidian_plugin.md`. The vault mode here is the
  "no plugin needed" path — Obsidian sees a folder of plain
  markdown + SVGs and renders them with its built-in pipeline.
- **No live editing-view rendering.** The watcher only updates files;
  Obsidian itself decides when to re-read them. Reading view shows
  the updated render as soon as Obsidian notices the file change
  (typically <1 s).
- **No bidirectional sync.** Edits made in the rendered `gallery/`
  output are *not* propagated back to source. The pattern stays
  one-way: `examples/notebooks/` → `gallery/`.
- **No `.canvas` or Dataview integration.** Both are vault-native
  Obsidian features; rustlab can be a citizen of those by virtue of
  emitting standard markdown + frontmatter, but explicit support is a
  separate request.

---

## Open questions

1. **Default attachments dir name.** `_attachments/` (groups at top of
   the file pane), `assets/` (no prefix, sorts alphabetically), or
   match the user's vault setting (`Files & Links → Default location
   for new attachments`)? The vault setting is the most "right" answer
   but requires reading `<vault>/.obsidian/app.json`, which is an
   Obsidian internal we shouldn't depend on. Recommend `_attachments/`
   default + `--attachments-dir` override.
2. **Frontmatter `cssclasses` value.** `rustlab-notebook` is verbose;
   `rustlab` alone would collide with the user's general tag. Pick
   `rustlab-notebook` and document.
3. **Watch debounce window.** 250 ms feels right for typical editors
   but may need tuning on Windows (slower fs events). Make it
   configurable via `--debounce-ms` from day one.

---

## Files touched (consolidated)

| File | Phase |
|---|---|
| `crates/rustlab-notebook/Cargo.toml` | B (notify dep) |
| `crates/rustlab-notebook/src/lib.rs` | A |
| `crates/rustlab-notebook/src/main.rs` | A, B |
| `crates/rustlab-notebook/src/render_markdown.rs` | A |
| `crates/rustlab-notebook/src/watch.rs` (new) | B |
| `crates/rustlab-cli/src/commands/notebook.rs` | B |
| `docs/notebooks.md` | A, B |

Total estimated diff: ~500 lines of production code, ~12 unit tests,
one new dependency (`notify`).
