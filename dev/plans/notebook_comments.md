# Plan: notebook highlights and comments (Obsidian syntax)

**Status:** implemented on this branch (read-only render, `check`, PDF, and `watch --annotate`). A per-comment reviewed/accepted flag is reserved and not implemented. Decisions are recorded below. No open questions remain.
**Created:** 2026-10-07  
**Updated:** 2026-10-09 — The Name field prefills from this tab's `sessionStorage`. A `%%` with no `#cN` is W012. `--editable` implies `--annotate`. `--no-comments` hides notes while drafting; a note added while hidden is still saved, with a short confirmation. Each person shares comments by git push/pull. `If-Match` is single-user. A mouse `mouseup` does not open the popover; touch and pen use that event's `pointerType`.
**Surfaces:** HTML (single and directory), `watch` (interactive server), LaTeX/PDF, Markdown, JSON, `check`.  
**Scope:** `.md` notebooks only. Standalone `.rlab` scripts, such as `run setup.rlab`, are code. They are never scanned for marks.

## Goal

Reviewers highlight a passage and leave a comment on it, or leave a comment on a whole code cell. The notes live **in the `.md` file**, in Obsidian's own syntax. rustlab renders highlights as `<mark>` and comments as margin notes.

The headline interaction is in `notebook watch` when annotation writes are on (`--annotate`, or `--editable`, which implies it): select text, right-click, and choose **Add comment** or **Highlight**. That is phase 2, immediately after read-only rendering. Phase 1 only displays marks that are already in the file.

There are **no tracked changes**. No insertions, deletions, or substitutions, and no `--critic` mode.

Collaboration is the file itself, through git. Each person edits their own working copy. Comments are shared by `git push` and `git pull`. When two people change the same lines, git reports a merge conflict and someone resolves it in git. `watch` serves that one working copy on loopback. It is not a multi-user editor, and two people do not annotate one running server (§4.5, §6). `rustlab remote` only forwards the plot-viewer socket; it does not carry notebook text.

**Non-goals:** sidecar annotation files, comment threads with server state, user accounts, tracked changes, line-level anchors inside a code cell, exposing `watch` beyond loopback, and multi-user concurrent editing of one `watch` server.

## Decisions

