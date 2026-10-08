# Plan: notebook highlights and comments (Obsidian syntax)

**Status:** design only. Nothing is implemented. Waiting on decisions in [Open questions](#open-questions).  
**Created:** 2026-10-07  
**Updated:** 2026-10-08 — CriticMarkup is dropped. Notes use Obsidian `==highlight==` and `%%comment%%`.  
**Surfaces:** HTML (single and directory), `watch` (interactive server), LaTeX/PDF, Markdown, JSON, `check`.  
**Scope:** `.md` notebooks only. Standalone `.rlab` scripts, such as `run setup.rlab`, are code. They are never scanned for marks.

## Goal

Reviewers highlight a passage and leave a comment on it. The notes live **in the `.md` file**, in Obsidian's own syntax. rustlab renders highlights as `<mark>` and comments as margin notes.

The headline interaction is in `notebook watch`: select text in the rendered notebook, highlight it, and attach a comment. That is phase 2, immediately after read-only rendering. Phase 1 only displays marks that are already in the file.

There are **no tracked changes**. No insertions, deletions, or substitutions, and no `--critic` mode.

Collaboration is the file itself: git or a shared folder. `rustlab remote` only forwards the plot-viewer socket; it does not carry notebook text.

**Non-goals:** sidecar annotation files, comment threads with server state, user accounts, tracked changes, and exposing `watch` beyond loopback.

---

## 1. Syntax and rendering

### 1.1 Marks

| Mark | Source | Rendered HTML (comments on) |
|---|---|---|
| Highlight | `==text==` | `<mark class="rl-cm rl-cm-mark">text</mark>` |
| Comment on that highlight | `==text== %%why?%%` | the `<mark>` plus a margin note, linked with `aria-describedby` |
| Standalone comment | `See note.%%why?%%` | a margin note at that point in the paragraph |
| Block comment | a `%%` pair on its own lines | a block note in the flow (see 1.3) |

A `%%...%%` is bound to the highlight before it when the comment opens on the **same line**, with **at most one space** between the closing `==` and the opening `%%`. `==text==%%why?%%` and `==text== %%why?%%` are bound. Two spaces, or a newline, leaves a standalone comment.

The bytes between the delimiters are kept as written. We do not trim a space inside `==` or `%%`, so a later edit can compare the rendered selection with the source slice.

The text inside a highlight is **ordinary inline markdown**. Emphasis, links, `inline code`, `$math$`, and footnote references work inside `==...==`. A comment body is inline markdown after the optional header (§2) is parsed.

Nesting is not supported. A second `==` or `%%` opener inside an open mark is literal text, and `check` warns (W009).

**Example**

```markdown
Group delay is ==constant== %%only for linear phase%%.
The cutoff is 0.25.%%@michael 2026-10-08: confirm against the lesson%%
```

```html
<p><span data-src-start="16" data-src-end="39">Group delay is </span><mark class="rl-cm rl-cm-mark" data-src-start="39" data-src-end="51" aria-describedby="cm-n1">constant</mark><span class="rl-cm-note" id="cm-n1" data-cm-id="c0" role="note" tabindex="0"><sup class="rl-cm-num">1</sup><span class="rl-cm-body">only for linear phase</span></span><span data-src-start="74" data-src-end="95">.
The cutoff is 0.25.</span><span class="rl-cm-note" id="cm-n2" role="note" tabindex="0">…</span></p>
```

The offsets in that sketch are byte offsets into the file. The real renderer stamps every prose text run, not only the marks (§4.2).

### 1.2 Divergence from Obsidian

Obsidian and rustlab read the same characters and show different things:

| Syntax | Obsidian preview | rustlab (comments on) |
|---|---|---|
| `==text==` | highlight | `<mark>` |
| `%%comment%%` | **hidden** | a visible margin note or block note |
| `==text== %%why?%%` | highlight, comment hidden | highlight plus margin note |

`docs/notebooks.md` must say this in the review-notes section. A vault opened in Obsidian will not show the comments rustlab shows. rustlab HTML, `watch`, and a PDF rendered with `--comments` are where the notes are visible.

`--format markdown --obsidian` passes `==` and `%%` through unchanged when comments are on, so the vault stays valid Obsidian. Obsidian will then highlight and hide the comments. That is expected, not a strip bug. `--no-comments` is the path that removes them (§3.2).

### 1.3 Notes

- Comments are numbered 1…N in document order **per notebook**, across markdown blocks. The render loop passes a counter into `markdown_to_html_linked`, the way `ProseAssets` is threaded today.
- **Inline notes, wide viewports** (≥ 1280px; `main` is `max-width: 960px`): the `.rl-cm-note` card floats in the right gutter, Tufte-sidenote style. No JS for placement.
- **Narrow viewports and the file-browser layout:** only the superscript shows. Focusing it (tap or Tab) expands the card inline through `:focus-within`.
- **Block notes.** A `%%` whose opener is the whole trimmed line, closed by a later line that is only `%%`, renders as `.rl-cm-blocknote` in the flow (a card under that paragraph), not in the gutter. Blank lines inside the block are part of the note. `==` still cannot cross a blank line.
- **Print:** inline cards become an endnote list at the end of each block. Block notes stay in place.
- **Accessibility:**
  - `role="note"` and `tabindex="0"` on inline notes.
  - A bound highlight points at its note with `aria-describedby`.
  - The highlight does not rely on color alone: `<mark>` has a 1px bottom border, and the note has a number.
- A note that belongs to a heading is emitted **after** the `</hN>`, not inside it. `inject_heading_ids` slugs the tag-stripped heading text, so a note inside the heading would change the anchor.

### 1.4 Where marks are not parsed

The scanner leaves these as literal text:

- ` ```rustlab ` cells, mermaid and widget fences, prose fences (bash, python, text, untagged), and indented code
- inline code spans
- `$…$` and `$$…$$` math, including `==` used as a relation inside math
- HTML comments (`<!-- hide -->` and the other directives) and raw HTML blocks
- YAML frontmatter

`==` inside a rustlab cell is ordinary equality (`a == b`). `check` does **not** warn about it. A warning there would fire on almost every numeric notebook.

**Comments on code go on the prose beside the cell**, not inside the fence:

````markdown
Design the filter. %%@lisa: should this be a Kaiser window?%%

```rustlab
h = fir1(64, 0.25);
```
````

Selecting inside a rendered code cell, a math span, or across two blocks is rejected (§4.2). Whether a fence directive should attach a note to the cell itself is open question 3.

### 1.5 Boundaries

- **Highlight:** one paragraph. A blank line ends it. An unclosed `==` renders as literal text plus a warning badge (§6.3).
- **Headings** may contain a highlight: `## Results ==revised==`. The heading id is the plain text (`results-revised`), from the existing tag-stripping slug. A bound comment sits after the heading element.
- **Tables:** a mark closes in the cell where it opened. On a table row the scan stops at an unescaped `|`, the same rule `protect_math` uses.
- **Lists, blockquotes, callouts:** a mark stays inside one item or line group. Callout bodies already go through `markdown_to_html_linked`.
- **Footnotes:** a `[^1]` reference inside a highlight works. A footnote definition inside a mark is not supported. A `%%` comment is not a footnote and has its own numbering.
- **Escaping:** a backslash before the delimiter is literal, so `\==` renders `==` and does not open a highlight. Inline code also shows the characters.
- **Git conflict markers** are not delimiters. `<<<<<<<` does not interact with `==` or `%%`.

### 1.6 Parsing

`==` and `%%` are not CommonMark syntax, so they do not collide with strikethrough the way CriticMarkup's `{~~ ~~}` did. A pre-pass is still required: the scanner must skip code and math, and the tags it emits carry `class` and `data-src-*`, which the attribute-free HTML sanitizer would strip.

Callers of `render.rs::markdown_to_html_linked` (HTML prose and callouts) run `transform_wikilinks` first. The function itself then does `protect_math` (placeholders `\u{E000}M{n}\u{E001}`), `parse_single_tilde_safe` (pulldown-cmark 0.13.3), a prose-image stash, `sanitize_raw_html_events`, `sanitize_dangerous_urls`, the link, image, and fence rewrites, `push_html`, then `restore_math`. Image placeholders are restored after that. JSON `html` fields enter through `markdown_to_html` (same function, no wikilink pass, no `ProseAssets`).

**Decision:** add `protect_comments` immediately after `protect_math`, and an event pass before `push_html`.

1. **Pre-pass** on the original block bytes, modelled on `protect_math`.
   - Skip fences, code spans, HTML comments, and math placeholders already in place.
   - Replace each delimiter with a PUA sentinel: open `\u{E002}{kind}{id}\u{E003}`, close `\u{E004}{id}\u{E003}`. Prose images already use `\u{E000}P{n}\u{E001}` (`prose_media.rs`), so comment sentinels stay on `\u{E002}`–`\u{E004}`.
   - Build a source map from protected offsets back to file offsets. `transform_wikilinks` and `protect_math` both rewrite the string, so a pulldown event range is not a file offset without this map.
   - Highlight inner text stays markdown. Comment bodies are stashed, the header (§2) is parsed, then the remainder is rendered as inline-only markdown.
2. **Event pass**, after `sanitize_raw_html_events` (same trust level as `rewrite_prose_fences`).
   - Merge adjacent `Text` events with `pulldown_cmark::TextMergeStream`.
   - Emit trusted `Event::InlineHtml` for `<mark>` and notes, and wrap every other prose text run in `<span data-src-start data-src-end>`.
   - Wrap restored math in `<span class="rl-math" data-src-kind="math" data-src-start data-src-end>`.
3. **Unbalanced input.** An unpaired sentinel becomes the literal delimiter plus the warning badge (§6.3). Content is not dropped.

`comments.rs` owns the scanner and the `Mark { kind, span, inner, comment_header }` list. The same scanner is used by:

- `render_latex.rs::markdown_to_latex_in` (its own pulldown pipeline, with `ENABLE_MATH`)
- `render_markdown.rs`, for `--no-comments` stripping
- `check`

`parse_notebook` splits rustlab, mermaid, and widget fences. It does **not** record byte offsets today (`Block` owns strings, and frontmatter is stripped before the split). Phase 2's write path scans the raw file and records spans; it does not guess offsets from the stripped block strings.

---

## 2. Author, date, id, and replies (rustlab-only)

Obsidian comments have no metadata. An optional header is parsed only when the comment contains a colon after the header fields:

```
%%[#id] [re #id] [@author] [YYYY-MM-DD]: text%%
```

| Source | Meaning |
|---|---|
| `%%looks off%%` | anonymous comment (plain Obsidian) |
| `%%@michael 2026-10-08: looks off%%` | author and date |
| `%%#c12 @michael: looks off%%` | stable id `c12` |
| `%%#c4 re #c3 @liz: fixed in the prose above%%` | reply to `c3`, rendered indented in `c3`'s card |

This header is **rustlab-specific**. Obsidian hides the whole `%%...%%`, so the header is invisible there. Other tools that show the raw characters will show the header as part of the comment text. `docs/notebooks.md` says so.

- **Resolve means delete** the comment and its replies (`re #id`). Git history is the archive. There is no `resolved` flag in the file.
- **Browser inserts** always write an id. The next free id is `max(#cN) + 1` in the file. The date is the UTC day of the insert. `@author` is included only when an author string is configured (open question 1). Until that is decided, inserts omit `@author` and still write `#cN` and the date.
- Hand-written comments may omit the header. Ids are required only for replies and for edit/delete from the margin card.

---

## 3. Showing and hiding comments

### 3.1 What the switch does

One control, two states. **Comments on** shows highlights and notes. **Comments off** keeps the words and hides the review chrome:

- `<mark>` loses its background, border, and note link styling, and uses the body text color
- `.rl-cm-note` and `.rl-cm-blocknote` are `display: none`

The words of a highlight are still there. Nothing is re-rendered to flip the switch.

### 3.2 CLI

Primary flags, on `render` and `watch`:

- `--comments` or `--comments=on`
- `--no-comments` or `--comments=off`

The two spellings are one option. Passing both is an error. There is no `--critic` flag.

Optional `~/.rustlabrc` key `[notebook] comments = "on"` or `"off"`. The CLI wins over the rc file, same precedence as the other notebook keys.

| Output | Default | `--no-comments` | `--comments` |
|---|---|---|---|
| HTML (file and directory) | **on** | highlights become plain text, comments are omitted, no toggle | marks, notes, and the toggle |
| `watch` | **on**, toggle starts on | markup is still rendered so the toggle works; the toggle **starts off** | markup rendered, toggle starts on |
| PDF / LaTeX | **off** (clean document) | plain text, comments omitted | `\hl` highlights and margin or block notes |
| Markdown | **on**: `==` and `%%` passed through | unwrap `==` to its text, delete `%%` | source marks unchanged |
| Markdown `--obsidian` | **on**: syntax passed through, then the usual vault rewrites | same strip, then vault rewrites | syntax passed through. Obsidian shows highlights and hides `%%` |
| JSON | **on**: `html` fields include marks and notes | `html` fields are the stripped render | same as on |

`--obsidian` is markdown-only, as it is today (wikilinks, `_attachments/`, frontmatter, iframe, vault index). It does not change HTML or PDF. It does not start interpreting `%%` as hidden; only Obsidian's own preview does that.

GitHub does not render `==` or `%%`. A committed markdown file with comments on shows the markers as characters. `--no-comments` is the clean publish path for that format. `--obsidian` output is for a vault, not for GitHub (wikilinks already have the same limitation).

### 3.3 The toggle in HTML and watch

The control is a checkbox in `header.topbar`, with a visible label **Comments**. It is in the initial HTML, not injected later and not given a `hidden` attribute.

```html
<label class="rl-comments-toggle">
  <input type="checkbox" id="rl-comments" checked>
  Comments
</label>
```

CSS does the work, so the switch functions with JS off:

```css
body:has(#rl-comments:not(:checked)) .rl-cm-note,
body:has(#rl-comments:not(:checked)) .rl-cm-blocknote { display: none; }
body:has(#rl-comments:not(:checked)) .rl-cm-mark {
  background: transparent;
  border-bottom-color: transparent;
  color: inherit;
}
```

A small **nonce'd** script, in the same head block as the existing page script, only remembers the choice:

- `addEventListener` on the checkbox, never an `on*` attribute
- `sessionStorage` key for the on/off state
- on load, set `checked` from that key before first paint when the script is in `<head>`

`watch` live reload replaces `<main>` only (`applyFull` / partial patches). The topbar checkbox survives. In watch the control sits clear of `#rl-toolbar` (`page.rs` already pads the topbar for that toolbar).

Static HTML with `--no-comments` omits the marks and the checkbox. `watch --no-comments` renders both, with the checkbox unchecked.

The CSS and script are inlined. No CDN.

### 3.4 PDF

- **Off (default):** the LaTeX emitter writes the highlight's text and drops every `%%`. No `\hl`, no margin notes.
- **On:** highlights use `\hl` from `soul` (or `\colorbox` if `soul` is unavailable). Inline comments use numbered `\marginpar`, or `\footnote` inside tables, where marginpar fails. Block comments are quote environments in the flow.
- Colors come from Latte's comment roles. LaTeX and PDF are always Latte on white paper.

### 3.5 Theme roles (`crates/rustlab-plot/src/theme.rs`)

Add three fields to `ThemeColors`, emitted as `--rl-cm-*` custom properties. `css_tokens` is a fixed 23-element array today; it grows with the new fields.

| Role | Use | Starting point |
|---|---|---|
| `cm_mark_bg` | `<mark>` background, a **solid** hex | yellow blended over `bg` |
| `cm_note_bg` | note card background | `bg_secondary` |
| `cm_note_border` | card rule and number | `accent_secondary` |

Every pair below must reach WCAG **4.5:1** on all four builtins. A unit test in `theme.rs` enforces it.

- `text` / `cm_mark_bg`
- `text` / `cm_note_bg`
- `cm_note_border` / `cm_note_bg`

Yellow at 25% over `bg` gives `text`/`cm_mark_bg` of about 5.6, 5.2, **4.5**, and 5.7 (Mocha, Macchiato, Frappé, Latte). Frappé needs about 20% so it is not sitting on the threshold. `<mark>` sets `color: var(--rl-text)`, never the browser's default black.

No insert/delete colors. Those marks are out of scope.

### 3.6 Live reload

Marks are part of the markdown blocks, so a write re-renders through the existing per-block diff. The topbar checkbox and `sessionStorage` survive partial patches and a `<main>` swap.

Comment numbers are display-only. Inserting an earlier note renumbers later ones. Cross-references use `#id`, not the numbers.

### 3.7 `check`

The `rustlab:` codes continue the existing numbering (`E001`–`E005`, `W001`–`W005`). None of the new codes are errors. The printer format is `{file}:{line} [rustlab:CODE] {severity}: {message}`.

| Code | Finding |
|---|---|
| W006 | unclosed `==`: opened on line N, no closing `==` before the blank line, block end, or fence |
| W007 | unclosed `%%`, or a stray `%%` closer |
| W008 | opener and closer are split by a fence, a heading, or (for `==`) a blank line |
| W009 | nested `==` or `%%` |
| W010 | duplicate comment id `#c12`, lines N and M |
| W011 | orphan reply: `re #c12` and no `#c12` |

`check --fix` does not rewrite comments. `==` inside a code fence is not a finding.

```
notes/filter.md:42 [rustlab:W006] warning: unclosed `==` (no closing `==` before blank line at 44)
notes/filter.md:57 [rustlab:W011] warning: orphan reply re #c4 (no #c4 in this file)
```

---

## 4. Selecting text to comment (phase 2, the headline)

Phase 1 displays marks. Phase 2 lets a person **select rendered prose, highlight it, and attach a comment** without typing delimiters.

### 4.1 Popover

Shown only in `watch`, and only when `--annotate` is on (§4.4). Static HTML never gets it.

- On `mouseup` inside `<main>`, if the selection is non-empty and valid (§4.2), a small popover opens at the selection rectangle.
- The popover contains a textarea and one primary button, **Comment**. That single action writes both the highlight and the comment.
- An empty textarea writes the highlight only: `==selected text==`.
- A one-line comment writes `==selected text== %%#cN @author YYYY-MM-DD: comment%%` (no `@author` until open question 1 is decided).
- A comment that contains a newline writes a block note after the highlight.
- The server rejects a comment body that contains `%%` or `==`, so the body cannot close the delimiter early.
- The popover is plain HTML inlined in the page. A nonce'd script binds `mouseup`, `click`, and `keydown` with `addEventListener`. No `on*` attributes. No CDN. Escape or a click elsewhere dismisses it.

### 4.2 Mapping a selection to source bytes

`parse_notebook` does not store offsets. The renderer stamps them.

- Each markdown `section.rl-block` gets `data-src-start` and `data-src-end`: the block's byte range in the **raw file**, including the frontmatter offset `parse.rs::body_offset` accounts for.
- Every prose text run inside that section gets the same attributes for the slice it rendered, via the source map in §1.6.
- Math is `data-src-kind="math"`. Code, mermaid, and widget sections are `data-src-kind="code"` and have no selectable prose spans.
- Existing highlights and comments carry their own spans.

The script walks the selection's start and end containers, reads the surrounding `data-src-*` span, and interpolates a partial selection by UTF-8 byte length of the text prefix. It then sends those file offsets.

**Reject in the browser, before any request, when:**

- the selection crosses two `section.rl-block` elements
- it intersects `.rl-math`, a code/mermaid/widget section, or an existing `.rl-cm-mark` / `.rl-cm-note`
- the offsets cannot be read (no `data-src-*` ancestor)

The server repeats the check against the file. The bytes `file[start..end]` must equal the `text` field exactly. The range must sit inside one markdown region of the raw file, and must not overlap a fence, a math span, an HTML comment, or an existing `==` / `%%`. A mismatch is **400**. The client asks the reader to select the text again.

Quoting the selected string and searching for it is not the mechanism. The same sentence can occur twice. Offsets plus an equality check are the mechanism.

### 4.3 Edit and delete from the margin card

With `--annotate`, each note card shows **Edit** and **Delete**. They are `<button type="button">` elements. The same nonce'd script handles them by delegation. No per-card listeners in the HTML, no `on*` attributes.

- **Edit** replaces the card body with a textarea and Save. Save sends `op: "edit"`.
- **Delete** sends `op: "delete"`. The server removes that `%%...%%` and any reply whose header is `re #that-id`. A highlight the comment was bound to is left in place. One preceding space is removed when it was the single space that bound `==` to `%%`.
- Both operations send `expect`: the exact `%%...%%` bytes the page rendered. The server checks `expect` against the file at `data-src-start/end` on the note. A mismatch is **409**, same as a bad `If-Match`.

### 4.4 Write route: `--annotate`, not `--editable`

**Recommendation: a new `--annotate` flag.** Do not reuse `--editable`.

`--editable` mounts `POST /save/{slug}`, which writes the whole file, and it turns on the CodeMirror source pane and the cell editor. Leaving a margin note should not grant that. `--annotate` mounts only the comment route.

| Flag | What it enables |
|---|---|
| *(neither)* | read-only `watch`, toggle still works |
| `--annotate` | selection popover, edit/delete on notes, `POST /annotate/{slug}` |
| `--editable` | today's whole-file save and cell editor. It does **not** mount the annotate route unless open question 2 says it should |

`POST /annotate/{slug}` lives next to `POST /save/{slug}` in `server/http.rs` and is registered only when `--annotate` is set.

- **Host** is the existing loopback check on every request. **Origin** is `authorize_mutate` (missing or non-loopback Origin → 403), the same rule as `/save` and the WebSocket upgrade.
- The handler writes only that notebook's `source_path`. The path is never taken from the JSON body. The file jail is unchanged.
- The body is small JSON, not the file: `{ "op": "insert"|"edit"|"delete", "start", "end", "text", "comment", "id", "expect" }`. Unknown fields are ignored. `comment` and `expect` are capped (64 KB).
- **`If-Match`** is required. The value is the lowercase hex SHA-256 of the file bytes the page was rendered from, sent as `If-Match: "<hex>"`. The render stamps `<meta name="rl-source-sha256" content="hex">`. If the hash differs from the file now on disk, the response is **409** and the file is not written. The popover says the notebook changed and leaves the selection in place so the reader can retry after reload.
- `nb.save_lock` is held across read, check, splice, and write, so an annotate and a `POST /save` or a cell save cannot interleave.
- **Round-trip guard** before the write is committed to the response: re-scan the spliced text, require exactly the expected new or removed mark, and require every byte outside the splice to be unchanged. A failed guard returns 400 and does not write.
- Comment text is inserted as markdown source, then the normal render escapes it. The client does not send HTML.

Insert splice, one-line comment:

```markdown
==Group delay is constant== %%#c3 2026-10-08: only for linear phase%%
```

### 4.5 Live reload after a write

The handler does not render and does not broadcast. It writes the file and returns 204. The existing watcher (notify, 250 ms debounce, `render_loop`) re-renders and sends the usual WebSocket patch. The same path already runs after `POST /save/{slug}`.

A 409 does not write, so it does not re-render.

---

## 5. Phases and tests

| Phase | Contents | Size |
|---|---|---|
| **1** | `comments.rs`; HTML and JSON rendering; theme roles; sidenotes and block notes; the Comments toggle; `--comments` / `--no-comments` for HTML, watch, markdown, and JSON; `check` W006–W011; `docs/notebooks.md` including the Obsidian divergence | 1 PR |
| **1b** | LaTeX/PDF, default off, `--comments` on | 1 small PR |
| **2** | **Headline.** Selection popover; `data-src-*` spans; `POST /annotate/{slug}` with Host, Origin, jail, `If-Match` → 409; edit and delete on the margin card; live reload through the existing watcher | 1 PR |
| **3** (optional) | CodeMirror overlay so `==` and `%%` are visible in the source pane; insert from that pane when a rendered selection is rejected | 1 PR |

Phase 1 has no write route. Marks get into the file from an editor. `watch` re-renders on save, as it already does.

### Tests

- **Golden HTML** in `render.rs`: highlight, bound comment, standalone comment, block comment, header variants, `\==` escape, highlight in a heading (slug ignores the note), table cell, list, callout, footnote reference, mark beside `$math$` and beside inline code, `==` inside a rustlab fence and inside math left literal, unclosed `==` and `%%` as literal text plus a badge.
- **Toggle:** with the checkbox unchecked, notes are `display: none` and `<mark>` has no highlight background. The words remain.
- **CLI:** `--no-comments` HTML contains neither `rl-cm-mark` nor `rl-cm-note`. `--comments` PDF contains the highlight text and the comment text. Default PDF contains the highlight text and not the comment text.
- **Markdown:** comments on passes `==` and `%%` through. `--no-comments` unwraps and deletes them. `--obsidian` with comments on still emits `%%` (Obsidian will hide it; we must not strip it by accident).
- **Property:** a file with no `==` or `%%` renders byte-identical to today.
- **Theme contrast** for the three pairs in §3.5, all four builtins, ≥ 4.5:1.
- **`check`:** each W006–W011, right line, sorted order. A rustlab cell with `a == b` produces no finding.
- **Server (phase 2):**
  - insert writes exactly `==text== %%#cN …%%` and no other bytes
  - a selection that crosses a block, or overlaps code or math, is 400 and does not write
  - `file[start..end] != text` is 400
  - `If-Match` mismatch is 409 and does not write
  - edit and delete rewrite only the `%%` span; delete also removes `re #id` replies
  - the route is absent without `--annotate`
  - Origin missing or foreign is 403
- **LaTeX:** `validate` on a fixture with `--comments` and with the default.

---

## 6. Keeping notes attached to the text

### 6.1 The mark is the anchor

`==constant== %%only for linear phase%%` is part of the sentence it comments on. Inserting lines above it, moving the section, or renaming the file carries the note. Nothing is re-resolved.

A sidecar that stores a quote (`exact` / `prefix` / `suffix`) has to find that quote again after every edit. It needs a tie-break when the quote occurs twice, and an orphan list when it occurs zero times. Inline marks have neither problem. The risk moves to the delimiters.

**Lines inserted above, section moved**

```markdown
==Group delay is constant== %%#c3 @lisa: only for linear phase%%
```

stays on that sentence after a new paragraph is added above it and the section is moved. A sidecar quote would survive only by being searched for again.

**Edit inside the highlight**

```markdown
before: ==The filter has 64 taps== %%#c4: why 64?%%
after:  ==The filter has 128 taps== %%#c4: why 64?%%
```

The note stays on the sentence. The comment text is what says "64". A sidecar `exact` of "The filter has 64 taps" would miss.

`data-src-*` offsets are for the write that **creates** the mark. They are recomputed on every render. They are not stored in the file and they are not the anchor.

### 6.2 Failure modes

| Failure | What happens | Mitigation |
|---|---|---|
| Edit inside a highlight | The text changes, the `==` stays | none needed |
| One delimiter deleted | Unbalanced mark | Literal text plus a badge (§6.3). `check` W006 or W007. |
| Copy and paste duplicates a note | Two notes; with ids, a duplicate `#c12` | Both render. `check` W010. Edit/delete refuses an ambiguous id. |
| Mark crosses a fence or a blank line (`==`) | `parse_notebook` splits rustlab, mermaid, and widget fences, so each half is unbalanced | The scanner does not cross a fence. `==` does not cross a blank line. `check` W008. |
| Two people comment on the same line | A normal git conflict | Resolve it as text. The markers are not conflict markers. |
| A formatter reflows the paragraph | A hard wrap inside `==` is fine. A blank line, or a space inserted so two spaces sit between `==` and `%%`, unbinds the comment | W006/W008. Docs: do not run a markdown formatter that rewrites notebooks, or keep each note on one line. |
| `--editable` cell edit | `replace_code_block_source` copies prose verbatim. Marks are not inside cells | The existing round-trip guard rejects a body that adds a `` ``` `` line. Prose notes are untouched. |
| Whole-file `POST /save/{slug}` | Last writer wins. This route has **no** `If-Match` today | Comment writes do not use it. They use `/annotate` and `If-Match`. |
| Live reload while the popover is open | The selection's offsets may go stale | Submit still sends `If-Match`. A stale hash is 409, and the file is not changed. |

### 6.3 Mitigations

1. **Confinement.** A highlight stays in one markdown block and one paragraph or table cell. It does not cross a code fence, a math span, or an HTML comment. A blank line ends a highlight. A block `%%` may contain blank lines, and it ends at the closing line. One broken mark cannot swallow the rest of the file.
2. **`check` with line numbers.** W006–W011 (§3.7).
3. **Degrade in place.** An unmatched `==` or `%%` is literal text followed by `<span class="rl-cm-warn" role="img" aria-label="Unclosed highlight" title="Unclosed == (line 42)">⚠</span>`. The rest of the paragraph renders normally.
4. **Ids.** Browser inserts always write `#cN`. Replies and delete use the id, not the display number.
5. **Exact splice.** Insert, edit, and delete change one byte range after the file hash matches and (for edit/delete) `expect` matches, then the round-trip guard runs (§4.4).
6. **Live reload is per block.** A broken mark affects its block. Fixing it clears the badge on the next render.

### 6.4 If a sidecar is ever needed

Not in this design. If a later version needs state that does not belong in the `.md` (reactions, a resolved archive), keep the `==` / `%%#id` anchor in the file and put only the thread body in the sidecar. Look up `#id` first. Do not look up a quote while the id is still in the file.

---

## 7. Open questions

1. **Default author:** take it from `~/.rustlabrc` `[notebook] author`, from `$USER`, or from `git config user.name`? Phase 1 writes nothing. Phase 2 needs a string when it inserts a comment. Until this is decided, inserts omit `@author` and still write `#cN` and the UTC date.
2. **`--editable` and `--annotate`:** `--annotate` is the flag that mounts `POST /annotate/{slug}`. Should `--editable` imply `--annotate`, or stay independent so the cell editor does not also enable margin-note writes?
3. **Comments on code:** a selection inside a code cell is rejected, and `==` in a cell is equality, not a highlight. Is a `%%` on the prose line beside the cell enough, or do you want a directive before the fence, like `<!-- note: … -->`, that renders as a note attached to the cell?
4. **Hand-written ids:** browser inserts always add `#cN`. Should `check` warn when a hand-written `%%` has no id, or are ids optional unless a reply or a margin-card edit needs one?
