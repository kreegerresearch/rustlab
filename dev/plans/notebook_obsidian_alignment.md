# Notebook ↔ Obsidian / GitHub Markdown Alignment

**Goal:** Make rustlab-notebook source `.md` files render correctly and
identically across our three primary viewing surfaces — **GitHub** (the
committed `book/<lesson>.md` view), **Obsidian** (vault paste-in), and
**rustlab's own renderer** (HTML / PDF / LaTeX).

The pattern is simple: prefer markdown source that **all three render
natively**, avoid syntaxes that work in only one of them.

---

## Current state (audit, 2026-05-09)

### Features rustlab-notebook already supports

| Feature | Source | GitHub | Obsidian | rustlab |
|---|---|:-:|:-:|:-:|
| Tables | GFM | ✓ | ✓ | ✓ |
| Strikethrough `~~text~~` | GFM | ✓ | ✓ | ✓ |
| Inline math `$x$` | LaTeX | ✓ KaTeX | ✓ MathJax | ✓ KaTeX |
| Display math `$$x$$` | LaTeX | ✓ | ✓ | ✓ |
| Math escape: `\$`, `\${...}` | rustlab | n/a | n/a | ✓ |
| Math interpolation `${expr}$` | rustlab | passthrough → `$value$` | passthrough → `$value$` | ✓ |
| Mermaid ` ```mermaid ` | GFM | ✓ | ✓ | ✓ static SVG |
| YAML frontmatter `---` | both | ignored (rendered as table on GH) | ✓ | ✓ |
| Code fences ` ```rustlab ` | own | renders as code | renders as code | **executes** |
| Cross-notebook links `.md` → `.html` | rustlab | `.md` link works | `.md` link works | rewrites |
| Callouts `<!-- note -->` | own | hidden HTML comment | hidden | ✓ styled box |
| Exercises / Solutions `<!-- exercise -->` | own | hidden | hidden | ✓ collapsible |
| Code-block directives (`hide`, `details:`, `grid:`) | own | hidden | hidden | ✓ |
| Template interpolation `${expr:%fmt}` | own | n/a (rendered already) | n/a | ✓ |

### Pulldown-cmark options currently enabled

Only `ENABLE_TABLES` and `ENABLE_STRIKETHROUGH`
(`crates/rustlab-notebook/src/render.rs:62-63`, `:225-226`). Everything
else listed below is available in the dependency (v0.13) but turned off.

### Existing plans (status)

| Plan | Status |
|---|---|
| `notebook_report.md` | Phases 1–6 complete (parse, exec, KaTeX, LaTeX/PDF, polish, multi-notebook) |
| `notebook_future.md` | Template interpolation done; cell arrays Phase 1+1b done; Phase 2 (heterogeneous cell arrays) deferred |
| `notebook_mermaid.md` | Phase 1 complete (pure-Rust SVG, hashed cache, directives) |
| `notebook_bugfixes.md` | Math backslash + TUI suppression both already fixed; archival pass not yet performed |

No active plan covers Obsidian-style markdown features. This document
fills that gap.

---

## Obsidian markdown features — what to adopt and what to skip

The selection rule: **adopt only what GitHub also renders natively, or
what we can transparently transform in the markdown emitter.** Anything
Obsidian-only that GitHub renders as literal text is a regression for our
primary surface (GitHub-hosted `book/`).

| Obsidian feature | GitHub support | Recommendation |
|---|---|---|
| Callouts `> [!NOTE]` | ✓ (since 2023) | **Adopt as primary syntax.** Phase A. |
| Footnotes `[^1]` | ✓ GFM | **Adopt** — flip pulldown flag. Phase B. |
| Task lists `- [ ]` | ✓ GFM | **Adopt** — flip pulldown flag. Phase B. |
| Heading IDs `# H {#id}` | ✓ rendered | **Adopt** — flip pulldown flag. Phase B. |
| Wikilinks `[[Page]]` | ✗ literal | **Adopt at source, transform on output** to `[Page](Page.md)`. Phase C. |
| Embeds `![[img.png]]` | ✗ literal | **Adopt at source, transform on output** to `![](img.png)`. Phase C. |
| Highlight `==text==` | ✗ literal `==` | **Skip** — would render as garbage on GitHub. |
| Comments `%%text%%` | ✗ literal `%%` | **Skip** — reserve `<!-- -->` (already used) for hidden comments. |
| Tags `#tag` | collides with `# heading` | **Skip** — frontmatter `tags:` covers vault organization. |
| Block refs `^block-id` | ✗ literal | **Skip** — niche, lossy on GitHub. |
| Definition lists `Term\n: def` | ✗ literal | **Optional** — Obsidian renders, GitHub doesn't. Defer. |
| Superscript `^2^` / Subscript `~2~` | ✗ literal | **Skip** — math via `$x^2$` already covers it. |

