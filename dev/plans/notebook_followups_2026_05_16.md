# Notebook / Plot Follow-ups (after the 2026-05-16 imagesc + watcher cache work)

**Status:** open. Each item is independent; pick them off in any
order. Severity column is honest about whether anything is broken today
vs. dormant.

**Context.** During the 2026-05-15 / 2026-05-16 imagesc-orientation
fix and the per-block watcher cache work, several adjacent issues
came up that were out of scope for those PRs. This plan documents
them so the next agent doesn't have to re-discover them.

## Snapshot

| # | Item | Severity | Effort |
|---|---|---|---|
| B1 | `cmd_render*` mutates process cwd and never restores it | **shipped 2026-05-16** | CwdGuard + process-wide RENDER_LOCK |
| B2 | Plot dir wiped *before* render → brief "missing image" window in Obsidian | **shipped 2026-05-16** | post-render `sweep_orphan_plots` keyed off referenced filenames |
| B3 | `write_output` reads file before every write | **shipped 2026-05-16** | process-wide hash cache; slow path only on first write / divergence |
| P1 | `ExecState` snapshot memory grows O(N\_blocks × symbol\_table\_size) | Documented | LRU cap deferred until real pain |
| P2 | Watcher's `self_writes: HashMap<PathBuf, Vec<u8>>` unbounded | **shipped 2026-05-16** | per-entry memory dropped from O(filesize) to 8 bytes via hash |
| P3 | `strip_render_artifacts` does 4 linear scans per render | Trivial (perf) | ~30 min |
| L1 | New `rustlab notebook check` linter subcommand | **shipped 2026-05-16** | done — 7 lints, `--fix`, `--strict`, CI exit codes |

---

## B1 — `cmd_render*` mutates process cwd and never restores it ✓ SHIPPED 2026-05-16

### Symptom

Every `cmd_render`, `cmd_render_cached`, and `cmd_render_dir` calls
`std::env::set_current_dir(dir)` so that the embed expander and the
script evaluator resolve relative paths against the notebook's parent
directory. The cwd is **never restored** afterwards.

In single-process / single-thread usage today the only visible effect
is that after a one-shot `rustlab notebook render foo.md`, the
process's cwd is permanently `foo.md`'s parent. The CLI exits
immediately so nothing notices. The watcher renders sequentially, so
the per-render cwd drift is invisible too.

### Why it's a latent bug

`set_current_dir` is **process-global** on Unix. There is no
per-thread cwd. Any future feature that:

* renders notebooks in parallel,
* runs a parmap inside a notebook that opens files by relative path,
* runs an animation frame writer using a relative output dir, or
* spawns a thread inside the watcher that reads relative paths,

…will see the cwd move under its feet. None of these exist today, but
parallel rendering is on the obvious roadmap (the per-block cache
implicitly assumes serial execution; a parallel-render mode would
change that).

### Reproducer

```rust
let cwd_before = std::env::current_dir().unwrap();
rustlab_notebook::cmd_render(/* ... */);
let cwd_after  = std::env::current_dir().unwrap();
assert_eq!(cwd_before, cwd_after);  // fails — cwd moved
```

### Fix sketch

Two viable approaches:

1. **Save and restore around each render.** Capture `current_dir()` at
   the top of `cmd_render` / `cmd_render_cached` / `cmd_render_dir` /
   the `index.md` render path, and restore it on every exit path
   (including panics — use a guard struct with a `Drop` impl).
   Smallest diff.
2. **Stop mutating cwd at all.** Thread the host directory through
   to embed expansion and script evaluation explicitly. The script
   evaluator's relative-path opens (`load`, `read`, etc.) would need
   a "base dir" argument. Bigger refactor, but correct.

(1) is the right first step. (2) can follow if parallel rendering
ever lands.

### Files touched (under approach 1)

- `crates/rustlab-notebook/src/lib.rs` — `cmd_render` (line ~352),
  `cmd_render_cached` (line ~404), `cmd_render_dir` (line ~554),
  the `index.md` branch (line ~617). All four sites change cwd; all
  four need a restore guard.

### Tests

