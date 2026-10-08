# Plan: inline CriticMarkup notes in notebooks

**Status:** design only. Nothing is implemented. Waiting on decisions in [Open questions](#open-questions).  
**Created:** 2026-10-07  
**Surfaces:** HTML (single and directory), `watch` (interactive server), LaTeX/PDF, Markdown, JSON, `check`.  
**Scope:** `.md` notebooks only. Standalone `.rlab` scripts, such as `run setup.rlab`, are code. They are never scanned for marks.

## Goal

Reviewers should be able to highlight text, leave comments, and suggest edits in a notebook. The notes live **in the `.md` file**, written in the [CriticMarkup](https://github.com/CriticMarkup/CriticMarkup-toolkit) syntax. Every renderer shows them, read-only, as highlights, tracked changes, and margin notes. Collaboration runs through the file itself: git or a shared folder. `rustlab remote` only forwards the plot-viewer socket; it does not carry notebook text.

Phase 1 adds **no new write route**. Authors type marks in their editor, and `watch` re-renders on save as it already does. Authoring in the browser comes in phase 2, behind `--editable`.

**Non-goals for v1:** sidecar annotation files, comment threads with server state, user accounts, and exposing `watch` beyond loopback.

---

## 1. Syntax and rendering

### 1.1 The five marks

| Mark | Source | Rendered HTML (show mode) |
|---|---|---|
| Addition | `{++ new text ++}` | `<ins class="rl-cm rl-cm-ins">new text</ins>` |
| Deletion | `{-- old text --}` | `<del class="rl-cm rl-cm-del">old text</del>` |
| Substitution | `{~~ old ~> new ~~}` | `<span class="rl-cm rl-cm-sub"><del class="rl-cm-del">old</del><ins class="rl-cm-ins">new</ins></span>` |
| Comment | `{>> text <<}` | numbered sidenote (see 1.3) |
| Highlight | `{== text ==}` | `<mark class="rl-cm rl-cm-mark">text</mark>` |
| Highlight + comment | `{== text ==}{>> why? <<}` | `<mark … aria-describedby="cm-n3">text</mark>` plus sidenote 3, linked |

CriticMarkup trims one space just inside each delimiter, so `{++ a ++}` and `{++a++}` render the same.

The text inside a mark is **ordinary inline markdown**. Emphasis, links, `inline code`, `$math$`, and footnote references all work inside it.

The spec forbids **nesting**. A second opener inside a mark is literal text, and `check` warns about it (W009). A highlight followed by a comment is *adjacent*, not nested. When `{>>…<<}` comes right after `==}`, with at most one space between, the comment is bound to that highlight.

**Example: source to HTML**

```markdown
The filter uses {~~64~>128~~} taps.{>>@michael 2026-10-06: 64 aliased at fs/2.<<}
{==Group delay is constant==}{>>Only for linear phase.<<}
```

```html
<p>The filter uses <span class="rl-cm rl-cm-sub" data-cm="0:0"><del class="rl-cm-del">64</del><ins class="rl-cm-ins">128</ins></span> taps.<span class="rl-cm-note" id="cm-n1" role="note" tabindex="0"><sup class="rl-cm-num">1</sup><span class="rl-cm-body"><b class="rl-cm-author">@michael</b> <time datetime="2026-10-06">2026-10-06</time> 64 aliased at fs/2.</span></span>
<mark class="rl-cm rl-cm-mark" aria-describedby="cm-n2" data-cm="0:2">Group delay is constant</mark><span class="rl-cm-note" id="cm-n2" role="note" tabindex="0"><sup class="rl-cm-num">2</sup><span class="rl-cm-body">Only for linear phase.</span></span></p>
```

`data-cm="<block>:<ordinal>"` names the mark's position: the Markdown block it sits in, then its order within that block. It is inert in phase 1. Phase 2 uses it as the target for accept and reject.

### 1.2 Theme roles (`crates/rustlab-plot/src/theme.rs`)

Add five fields to `ThemeColors`, emitted as `--rl-cm-*` CSS custom properties:

| Role | Use | Catppuccin source (starting point) |
|---|---|---|
| `cm_ins` | Text and underline color of `<ins>` | green |
| `cm_del` | Text and strike color of `<del>` | red (maroon if red fails) |
| `cm_mark_bg` | `<mark>` background, a **solid** hex | yellow blended over `bg` |
| `cm_note_bg` | Sidenote card background | `bg_secondary` |
| `cm_note_border` | Sidenote card rule and number color | `accent_secondary` |

Every fg/bg pair must reach WCAG **4.5:1** on all four builtins, and a unit test enforces it (see §5). Estimates from the stock palettes:
- Mocha, Macchiato, and Frappé green and red all pass. Frappé red is 4.65, so tight.
- **Latte green `#40a02b` is only about 2.96:1 on `#eff1f5`.** Latte needs a darker green, about `#2f7a1f` (≈4.7:1).
- Yellow at 25% over `bg` gives `text`/`cm_mark_bg` of 5.6, 5.2, **4.5**, and 5.7. Frappé needs about 20%.

That rules out dark blue on black and light yellow on white. `<mark>` always sets `color: var(--rl-text)`, never the browser default black.

LaTeX and PDF always use Latte, so they read the same Latte roles.

### 1.3 Comments: numbered sidenotes, CSS only

- Comments are numbered 1…N in document order **per notebook**, across all Markdown blocks. The render loop passes a counter into `markdown_to_html_linked`, the way `ProseAssets` is threaded today.
- **Wide viewports** (≥ 1280px; content is `max-width: 960px`): the `.rl-cm-note` card floats into the right gutter beside its line, Tufte-sidenote style. No JS.
- **Narrow viewports and the file-browser layout:** only the superscript number shows. Focusing it (tap or Tab) expands the card inline through `:focus-within`. There are no form controls and no handlers.
- **Print:** cards become an endnote list at the end of each block.
- **Accessibility:**
  - `role="note"` with `tabindex="0"`.
  - Highlights point to their note through `aria-describedby`.
  - `<ins>` and `<del>` carry visually hidden "insertion" and "deletion" labels (`.rl-sr`), because screen readers don't announce those tags reliably.
  - Meaning never depends on color alone: insertions are underlined, deletions are struck through, and `<mark>` has a 1px bottom border.

### 1.4 Where marks are *not* parsed

Inside any of these, marks are left as literal text:
- ` ```rustlab ` cells, mermaid and widget fences, prose fences (bash/python/text/untagged), and indented code
- inline code spans
- `$…$` and `$$…$$` math
- HTML comments (`<!-- hide -->` and the other directives) and raw HTML blocks
- YAML frontmatter

**Comments on code go next to the cell.** Put a highlight and comment on the prose line just before the fence, or put a comment on its own line right after the fence:

````markdown
Design the filter:{>>@lisa: should this be a Kaiser window?<<}

```rustlab
h = fir1(64, 0.25);
```
````

`check` gives an info finding (I001) when a rustlab cell contains text that looks like CriticMarkup. That tells the author the mark won't render.

### 1.5 Boundaries, escaping, and markdown structures

- **One block, one paragraph.** A mark can contain soft line breaks, but a **blank line ends it**. A mark left open at the blank line is *unclosed* (§6.3). For a change that spans several paragraphs, use one mark per paragraph. This follows the spec's own advice and keeps every mark local.
- **Headings** take inline marks: `## Results {++(revised)++}`. Heading **ids and the sidebar outline use the *accepted* text**, so accepting a change never breaks `#anchor` links.
- **Tables:** a mark must close in the cell where it opened. The scan stops at an unescaped `|`, the same rule `protect_math` uses.
- **Lists and blockquotes:** a mark must stay inside one item or line group. Block syntax inside a mark (`#`, `-`, `>`, `|` at line start) is literal.
- **Footnotes:** a `[^1]` reference inside a mark works. A footnote *definition* inside a mark is not supported. A CriticMarkup comment is **not** a footnote and has its own numbering.
- **Callouts:** marks work inside `> [!NOTE]` bodies, which already go through `markdown_to_html_linked`.
- **Escaping:** `\{` makes the brace literal, so `\{++` renders `{++`. CommonMark already treats `\{` as an escape, so GitHub renders it the same way. Inline code also works.
- **Git conflict markers:** `<<<<<<<` and `>>>>>>>` never count as delimiters. Only the exact tokens `{>>` and `<<}` do.

### 1.6 Parsing approach: a source pre-pass to sentinels, then event substitution

Callers of `render.rs::markdown_to_html_linked` (HTML prose and callouts) run `transform_wikilinks` first. The function itself then does `protect_math` (PUA placeholders `\u{E000}M{n}\u{E001}`), `parse_single_tilde_safe` (pulldown-cmark 0.13.3), a prose-image stash, `sanitize_raw_html_events`, `sanitize_dangerous_urls`, the link, image, and fence rewrites, `push_html`, then `restore_math` (image placeholders are restored after that). JSON `html` fields enter the same function through `markdown_to_html`, with no wikilink pass and no `ProseAssets`.

**Decision:** add a `protect_critic` pre-pass right after `protect_math`, with a matching event pass before `push_html`.

1. **Pre-pass (byte scanner, modelled on `protect_math`).**
   - It skips fences, code spans, HTML comments, and the math placeholders already in place.
   - It replaces each **delimiter** with a PUA sentinel: open is `\u{E002}{kind}{id}\u{E003}`, close is `\u{E004}{id}\u{E003}`. For substitution, `~>` becomes a separator sentinel. Prose images already reuse the math brackets as `\u{E000}P{n}\u{E001}` (`prose_media.rs`), so these sentinels stay on `\u{E002}`–`\u{E004}`.
   - The **inner text stays markdown**, so pulldown still parses `**bold**` inside an insertion.
   - Comment bodies are pulled out to a stash and rendered separately: header parsing (§2) first, then `markdown_to_html` for inline-only markdown.
2. **Event pass.**
   - Merge adjacent `Text` events with `pulldown_cmark::TextMergeStream`, so a sentinel is never split.
   - Split on sentinels and emit trusted `Event::InlineHtml` for `<ins>`, `<del>`, `<mark>`, and the sidenote markup.
   - This runs *after* `sanitize_raw_html_events`, the same trust level as `rewrite_prose_fences`, so the class attributes survive. The sanitizer's attribute-free allow-list would strip them from raw HTML.
3. **Unbalanced input.** Sentinels left unpaired at the end of a block are turned back into their literal delimiter text, with a warning badge (§6.3).

**Why not the alternatives:**
- **Pure event-level** fails because the delimiters collide with markdown. `{~~a~>b~~}` contains `~~a~>b~~`, which `ENABLE_STRIKETHROUGH` turns into a strikethrough. `{++**x**++}` splits `{++` and `++}` across `Strong` events, and `{>>` can become blockquote syntax at line start.
- **Pure string replacement to HTML before parsing** puts `<ins class=…>` through the raw-HTML sanitizer, which drops attributes. It also changes CommonMark block detection.

**The same scanner is reused** in three places:
- `render_latex.rs::markdown_to_latex_in`, which has its own pulldown pipeline with `ENABLE_MATH`. There the sentinels become `\uline`, `\sout`, `\hl`, and `\marginpar`/`\footnote`; see §3.4.
- `render_markdown.rs`, for `--critic accept|reject|strip`.
- `check`.

One module, `critic.rs`, owns the scanner and the `Mark { kind, span, inner, comment_header }` list.

---

## 2. Author, date, id, and replies (rustlab convention, **non-standard**)

CriticMarkup has no metadata. We propose an **optional** header at the start of a comment body. It is parsed only when the trailing `:` is present:

```
{>> [#id] [re #id] [@author] [YYYY-MM-DD]: text <<}
```

| Source | Meaning |
|---|---|
| `{>>looks off<<}` | anonymous comment (pure CriticMarkup) |
| `{>>@michael 2026-10-06: looks off<<}` | author and date |
| `{>>#c12 @michael: looks off<<}` | stable id `c12`, which replies and phase-2 resolve use |
| `{>>re #c12 @lisa 2026-10-07: fixed in cell 3<<}` | reply to `c12`, rendered indented inside `c12`'s card |

- **Resolve means delete the comment** (and its replies). Git history is the archive. We don't add a `resolved` state for v1, because it would add syntax for little gain.
- **Author default** for phase-2 insertion: the `~/.rustlabrc` `[notebook] author = "michael"` setting, otherwise `$USER`. Phase 1 needs no default, because humans type the header.
- **Degradation elsewhere:** other CriticMarkup tools show the header as plain comment text, which is harmless. This is flagged as non-standard in `docs/notebooks.md`.

---

## 3. Read-only preview

### 3.1 View modes

| Mode | Additions | Deletions | Substitution | Highlights | Comments |
|---|---|---|---|---|---|
| `show` | `<ins>` styled | `<del>` styled | both, styled | `<mark>` | sidenotes |
| `accept` | plain text | removed | new only | plain text | removed |
| `reject` | removed | plain text | old only | plain text | removed |
| `strip` | `<ins>` styled | `<del>` styled | both, styled | plain text | removed (changes stay visible, chatter removed) |

### 3.2 CLI

- `render` and `watch` take `--critic <show|accept|reject|strip>`.
- `~/.rustlabrc` can set `[notebook] critic = "…"` as a default.

| Output | Default | Notes |
|---|---|---|
| HTML (file and directory) | `show` | toggle included (3.3) |
| `watch` | `show` | toggle included |
| PDF / LaTeX | `accept` | clean document. `--critic show` prints changes plus margin notes (open question 2) |
| Markdown (`-f markdown`, `--obsidian`) | `show` = **source marks passed through unchanged** | `--critic accept` produces clean GitHub markdown |
| JSON | `show` | the `html` fields carry the show markup |

### 3.3 The toggle in HTML and watch

- The server always renders **`show` markup**. The other modes are pure CSS on a body class: `rl-cm-accept`, `rl-cm-reject`, or `rl-cm-strip`. Switching modes never needs a re-render.
- The page header gets a three- or four-button group (Markup / Accepted / Original). It is emitted with the `hidden` attribute.
- One small **nonce'd** script, in the same block as the existing page script, does three things:
  - removes `hidden` from the group
  - binds `click` with `addEventListener`, never with `on*` attributes
  - stores the choice in `sessionStorage`
- **JS off:** the group stays hidden and the page shows `show`, the most informative mode.
- Static HTML stays self-contained. The CSS and script are inlined in the page, with no CDN.
- The group is emitted only when the notebook contains at least one mark.

### 3.4 PDF

- In `accept` and `reject` modes, the LaTeX emitter writes the resolved text and nothing else.
- In `show` mode:
  - insertions use `\uline` (green) and deletions use `\sout` (red), from `ulem`
  - highlights use `\hl` from `soul`, or `\colorbox` as a fallback
  - comments use numbered `\marginpar`, or `\footnote` inside tables, where marginpar fails
- Colors come from Latte's `cm_*` roles.

### 3.5 Live reload

- Marks are part of the Markdown blocks, so editing one re-renders only that block through the existing per-block diff. The body class and `sessionStorage` survive both partial patches and full reloads.
- Comment numbers are display-only. Adding an early comment renumbers later notes, which means extra patched blocks but correct output.
- Cross-references use `#id`, never numbers.

### 3.6 `check`

The `rustlab:` codes continue the existing numbering. None are errors, because rendering always degrades gracefully. Each finding carries a 1-based line number.

| Code | Finding |
|---|---|
| W006 | unclosed mark: `{++` opened on line N, no `++}` before the blank line, block end, or fence |
| W007 | stray closer: `++}` with no opener |
| W008 | cross-boundary: an opener and its matching closer sit in different blocks, i.e. split by a fence, heading, or blank line |
| W009 | nested mark |
| W010 | substitution without `~>` |
| W011 | duplicate comment id `#c12`, lines N and M |
| W012 | orphan reply: `re #c12` with no `#c12` |
| W013 | git conflict marker in the file (useful generally) |
| I001 | CriticMarkup-looking text inside a code cell or fence (not rendered) |

`check --fix` makes no automatic critic fixes, because the author's intent can't be guessed.

---

## 4. Editing UX

### Phase 1: author in the editor (no new write path)

The author types marks in any editor or in Obsidian, saves, and `watch` re-renders. That is the whole feature.

`docs/notebooks.md` gets a "Review notes" section covering:
- the five marks
- the header convention
- the one-paragraph rule
- the "comments on code go next to the cell" rule
- `--critic`

### Phase 2: `--editable` integration (opt-in, same security envelope)

1. **Source-pane highlighting.** Add a small CodeMirror 5 overlay mode for the five delimiters, so a broken or unbalanced mark is visible as you type.
   - This needs the vendored `addon/mode/overlay.js` (about 1 KB) next to `mode/markdown`. It is served from `/assets/codemirror/…` and listed in `VENDOR.md`.
2. **Insert from a selection in the source pane.** Select text in CodeMirror and press **Comment** (Ctrl/Cmd-Alt-M) or **Highlight**. This inserts `{==sel==}{>>#cN @author YYYY-MM-DD: <<}` and puts the cursor inside the comment.
   - Saving uses the existing `POST /save/{slug}`, which is Origin-checked and serialized by `save_lock`.
   - This is pure client-side text editing, with no new server code.
3. **Insert from a selection in the rendered view** (phase 3). Map the DOM selection to source by quoting the selected text against the block's source, and require a unique match. If the match isn't unique, fall back to "select it in the source pane".
4. **Accept and reject per mark.**
   - With `--editable`, hovering a `[data-cm]` element shows ✓ and ✗ buttons. A delegated listener in the nonce'd script sends a WS message: `{op:"critic", block, ordinal, expect:"{~~64~>128~~}", action:"accept"}`.
   - The server, under `save_lock`:
     - re-reads the file
     - re-scans that block
     - checks that the mark at `ordinal` has exactly the source text `expect` (**CAS**)
     - rewrites only that byte span
     - **round-trip guard:** re-scans and checks that the block now has one fewer mark and that everything outside the span is unchanged
     - writes the file, then lets the normal re-render broadcast run
   - If `expect` doesn't match, the server sends a `stale` reply and the page reloads the block. This mirrors the WS cell editor's existing stale and round-trip checks in `server/ws.rs`.
   - "Resolve" on a comment is the same operation with `action:"delete"`. It also removes replies whose `re #id` matches.

### Security notes for any write

- No new HTTP route. Accept and reject ride the existing WS, which already checks Origin on upgrade, and are honoured only when `--editable` is set.
- The server writes **only** the notebook's own `source_path`. It never writes a path taken from the client, and the jail is unchanged.
- `expect` is limited to 64 KB. The server rejects ordinals that don't exist.
- Comment bodies are HTML-escaped before inline markdown. They go through the same path as prose, with raw HTML through the sanitizer. The author header is escaped text.
- No `on*` attributes anywhere. Listeners are delegated from one nonce'd script.

**Note:** `POST /save/{slug}` has **no CAS today**. It is a whole-file write where the last writer wins. Phase 2 should add an optional `If-Match: <sha256 of the text the pane loaded>` that returns 409 on mismatch. That way a collaborator's on-disk edit, such as a teammate's new comment arriving through git or a sync, isn't silently overwritten. See open question 8.

---

## 5. Phases and test plan

| Phase | Contents | Size |
|---|---|---|
| **1a** | `critic.rs` scanner and marks; HTML/JSON rendering in `show`; theme roles; CSS sidenotes; the view toggle; `--critic` for HTML, watch, and markdown; `check` W006–W013 and I001; docs | **1 PR, ~600–900 lines including tests** |
| **1b** | LaTeX/PDF emission (`ulem`/`soul`/marginpar) and the PDF default | 1 small PR |
| **2** | CodeMirror overlay; Comment/Highlight insert in the source pane; WS accept/reject/resolve with CAS; `If-Match` on `/save` | 1 PR |
| **3** (optional) | Selecting in the rendered view to comment; Obsidian `==`/`%%`, if wanted | 1 PR each |

### Tests

- **Golden render tests** in `render.rs`, from source to HTML, for:
  - each of the five marks
  - highlight plus comment
  - the header variants
  - escaping (`\{++`)
  - marks inside headings (accepted-text ids), table cells, list items, callouts, and footnote references
  - a mark next to `$math$` and next to inline code
  - marks inside a rustlab or prose fence staying literal
  - unclosed, stray, nested, and cross-blank-line cases: literal text plus a badge, with no content lost
- **Mode tests:** for each mode, the text content of the CSS-applied result matches a reference. Mode resolution is also implemented server-side for `-f markdown`, so that can be asserted directly.
- **Property test:** `accept(src)` and `reject(src)` contain no CriticMarkup tokens. A file with no marks renders byte-identical to today.
- **Theme contrast test** in `rustlab-plot/src/theme.rs`. For all four builtins, the following must each be ≥ 4.5:1:
  - `cm_ins`/`bg`
  - `cm_del`/`bg`
  - `text`/`cm_mark_bg`
  - `text`/`cm_note_bg`
  - `cm_note_border`/`cm_note_bg`
- **`check` tests** for each W/I code: right line number, sorted order.
- **Server tests (phase 2):**
  - accept, reject, and delete rewrite exactly one span
  - a stale `expect` is rejected
  - the WS op is ignored without `--editable`
  - `If-Match` mismatch returns 409
- **LaTeX:** `validate` runs the PDF lint with marks in `show` and in `accept`.

---

## 6. Keeping marks in sync with notebook content

### 6.1 Inline marks anchor themselves

A mark *is* part of the text it annotates. Inserting lines above it, moving cells, reordering sections, and renaming the file all carry the mark along. There is nothing to recompute.

A **sidecar** (W3C Web Annotation `TextQuoteSelector`) instead stores `exact`, `prefix`, and `suffix` strings and has to **re-find** them after every edit. That needs fuzzy matching to survive small edits, a policy for ties when the same quote appears twice, and an **orphan list** for notes whose quote no longer exists. Inline storage removes all three. It moves the risk to the **delimiters** instead.

**Before and after: lines inserted above, plus a cell moved**

```markdown
<!-- before -->
## Filter
{==Group delay is constant==}{>>#c3 @lisa: only for linear phase<<}
```

```markdown
<!-- after: a new paragraph above, and this section moved below "Results" -->
## Results
...
## Filter
New intro sentence.
{==Group delay is constant==}{>>#c3 @lisa: only for linear phase<<}
```

The note is still on the right text, and nothing needs fixing. A sidecar quote would also survive this case, but only through re-anchoring.

**Before and after: an edit inside the marked span**

```markdown
before: {==The filter has 64 taps==}{>>#c4: why 64?<<}
after:  {==The filter has 128 taps==}{>>#c4: why 64?<<}
```

The inline note stays anchored, and the comment text tells the story. A sidecar `exact:"The filter has 64 taps"` would **fail** to match here and land in the orphan list.

### 6.2 Failure modes

| Failure | What happens | Mitigation |
|---|---|---|
| Edit inside a span | Text changes and the mark stays | none needed. A substitution's `old` is still the author's original. |
| One delimiter deleted | Unbalanced mark | Renders literally with a ⚠ badge (6.3). `check` W006/W007 with the line number. |
| Copy and paste duplicates a mark | Two notes; with ids, a duplicate `#c12` | Both render. `check` W011. Phase-2 resolve refuses an ambiguous id. |
| Mark spans a cell, fence, or paragraph | `parse_notebook` splits blocks at rustlab, mermaid, and widget fences, so each half is unbalanced | The scanner never crosses a fence or blank line. Both halves render literally with badges. `check` W008. |
| Git merge conflict | Two people annotated the same line: a normal textual conflict | Conflict markers never parse as delimiters. `check` W013 flags leftover markers. Resolve as you would any text conflict. |
| An editor or formatter reflows text | Hard wraps inside a mark are soft breaks, which is fine. A formatter that adds a **blank line**, or escapes `{`/`~`, breaks the mark. | W006/W008 catch it. Docs say to exclude notebooks from markdown formatters, or to keep notes on one line. |
| `--editable` inline cell edit | Only rustlab fence bodies are spliced (`replace_code_block_source` copies prose verbatim), and marks never live in cells | The existing round-trip guard rejects an edit that adds a ```` ``` ```` line. Prose marks are untouched. |
| `--editable` source pane, whole-file save | Last writer wins over a concurrent on-disk edit | Phase 2 `If-Match` (409) (§4). Accept and reject use per-span CAS. |
| Live reload mid-typing | An unbalanced intermediate state shows briefly | The badge appears and disappears on the next save. Content is never dropped. |

### 6.3 Mitigations in detail

1. **Confinement rules in the scanner.**
   - A mark lives inside one Markdown block and one paragraph or table cell.
   - It never crosses a code fence, a math span, or an HTML comment.
   - A blank line closes the search.
   
   This keeps every breakage **local**: one broken mark can't swallow the rest of the notebook.
2. **`check` diagnostics with line numbers.** Codes W006–W013 and I001 (§3.6). Example:
   ```
   notes/filter.md:42 [rustlab:W006] warning: unclosed `{==` (no `==}` before blank line at 44)
   notes/filter.md:57 [rustlab:W011] warning: duplicate comment id #c4 (also line 31)
   ```
3. **Graceful degradation.** An unmatched delimiter is emitted as literal text, followed by `<span class="rl-cm-warn" role="img" aria-label="Unclosed critic mark" title="Unclosed {== (line 42)">⚠</span>`. The rest of the paragraph renders as normal markdown. **Content is never dropped or hidden.**
4. **Visible delimiters while editing.** The phase-2 CodeMirror overlay colors delimiters, so a missing `<<}` shows up at once in the source pane.
5. **Stable ids.** The optional `{>>#c12 @michael: …<<}` keeps replies (`re #c12`) and resolve working across moves, edits, and copies.
   - Phase-2 inserts always add an id. The next free one is `max(#cN)+1` in the file.
   - Numbers shown on screen are display-only.
6. **Exact-span rewrites.** Accept, reject, and resolve rewrite one byte span after a CAS on its exact source text, then run the round-trip guard (§4). A stale view can never edit the wrong mark.
7. **Live reload.** Re-render is per block. A broken mark affects only its block, and fixing it clears the badge on the next save.

### 6.4 Hybrid anchoring if a sidecar ever arrives

If threads later need state that doesn't belong in the `.md` (resolved history, reactions, long discussions), keep **anchors inline** and move only **thread bodies** into the sidecar:

```markdown
{==Group delay is constant==}{>>#c3<<}
```
```json
{ "c3": { "thread": [ {"author":"lisa","date":"2026-10-06","text":"only for linear phase"} ],
          "resolved": false,
          "target": { "type":"TextQuoteSelector", "exact":"Group delay is constant",
                      "prefix":"## Filter\n", "suffix":"" } } }
```

Anchors resolve in this order:
1. the inline `#id` (exact, survives edits)
2. otherwise the stored `TextQuoteSelector`, scoped to the same block, with a unique match required
3. otherwise **Detached notes**, a panel listed by `check` (W014)

The sidecar's `target` is refreshed on each phase-2 write, so the fallback stays current. This keeps the inline guarantee for the common case and needs fuzzy matching only for notes whose inline id was deleted.

---

## 7. Open questions

1. **Author identity:** take the default author from `~/.rustlabrc` `[notebook] author`, from `$USER`, or from `git config user.name`? (Phase 1 doesn't need it.)
2. **PDF:** should PDF default to `accept` (clean), or show changes and margin notes by default?
3. **Default view in HTML and watch:** `show` (proposed), or `accept` with notes one click away?
4. **Obsidian syntax:** also render Obsidian `==highlight==` and treat `%%comment%%` as hidden (never shown)? Each costs little, but it's a second dialect.
5. **Comments on code cells:** is "comment on the line next to the cell" enough, or do you want a `<!-- note: … -->` directive before the fence, like `<!-- hide -->`, that renders as a note attached to the cell?
6. **Resolve semantics:** resolve = delete (proposed, git keeps history), or a `resolved` flag kept in the file?
7. **GitHub raw-brace noise:** CriticMarkup shows as raw `{++ ++}` on GitHub. Is that acceptable for committed notebooks, or should publishing paths (`-f markdown`, `--obsidian`) default to `--critic accept`?
8. **`/save` CAS:** add `If-Match` to `POST /save/{slug}` in phase 2 (proposed), so a whole-file save can't overwrite a teammate's newer on-disk comment? It's a small behavior change for the existing `--editable` pane.
9. **Stable ids:** should hand-written comments be encouraged (or `check`-nudged) to carry `#cN` ids, or should ids only be added by phase-2 browser inserts?
10. **`strip` mode:** is it worth having (changes visible, notes hidden), or is `show|accept|reject` enough?
