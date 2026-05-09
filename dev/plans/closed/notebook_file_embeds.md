# Plan: File embeds (Obsidian-style transclusion)

Implementation contract: `dev/requests/notebook-file-embeds.md`. This
plan covers architecture, phasing, file-by-file changes, tests, and
the resolved answer to the request's open questions. Decisions already
made in the request are not re-litigated.

## Scope summary

Three forms of `![[...]]` in source notebooks:

| Form | Behaviour |
|---|---|
| `![[Document]]`             | Inline the entire body of `Document.md`. |
| `![[Document#Heading]]`     | Inline the section under `## Heading` until the next sibling/parent heading. |
| `![[Document#^block-id]]`   | Inline the paragraph or list item tagged `^block-id` end-of-line. |

Plus: strip `^block-id` markers from any rendered output (whether the
host file or the embed target), so the marker is invisible in HTML /
PDF / committed Markdown but the source stays Obsidian-portable.

---

## Architecture — text-level pre-process pass

The request suggests two implementation shapes; this plan picks one
and commits to it:

> **Embed expansion happens before `parse_notebook`. The expander
> assembles one flat source string; the parser and executor never know
> embeds existed.**

### Why this shape

- `parse_notebook(src: &str)` and `execute_notebook(blocks)` stay
  byte-for-byte unchanged. Embedded `rustlab` blocks naturally execute
  in the host's evaluator because they end up as ordinary `Block::Code`
  entries in one block sequence (Option A in the request).
- Embedded directives (`<!-- hide -->`, `<!-- details: -->`,
  callouts, mermaid, code-block stacking rules) all "just work" with
  zero parser changes — they're parsed in their natural spot inside
  the flattened source.
- Template interpolation `${expr}` runs at execute time on the already-
  flattened source, so embedded prose can reference host variables and
  vice versa with no special plumbing.

### Where the expander runs

Two call sites in `crates/rustlab-notebook/src/lib.rs`:

- `cmd_render` (line 39): `parse_notebook(&source)` becomes
  `parse_notebook(&expand_embeds(&source, host_dir, root_dir))`.
- `cmd_render_dir` (line 173): same shape, called per notebook with
  per-file `host_dir = md_path.parent()` and `root_dir = dir`.

Plus `read_and_render_index_md` (line 213) for completeness — `index.md`
should be able to embed too.

### Module layout

A new module `crates/rustlab-notebook/src/embed.rs` owns:

- `pub fn expand_embeds(src, host_dir, root_dir) -> String` — entry.
- `EmbedRef { target, anchor }` parser — recognises `![[...]]` only
  outside fenced code blocks and inline code spans (re-uses the same
  fence-tracking logic the existing math-protection pass uses).
- Path resolver — the rules below.
- Source loader with cache (`HashMap<PathBuf, String>`).
- Section slicer (`#Heading`).
- Block-id slicer (`#^id`).
- Block-id strip pass (applies to every loaded source, embed or host).
- Heading demoter (line-aware, respects fenced code blocks).
- Recursive expander with depth + cycle tracking.

Public API is just `expand_embeds`. Everything else is `pub(crate)` or
private; tested via the public entry plus unit tests on individual
helpers.

---

## Phase 1 — Resolver, slicer, and strip pass (no recursion)

Build the leaf operations first. All callable from unit tests; no
integration with the renderer yet.

### Tasks

1. **`EmbedRef` parser.**
   - Regex-free hand-roll for the `![[target#anchor]]` token, mirroring
     the style of the wikilink parser already in `render.rs:1078`.
   - `target` is everything before `#` or `]]`, trimmed.
   - `anchor` is empty (whole file), `Heading` (section), or `^id`
     (block ref). Tag the variant in the returned struct.
   - Skip occurrences inside fenced code blocks (` ``` `, `~~~`) and
     inline code (`` ` ``). Reuse the fence-state machine the existing
     `protect_math` pass uses (`render.rs` ~line 957).

2. **Path resolver.**
   ```rust
   fn resolve(target: &str, host_dir: &Path, root_dir: &Path)
       -> Result<PathBuf, EmbedError>
   ```
   Try (in order):
   1. `host_dir.join(target).with_extension("md")` if the target has no
      extension; or `host_dir.join(target)` if it does.
   2. Same against `root_dir`.
   3. Case-insensitive basename fallback in each directory (Obsidian
      compat — useful on Linux fs).
   4. Error `EmbedNotFound { target }`.