- `cmd_render_does_not_leak_cwd` — capture cwd, render, assert cwd
  unchanged. One test per public entry point.

---

## B2 — Plot dir wiped before render ✓ SHIPPED 2026-05-16

Implemented option (2) of the original sketch: the pre-render
`remove_dir_all(plot_dir)` in `render_one_with_tracking` is gone, and
in its place we sweep **after** the renderer has emitted its outputs.
The sweep is reference-driven — `sweep_orphan_plots` reads the
just-written `.md`, extracts every `.svg` / `.gif` basename from
`![alt](url)` references via `referenced_plot_basenames`, and deletes
only renderer-emitted files (`plot-N-<hex>.svg`, `anim-N-<hex>.gif`,
plus the un-hashed fallback names) that aren't in that set. Files
outside the renderer's naming scheme (e.g. `.gitkeep`, hand-dropped
SVGs) are left alone. Tests live in `watch.rs`'s `tests` module:
`referenced_plot_basenames_*`, `is_render_emitted_plot_name_*`,
`sweep_orphan_plots_*`.

### Symptom

`render_one_with_tracking` (watcher) calls `remove_dir_all(plot_dir)`
*before* the renderer writes new plots. For tens-to-thousands of ms
between wipe and write, every plot file is missing. If Obsidian is
loading the previous render's images during that window it can show
a broken-image icon briefly.

Mostly self-healing now that plot filenames carry content hashes
(`plot-1-<hash>.svg`) — when the new render's `.md` references a new
filename, Obsidian's image cache misses and refetches; if the file
exists it gets the new content, if not the user sees one briefly.

### Fix sketch

Two options:

1. **Don't wipe at all.** Hashed filenames make stale files harmless;
   they just sit there until the user runs `notebook clean`. Drop the
   `remove_dir_all` and add a periodic / on-demand cleanup pass.
2. **Wipe after, not before.** Track the set of plot files the
   renderer wrote in this pass (passing it through the cache or a
   counter), then sweep the directory for anything not in that set
   *after* the write.

(2) is the cleanest. (1) trades brief glitches for orphan files; with
hashed names, orphans accumulate unboundedly.

### Files touched

- `crates/rustlab-notebook/src/watch.rs` — the `remove_dir_all` at the
  top of `render_one_with_tracking`.
- `crates/rustlab-notebook/src/render_markdown.rs` — needs to surface
  the set of filenames it wrote so the watcher can post-sweep.

### Tests

- `plot_dir_post_sweep_keeps_current_files_and_removes_orphans` —
  pre-populate a plot dir with mixed files, render, assert only the
  render-produced files remain.

---

## B3 — `write_output` reads the file before every write ✓ SHIPPED 2026-05-16

Implemented the in-memory variant from the original sketch.
`write_output` now consults a process-wide
`WRITE_OUTPUT_HASHES: HashMap<PathBuf, u64>` (built lazily via
`OnceLock`) before doing anything else: if the cached hash equals
`hash_bytes(data)`, we return immediately — no `std::fs::read`, no
`std::fs::write`. Slow path runs only on first write to a path
(cache miss) or when the new bytes differ from what we wrote last;
in that case the existing defensive read-and-compare still applies
and seeds the cache so the next repeat is a pure in-memory hit. The
`hash_bytes` helper that was already in `watch.rs` (added for P2)
was hoisted to `crate::hash_bytes` and shared. Tests:
`write_output_in_memory_hash_skips_disk_read_on_repeat`,
`write_output_slow_path_skips_when_disk_already_matches`, plus the
pre-existing `write_output_*` tests still pass.

### Symptom

`write_output` reads the existing file bytes, compares to what's
about to be written, and skips the write if equal. That's right for
avoiding fs events on a no-op render — but on every render with
actual changes we pay 2× the IO (read + write). For typical notebook
sizes (kilobytes) this is irrelevant. For pathologically large
generated `.md` (a notebook with many embedded notebooks producing
thousands of code-block outputs) it becomes a measurable cost.

### Fix sketch

Use mtime + content hash as a cheap pre-check; only read the file's
bytes if mtime indicates a possible match. Or stash the bytes of the
last successful write in memory (mirroring the watcher's `self_writes`
map) and skip the disk read entirely.