---

## Phase A — GitHub / Obsidian-native callouts ✓ DONE

**The biggest user-visible win.** Both GitHub and Obsidian render the
same `> [!NOTE]` blockquote callout syntax natively. Adopting it means
the source notebook *displays as a callout* on GitHub directly,
without our renderer needing to translate anything for the markdown
output. Today's `<!-- note -->` syntax shows as nothing on GitHub.

### Source syntax to support

```markdown
> [!NOTE]
> Plain note, single-paragraph or multi-line.

> [!TIP] Custom title
> Optional title after the type tag.

> [!WARNING]+
> Foldable, expanded by default (Obsidian-only — GitHub renders the
> static box). The `+` and `-` suffixes are silently consumed.

> [!IMPORTANT]
> See [link](other.md) and inline math $x = 1$ inside the callout.
```

Recognized types (GitHub + Obsidian shared set):
`NOTE`, `TIP`, `IMPORTANT`, `WARNING`, `CAUTION`. Map our existing
internal `CalloutKind` to this superset:

| Source tag | `CalloutKind` |
|---|---|
| `[!NOTE]`, `[!INFO]` | `Note` |
| `[!TIP]`, `[!HINT]` | `Tip` |
| `[!IMPORTANT]` | new `Important` |
| `[!WARNING]` | `Warning` |
| `[!CAUTION]`, `[!DANGER]` | new `Caution` |

### Implementation

- New parser branch in `parse.rs::parse_notebook` that detects a
  blockquote starting with `> [!TYPE]` and consumes contiguous `> `
  lines until a non-blockquote line.
- Extract optional title on the same line (`> [!TIP] My title`).
- Strip optional `+`/`-` foldable suffix.
- Reuse existing `Block::Callout { kind, content }` — extend
  `CalloutKind` with `Important` and `Caution`. Update `render.rs` and
  `render_markdown.rs` to color the two new kinds.
- **Keep `<!-- note -->` working** — soft deprecation only. Doc note
  saying new notebooks should prefer `> [!NOTE]`.

### Markdown-format output

Emit the original `> [!NOTE]` blockquote verbatim — GitHub and Obsidian
will style it natively. Our HTML/PDF rendering already styles it via the
shared `Block::Callout` pipeline, so authors get consistent visuals on
all three surfaces.

### Tests

- `parse_callout_github_note` — `> [!NOTE]\n> body` → `Block::Callout(Note, "body")`.
- `parse_callout_with_title` — `> [!TIP] Heads up\n> body` → kind=Tip, title="Heads up".
- `parse_callout_foldable_suffix` — `> [!WARNING]+` accepted, suffix discarded.
- `parse_callout_multi_paragraph` — blockquote with internal blank-line `>` continues.
- `parse_callout_unknown_type_falls_through` — `> [!FOO]` left as plain blockquote.
- `render_markdown_callout_round_trip` — `Block::Callout` re-emits as `> [!NOTE]` form.

---

## Phase B — Flip the pulldown-cmark flags we already paid for ✓ DONE

Three options are on by default in GitHub's renderer and Obsidian's
viewer; they're disabled in ours and are pure wins to enable.

### Footnotes (`ENABLE_FOOTNOTES`)

```markdown
Citation needed[^src].

[^src]: Smith et al., 2024.
```

GitHub: ✓. Obsidian: ✓. Today: rustlab passes the inline `[^src]` and
the `[^src]: ...` definition through as raw text, breaking layout.

### Task lists (`ENABLE_TASKLISTS`)

```markdown
- [x] Filter design
- [ ] Spectral analysis
```

GitHub: ✓ checkboxes. Obsidian: ✓ interactive. Today: pulldown emits
literal `[ ]` characters.

### Heading attributes (`ENABLE_HEADING_ATTRIBUTES`)

```markdown
# Filter analysis {#filters}

See [the filters section](#filters).
```

GitHub: ✓. Obsidian: ✓. Today: rustlab uses auto-generated heading IDs
from text, so `{#id}` is rendered literally. Enabling lets authors pin
stable cross-notebook anchors.

