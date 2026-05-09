# Feature Request: File embeds (Obsidian-style transclusion)

> **STATUS: RESOLVED.** Implemented via `dev/plans/notebook_file_embeds.md`.
> All three forms (`![[Doc]]`, `![[Doc#Heading]]`, `![[Doc#^block-id]]`)
> ship; embedded `rustlab` blocks share the host evaluator (Option A);
> errors render as inline `[!CAUTION]` callouts; recursion capped at
> depth 4 with cycle detection. See `crates/rustlab-notebook/src/embed.rs`,
> docs in `docs/notebooks.md` § "File embeds (transclusion)", working
> example in `examples/notebooks/_setup.md` + `embeds_demo.md`.


## Problem

Notebooks frequently want to pull content from a sibling file:
share a setup section across several lessons, embed a project's README
or `LICENSE` paragraph, or quote a specific definition from a glossary
without copy-pasting it. Today the only option is duplication, which
drifts as the source file evolves.

Obsidian's transclusion syntax — the `![[...]]` embed link — is a clean
fit. It uses Markdown's existing image-link grammar (`![alt](src)`),
swaps to the wiki-link form (`[[...]]`), and the leading `!` distinguishes
embed from plain wiki-link. Every Markdown viewer that does not
understand transclusion silently treats the `![[...]]` as text or a
broken image, so the source `.md` stays portable.

## Proposed syntax

Three forms, all keyed off `![[...]]`:

| Form | Behavior |
|---|---|
| `![[Document]]`             | Embed the whole `Document.md` (resolved relative to the current notebook, then to the notebook root). |
| `![[Document#Heading]]`     | Embed only the section under `## Heading` (and any nested subheadings) until the next sibling/parent heading. |
| `![[Document#^blockid]]`    | Embed only the paragraph or list item tagged with `^blockid` at the end of its line. |

`![[Document#Heading]]` matches headings case-insensitively, trimming
trailing whitespace, the same way Obsidian and most static site
generators do.

`![[Document#^blockid]]` requires the source to mark the block with a
trailing `^blockid` token, e.g.:

```markdown
The Nyquist rate is twice the highest frequency component. ^nyquist-def
```

The `^blockid` token is stripped from the embedded output and from any
non-embed render of the source file.

## Resolution rules

1. Resolve the link relative to the directory of the **current** notebook
   first.
2. If not found, walk up to the notebook root (the directory passed to
   `rustlab notebook render <dir>`) and try there.
3. If still not found, render an inline error placeholder
   (`<EMBED ERROR: Document not found>` in HTML, `[EMBED ERROR: …]` in
   LaTeX) and continue. Errors do not abort the render.

`Document` is matched against:
- exact filename (with or without `.md` extension),
- case-insensitive match on the basename if the case-sensitive lookup
  fails (Obsidian compatibility — useful on Linux file systems).

## Output format mapping

| Format | Rendering |
|---|---|
| HTML | The embedded source is parsed as Markdown and inlined at the embed point. Headings inside the embed are demoted by one level so they nest under the surrounding context (e.g. an `h2` becomes `h3`). Code blocks, math, and other directives inside the embed render normally. |
| LaTeX | Same — embedded markdown is parsed and emitted in place; section commands are demoted (`\section` → `\subsection`, etc.). |
| PDF | Same as LaTeX. |

## Recursion

Embeds are resolved recursively up to a fixed depth (default: 4) to
prevent runaway loops on circular references. A self-embed
(`A.md` → `A.md`) is detected and rendered as an inline cycle warning.

## Interaction with other directives

- An embedded notebook's `<!-- hide -->`, `<!-- details: -->`, and
  callout directives are honored as if they appeared at the embed site.
- Embedded `mermaid` and `rustlab` code blocks render normally —
  `rustlab` blocks **do execute** and contribute variables to the
  enclosing notebook's evaluator state, which lets you build a shared
  setup file (e.g. `_setup.md`) that other notebooks transclude.
  *(This is opt-in: see "Execution semantics" below for the open
  question of whether to gate this behind a directive.)*
- `${var}` template interpolation runs on the embedded markdown after
  inlining, with the host notebook's evaluator state.

## Execution semantics — open question

Two reasonable defaults for `rustlab` code blocks inside an embed:

A. **Execute as if local.** A `_setup.md` that defines `Fs = 48000` and
   loads a data file becomes reusable across many lesson notebooks via
   one `![[_setup]]` line. Powerful but means embeds have side effects.
B. **Display only, do not execute.** Quoting a code snippet from another
   document is purely documentary. Safer but loses the shared-setup use
   case.

Recommendation: **A by default**, with an opt-out marker the host can
use, e.g. `![[_setup quote]]` or a `<!-- embed-noexec -->` directive
preceding the link. Match how Obsidian behaves (which is "render-only,
no execution" — but Obsidian has no executable language to compare to).

## Files touched (estimate)

| File | Change |
|---|---|
| `crates/rustlab-notebook/src/parse.rs`  | Pre-process pass: resolve `![[...]]` links into inlined source before block parsing. Handle `#Heading` and `#^blockid` slicing. Strip `^blockid` tokens from output. |
| `crates/rustlab-notebook/src/execute.rs` | No change if the pre-process pass inlines source — the executor sees one flat block sequence. |
| `crates/rustlab-notebook/src/render.rs`  | Demote heading levels of inlined content; render error placeholders for unresolved embeds. |
| `crates/rustlab-notebook/src/render_latex.rs` | Same heading demotion for LaTeX. |

## Test cases

- `![[Other]]` with `Other.md` adjacent: full content inlined, headings demoted.
- `![[Other#Heading]]`: only the matching section + nested subheadings emitted.
- `![[Other#^id]]`: only the tagged block emitted, with `^id` token stripped.
- Heading match is case-insensitive and whitespace-trimmed.
- Missing target file produces an inline error and a single-line stderr warning, render continues.
- Self-embed (`A.md` containing `![[A]]`) detected, cycle warning emitted, render continues.
- Recursion depth cap (4) prevents runaway expansion.
- `^blockid` token on a paragraph not referenced by any embed is stripped from non-embed renders.
- Embedded `rustlab` block defines a variable used by a later block in the host notebook (Option A behavior).
- HTML escaping is correct when the embed target contains `<`, `>`, `&` in prose.

## Open questions

1. **Wikilink-in-prose without embed.** `[[Document]]` (no `!`) could
   render as a hyperlink to `Document.html`. Useful for cross-notebook
   navigation. Out of scope for this request — track separately if
   wanted; otherwise stick with standard `[text](Document.md)` links.
2. **Heading collision.** Two `## Examples` sections in the same target
   file: which one wins? Obsidian picks the first; match that behavior.
3. **Block-id placement.** Obsidian only allows `^id` at end-of-line.
   Should we extend to allow `^id` on its own line below a paragraph?
   Default: match Obsidian (end-of-line only).
4. **Performance.** Each `![[...]]` reads a file; large notebook trees
   could read the same file many times. Cache parsed-source by canonical
   path within a single `render` invocation. Implement from day one;
   it's a small optimization that matters at scale.

## Motivation examples

- `_setup.md` shared across all six quantum_lab lessons: one file
  defines constants and loads data, every lesson opens with `![[_setup]]`.
- Glossary file `glossary.md` with `^nyquist-def`, `^aliasing-def`, etc.
  Lesson notebooks transclude individual definitions where they're
  introduced.
- Reusable derivation appendices: a long algebraic proof lives in
  `appendix-fft-derivation.md` and is embedded into the lesson where
  it's referenced.
- Project README transcluded into the index notebook so the landing
  page stays consistent with the README without duplication.