The watcher already does the latter for self-write suppression — could
be unified. Worth ~10 lines of code.

### Files touched

- `crates/rustlab-notebook/src/lib.rs::write_output` (the byte-equal
  skip branch).

### Tests

- Existing `write_output_skips_when_content_unchanged` still applies;
  add `write_output_does_not_read_when_in_memory_hash_matches`.

---

## P1 — `ExecState` snapshot memory

### Status

Already documented in `crates/rustlab-notebook/src/cache.rs` as a
known limitation. **Not actionable yet.**

### What's coming

Per-block cache stores `evaluator.deep_clone() + PlotSnapshot +
RngSnapshot` per executable block. For a notebook with N blocks where
the symbol table holds M MB of state, total cache memory is
**O(N × M)**. The cache is per-notebook; the watcher's
`HashMap<PathBuf, NotebookCache>` adds an outer factor of "number of
notebooks the watcher has ever rendered".

### Next steps when this becomes painful

1. **Measure.** Add an instrumentation hook to report cache size after
   each render. `--cache-debug` flag on `notebook watch`.
2. **LRU cap.** Bound `entries.len()` per notebook. When the cap is
   hit, evict the *oldest* snapshot, NOT the most recent — because
   the chain depends on the previous block's snapshot, evicting from
   the middle would force re-execution from the eviction point on the
   next render. Easier: just cap *total* memory across all caches
   with a config flag; when exceeded, evict whole-notebook caches LRU.
3. **Opt-out directive.** `<!-- nocache -->` on a block tells the
   executor not to snapshot after it. Useful for blocks that
   produce huge intermediates (large matrices) where re-running is
   cheaper than holding the snapshot.

Don't ship any of this until profiling on a real notebook shows it
matters.

---

## P2 — Watcher's `self_writes` unbounded ✓ SHIPPED 2026-05-16

### Symptom

`cmd_watch` initializes `let mut self_writes: HashMap<PathBuf,
Vec<u8>> = HashMap::new();` and never bounds it. Every rendered file's
*entire byte content* sits in the map for the lifetime of the watch
session. Each entry ≈ MB per large notebook × number of notebooks ever
seen. A long-running watcher (days/weeks) over a vault that grows
will accumulate state indefinitely.

### Fix sketch

Replace `Vec<u8>` with a hash digest — we only need byte-equality to
detect self-write echoes, not the bytes themselves:

```rust
let mut self_writes: HashMap<PathBuf, [u8; 32]> = HashMap::new();
```

Hash via the same `DefaultHasher` we already use in `cache.rs`, or
pull `blake3` (already a feature dep). Memory drops from O(file size)
to O(32 bytes) per entry — practically unbounded becomes practically
free.

Also add an LRU cap on the map size as belt-and-suspenders: at most N
recent notebook paths. If the user has a vault with 10 000 notes and
ever opens each in a long-running watcher, the path keys alone could
add up.

### Files touched

- `crates/rustlab-notebook/src/watch.rs` — the `self_writes` map, the
  insertion sites (lines ~103, ~199), and `is_self_write_echo`
  (line ~404) to compare hashes instead of bytes.

### Tests

- `is_self_write_echo_*` tests need their fixture to set hashes
  instead of bytes. Coverage already exists; just adjust the input
  shape.

---

## P3 — `strip_render_artifacts` does 4 linear scans

### Symptom

`strip_render_artifacts` calls:

1. `source.replace(HEADER_emit, "")` — one scan.
2. `strip_legacy_iframes(&s)` — one scan over the result.
3. `strip_legacy_text_outputs(&s)` — one scan.
4. The inline sentinel-region strip — one scan.

For a 1 MB notebook source, that's 4 MB of scanning per render.
Negligible compared to actual rustlab execution, but worth knowing
exists.

### Fix sketch

Single-pass state machine over the source: walk once, emitting bytes
to an output buffer, tracking whether we're inside a sentinel region
or whether the current line is one of the legacy patterns. Cleaner
test surface too — the helpers can stay as documentation, with one
combined hot path.

