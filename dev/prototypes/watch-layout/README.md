# Watch layout prototypes

These four HTML files are design mockups. They are not the `notebook watch`
page, and opening one does not run the server. Each file is self-contained
(inline CSS and JS, no network, no libraries). `?state=expanded` and
`?state=collapsed` force the two screenshot states. `shot=1` locks the
viewport to one screen. The text meter in the mock top bar is the width of
the prose column in that mock, not a measurement of the live watch page.

The page body is `examples/notebooks/spectral_estimation.md` (prose, code,
and headings). The waveform and spectra are stand-in figures, not gallery
SVGs. The two margin notes are sample chrome so the comment column is
visible; they are not in that notebook. The file tree lists real names
from `examples/notebooks/`.

## Status

**Option B is the production direction, and it has shipped.** `notebook
watch` in 0.3.41 (PR #121, on main) puts the file tree and the section
outline in one overlay drawer. The drawer reserves no page width. **Contents**
in the top bar opens it, and `\` toggles it, except while typing in a field
or the editor. It closes on a click outside, on Escape, or when a file or a
heading is chosen. A folder summary only opens or closes that folder.
Folders still start closed, except the folders that contain the notebook
on the page. On a wide window, headings and plots shift clear of the open
drawer so a plot title or axis label is not covered. Under 768px the open
drawer covers the page until it is dismissed. `?contents=open` starts a
watch page with the drawer open. Static `notebook render` still uses the
fixed file browser and the 220px outline.

This draft does not implement that layout again. The HTML files stay as
the design artifacts they were. The screenshots below are of those files,
not of the watch server.

### What the live watch page also has

The mockups were drawn against an earlier page, so they omit chrome that
is on `notebook watch` now:

- The top bar still has previous, the parent link, the title, and next.
  **Contents** sits at its left. A failed code cell adds a `1 error` /
  `N errors` chip that links to `#rl-error-1` (0.3.55). The failed cell
  uses `.rl-cell-error` and opens its source.
- The right toolbar is `⋯` and **Source**. `⋯` holds **Comments** and
  **Show changes**. Comments on or off is remembered per tab
  (`rl-comments-visible`). Git comparison stays off unless `watch --git-diff`
  or `[notebook] git_diff = true`. While it is on, `[` and `]` move
  between changes when you are not typing.
- The comment margin is still a 14rem column (a 2.5rem number strip when
  collapsed, a drawer under the section under 800px). Highlights and the
  right-click comment menu are unchanged.
- **Source** is the slide-in source pane. It is an editor only with
  `--editable`.

### What shipped, and what these files still show

| Mock | Role now |
|---|---|
| [B · overlay drawer](b-overlay-drawer.html) | The direction that shipped. The mock is not the implementation. It uses a vertical edge tab, hides on pointer-leave unless **Pin** is on, and toggles with `[`. Production uses the **Contents** button, does not hide on pointer-leave, has no Pin, and toggles with `\`. `[` and `]` are git-diff navigation. |
| [A · icon rails](a-icon-rails.html) | Not chosen. Both panels would collapse to icon rails, with `[`, `]`, and `\`. |
| [C · tabbed sidebar](c-tabbed-sidebar.html) | Not chosen. One tabbed column, width remembered in `localStorage` (`rl-proto-c-width`). |
| [D · focus](d-zen-dock.html) | Not chosen. A focus mode hides the tree, the outline, and the comment column, and docks comments on a right rail. |

### What is not left to build from this draft

The shared overlay drawer is already in `notebook watch`. Do not rebuild it
from these files.

These ideas were not selected, and this draft does not add them to the
server:

- A's icon rails.
- B's edge tab, Pin, and pointer-leave auto-hide.
- C's single tabbed column and remembered width.
- D's focus mode, and docking the comment margin so it no longer reserves
  14rem.

The comment column is still part of the live page. Narrowing or docking it
was D's proposal, not part of the B decision.

## How the mocks behave

`?state=expanded` and `?state=collapsed` are the screenshot states. The
meter is the mock prose column.

| | Open | Closed | How you get it back in the mock |
|---|---|---|---|
| [A · icon rails](a-icon-rails.html) | Tree and outline side by side | Each becomes a 2.75rem icon rail | Click a rail, or `[` files, `]` sections, `\` both |
| [B · overlay drawer](b-overlay-drawer.html) | One drawer over the page (files, then sections) | Nothing reserved; a vertical edge tab remains | Edge tab, `[`, or `Esc`. Pointer-leave auto-hides unless pinned |
| [C · tabbed sidebar](c-tabbed-sidebar.html) | One sidebar, Files or Sections | A 2.75rem strip with the two tab icons | Click a tab. Drag the edge; the width is stored in this browser |
| [D · focus](d-zen-dock.html) | Tree, outline, and the comment column | All three hide. Comments dock to a right rail | **Focus** / `z`. `Esc` exits |

A, B, and C leave the comment margin in the page (the `‹` button collapses
it to a number strip). D takes that margin out of the flow. Under 800px,
D's dock drops to a bottom sheet.

Colors follow the mocha watch page as it was when the mocks were drawn:
file tree `16.5rem`, outline `220px`, comment margin `14rem`. At that time
the tree could already shrink to a 2.75rem strip and the outline could not,
so both open columns reserved about **484px**. Those numbers are the
baseline the table below was measured against. They are not the live watch
layout after 0.3.41.

## Width of the prose column in the mocks

Measured by each mock's on-page meter when the screenshots were taken.
"Open" is `?state=expanded`. "Closed" is `?state=collapsed`.

| Prototype | 1440 open | 1440 closed | 1024 open | 1024 closed |
|---|---:|---:|---:|---:|
| A icon rails | 636 | 760 | 220 | 616 |
| B overlay drawer | 760 | 760 | 704 | 690 |
| C tabbed sidebar | 760 | 760 | 424 | 660 |
| D focus | 636 | 804 | 220 | 804 |

At 1440 the mock reading cap (760px, or 820px in focus) is the limit, so
closing A's rails only added about 124px of text. The painful mock width
was 1024, where 484px of left chrome plus a 224px comment column left a
220px article.

B does not reflow when its drawer opens, which is why its open and closed
text widths match. The mock drawer is `22rem` and covers the left of the
article. C's open state is already one column (280px, remembered), so at
1440 the prose is at the cap even while a panel is open. D is the only mock
that also drops the comment column.

## Trade-offs, as decided

**Discoverability.** A's rails stay on screen. C's strip is the same idea
for one column. B's edge tab is easy to miss, and pointer-leave closes the
drawer while someone is still reading a filename unless they hit Pin.
Production avoided that by putting **Contents** in the top bar and closing
only on an explicit outside click, Escape, or a file or heading choice.
D hides the most, including previous and next.

**Comment margin.** A, B, and C keep the 14rem column, so left chrome is
only half the squeeze. D docks it. Production followed B and left the
margin in place, including the right-click menu and the numbered highlights.

**Mobile.** The mock page overlays the tree and the outline under 768px,
and drops comments under the section under 800px. A's `[` `]` shortcuts
need a keyboard, and those keys are now git-diff navigation on the live
page. B's vertical tab is a small touch target. C overlays under 768px so
a remembered width cannot come back as a column on a phone. D's bottom
sheet is the phone form of its dock. Production's drawer uses the same
idea as B on a phone: it covers the page until it is dismissed, and it
does not reserve width when closed.

**What not to combine.** A rail plus a focus mode is two ways to hide the
same chrome. A remembered width does not matter for a drawer that never
reserves space. The decision was the shared overlay, not the C-plus-D
pairing this note originally suggested.

## Screenshots

1440×900 and 1024×900, expanded and collapsed. These are the mockups, not
screenshots of `notebook watch`.

### A · icon rails

| 1440 expanded | 1440 collapsed |
|---|---|
| ![A 1440 expanded](screenshots/a-icon-rails-1440-expanded.png) | ![A 1440 collapsed](screenshots/a-icon-rails-1440-collapsed.png) |

| 1024 expanded | 1024 collapsed |
|---|---|
| ![A 1024 expanded](screenshots/a-icon-rails-1024-expanded.png) | ![A 1024 collapsed](screenshots/a-icon-rails-1024-collapsed.png) |

### B · overlay drawer

| 1440 expanded | 1440 collapsed |
|---|---|
| ![B 1440 expanded](screenshots/b-overlay-drawer-1440-expanded.png) | ![B 1440 collapsed](screenshots/b-overlay-drawer-1440-collapsed.png) |

| 1024 expanded | 1024 collapsed |
|---|---|
| ![B 1024 expanded](screenshots/b-overlay-drawer-1024-expanded.png) | ![B 1024 collapsed](screenshots/b-overlay-drawer-1024-collapsed.png) |

### C · tabbed sidebar

| 1440 expanded | 1440 collapsed |
|---|---|
| ![C 1440 expanded](screenshots/c-tabbed-sidebar-1440-expanded.png) | ![C 1440 collapsed](screenshots/c-tabbed-sidebar-1440-collapsed.png) |

| 1024 expanded | 1024 collapsed |
|---|---|
| ![C 1024 expanded](screenshots/c-tabbed-sidebar-1024-expanded.png) | ![C 1024 collapsed](screenshots/c-tabbed-sidebar-1024-collapsed.png) |

### D · focus, comments docked

| 1440 expanded | 1440 collapsed |
|---|---|
| ![D 1440 expanded](screenshots/d-zen-dock-1440-expanded.png) | ![D 1440 collapsed](screenshots/d-zen-dock-1440-collapsed.png) |

| 1024 expanded | 1024 collapsed |
|---|---|
| ![D 1024 expanded](screenshots/d-zen-dock-1024-expanded.png) | ![D 1024 collapsed](screenshots/d-zen-dock-1024-collapsed.png) |