3. **Source loader with cache.**
   - `HashMap<PathBuf, String>` keyed by the canonical resolved path.
   - One read per file per render invocation, regardless of how many
     embeds reference it. Frontmatter is **not** stripped — embedded
     files often have frontmatter that the embedder may want hidden.
     Strip frontmatter before slicing/stripping (it never appears in
     the inlined output).

4. **Section slicer.** `slice_section(src: &str, heading: &str) -> Option<&str>`
   - Walk lines, track fence state, find the first heading line whose
     trimmed text matches `heading` case-insensitively.
   - Capture from that heading line through the line *before* the next
     heading at the same level or higher (or EOF).
   - Heading collision: pick the first match (matches Obsidian; per
     request open question #2).
   - Return `None` if not found (callers convert to error block).

5. **Block-id slicer.** `slice_block_id(src: &str, id: &str) -> Option<String>`
   - Walk lines, track fence state, find the first non-code line whose
     trimmed end matches `r"\s\^<id>$"` (literal id). Per request open
     question #3, end-of-line only (matches Obsidian).
   - Return the surrounding paragraph (lines from the previous blank
     line through the next blank line), with the `^id` token stripped.
   - For list items (`- `, `* `, `1. `), return only that one item
     line, marker-stripped.

6. **Block-id strip pass.** `strip_block_ids(src: &str) -> String`
   - Walk lines, track fence state, strip the `\s\^[a-zA-Z0-9_-]+\s*$`
     suffix from non-code lines.
   - Applied unconditionally to every loaded source — host or embed —
     so block-id markers never leak to rendered output. (Request line
     118: "`^blockid` token on a paragraph not referenced by any embed
     is stripped from non-embed renders.")

### Files touched (Phase 1)

| File | Change |
|---|---|
| `crates/rustlab-notebook/src/embed.rs` | New module — all of the above. |
| `crates/rustlab-notebook/src/lib.rs` | `pub mod embed;` declaration only. |

### Tests (Phase 1)

In `embed.rs::tests`:

1. `parse_embed_ref_simple` — `![[Doc]]` → `target="Doc"`, anchor=None.
2. `parse_embed_ref_heading` — `![[Doc#Foo Bar]]` → anchor=Heading("Foo Bar").
3. `parse_embed_ref_block_id` — `![[Doc#^my-id]]` → anchor=BlockId("my-id").
4. `parse_embed_ref_skipped_in_fence` — `![[X]]` inside a ``` ``` `` fence
   not detected.
5. `parse_embed_ref_skipped_in_inline_code` — same for `` `![[X]]` ``.
6. `resolve_host_dir_first` — file in host dir wins over file in root.
7. `resolve_root_dir_fallback` — host doesn't have it, root does.
8. `resolve_case_insensitive_fallback` — `setup.md` resolves
   `[[SETUP]]`.
9. `resolve_missing_returns_error`.
10. `slice_section_basic` — extracts the right subtree.
11. `slice_section_until_sibling_heading` — sibling `## Other` ends section.
12. `slice_section_includes_nested_subheadings` — `### Sub` is part of `## Foo`.
13. `slice_section_case_insensitive_match`.
14. `slice_section_first_collision_wins`.
15. `slice_section_skips_headings_in_code_fences`.
16. `slice_block_id_paragraph` — paragraph extracted, marker stripped.
17. `slice_block_id_list_item_only` — list line only, not whole list.
18. `slice_block_id_missing_returns_none`.
19. `strip_block_ids_paragraph` — `... ^foo` → `...`.
20. `strip_block_ids_preserves_code_fences` — `^id` inside ``` ``` `` left alone.
21. `strip_block_ids_preserves_inline_code` — `` `^id` `` left alone.

### Done criteria (Phase 1)

- All Phase 1 tests green.
- No integration yet — `cmd_render` still calls `parse_notebook`
  directly. Phase 1 is dead code at this point, integrated in Phase 3.

### Effort

~1 day. Most of the code is straightforward line-walking with fence
tracking — the same idiom used by `protect_math`.

---

## Phase 2 — Heading demoter and recursive expander

### Heading demoter

`fn demote_headings(src: &str, levels: usize) -> String`

- Walk lines, track fence state.
- For each `# `, `## `, `### `, … line outside fences, prepend
  `levels` extra `#` characters.
- Cap demotion at level 6 (HTML/Markdown's max). Headings already at
  level 6 stay there with no demotion (a one-line "depth exceeded"
  warning to stderr, but not a hard error — the source still renders
  correctly).
- Heading attributes `# Title {#id}` survive demotion (the trailing
  `{#id}` is part of the line, untouched).

### Recursive expander

```rust
fn expand_recursive(
    src: &str,
    host_dir: &Path,
    root_dir: &Path,
    depth: usize,
    visiting: &mut HashSet<PathBuf>,
    cache: &mut HashMap<PathBuf, String>,
) -> String
```

For each `![[ref]]` found in `src`:

1. Resolve to `path`. If unresolved → emit a callout-formatted error
   block in place (see "Error rendering" below) and continue.
2. If `depth + 1 > MAX_DEPTH (4)` → emit "max depth exceeded" error
   block and continue.
3. If `path ∈ visiting` → emit "cycle detected" error block (with the
   path of the chain), continue.
4. Insert `path` into `visiting`.
5. Load source via cache (frontmatter stripped, block-ids stripped).
6. Slice by anchor (if any). If the anchor doesn't resolve → emit
   "heading not found" or "block id not found" error; continue.
7. Recursively `expand_recursive(sliced, path.parent(), root_dir,
   depth + 1, ...)`.
8. Demote headings in the recursively-expanded result by 1.
9. Substitute `![[ref]]` → demoted result in the host source.
10. Remove `path` from `visiting`.

Substitution policy: the embed link occupies its own line in source
(the typical Obsidian style). Inline embeds (`See ![[X]] for ...`) are
replaced byte-for-byte with the embed contents — for a single-paragraph
or single-list-item embed this reads naturally; for a whole-document
embed it produces a paragraph break which is acceptable. No special
handling for "block vs inline" position; user-visible behaviour is
predictable.

### Error rendering

Errors emit as a GFM callout that the existing pipeline already
styles consistently across HTML / Markdown / LaTeX / PDF:

```
> [!CAUTION] Embed error
> Document not found: setup.md
```

This is already supported by the callout parser (added in Phase A of
`notebook_obsidian_alignment.md`). Zero new renderer code, sensible
visuals everywhere, and the error survives in the committed Markdown
output for review.

A single-line stderr warning accompanies each error so CI logs flag
broken embeds:

```
warning: embed error in lessons/02.md: target not found: setup
```

### Public entry

```rust
pub fn expand_embeds(src: &str, host_dir: &Path, root_dir: &Path)
    -> String
{
    let mut visiting = HashSet::new();
    let mut cache = HashMap::new();
    let stripped = strip_block_ids(strip_frontmatter(src));
    expand_recursive(&stripped, host_dir, root_dir, 0,
                     &mut visiting, &mut cache)
}
```

Note: the host file goes through `strip_block_ids` too, so a notebook
that uses `^id` markers internally has them removed even if no one
embeds it.

### Files touched (Phase 2)

| File | Change |
|---|---|
| `crates/rustlab-notebook/src/embed.rs` | Add `demote_headings`, `expand_recursive`, `expand_embeds`. |

### Tests (Phase 2)

22. `demote_headings_increments_each_level`.
23. `demote_headings_caps_at_h6`.
24. `demote_headings_skips_fenced_code`.
25. `demote_headings_preserves_attributes` — `# Title {#id}` → `## Title {#id}`.
26. `expand_simple_full_file_inlines_content`.
27. `expand_anchored_section_only_emits_subtree`.
28. `expand_block_id_emits_paragraph_marker_stripped`.
29. `expand_recursive_demotes_headings_per_level` — three-level chain
    `A → B → C`; H1 in C ends up as H3 after expansion in A.
30. `expand_cycle_emits_caution_callout` — `A` embeds `A`.
31. `expand_indirect_cycle_emits_caution_callout` — `A → B → A`.
32. `expand_max_depth_emits_caution_callout`.
33. `expand_unresolved_target_emits_caution_callout`.
34. `expand_unresolved_heading_emits_caution_callout`.
35. `expand_cache_loads_each_source_once` — instrument via test-only
    counter; assert two `![[setup]]` references trigger one read.
36. `expand_strips_block_ids_from_host_source` — host has `... ^id`;
    rendered output has no `^id`.
37. `expand_strips_frontmatter_from_embed_source` — embedded file's
    YAML frontmatter does not appear in output.

### Done criteria (Phase 2)

- All Phase 2 tests green.
- Still no production callers; integration is Phase 3.

### Effort

~1.5 days. Recursive walker, cycle/depth bookkeeping, error formatting.

---

## Phase 3 — Wire into render entry points and ship

### Tasks

1. **`cmd_render`** — read `host_dir` from `input.parent()`,
   `root_dir = host_dir` (single-file render has no notebook
   directory), call `expand_embeds` before `parse_notebook`.
2. **`cmd_render_dir`** — for each pending notebook,
   `host_dir = md_path.parent()`, `root_dir = dir`. Wrap the existing
   `parse_notebook(&p.source)` call.
3. **`read_and_render_index_md`** — call `expand_embeds(&source,
   index_md_path.parent(), dir)` so `index.md` can embed too. Cheap
   and consistent.
4. **Documentation** — new section in `docs/notebooks.md` between
   "Wikilinks and embeds" and "Directives". Cover:
   - All three syntaxes with one example each.
   - Resolution order.
   - Heading demotion behaviour.
   - Cycle / depth limits.
   - Embedded `rustlab` blocks share the host evaluator (Option A).
   - `^id` markers are stripped from rendered output.
   - Errors render as `[!CAUTION]` callouts inline.
5. **AGENTS.md update** — per the workflow rule, mention the new
   feature in the rustlab `AGENTS.md` notebook section. One-liner
   pointing readers at `docs/notebooks.md`.
6. **Example notebook** — add `examples/notebooks/_setup.md` and a
   `embeds_demo.md` that transcludes from it. This becomes both
   documentation-by-example and a smoke test for the gallery build.

### Tests (Phase 3 — integration)

In `lib.rs::tests` (or a new `tests/embeds.rs` integration test):

38. `integration_embed_full_file_renders_inlined` — fixture dir with
    `host.md` and `setup.md`; render `host.md`; assert HTML contains
    setup's contents.
39. `integration_embedded_rustlab_block_shares_evaluator_state` —
    `setup.md` defines `Fs = 48000`; `host.md` runs `2 * Fs` after
    `![[setup]]`; assert the printed value is 96000.
40. `integration_embed_heading_demotion_visible_in_html` —
    embedded `# Foo` becomes `<h2>Foo</h2>`.
41. `integration_unresolved_embed_renders_caution_callout` — assert
    the rendered HTML has `class="callout caution"` containing the
    error message.
42. `integration_block_id_marker_stripped_from_host_render` —
    `host.md` contains `... ^my-id`; rendered output has no `^my-id`.

### Done criteria (Phase 3)

- All test phases green: `cargo test -p rustlab-notebook`.
- `make notebooks` succeeds with the new `_setup.md` / `embeds_demo.md`
  example included.
- Manually verify the rendered `embeds_demo.html` shows: inlined
  setup content, demoted headings, executed shared variables.
- Request `dev/requests/notebook-file-embeds.md` archived — annotate
  "RESOLVED in <commit>".

### Effort

~0.5 day for wiring + docs + example. Integration tests are
straightforward `tempfile`-based fixtures.

---

## Resolved decisions on the request's open questions

| # | Question | Decision |
|---|---|---|
| 1 | Wikilink-in-prose without embed | **Out of scope.** Already implemented as a wikilink → markdown-link transform in `render.rs:1078`. This plan only handles the `![[...]]` (with bang) form. |
| 2 | Heading collision in target | **First match wins.** Matches Obsidian. Implemented in `slice_section`. |
| 3 | Block-id placement | **End-of-line only.** Matches Obsidian. Future extension if requested; not in v1. |
| 4 | Caching | **Implemented from day one.** `HashMap<PathBuf, String>` per `expand_embeds` invocation. |
| — | Execution semantics (Option A vs B) | **Option A — execute embedded code as if local.** No opt-out marker in v1. Rationale: matches the strongest motivation example (`_setup.md`); the surface stays minimal; an opt-out can be added later as a separate request without breaking changes. |

---

## What we are explicitly NOT doing

- **No `[[wikilink]]` (without `!`) changes.** Already handled by the
  existing wikilink transform; this plan touches only `![[...]]`.
- **No `<!-- embed-noexec -->` opt-out marker.** Defer until someone
  asks; Option A is the unanimous use case in the motivation list.
- **No support for `^id` markers on their own line.** Match Obsidian
  (end-of-line only); revisit only if vault users complain.
- **No partial range syntax** (e.g. `![[Doc#Heading-1..Heading-2]]`).
  Out of scope; not in the request.
- **No image-or-PDF embed handling.** `![[diagram.svg]]` and
  `![[paper.pdf]]` are already covered by the existing wikilink-embed
  transform in `render.rs` which routes them to `<img>` and link tags.
  This plan handles `.md` transclusion only; the existing image-embed
  path keeps working unchanged because the expander leaves
  non-`.md`-resolving targets to fall through.
- **No watcher / dependency-graph integration.** Sibling plan
  `notebook_obsidian_vault.md` Phase C will consume this plan's
  cache to track which notebooks need re-render when an embedded
  source changes; that integration is in that plan, not here.

---

## Risks and edge cases

1. **Circular `_setup.md` chains across many lessons.** The
   per-invocation `visiting` set is per-render-tree, not global, so a
   shared `_setup.md` embedded by 10 lessons doesn't trip the cycle
   detector — each lesson's expansion starts with a fresh set.
2. **Heading demotion past h6.** Capped at h6 with a stderr warning;
   the heading still renders, just without further demotion. Tested.
3. **Image-embed regression.** Today `![[diagram.svg]]` is a wikilink
   embed handled by `render.rs:1078`. The new expander must leave
   these alone — implementation rule: if `target` (after extension
   guess) does **not** resolve to a `.md` file, the expander returns
   the original `![[...]]` text unchanged so the existing wikilink
   transform handles it downstream. Test: `expand_leaves_image_embeds_alone`.
4. **`^id` inside math.** `^` is exponentiation in inline math. The
   strip pass and slicer must not touch text inside `$...$` or `$$...$$`.
   The fence-state machine already used by `protect_math` covers this;
   reuse it. Test: `strip_block_ids_preserves_math`.
5. **CWD reliance.** `cmd_render` already does `set_current_dir`. The
   expander does not depend on CWD — it uses explicit `host_dir` and
   `root_dir` args. Means embed resolution stays correct even if a
   future change drops the `set_current_dir` call.
6. **Frontmatter in embedded files.** Stripped before inlining (the
   embed point inherits no frontmatter from its source). Tested.
7. **Performance.** The test fixture for caching (test 35) measures
   load count, not wall-clock. For real-world numbers: a 6-lesson
   vault each embedding `_setup.md` reads it once; previously without
   embeds it would have been duplicated text in every file. Net win.

---

## Files touched (consolidated)

| File | Phase | Change |
|---|---|---|
| `crates/rustlab-notebook/src/embed.rs` (new) | 1, 2 | All resolver / slicer / expander / strip / demote logic. |
| `crates/rustlab-notebook/src/lib.rs` | 1, 3 | `pub mod embed;` and three call-site updates. |
| `docs/notebooks.md` | 3 | New "File embeds" section. |
| `AGENTS.md` (rustlab) | 3 | One-line pointer. |
| `examples/notebooks/_setup.md` (new) | 3 | Reusable setup example. |
| `examples/notebooks/embeds_demo.md` (new) | 3 | End-to-end demo for the gallery. |
| `dev/requests/notebook-file-embeds.md` | 3 | Archive — annotate "RESOLVED in <commit>". |

Total estimated diff: ~600 lines of production code in `embed.rs`,
~40 unit + integration tests, two new example notebooks, no new
dependencies.

---

## Sequencing

1. **Phase 1** (resolver, slicer, strip): self-contained module, all
   leaf operations, ~1 day.
2. **Phase 2** (demoter, recursive expander, error formatting):
   builds on Phase 1, ~1.5 days.
3. **Phase 3** (wire into call sites, docs, example): one shot, ~0.5
   day.

Ship as a single PR or three small ones — author's choice. The phases
are layered for review clarity, not for independent shippability;
nothing in Phases 1 or 2 is user-visible without Phase 3 wiring.

Per the workflow rule: pause before commit; require user approval.