Defer until profiling shows the strip pass actually matters. Notebook
renders are dominated by `execute_notebook` cost; the strip is in the
noise.

### Files touched

- `crates/rustlab-notebook/src/lib.rs::strip_render_artifacts` and
  the three helpers it composes.

### Tests

- Existing strip tests (10+ in `mod tests`) cover the cases. Reuse
  them against the single-pass implementation.

---

## L1 — `rustlab notebook check` linter subcommand ✓ SHIPPED 2026-05-16

### Motivation

A *generic* "valid markdown" validator is the wrong shape — markdown
is forgiving by design, and "valid" is renderer-specific (GitHub
CommonMark vs Obsidian Live-Preview vs KaTeX vs Plotly). False
positives would eat more time than they save.

A **targeted notebook linter** that catches specific rustlab-shaped
failures the renderer can't (or doesn't surface up-front) would be
genuinely useful — especially as a pre-commit hook.

### Proposed surface

```
rustlab notebook check <path>         # file or dir, recursive
rustlab notebook check <path> --fix   # auto-correct safe issues
rustlab notebook check <path> --strict # warnings count as errors
```

Exit code 0 = clean, 1 = warnings, 2 = errors. CI-friendly.

### Checks to implement (in priority order)

| # | Check | Severity | Auto-fixable? |
|---|---|---|---|
| 1 | Unmatched output sentinels (`<!-- rustlab:output-start -->` without matching end, or end without start) | error | yes — strip via `cmd_clean` |
| 2 | Unclosed ` ```rustlab ` fence | error | no — needs user judgement |
| 3 | `![[Embed]]` reference that doesn't resolve through the embed expander | error | no |
| 4 | Frontmatter YAML that fails to parse | error | no |
| 5 | Plot URL in rendered `.md` whose hashed filename doesn't exist on disk (catches partial writes, watcher race fallout) | error | no — re-render fixes it |
| 6 | Orphan plot files in `_attachments/` or `plots/` that nothing references | warning | yes — delete |
| 7 | Duplicate `Generated by rustlab-notebook` headers that escaped the strip | warning | yes — `cmd_clean` |
| 8 | Mismatched `<details>` blocks (open without close, vice versa) | warning | no |
| 9 | Wikilinks (`[[Target]]`) that don't resolve in the vault | info | no |

### Files touched

- `crates/rustlab-notebook/src/lib.rs` — new `cmd_check` function;
  reuse `parse_notebook`, `expand_embeds`, `strip_render_artifacts`
  for input parsing.
- `crates/rustlab-notebook/src/check.rs` (new) — the lint passes.
  Each check is a small function over the parsed block list and the
  raw source bytes.
- `crates/rustlab-notebook/src/main.rs` — wire the subcommand into
  the standalone `rustlab-notebook` binary. (Originally this plan
  also listed `crates/rustlab-cli/src/commands/notebook.rs`, but the
  notebook subcommand was subsequently removed from the main
  `rustlab` CLI in `aa7bc28` per the "keep rustlab binary small"
  rule, so `check` lives only in the standalone binary.)
- `docs/notebooks.md` — new "§ notebook check — lint rustlab
  notebooks" section.

### Tests

Per-check fixtures:

- A clean notebook → exit 0, no warnings.
- One per check: a corrupted notebook → that specific check fires
  and only that check.
- `--fix` mode round-trip: lint, fix, lint again → clean.
- `--strict` mode: warnings cause exit 1.

### Effort

Estimated **~1 day** for the full set of 9 checks plus CLI wiring,
docs, and tests. The first 5 (errors) are the load-bearing ones; the
last 4 (warnings / info) can ship in a follow-up.

---

## How to pick these up

1. **Start with B1** — small, clear, and prevents a future class of
   bugs. Quick win.
2. **Then P2** — also small, makes long-running `notebook watch`
   safer.
3. **Then L1** — meaningful product surface; tackle as one session.
4. **Then B2 / B3 / P3** — defer until profiling or a user report
   says they matter.

Each section above is self-contained — file paths, fix sketch, and
tests are all listed so the next agent can dive directly into the
fix.