### Implementation

Single edit in `crates/rustlab-notebook/src/render.rs` — both call sites:

```rust
let mut opts = Options::empty();
opts.insert(Options::ENABLE_TABLES);
opts.insert(Options::ENABLE_STRIKETHROUGH);
opts.insert(Options::ENABLE_FOOTNOTES);          // new
opts.insert(Options::ENABLE_TASKLISTS);          // new
opts.insert(Options::ENABLE_HEADING_ATTRIBUTES); // new
```

The markdown-format renderer already emits via pulldown-cmark, so it
gets the same treatment.

### Tests

- `render_html_footnote_reference` — `[^a]` becomes `<sup><a href="#fn-a">`.
- `render_html_footnote_definition` — `[^a]: text` becomes a footnote section.
- `render_html_task_list_unchecked` — `- [ ] todo` becomes `<input type="checkbox" disabled>`.
- `render_html_task_list_checked` — `- [x] done` adds `checked`.
- `render_html_heading_explicit_id` — `# Title {#custom}` produces `<h1 id="custom">`.

---

## Phase C — Obsidian wikilink/embed sugar with GitHub-safe transforms ✓ DONE

Obsidian users heavily rely on `[[Note]]` and `![[image.png]]`. GitHub
renders those literally, which looks broken. The fix: enable parsing
in the source, transform to standard markdown links in the
markdown-format emitter so the committed `book/*.md` is GitHub-safe.

### Source syntax

```markdown
See [[filter_design]] for the FIR derivation.

See [[filter_design#Frequency Response]] for the magnitude plot.

See [[filter_design|the FIR derivation]] for context.

![[diagram.svg]]
```

### HTML / PDF rendering

Use pulldown-cmark's `ENABLE_WIKILINKS` flag — it surfaces wikilinks as
`Tag::Link` events with a marker we can detect. Resolution policy:

- `[[Foo]]` → look for `Foo.md` in the same directory; link to `Foo.html`
  (HTML output) or `Foo.md` (markdown output). Title: `Foo`.
- `[[Foo|alias]]` → same target, `alias` as link text.
- `[[Foo#Section]]` → append `#section-slug` to the URL.
- `![[image.svg]]` → embed as `<img src="image.svg">` / `![](image.svg)`.
- Unresolved target → emit a styled `<span class="broken-wikilink">`
  in HTML; emit literal `[[Foo]]` in markdown so the author can spot it.

### Markdown emitter — GitHub-safety transform

When the markdown emitter sees a wikilink AST node:

- `[[Foo]]` → `[Foo](Foo.md)`
- `[[Foo|alias]]` → `[alias](Foo.md)`
- `[[Foo#Section]]` → `[Foo § Section](Foo.md#section-slug)`
- `![[image.svg]]` → `![](image.svg)`

This way: source stays Obsidian-vault-compatible; GitHub view sees
ordinary markdown links/images.

### Tests

- `parse_wikilink_simple` — `[[Foo]]` → `Link{ kind: Wiki, target: "Foo", text: "Foo" }`.
- `parse_wikilink_alias` — `[[Foo|bar]]` → text="bar", target="Foo".
- `parse_wikilink_with_anchor` — `[[Foo#Bar]]` parses target+anchor.
- `render_markdown_wikilink_to_md` — emitter rewrites to `[Foo](Foo.md)`.
- `render_html_wikilink_resolves` — when `Foo.md` exists in the
  directory, link points to `Foo.html`.
- `render_html_wikilink_unresolved` — emits `class="broken-wikilink"`.
- `render_markdown_embed_to_image` — `![[diagram.svg]]` → `![](diagram.svg)`.

---

## Phase D — Documentation pass ✓ DONE

After A–C land, update:

- `docs/notebooks.md` — new "Obsidian-native syntax" section listing the
  three feature groups, each with one-line example and rendering note
  per surface. Move the existing `<!-- note -->` paragraph under a
  "Legacy syntax (still supported)" subhead.
- `AGENTS.md` (rustlab) — update the "Template interpolation & math
  escaping" highlight to also point at the new section.
- `rustlab_llm/AGENTS.md` and `rustlab_em/AGENTS.md` — extend the
  inline checklist with: prefer `> [!NOTE]` callouts, footnotes work,
  wikilinks survive on GitHub.

---

