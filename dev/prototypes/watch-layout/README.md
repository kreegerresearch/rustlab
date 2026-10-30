# Watch layout prototypes

Static mockups for giving the notebook column more room in `notebook watch`.
The production server, CSS, and client script are unchanged.

Each file is self-contained (inline CSS and JS, no network, no new libraries).
Open one in a browser. `?state=expanded` and `?state=collapsed` force the
two screenshot states. The text meter in the top bar is the width of the
prose column.

The page body is `examples/notebooks/spectral_estimation.md` (prose, code,
and headings). The waveform and spectra are stand-in figures, not gallery
SVGs. The two margin notes are sample chrome so the comment column is
visible; they are not in that notebook. The file tree lists real names
from `examples/notebooks/`.

Colors and column sizes follow the current mocha watch page: file tree
`16.5rem`, outline `220px`, comment margin `14rem` (strip `2.75rem` /
`2.5rem`). Today the tree already collapses, by closing its root
disclosure. The outline does not, so both open columns reserve **484px**
on a desktop. These mockups collapse the outline as well.

## What each one tries

| | Open | Closed | How you get it back |
|---|---|---|---|
| [A · icon rails](a-icon-rails.html) | Tree and outline side by side, as now | Each becomes a 2.75rem icon rail | Click a rail, or `[` files, `]` sections, `\` both |
| [B · overlay drawer](b-overlay-drawer.html) | One drawer over the page (files, then sections) | Nothing reserved; a vertical edge tab remains | Edge tab, `[`, or `Esc` to close. Pointer-leave auto-hides unless pinned |
| [C · tabbed sidebar](c-tabbed-sidebar.html) | One sidebar, Files or Sections | A 2.75rem strip with the two tab icons | Click a tab. Drag the edge; the width is stored in `localStorage` (`rl-proto-c-width`, 200–420px) |
| [D · focus](d-zen-dock.html) | Tree, outline, and the comment column, as now | All three hide. Comments dock to a right rail (click it to read) | **Focus** / `z`. `Esc` exits |

A, B, and C leave the comment margin in the page (the `‹` button still collapses it to a number strip, same idea as today). D is the one that takes the margin out of the flow.

## Width of the prose column

Measured by the on-page meter. “Open” is the expanded screenshot; “closed” is the collapsed one. Production at the same widths is the same math as A/D open: about **636px** at 1440 and **220px** at 1024.

| Prototype | 1440 open | 1440 closed | 1024 open | 1024 closed |
|---|---:|---:|---:|---:|
| A icon rails | 636 | 760 | 220 | 616 |
| B overlay drawer | 760 | 760 | 704 | 690 |
| C tabbed sidebar | 760 | 760 | 424 | 660 |
| D focus | 636 | 804 | 220 | 804 |

At 1440 the reading cap (760px, or 820px in focus) is the limit, so closing A’s rails only adds about 124px of text — the rest comes back as margin. The painful width is 1024, where 484px of left chrome plus a 224px comment column leaves a 220px article. Closing both rails there adds about 400px.

B does not reflow when the drawer opens, which is why its open and closed text widths match. The drawer is `22rem` and covers the left of the article instead of pushing it. C’s open state is already one column (280px, remembered), so at 1440 the prose is at the cap even while a panel is open. The 1024 open state (424px) is the interesting one: wider than today, narrower than a collapsed rail.

D’s focus mode is the only layout that also drops the comment column, which is why it passes 800px at both widths.

## Trade-offs

**Discoverability.** A’s rails stay on screen, so a collapsed tree still looks like a control. C’s strip is the same idea for one column. B’s edge tab is easy to miss, and auto-hide will close the drawer while someone is still reading a filename unless they hit Pin. D hides the most, including previous / next; the way back is the Focus button, which is the whole point and also easy to lose if the top bar is ever hidden too.

**Comment margin.** Keeping the 14rem column (A, B, C) means the left chrome is only half the squeeze. At 1024, comments still cost 224px after the rails collapse. D docks that column: a 2.75rem rail of note numbers, click to open an overlay that does not change the prose width. Under 800px that dock drops to a bottom sheet, which matches today’s narrow-screen comment drawer better than a right rail does.

**Mobile.** The real page already overlays the tree and the outline under 768px, and drops comments under the section under 800px. A keeps that overlay (small launch buttons) but the `[` `]` shortcuts need a keyboard. B is the overlay pattern already, so it fits a phone; the vertical tab is a small target and auto-hide fights a tap that was just trying to scroll. C should overlay rather than reserve its remembered width under 768px, which this mock does — a saved 420px sidebar must not come back as a column on a phone. D’s focus mode is the reading mode; the bottom-sheet dock is the part that has to be right on a phone, because a right rail sits under the scrollbar and the thumb.

**What I would not combine.** A full-time icon rail plus a focus mode is two ways to hide the same chrome. B’s auto-hide plus C’s remembered width fight each other (a saved width does not matter if the panel never reserves space). The useful pairing is C’s single sidebar for people who navigate, and D’s focus mode for people who are reading a comment thread — focus can sit on top of whichever sidebar we keep.

## Screenshots

1440×900 and 1024×900, expanded and collapsed. `shot=1` in the query locks the viewport so the capture is one screen.

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