- **Syntax.** Obsidian `==highlight==` and `%%comment%%`. No CriticMarkup, no tracked changes, no `--critic`.
- **Cell comments.** A `%%` that ends immediately before a fence, or before that fence's code directives, comments on the whole cell (§7).
- **Mouse path.** Select, then right-click (or the ContextMenu key / Shift+F10), then **Add comment**. `mouseup` does not open the popover for a mouse, including on a hybrid device. A `pointerup` opens it only when that event's `pointerType` is `"touch"` or `"pen"`. `matchMedia('(pointer: coarse)')` sizes hit targets and does not choose the trigger (§4.2).
- **Resolve.** Delete the `%%` and its `re #id` replies. Git is the archive. There is no resolved flag.
- **Name.** `@name` is an optional one-token field. Nothing reads `~/.rustlabrc`, `$USER`, or `git config`. The last non-empty token typed in this browser tab is stored in `sessionStorage` under `rl-comment-name` and prefills the field. Closing the tab clears it. A blank field omits `@name` and does not clear the stored token. A value with a space, `%`, or `:` is rejected and is not stored (§2).
- **Missing id.** A well-formed `%%` with no `#cN` is **W012**, on the line of the opening `%%`. That covers an inline comment after a highlight, a standalone comment, a block comment, and a cell comment, including a private Obsidian `%%` note. `check` does not look at `--obsidian`. `check --fix` does not add ids (§3.7).
- **Write versus display.** `--annotate` means the session can write comments. `--editable` implies `--annotate`. `--annotate` alone is a comment-only session. `--comments` / `--no-comments` set the initial display, and whether PDF and the other non-watch formats include notes. The in-page Comments checkbox flips display at runtime and a write does not change it. A note saved while the checkbox is off is written, a short confirmation is shown, and the new mark stays hidden until the reader turns Comments on (§3.2, §4.5).
- **Who shares a file.** One working copy per person. Git push and pull carry the comments. `POST /annotate/{slug}` with `If-Match` / 409 only notices that this copy changed under the page (the same person's editor, or another tab). It is not a multi-user lock (§4.5, §6).
- **Reviewed / accepted.** Deferred. A checkbox was discussed and the spec is not settled, so nothing is parsed or written for it. `CommentHeader.state` is the slot a later flag will use. Spans and `#cN` ids do not depend on it.

---

## 1. Syntax and rendering

### 1.1 Marks

| Mark | Source | Rendered HTML (comments on) |
|---|---|---|
| Highlight | `==text==` | `<mark class="rl-cm rl-cm-mark">text</mark>` |
| Comment on that highlight | `==text== %%why?%%` | the `<mark>` plus a margin note, linked with `aria-describedby` |
| Standalone comment | `See note.%%why?%%` | a margin note at that point in the paragraph |
| Block comment | a `%%` pair on its own lines | a block note in the flow (see 1.3) |
| Comment on a code cell | a `%%` line or block immediately before the fence | the same margin note, attached to that cell (§7) |

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
<p><span data-src-start="16" data-src-end="39">Group delay is </span><mark class="rl-cm rl-cm-mark" data-src-start="39" data-src-end="51" aria-describedby="cm-n1">constant</mark><span class="rl-cm-note" id="cm-n1" role="note" tabindex="0"><sup class="rl-cm-num">1</sup><span class="rl-cm-body">only for linear phase</span></span><span data-src-start="74" data-src-end="95">.
The cutoff is 0.25.</span><span class="rl-cm-note" id="cm-n2" role="note" tabindex="0">…</span></p>
```

The offsets in that sketch are byte offsets into the file. An `--annotate` session stamps them on marks and on verbatim prose runs (bytes CommonMark will not rewrite). Static HTML, and a watch that is not annotating, omit the offsets so a length change above a block does not force a full page reload (§4.3).

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

The rlab lexer treats `#` and `%` as line comments (`crates/rustlab-script/src/lexer.rs`). A line `%% note` inside a fence is an rlab comment. The notebook scanner does not read it as a review mark.

A review note on a cell is a `%%` **outside** the fence, on the lines immediately before it (§7):

````markdown
%%#c3 2026-10-08: should this be a Kaiser window?%%
```rustlab
h = fir1(64, 0.25);
```
````

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

## 2. Date, id, replies, and an optional name (rustlab-only)

Obsidian comments have no metadata. An optional header is parsed only when the comment contains a colon after the header fields:

```
%%[#id] [re #id] [@name] [YYYY-MM-DD]: text%%
```

| Source | Meaning |
|---|---|
| `%%looks off%%` | anonymous comment (plain Obsidian). Renders. `check` warns W012 |
| `%%@michael 2026-10-08: looks off%%` | optional name and date. No `#cN`, so `check` warns W012 |
| `%%#c12 @michael: looks off%%` | stable id `c12` |
| `%%#c4 re #c3 @liz: fixed in the prose above%%` | reply to `c3`, rendered indented in `c3`'s card |

This header is **rustlab-specific**. Obsidian hides the whole `%%...%%`, so the header is invisible there. Other tools that show the raw characters will show the header as part of the comment text. `docs/notebooks.md` says so.

`@name` is one token: the characters after `@` up to the next space. It is **optional**. The system does not look up a name. There is no `[notebook] author` key, no read of `$USER`, and no read of `git config`.

- **Resolve means delete** the comment and its replies (`re #id`). Git history is the archive. There is no `resolved` flag in the file.
- **Browser inserts** always write an id and the UTC date. The next free id is `max(#cN) + 1` in the file. The popover has an optional Name field. It is prefilled from `sessionStorage` key `rl-comment-name` when that key holds a token, and empty otherwise. The key is per browser tab and is cleared when the tab closes. It is not `localStorage`, and it is not the Comments-toggle key (`rl-comments-visible`, §3.3). A blank field omits `@name` on that comment and does not clear the stored token. A non-empty value is stored only after a successful submit, and only when it is one token with no space, `%`, or `:`. Those characters are rejected in the popover and are not written. The field is not saved to `~/.rustlabrc` and is not filled from `$USER` or `git config`.
- Hand-written comments may omit `@name` and the date. Those two fields stay optional. A well-formed `%%` that omits `#cN` is W012 (§3.7). Edit and delete from the margin card still match the `%%` byte span (`expect`), so a missing id does not block those buttons. Replies, and deleting the replies that point at a comment, need that comment's id. A hand-written `@name` is kept as written.

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

**Write and display are different controls.**

- **`--annotate` writes.** It mounts the context menu, the popover, note-card Edit and Delete, and `POST /annotate/{slug}`. It can be used alone, for a session that comments and does not edit cells or replace the file. **`--editable` implies `--annotate`**, so the cell editor also gets that write route and that menu. `--annotate` does not imply `--editable`. There is no flag that turns annotation writes off while `--editable` is on. Hiding notes is not done by withholding the write route.
- **`--comments` / `--no-comments` display.** They set the initial state of the Comments checkbox in `watch` and in HTML that includes the toggle. They also choose whether PDF, LaTeX, static HTML, markdown, and JSON include the marks (the table above). They do not mount or unmount `POST /annotate/{slug}`.
- **The checkbox flips display at runtime.** Checking or unchecking Comments changes what is shown, whatever the CLI default was. `watch` keeps the markup in the page so the checkbox can do this, including under `--no-comments`. Static HTML `--no-comments` is the exception: the marks and the checkbox are omitted, and a static file has no annotate route.
- **A write while display is off.** `watch --no-comments`, or a checkbox the reader has unchecked, still accepts an annotation when a write flag is on. The `==` or `%%` is saved. The checkbox stays unchecked, so the new mark is in the re-rendered page and stays hidden by the CSS in §3.3. The page chrome, outside `<main>` so the reload does not remove it, shows a short confirmation: "Comment saved. Turn Comments on to show it." A highlight uses "Highlight saved. Turn Comments on to show it." The line clears after a few seconds, or immediately when the reader checks Comments. The write does not check the box. Checking it as a side effect would show every other note and undo a drafting view that had comments hidden on purpose.

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
- `sessionStorage` key `rl-comments-visible` for the on/off state. This is not `rl-comment-name` (§2)
- on load, set `checked` from that key before first paint when the script is in `<head>`

`watch` live reload replaces `<main>` only (`applyFull` / partial patches). The topbar checkbox survives. In watch the control sits clear of `#rl-toolbar` (`page.rs` already pads the topbar for that toolbar).

Static HTML with `--no-comments` omits the marks and the checkbox. `watch --no-comments` renders both, with the checkbox unchecked.

The CSS and script are inlined. No CDN.

### 3.4 PDF

- **Off (default):** the LaTeX emitter writes the highlight's text and drops every `%%`. No `\hl`, no margin notes. A cell comment is omitted. The listing is the code alone.
- **On:** highlights use `\hl` from `soul` (or `\colorbox` if `soul` is unavailable). Inline comments use numbered `\marginpar`, or `\footnote` inside tables, where marginpar fails. Block comments are quote environments in the flow. A cell comment is a note immediately above that listing.
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

The annotate context menu (§4.1) uses these same roles: `text` on `cm_note_bg`, and a `cm_note_border` rule. It does not add a fourth color. Hover keeps the text color and draws the border as a 2px focus/hover outline, so the contrast pairs above cover the menu.

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
| W012 | well-formed `%%` with no `#cN` id. The line number is the opening `%%`. Applies to an inline comment after a highlight, a standalone comment, a block comment (the opener line), and a cell comment. A reply that carries `re #id` and no id of its own is included |

`check --fix` does not rewrite comments and does not insert ids. A later opt-in that writes the next free `#cN` immediately after the opening `%%`, keeping the single binding space and the header order `#id`, `re #id`, `@name`, date, and skipping fences, is a possible follow-up. It is not a decision, and it is not what `--fix` does.

`==`, `%%`, `#`, and `%` inside a code fence are not findings. A bare `==` with no `%%` is not W012. An unclosed `%%` is W007 only, not also W012. A cell comment is an ordinary `%%` and uses W006–W012 when it is unbalanced, nested, missing an id, or has a bad id.

`check` does not read `--obsidian`. That flag is a markdown render mode, not a property of the file, and the same notebook is what HTML shows with comments on. A private Obsidian note (`%%aside%%`, no id) is the same bytes as an anonymous review comment, so it is W012. `--obsidian` still passes the `%%` through (§3.2). The warning does not strip it and does not block the render.

```
notes/filter.md:18 [rustlab:W012] warning: comment has no #cN id
notes/filter.md:42 [rustlab:W006] warning: unclosed `==` (no closing `==` before blank line at 44)
notes/filter.md:57 [rustlab:W011] warning: orphan reply re #c4 (no #c4 in this file)
```

---

## 4. Selecting text to comment (phase 2, the headline)

Phase 1 displays marks. Phase 2 lets a person **select rendered text and annotate it** without typing delimiters. With a mouse, the gesture is select, then right-click, then a menu item. The popover is where the comment is typed. A mouse `mouseup` does not open it. A touch or pen `pointerup` does, judged by that event's `pointerType` (§4.2).

### 4.1 Context menu

Shown only in `watch`, and only when annotation writes are on: `--annotate`, or `--editable`, which implies it (§4.5). Static HTML never gets it. The menu is available when the Comments checkbox is unchecked. The marks stay hidden until the reader turns Comments on (§3.2). The menu is an element in the page, `role="menu"`, hidden until opened. A nonce'd script binds `contextmenu` and `keydown` with `addEventListener`. No `oncontextmenu` attribute, no other `on*` attribute, no CDN. Item labels are fixed strings in that script. The script does not build the menu from notebook text.

**Open the menu** (`preventDefault`, then show it) only when all of these hold:

- the event target is inside `<main>`, and not inside `.CodeMirror`, a `textarea`, an `input`, or `[contenteditable]`
- for a pointer `contextmenu`, Shift is not held (`!event.shiftKey`). Shift+F10 is a `keydown`, not this event, and still opens the menu
- there is a valid selection (§4.3), or the target is inside one existing `.rl-cm-mark` or `.rl-cm-note`

The menu is positioned at the pointer. A keyboard open uses the selection rectangle. It flips to stay inside the viewport.

**Leave the native browser menu alone** when any of these hold. The handler returns without `preventDefault` and without showing our menu:

- there is no selection, or the selection is collapsed, and the target is not an existing mark or note
- the selection is invalid: it crosses two blocks, crosses from a cell into prose, or intersects `.rl-math`
- Shift is held on a pointer `contextmenu` (right-click). Shift+F10 still opens the menu
- the target is outside `<main>`, or inside the source pane, the cell editor, or another text control

**Items.** A valid selection of unmarked prose:

| Item | What it does |
|---|---|
| **Add comment** | Opens the popover (§4.2). Submit writes `==selected== %%#cN …%%`. |
| **Highlight** | Writes `==selected==` immediately. No popover. |

A valid selection inside one code, mermaid, widget, or prose fence uses this same menu and writes the whole-cell `%%` (§7). **Highlight** is omitted there. Wrapping code in `==` is not a cell comment. **Add comment** opens the popover; submit writes the `%%` above the directive stack.

A right-click on an existing mark or its note:

| Target | Items |
|---|---|
| Highlight with a bound `%%`, or the note card | **Edit comment**, **Delete** |
| Bare `<mark>` (no `%%`) | **Add comment**, **Delete** |

**Edit comment** opens the popover with the current body. Save sends `op: "edit"`. **Delete** sends `op: "delete"`. On a note, delete removes that `%%` and its `re #id` replies and leaves a bound highlight, the same as the card buttons (§4.4). On a bare highlight, delete unwraps `==` and leaves the words. **Add comment** on a bare highlight writes a `%%` bound to the existing `==` and does not wrap the text again.

**Keyboard.** The menu is a WAI-ARIA menu with roving tabindex:

- `ContextMenu` and Shift+F10, while the selection is valid and focus is inside `<main>`, open it. The same fall-through rules apply: an invalid selection does not call `preventDefault`.
- ArrowUp and ArrowDown move between items. Home and End move to the ends.
- Enter and Space activate the focused item.
- Esc closes the menu and leaves the selection in place.
- Tab closes the menu.

Focus moves to the first item when the menu opens. A click outside, a scroll, a selection change, or a live reload that replaces `<main>` closes it.

### 4.2 Popover

The popover is the composer. What opens it depends on the input that just happened, not on a device-wide media query.

**Who opens it**

- **Mouse.** `mouseup` is not a trigger. A `pointerup` whose `PointerEvent.pointerType` is `"mouse"` does not open the popover, including on a hybrid device that also has a touchscreen. A mouse user opens the context menu with a right-click (`contextmenu`), or with the ContextMenu key or Shift+F10, and then chooses **Add comment** or **Edit comment**. That menu item is the only mouse path into the popover.
- **Touch and pen.** A `pointerup` whose `pointerType` is `"touch"` or `"pen"` opens the popover when the selection is valid (§4.3). A long-press `contextmenu` is not reliable for those inputs, so the popover is their menu. `matchMedia('(pointer: coarse)')` is only a layout hint, for larger hit targets. It does not decide whether `pointerup` opens the popover. A mouse event on a coarse-primary device stays on the mouse rule above.
- **Keyboard.** ContextMenu or Shift+F10 opens the menu (§4.1). **Add comment** or **Edit comment** then opens the popover. The menu has no text field.

The touch or pen popover shows the same actions the menu would have: **Highlight** next to **Comment** for unmarked prose, **Comment** alone for a cell, and **Edit** / **Delete** for an existing mark. **Highlight** and **Delete** from the mouse menu do not open the popover.

- It contains a comment textarea, an optional Name field (§2), and one primary button, **Comment** (or **Save** when editing).
- A one-line prose comment writes `==selected text== %%#cN YYYY-MM-DD: comment%%`. `@name` is added only when the Name field is non-empty.
- A comment that contains a newline writes a block note after the highlight.
- A cell selection writes a cell comment (§7). The textarea is required. An empty cell comment is not written.
- The server rejects a comment body that contains `%%`, `==`, or a line of three or more backticks, so the body cannot close the delimiter or open a fence.

The popover is plain HTML inlined in the page. The same nonce'd script binds `pointerup`, `contextmenu`, `click`, and `keydown` with `addEventListener`, and reads `pointerType` on the `pointerup` event. No `on*` attributes. No CDN. Esc or a click elsewhere dismisses it.

### 4.3 Mapping a selection to source bytes

`parse_notebook` does not store offsets. The renderer stamps them.

Shipped v1 stamps those offsets only while `watch --annotate` is on, and only on marks plus verbatim prose runs (no `*_[]()``#|<>$\!~&` and no newline). A selection across markdown syntax has no span, so the menu does not open. Math, callouts, and a rendered string that no longer matches the host file (template interpolation, an embed) are not mapped. Static HTML omits the offsets.

- Each markdown `section.rl-block` gets `data-src-start` and `data-src-end`: the block's byte range in the **raw file**, including the frontmatter offset `parse.rs::body_offset` accounts for.
- Every prose text run inside that section gets the same attributes for the slice it rendered, via the source map in §1.6.
- Math is `data-src-kind="math"`. Code, mermaid, widget, and prose-fence sections are `data-src-kind="code"` and carry the fence's byte range on the section. They have no prose spans inside the listing.
- Existing highlights and comments carry their own spans.

The script walks the selection's start and end containers, reads the surrounding `data-src-*` span, and interpolates a partial selection by UTF-8 byte length of the text prefix. It then sends those file offsets. A selection contained in one `data-src-kind="code"` section does not send a highlight range. It sends `target: "cell"` and the fence opener's offset (§7).

**Reject in the browser, before any request, when:**

- the selection crosses two `section.rl-block` elements, or starts in a code section and ends outside it
- it intersects `.rl-math`
- it covers both unmarked text and an existing `.rl-cm-mark` or `.rl-cm-note`, or it covers two marks
- the offsets cannot be read (no `data-src-*` ancestor)

A selection contained in one existing mark is valid. The menu then offers Edit / Delete (or Add comment / Delete on a bare highlight), not a second `==` wrap (§4.1).

The server repeats the check against the file. For a prose insert, the bytes `file[start..end]` must equal the `text` field exactly. The range must sit inside one markdown region of the raw file, and must not overlap a fence, a math span, an HTML comment, or an existing `==` / `%%`. A mismatch is **400**. The client asks the reader to select the text again. The cell-insert check is in §7.

Quoting the selected string and searching for it is not the mechanism. The same sentence can occur twice. Offsets plus an equality check are the mechanism.

### 4.4 Edit and delete from the margin card

When annotation writes are on (`--annotate`, or `--editable`, which implies it), each note card shows **Edit** and **Delete**. They are `<button type="button">` elements. The same nonce'd script handles them by delegation. No per-card listeners in the HTML, no `on*` attributes.

- **Edit** replaces the card body with a textarea and Save. Save sends `op: "edit"`.
- **Delete** sends `op: "delete"`. The server removes that `%%...%%` and any reply whose header is `re #that-id`. A highlight the comment was bound to is left in place. One preceding space is removed when it was the single space that bound `==` to `%%`.
- Both operations send `expect`: the exact `%%...%%` bytes the page rendered. The server checks `expect` against the file at `data-src-start/end` on the note. A mismatch is **409**, same as a bad `If-Match`.

### 4.5 Write route: `--annotate`, implied by `--editable`

**Decision:** `--editable` enables `--annotate`. Passing `--editable` mounts `POST /annotate/{slug}`, the context menu, the popover, and the note-card Edit and Delete buttons, in addition to today's whole-file save and the cell editor.

`--annotate` is the write flag. It can still be passed alone. That session gets the menu and `POST /annotate/{slug}` and does not get `POST /save/{slug}`, the CodeMirror source pane, or the cell editor. `--annotate` does not imply `--editable`. There is no `--no-annotate`. A separate flag is not how annotation writes are turned off. `--no-comments`, and the Comments checkbox, are how notes are hidden (§3.2).

| Flag | What it enables |
|---|---|
| *(neither)* | read-only `watch`. The Comments toggle still works. No menu, no annotate route |
| `--annotate` | context menu, comment popover, edit/delete on notes, `POST /annotate/{slug}`. Display is still the §3.2 default unless `--comments` or `--no-comments` is also passed |
| `--editable` | today's whole-file save and cell editor, **and** everything `--annotate` enables |
| `--no-comments` | does not remove a write route. In `watch`, markup stays and the toggle **starts off**. Static HTML omits the marks and the toggle |

`POST /annotate/{slug}` lives next to `POST /save/{slug}` in `server/http.rs` and is registered when `--annotate` is set or when `--editable` is set.

- **Host** is the existing loopback check on every request. **Origin** is `authorize_mutate` (missing or non-loopback Origin → 403), the same rule as `/save` and the WebSocket upgrade.
- The handler writes only that notebook's `source_path`. The path is never taken from the JSON body. The file jail is unchanged.
- The body is small JSON, not the file: `{ "op": "insert"|"edit"|"delete", "target": "prose"|"cell", "start", "end", "text", "comment", "author", "id", "expect" }`. Unknown fields are ignored. `comment` and `expect` are capped (64 KB). `author` is omitted or a single token (§2). The server does not store it anywhere except inside the `%%` it writes.
- **`If-Match`** is required. The value is the lowercase hex SHA-256 of the file bytes the page was rendered from, sent as `If-Match: "<hex>"`. The render stamps `<meta name="rl-source-sha256" content="hex">`. If the hash differs from the file now on disk, the response is **409** and the file is not written. The popover says the notebook changed and leaves the selection in place so the reader can retry after reload.
- **Single-user only.** This 409 is for one person and one working copy: the browser against that person's editor, or two tabs of the same `watch`. It is not a lock for two people editing one server. Another person gets the notes by pulling the git commit, and a conflict is resolved in git (§6). `watch` stays on loopback and is not a shared editor.
- `nb.save_lock` is held across read, check, splice, and write, so an annotate and a `POST /save` or a cell save cannot interleave.
- **Round-trip guard** before the write is committed to the response: re-scan the spliced text, require exactly the expected new or removed mark, and require every byte outside the splice to be unchanged. A failed guard returns 400 and does not write.
- Comment text is inserted as markdown source, then the normal render escapes it. The client does not send HTML.

Insert splice, one-line comment:

```markdown
==Group delay is constant== %%#c3 2026-10-08: only for linear phase%%
```

### 4.6 Live reload after a write

The handler does not render and does not broadcast. It writes the file and returns 204. The existing watcher (notify, 250 ms debounce, `render_loop`) re-renders and sends the usual WebSocket patch. The same path already runs after `POST /save/{slug}`.

A 409 does not write, so it does not re-render.

When the Comments checkbox is unchecked, the reload still leaves it unchecked. The new mark is in `<main>` and the CSS in §3.3 keeps it hidden. The confirmation line in the page chrome is the signal that the write landed (§3.2). When the checkbox is already checked, the re-render shows the mark and no confirmation line is added.

---

## 5. Phases and tests

| Phase | Contents | Size |
|---|---|---|
| **1** | `comments.rs`; HTML and JSON rendering, including cell comments (§7); theme roles; sidenotes and block notes; the Comments toggle; `--comments` / `--no-comments` for HTML, watch, markdown, and JSON; `check` W006–W012; `docs/notebooks.md` including the Obsidian divergence | 1 PR |
| **1b** | LaTeX/PDF, default off, `--comments` on, cell notes above the listing | 1 small PR |
| **2** | **Headline.** Right-click menu (Add comment, Highlight, Edit, Delete). Mouse `mouseup` does not open the popover; a `pointerup` opens it only for `pointerType` `"touch"` or `"pen"`. Cell selections write a whole-cell `%%`. `data-src-*` spans. `POST /annotate/{slug}` with Host, Origin, jail, `If-Match` → 409, mounted for `--annotate` and for `--editable`, and documented as single-user (this working copy only). Name field prefilled from `rl-comment-name`. A write while Comments is off saves the note, shows the confirmation, and leaves the checkbox unchecked. Live reload through the existing watcher | 1 PR |
| **3** (optional) | CodeMirror overlay so `==` and `%%` are visible in the source pane; insert from that pane when a rendered selection is rejected | 1 PR |

Phase 1 has no write route. Marks get into the file from an editor. `watch` re-renders on save, as it already does.

### Tests

- **Golden HTML** in `render.rs`: highlight, bound comment, standalone comment, block comment, header variants, `\==` escape, highlight in a heading (slug ignores the note), table cell, list, callout, footnote reference, mark beside `$math$` and beside inline code, `==` inside a rustlab fence and inside math left literal, unclosed `==` and `%%` as literal text plus a badge. A `%%` on its own line immediately before a fence is a note on that cell, not a paragraph. A blank line between them leaves an ordinary block note. `<!-- hide -->` between the `%%` and the fence still hides the cell and still binds the note. `%%`, `#`, and `%` inside the fence stay in the listing.
- **Toggle:** with the checkbox unchecked, notes are `display: none` and `<mark>` has no highlight background. The words remain.
- **CLI:** `--no-comments` HTML contains neither `rl-cm-mark` nor `rl-cm-note`. `--comments` PDF contains the highlight text and the comment text. Default PDF contains the highlight text and not the comment text.
- **Markdown:** comments on passes `==` and `%%` through. `--no-comments` unwraps and deletes them. `--obsidian` with comments on still emits `%%` (Obsidian will hide it; we must not strip it by accident).
- **Property:** a file with no `==` or `%%` renders byte-identical to today.
- **Theme contrast** for the three pairs in §3.5, all four builtins, ≥ 4.5:1. The context menu uses those pairs and adds no new color.
- **Context menu (phase 2):** a `contextmenu` on a valid selection inside `<main>` calls `preventDefault` and shows Add comment and Highlight. The same event with no selection, an invalid selection (cross-block or math), Shift held, or a target outside `<main>` or inside CodeMirror does not call `preventDefault`. A cell selection shows Add comment and not Highlight. A right-click on a note shows Edit comment and Delete. Shift+F10 opens the menu, Esc closes it, and Enter activates the focused item.
- **Popover trigger:** a `pointerup` with `pointerType` `"mouse"` does not open the popover, even when `matchMedia('(pointer: coarse)')` matches. A `pointerup` with `pointerType` `"touch"` or `"pen"` and a valid selection does. Choosing Add comment from the menu opens it. `mouseup` alone does not.
- **`check`:** each W006–W012, right line, sorted order. `%%looks off%%` is W012 on that line. `%%#c1: text%%` is not. An unclosed `%%` is W007 and not also W012. A bare `==` is not W012. A rustlab cell with `a == b`, `# comment`, or `% comment` produces no finding. `check --fix` does not insert a `#cN`. A cell comment uses the same codes as any other `%%`.
- **Server (phase 2):**
  - **Highlight** writes exactly `==text==` and no `%%`
  - a prose **Add comment** writes exactly `==text== %%#cN …%%` and no other bytes, with `@name` only when `author` is a non-empty token
  - a cell insert writes one `%%` line immediately before the fence's directive stack, and does not wrap any code in `==`
  - a selection that crosses a block, or crosses from a cell into prose, or overlaps math, is 400 and does not write
  - an empty cell comment is 400 and does not write
  - `file[start..end] != text` on a prose insert is 400
  - `If-Match` mismatch is 409 and does not write
  - edit and delete rewrite only the `%%` span; delete also removes `re #id` replies
  - the route is absent when neither `--annotate` nor `--editable` is set, and present for either flag
  - a successful insert while `#rl-comments` is unchecked leaves the checkbox unchecked, shows the confirmation outside `<main>`, and the new note is hidden by the comments-off CSS
  - a successful submit with a valid Name stores `rl-comment-name`; a blank Name omits `@name` and leaves that key as it was
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
| Two people comment on the same lines | Each has their own working copy. Git reports a conflict when those copies are pushed and pulled | Resolve it in git. The markers are ordinary text. `watch` does not merge two people's servers (§4.5). |
| This working copy changed under the page | The same person's editor, or another tab, wrote the file after the page rendered | `If-Match` fails: 409, no write. The popover says the notebook changed. This is one user, not a second person on the server. |
| A formatter reflows the paragraph | A hard wrap inside `==` is fine. A blank line, or a space inserted so two spaces sit between `==` and `%%`, unbinds the comment | W006/W008. Docs: do not run a markdown formatter that rewrites notebooks, or keep each note on one line. |
| `--editable` cell edit | `replace_code_block_source` copies the lines outside the fence verbatim. The preceding `%%` stays on that cell | The existing round-trip guard rejects a body that adds a `` ``` `` line. |
| Blank line inserted between a cell comment and its fence | The `%%` becomes an ordinary block note. It is no longer drawn on the cell | The binding rule is adjacency (§7). `check` does not invent a new warning for it. |
| The fence is moved and the preceding `%%` is left behind | The note stays on the lines that stayed | Adjacency is the anchor. There is no cell id to follow the fence. |
| Whole-file `POST /save/{slug}` | Last writer wins. This route has **no** `If-Match` today | Comment writes do not use it. They use `/annotate` and `If-Match`. |
| Live reload while the menu or popover is open | The selection's offsets may go stale. The reload closes both. | Submit still sends `If-Match`. A stale hash is 409, and the file is not changed. |

### 6.3 Mitigations

1. **Confinement.** A highlight stays in one markdown block and one paragraph or table cell. It does not cross a code fence, a math span, or an HTML comment. A blank line ends a highlight. A block `%%` may contain blank lines, and it ends at the closing line. One broken mark cannot swallow the rest of the file.
2. **`check` with line numbers.** W006–W012 (§3.7).
3. **Degrade in place.** An unmatched `==` or `%%` is literal text followed by `<span class="rl-cm-warn" role="img" aria-label="Unclosed highlight" title="Unclosed == (line 42)">⚠</span>`. The rest of the paragraph renders normally.
4. **Ids.** Browser inserts always write `#cN`. A hand-written `%%` without one is W012. Replies and deleting those replies use the id, not the display number. Edit and delete of the comment itself still match its byte span when the id is absent.
5. **Exact splice.** Insert, edit, and delete change one byte range after the file hash matches and (for edit/delete) `expect` matches, then the round-trip guard runs (§4.5).
6. **Live reload is per block.** A broken mark affects its block. Fixing it clears the badge on the next render.
7. **One working copy per person.** Comments move between people by `git push` and `git pull`. A merge conflict is resolved in git. `If-Match` only notices that this copy changed locally (§4.5).

### 6.4 If a sidecar is ever needed

Not in this design. If a later version needs state that does not belong in the `.md` (reactions, a resolved archive), keep the `==` / `%%#id` anchor in the file and put only the thread body in the sidecar. Look up `#id` first. Do not look up a quote while the id is still in the file.

---

## 7. Comments on code cells

A comment on code is a comment on the **whole cell**. It is a `%%` line, or a block `%%`, placed on the lines before that cell's fence. The same margin card is used. There is no highlight inside the listing, no line number, and no gutter marker on a line of code.

### 7.1 What other tools do

Two patterns cover the tools below. **Cell-level** means the note hangs off the cell as a unit. **Line-level** means it hangs off a line or a selected range, and has to be moved when that line moves. **In file** means a clone of the notebook still has the note. **Beside the file** means the note lives in a service, a database, or a pull-request thread.

| Tool | Anchor | Where the note lives | What survives an edit |
|---|---|---|---|
| Jupyter cell metadata (proposed) | cell | `metadata` inside the `.ipynb` | Travels with the file. A cell id keeps the note when the cell moves. [JupyterLab #12709](https://github.com/jupyterlab/jupyterlab/issues/12709) discusses `metadata.comments`. [JEP: cell ids](https://jupyter.org/enhancement-proposals/cell-id/) lists "associate comments to a cell" as a reason for stable ids. |
| `jupyterlab-comments` | cell in notebooks; line or selection in text files | sidecar `comments.db` | The db can be copied beside the tree. It is a second file. [usage.md](https://github.com/jupyterlab/jupyterlab-commenting/blob/master/docs/usage.md) says comments save in `comments.db`. Cell-level notebook comments were the shipped notebook gesture; character and single-line comments inside a notebook cell were still unchecked on the project list. |
| JupyterLab comment discussion / `jupyter-collaboration` | character range, via Yjs relative positions | a shared model beside the notebook content | Positions move when the text is edited in the live session. [JupyterLab #9885](https://github.com/jupyterlab/jupyterlab/issues/9885). [`jupyter-collaboration`](https://github.com/jupyterlab/jupyter-collaboration) syncs the Y document. It is not a comment syntax in the file. |
| Google Colab | margin comments on the Drive file | Drive, outside the `.ipynb` | Lost on "Save a copy". [colabtools #1927](https://github.com/googlecolab/colabtools/issues/1927). |
| Deepnote | a block, from the comment button on that block | the Deepnote project | [Comments](https://deepnote.com/docs/comments): the sidebar jumps back to the block; threads can be resolved. The [code-review](https://deepnote.com/docs/code-reviews) page also describes highlighting lines. Neither page describes a note inside an exported notebook. |
| Hex | a cell | the Hex project | [Commenting](https://learn.hex.tech/docs/collaborate/comments): notebook comments and published-app comments are separate. Notebook comments are visible in the Notebook view. The docs do not describe an exported file format. |
| Observable | a cell, with a count in the left margin; threads hidden until opened | the Observable notebook | [Comments](https://observablehq.com/documentation/collaboration/comments): anyone who can see the notebook can see the comments; editors resolve and delete. This is the hosted notebook, not a markdown file. |
| Databricks | a highlighted code section, then a comment bubble | the workspace notebook | [Collaborate using notebooks](https://docs.databricks.com/aws/en/notebooks/notebooks-collaborate) documents the highlight-then-comment gesture and `@` mentions. It does not document the comment as text inside the cell. |
| VS Code notebooks / GitHub | GitHub attaches a review comment to a line of the raw `.ipynb` JSON | the pull request | [VS Code #214017](https://github.com/microsoft/vscode/issues/214017): the notebook editor shows comments on a cell URI, and a GitHub line comment on the JSON does not appear there unless something maps the file range onto a cell. [VS Code #246317](https://github.com/microsoft/vscode/pull/246317) is that mapper. GitHub's rendered notebook diff is not itself a comment target; the commentable view is the JSON. [ReviewNB](https://blog.reviewnb.com/how-to-add-comments-to-notebook-diffs-github/). |
| ReviewNB | cell, and later line, on a rich diff | GitHub/Bitbucket for PR comments; ReviewNB's own store for JDoc | [ReviewNB](https://www.reviewnb.com/) posts PR comments to the forge. [JDoc](https://blog.reviewnb.com/commenting-for-jupyter/) stores comments on a standalone notebook at ReviewNB, because GitHub has no comment on a file outside a commit or PR. A rename looks like a new file, and the old thread does not follow. [Line-level comments](https://github.com/ReviewNB/support/issues/17) shipped after cell-level; the earlier workaround was to paste the line into the comment. |
| Quarto | a line, via a language comment `# <n>` inside the cell, plus an ordered list immediately after the cell | in the source | [Code annotation](https://quarto.org/docs/authoring/code-annotation.html). This is authored explanation, not a review thread. The marker is inside the program. `code-annotations: none` strips the markers from the output. |
| R Markdown / knitr | chunk options (`echo`, `eval`, …) in the chunk header or in `#|` lines at the top of the chunk | in the source | [knitr options](https://yihui.org/knitr/options/). `#|` is execution and display metadata. The chunk option named `comment` is the prefix on printed output (`##` by default), not a review note. |
| MyST | `{code-cell}` takes `:key: value` metadata. A `%` at the start of a line is a hidden comment | in the markdown | [Executable markdown](https://mystmd.org/guide/notebooks-with-markdown), [blocks and comments](https://mystmd.org/guide/blocks). The `%` comment is dropped from the output. It is not attached to the next code cell. |
| Obsidian | `%%` is hidden in reading view. There is no code-cell comment syntax | in the note | [Basic formatting syntax](https://obsidian.md/help/syntax). A `%%` that tries to wrap a fence is unreliable: a code block inside the comment can still render. [Forum thread](https://forum.obsidian.md/t/comment-semantics/60311). The Document Comments plugin stores an in-file HTML-comment anchor, including a line range inside a fence, and shows an orphan when the quoted text is gone. [Plugin page](https://community.obsidian.md/plugins/document-comments). |

The durable in-file anchors are the ones that sit in the text the cell already has: Jupyter metadata next to the cell, Quarto's list after the cell, a MyST or Obsidian comment in the markdown around the fence. Line numbers and quoted line ranges go stale when the cell is edited (ReviewNB's old paste-the-line workaround, the Document Comments orphan, GitHub comments on shifting JSON lines). Notes that live in a product's database do not come along with a git clone.

### 7.2 Recommendation

**A `%%` that ends immediately before a fence, or immediately before that fence's code directives, is a comment on that cell.** One-line and block forms both count. Several of them may stack. Replies stay `re #id` comments in that same stack.

````markdown
%%#c8 2026-10-08: why 64 taps?%%
```rustlab
h = fir1(64, 0.25);
```
````

Code-block directives stay between the comment and the fence. `parse.rs::extract_code_directives` walks backward from the fence and stops at the first line that is not a directive, so a `%%` inserted between `<!-- hide -->` and the fence would leave `hide` unapplied. The write path inserts **above** the directive stack:

````markdown
%%#c8 2026-10-08: why 64 taps?%%
<!-- hide -->
```rustlab
h = fir1(64, 0.25);
```
````

Binding rule, scanned forward from the comment's closing line: the following non-empty lines may only be `<!-- hide -->`, `<!-- code: … -->`, `<!-- caption: … -->`, `<!-- details: … -->`, or `<!-- grid: … -->`, and then a fence opener. A blank line, or any other line, leaves an ordinary block note. An inline `%%` in the paragraph above the fence stays a prose comment.

This applies to every fence `parse_notebook` splits out: `rustlab`, mermaid, and widget. A `bash` / `python` / `text` fence stays inside the markdown block, so a `%%` before it is an ordinary prose note.

**Why this one.** It is the prose comment syntax, in the file, on the cell as a whole. Editing the cell body does not move it, because `replace_code_block_source` copies the surrounding markdown unchanged. Moving the section in the markdown carries the note when the author moves those lines with the fence. Obsidian already hides a `%%` in that position, which matches §1.2. No new delimiter, no fence attribute, and no change to the rlab lexer.

**Rendering.** The note is not a paragraph above the cell. The renderer takes a trailing cell-comment `%%` off the preceding markdown block and draws it as `.rl-cm-note` on the code section, with the same number superscript used as a badge on the cell. Wide viewports float the card in the gutter. Narrow viewports show the badge, and focus expands the card (`:focus-within`), the same as a prose note. The listing itself is unchanged: no `<mark>`, no per-line marker.

**Toggle, CLI, PDF.** The Comments switch and `--no-comments` hide or omit the card and the badge the same way they omit any other note (§3). The code remains. `--comments` on PDF prints the note immediately above the listing. The default PDF drops it.

**Selection in `--annotate`.** A non-empty selection that starts and ends inside one code section uses the same context menu (§4.1). **Add comment** opens the popover and writes a cell comment. **Highlight** is not in that menu. A touch or pen `pointerup` inside the cell opens that popover directly, still without Highlight (§4.2). A mouse `pointerup` does not. The request is `target: "cell"` with the fence opener's byte offset. The server checks that offset is still a fence opener, inserts one `%%` line above the directive stack, and requires a non-empty comment. It does not copy the selected lines into the comment. A selection can contain `%%`, `==`, or a fence-like string of backticks, and copying it into the `%%` body would close the comment or break `parse_notebook`. The popover shows the selected lines as read-only context so the reader can see what they pointed at. Citing a line is something they type, in their own words. A selection that leaves the cell is rejected (§4.3).

**Sync.** The note survives a cell-body edit. It detaches when a blank line, or a line that is not a code directive, is inserted between it and the fence. It does not follow a fence that is moved without it. That is the trade for having no cell id. The `%%` is visible in the markdown next to the fence, so a diff shows whether the note moved with the cell. Another person sees it by pulling that commit. Two people do not attach to one `watch` and edit the cell together (§4.5, §6).

**`check`.** No cell-specific code. Unclosed, nested, duplicate, orphan-reply, and missing-id cases are W006–W012. A well-formed cell `%%` with no `#cN` is W012 on the opening line. A `#` line, a `%` line, or `a == b` inside the fence is not a finding.

### 7.3 Alternatives left out

- **A sidecar or a service store** (Colab, Deepnote, Hex, Observable, Databricks, `comments.db`, ReviewNB JDoc). The note does not travel in the `.md`. Sidecars are already a non-goal.
- **A cell id** on the fence, with the note looked up by that id. Jupyter needs this because the note is not the previous line. Adjacency is the whole mechanism here.
- **Line markers inside the cell** (`# <1>` as in Quarto, a gutter dot, a stored line number). They edit the program or go stale when a line is inserted above them. `#` and `%` are already rlab comments, so a `%%` inside the fence cannot be a review mark without a lexer change.
- **A note after the fence.** Quarto puts its annotation list after the cell. In this syntax a `%%` after the fence is easier to read as a comment on the next paragraph. Before the fence, the next construct is the cell.
- **`<!-- note: … -->`.** rustlab already uses HTML comments for renderer directives (`hide`, `code`, `caption`, `details`, `grid`). A review note in that form would be a second comment syntax. `%%` is the one review syntax.
- **The Obsidian Document Comments anchor** (HTML comments recording a line range and a quote). In-file, and heavier. The quote becomes an orphan when the line changes. §7.2 does not store a quote.

---

## 8. Open questions

None. The calls that were open are recorded under [Decisions](#decisions).