## Phase E — Migration of existing notebooks ✓ DONE (no work needed)

Audit of `rustlab_llm/notebooks/` and `rustlab_em/notebooks/` (2026-05-09)
found **zero** uses of the legacy `<!-- note -->` syntax — neither
project ever adopted it. The migration is therefore a no-op for the
existing lesson sites.

For any future project that does have legacy callouts, the markdown
emitter handles migration automatically: re-rendering with the current
`rustlab` produces GFM-native `> [!NOTE]` output regardless of which
form the source uses. So the migration path is "run `make notebooks`,
copy the rendered form back to the source" — no separate script needed.

---

## What we're explicitly NOT doing

- **No Obsidian-only syntax that breaks on GitHub.** Highlights
  (`==text==`), comments (`%%text%%`), tags (`#tag`) are not adopted —
  they would render as literal characters on GitHub and degrade the
  primary surface.
- **No display-only Obsidian features.** Block references (`^id`),
  superscript/subscript shorthand are not worth the parser complexity
  for our content.
- **No reverse — i.e. no "render Obsidian dialect from rustlab markdown
  output."** The flow is one-way: source → committed markdown that is
  Obsidian + GitHub safe.
- **No editor integration.** This plan is about the file format and
  renderer, not about live editing.

---

## Sequencing and effort estimate

| Phase | Effort | Dependencies | Risk |
|---|---|---|---|
| A. Native callouts (`> [!NOTE]`) | ~200 lines parser + 6 tests | none | low — pure additive |
| B. Pulldown flag flip (footnotes, tasks, heading attrs) | ~10 lines + 5 tests | none | low |
| C. Wikilinks + embeds | ~150 lines parser glue + emitter transform + 7 tests | depends on pulldown wikilink event shape | medium — emitter rewriting |
| D. Docs | one-shot edit | A, B, C | low |
| E. Migration of `<!-- note -->` callouts | one-shot script + per-project commits | A | low |

Suggested order: **B → A → C → D → E.** B is a 10-line warm-up and
unblocks footnote use immediately. A is the highest-impact compatibility
win and slots in cleanly. C requires more design (anchor resolution,
broken-link policy) and benefits from A landing first so callouts inside
notes work via the new syntax.

Per-phase PR approach. Each phase is independently shippable and
testable.

---

## Open questions

1. **Should `[!IMPORTANT]` and `[!CAUTION]` get distinct colors, or
   collapse to existing `Tip`/`Warning`?** GitHub uses 5 distinct hues;
   matching them is cheap and aligns the visual vocabulary across
   surfaces. Recommend: 5 kinds.
2. **`#section-slug` algorithm for wikilink anchors.** GitHub uses
   lowercase + replace runs of non-alphanumerics with `-`. Pulldown
   already does this for our own `inject_heading_ids`. Reuse that
   function so cross-notebook anchors round-trip.
3. **`title:` frontmatter and Obsidian property panel.** Obsidian also
   reads `tags:`, `aliases:`, `cssclasses:`. We can ignore those (we
   already silently skip unknown keys), but if we ever want a vault
   feature like the index page picking up `aliases:`, the metadata is
   already there. No action required now.
4. **Mermaid in callouts.** Currently a `> [!NOTE]` containing a
   ` ```mermaid ` block would not work because callout content is one
   string. Could either (a) re-parse callout content recursively or
   (b) document the limitation. Recommend (a) when Phase A lands.

---

## Files touched

| File | Phase | Change |
|---|---|---|
| `crates/rustlab-notebook/src/parse.rs` | A | Add GitHub-callout branch; add `Important`/`Caution` to `CalloutKind` |
| `crates/rustlab-notebook/src/render.rs` | A, B | Style new callout kinds; flip pulldown options |
| `crates/rustlab-notebook/src/render_markdown.rs` | A, C | Re-emit GH callout verbatim; rewrite wikilinks → md links |
| `crates/rustlab-notebook/src/render_latex.rs` | A | Add tcolorbox styles for `Important`, `Caution` |
| `docs/notebooks.md` | D | New "Obsidian-native syntax" section |
| `AGENTS.md` (rustlab) | D | Update highlight pointer |
| `rustlab_llm/AGENTS.md`, `rustlab_em/AGENTS.md` | D | Extend authoring checklist |
| `dev/scripts/migrate_callouts.py` | E | One-shot migration script |

Total estimated diff: ~400 lines of production code, ~25 unit tests.
