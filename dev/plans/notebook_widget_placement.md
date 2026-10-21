# Notebook widget placement — controls beside the plot they drive

**Status:** implemented (2026-10-16) on `cursor/notebook-widget-panel-8b5b`. The comments column shipped in #72 at 14rem and is resizable, so the panel nests in the cell's content column rather than assuming the 18rem track in the ASCII below.
**Created:** 2026-10-16
**Decisions:** Michael, 2026-10-16 (recorded below; not renegotiated here).

## Goal

A `rustlab-widget` fence that sits immediately above a `rustlab` cell should read as that cell's control panel: left of the plot, in one card, with readable theme colors. Widgets left in the prose flow keep today's full-width bar. No new fence type, no layout block, no change to what re-runs.

`examples/notebooks/widgets_demo.md` has a heading and a paragraph between its three widgets and the plot cell. That notebook keeps full-width bars.

## What the code does today

Verified in this tree. The older widgets plan is the history; this section is the current behavior.

- Parse: ` ```rustlab-widget ` in `parse.rs` becomes `Block::Widget` via `widget::parse_widget`. Bad TOML becomes a Caution callout. Blank lines between fences do not become blocks (an empty line never fills `markdown_buf`). `<!-- grid: N -->`, `<!-- hide -->`, `<!-- details: -->`, `<!-- caption: -->`, and `<!-- code: -->` are stripped onto the following `rustlab` cell by `extract_code_directives` and also do not become blocks.
- Values: `widget("name")` reads a server-side table. `CacheEntry.widget_reads` records which executable block read which name. `NotebookCache::valid_prefix_widget_aware` drops the cache at the first code/mermaid block whose source hash or read-set changed, then runs that block through the end. Widget fences are not cache slots.
- HTML: `render_widget_html` emits one full-width `<form class="rl-widget">` per fence, in source order, each its own `<section class="rl-block">`. CSS is a flex row using `--rl-*` tokens (`render.rs`).
- Live: `server/ws.rs` listens for `input` and `change` on `.rl-widget-input`, updates `<output>` immediately, and sends `widget_update` after 50 ms. Watch injects that script; batch HTML does not.
- Batch HTML still emits enabled range/number/radio controls. They look live and do nothing. Markdown emits `**Label:** \`value\``. LaTeX/PDF emit `\textbf{Label:} \texttt{value}`. JSON emits `{name, widget_type, label, value}`.
- Comments: when a page has notes, `body.has-cm-margin` lays each `section.rl-block` out as `minmax(0, 1fr) | 18rem` (`--rl-cm-col`). Direct children other than `.rl-cm-margin` are forced into column 1. At `max-width: 799px` the column width goes to 0 and the cards drop under the section. The scanner skips fence interiors, so `==` / `%%` inside a widget body are not highlights. A `%%` immediately before a fence (no blank line) is a cell comment on that fence (`is_fence_rendered` includes widgets). Comments never rewrite the fence.
- Live reload: `applyReconcile` only moves `main > section.rl-block` (`:scope >`). A wrapper div around a widget section and a plot section would drop those sections out of reconcile. `is_flat` only rejects exercise/solution wrappers, so it would not save us.
- `widget_reads` hangs off `CacheEntry`, not `Rendered`. The renderer never sees it.
- Plot divs are `id="plot-N"` (`render_figure_plotly_div`). Animations share the same counter and use `anim-N`. There is no "Figure N" caption today.
- `run` is not a TOML key. Serde's default ignores unknown fields, so a `run` line would be dropped on the floor.
- `widget.rs`'s module comment still says `option` and `number` parse-error. They ship. `docs/notebooks.md` says static formats show label and value instead of a control; that is true for Markdown, LaTeX, PDF, and JSON, and false for batch HTML.
- v1 non-goal, `dev/plans/notebook_interactive_widgets.md`: "Layout primitives (rows, columns, tabs). One widget per declaration block, stacked vertically in source order." Phase 5 of that plan is more widget types (checkbox, dropdown, text, color, log-slider), not placement.

Watch already rewrites the Plotly/KaTeX CDN URLs to vendored `/assets`. Batch HTML still loads `cdn.plot.ly` and `cdn.jsdelivr.net`. This design adds no script and no stylesheet from a CDN. New CSS uses the existing `--rl-*` tokens.

## MATLAB, and what we are not copying

Sources:

- [Add Interactive Controls to a Live Script](https://www.mathworks.com/help/matlab/matlab_prog/add-interactive-controls-to-a-live-script.html)
- [Create Live Scripts](https://www.mathworks.com/help/matlab/matlab_prog/create-live-scripts.html) (Output on Right / Output Inline / Hide Code)
- [Share Live Scripts](https://www.mathworks.com/help/matlab/matlab_prog/share-live-scripts.html)
- [uigridlayout](https://www.mathworks.com/help/matlab/ref/uigridlayout.html) (App Designer: fixed control column, `1x` axes)

Live Editor controls replace a literal in the code line, so the control lives in the section it drives. Run-on-change is per control: current section (default), current section plus stale sections above, current section to end, all sections, or nothing. A button control can run user-defined code; the docs suggest setting every other control to Nothing and letting the button run. Sliders and spinners choose `Value changing` (while dragging; the default) or `Value changed` (on release). Three page layouts: Output on Right, Output Inline, Hide Code (labeled controls and output only). Export to PDF, Word, HTML, LaTeX, Markdown, or Jupyter substitutes the control's current value into the code and drops the button.

App Designer `uigridlayout` is the layout we actually want a reader to see: a fixed left column of controls (~150–220 px in MathWorks' examples) and a `1x` axes that takes the rest. One grid, one card, controls not repeated per plot.

We take the left column and the release-vs-live slider choice. We do not take in-source literals, per-control run scope, a Run button, Hide Code, or export-by-rewriting-the-code. rustlab source stays the markdown the author wrote. Export keeps showing label = value (Markdown / LaTeX / PDF) or a disabled panel (batch HTML).

## Decisions

1. **Adjacency is the only grouping rule.** One or more `rustlab-widget` fences immediately before a `rustlab` cell, with only blank lines between them, are that cell's control panel. No new syntax.
2. **The panel sits left of the plot.** About `16rem`, plot `1fr`, one shared card, colors from `--rl-*` only. Narrow screens stack the controls above the plot. The panel is nested inside the section's content column so it never becomes a third track next to the 18rem comments column.
3. **Everyone else keeps the full-width bar.** Those bars gain a small "drives: Figure N" link from the recorded read-sets. Plots that re-ran because of a widget change get a brief outline flash.
4. **Re-run scope stays put.** First block that read the changed widget, through the end. `valid_prefix_widget_aware` is not modified.
5. **Batch HTML** uses the same panel with controls `disabled` and one note: `default values — open with notebook watch to interact`. Markdown, LaTeX, and PDF keep label = value text.
6. **`run = "release"`** on a slider sends the value when the thumb is released. The default stays live-while-dragging (today's `input` path, 50 ms debounce).
7. **Out of scope** (see Alternatives): mirroring one widget beside every plot that reads it, a Run button, a page-wide hide-code view, an explicit row or layout block.

## Syntax and edge cases

A panel run is a maximal sequence of `Block::Widget` that ends at a `Block::Code`, with nothing between them in the block list. Blank lines already disappear at parse time, so "only blank lines" means "consecutive widget blocks then a code block."

```rustlab-widget
name = "amp"
type = "slider"
min = 0.1
max = 2
step = 0.1
default = 1
label = "Amplitude"
run = "release"
```

```rustlab
amp = widget("amp");
t = linspace(0, 1, 400);
plot(t, amp * sin(2 * pi * 3 * t));
```

| Situation | Result |
|---|---|
| Several widgets, then a `rustlab` cell | One panel, widgets stacked in source order, attached to that cell only. |
| A second `rustlab` cell after that | No second panel. Downstream plots still re-run (decision 4) and flash (decision 3). |
| Heading, paragraph, callout, or list between the widget and the cell | Not a panel. Full-width bars. This is `widgets_demo.md`. |
| `<!-- grid: N -->` / hide / details / caption / `<!-- code: -->` between them | Still a panel. Those lines are directives on the cell, not blocks. The grid tiles that cell's figures inside the plot column. |
| Widget, then mermaid / callout / another heading | Not a panel. |
| Malformed widget (Caution callout) in the run | The callout is a block, so it ends the run. Widgets after the callout can still form a panel with the cell; widgets before it stay bars. |
| Cell with no figure and no animation | Still a panel, but a single column: the controls use the card width. An empty 16rem column beside nothing looks like a bug. |
| Cell with figures | Source, printed output, and errors stay full width above the split, same as today. The split is controls \| plots (and animations). `<!-- grid: N -->` applies only inside the plot column. |
| `<!-- hide -->` | Hides source, as today. The panel and the plots stay. |
| `%%` immediately before a widget fence | Cell comment on that widget. When the widget joins a panel, the note moves to the shared section's right margin. The fence bytes are not edited. |
| `%%` between the widget and the cell, no blank line | That is a markdown block. It breaks adjacency (the widgets stay bars) and remains a cell comment on the `rustlab` fence. A blank line above that `%%` already unbinds it (`preserve_unbound_comment_gap`); same layout result. |
| Highlight or `%%` inside the widget fence | Unchanged: the comment scanner skips fences. Not a highlight, not a panel signal. |
| `run` on a number or option | Parse error, Caution callout, same as any other bad widget. `run` is slider-only. |
| `run = "live"` or omitted | Today's drag behavior. |
| Duplicate `name` | Pre-existing: the value map keeps the last declaration, and `id="rlw-{name}"` would clash. Not this project. |

Placement is source order, not "who called `widget()`". A panel widget that a later cell reads does not grow a drives link; the link is only for bars. The later plot still flashes.

## Layout sketch

Wide. The comments column exists only when the page has notes. It is the section's second track, not a sibling of the 16rem column.

```
section.rl-block                         [ comments 18rem ]
┌────────────────────────────────────────┐  only if this page
│ rustlab source (full width)            │  has notes; cards
│ printed output / error                 │  stay in this track
├────────────────┬───────────────────────┤
│ Amplitude      │                       │
│ [====|-----] 1 │        plot           │
│ Frequency      │        (1fr)          │
│ [ 3 ]          │                       │
│ ~16rem         │                       │
└────────────────┴───────────────────────┘
```

Narrow (`max-width: 799px`, same query as the comments drawer):

```
┌──────────────────────────┐
│ source                   │
│ Amplitude  [====|-----]  │
│ Frequency  [ 3 ]         │
│ plot                     │
│ comments (under section) │
└──────────────────────────┘
```

A non-adjacent bar, unchanged except for the link:

```
┌─────────────────────────────────────────┐
│ Amplitude   [====|-----]  1             │
│ drives: Figure 2                        │
└─────────────────────────────────────────┘
```

## HTML and CSS

Fold the panel into the code cell's existing `section.rl-block`. Do not wrap sections. Reconcile, `data-code-idx`, and ▶ Run keep addressing that one section. A grouped widget does not emit its own section; a bar still does. Dropping or restoring those sections is a normal structural reconcile (the count changed).

Inside the code section:

```html
<section class="rl-block" data-code-idx="0" data-src-kind="code">
  <div class="code-block">
    <div class="rl-cell">…source, output, error…</div>
    <div class="rl-drive">
      <div class="rl-drive-controls">
        <!-- the same <form class="rl-widget"> elements as today -->
      </div>
      <div class="rl-drive-plot">
        <!-- plot-container / image-grid / animation, unchanged -->
      </div>
    </div>
  </div>
  <aside class="rl-cm-margin">…</aside>
</section>
```

- `.rl-drive` is `grid-template-columns: 16rem minmax(0, 1fr)` with a gap, border, radius, and `background: var(--rl-bg-secondary)`. Border `var(--rl-border)`. Label `var(--rl-text)`. Readout `var(--rl-accent-primary)`. Note `var(--rl-text-dim)`.
- No plot and no animation: omit `.rl-drive-plot` and use one column. Controls match today's stacked bars, just inside the card.
- `.rl-cm-margin` stays a direct child of the section, in grid column 2. The drive grid is inside column 1, under `.code-block`. On a comments page the 16rem is a slice of the content track, not a fight with the 18rem track. The file browser and the TOC are outside `<main>` and are unchanged.
- At `max-width: 799px`, `.rl-drive` becomes one column, controls first.
- Widget `id="rlw-{name}"` is unchanged and still unique per name. Do not duplicate a control into the panel and a bar.
- Cell comments peeled onto a grouped widget are appended to that section's margin aside, in source order, then the code cell's own notes. `data-src-*` for annotate stays on the section for the code fence; each form carries the widget fence's range in `data-src-start` / `data-src-end` so a later comment target can still find the fence. No write path in this work. `--annotate` must not gain a reason to edit a widget body.
- A dirty inline cell editor already vetoes partial replacement of its section (`rl-cell-stale`). The panel lives in that section, so it vetoes too. Leave that as is.

## Client JS

All of this stays in the existing watch script in `server/ws.rs`. No new file, no CDN.

- **Release.** Slider forms with `run = "release"` get `data-widget-run="release"`. On `input`, still update `<output>` so the number tracks the thumb, and do not send. On `change` (thumb released), send `widget_update` through the same 50 ms timer. Omitted or `"live"` keeps both listeners.
- **Flash.** The page tags `widget_update` with a request id. The server echoes that id on the `partial`, `reconcile`, or `full` envelope that render publishes, and the page adds `rl-plot-flash` to `#plot-N` / `#anim-N` inside sections the update replaced only when the echoed id matches. A missing or different id clears the outstanding id and does not flash. A preempted render publishes nothing, so a later file save (no id) cannot flash. CSS: a short outline animation on `var(--rl-accent-primary)`, on the order of 700 ms, no layout shift. The first cut used a sticky client flag; a preempted render left it set, and the next save flashed. The id replaced that flag.
- **Drives link.** Ordinary `<a href="#plot-N">` (or `#anim-N`). No script. The browser scrolls to the div that already has that id.

## Drives link

After execution, copy each code block's read names onto the rendered block (a `Vec<String>` on `Rendered::Code` is enough; mermaid reads nothing). The HTML renderer already assigns `plot-N` / `anim-N` in order. A bar whose name appears in a block's read-set links to each figure or animation that block produced: `drives: Figure 2`, or `drives: Figure 2, Figure 4` if several. N is that shared 1-based counter. No figures means no link (a text-only reader is not "Figure N").

Panel widgets do not get the link. The flash still marks every plot in a section a widget update replaced, including plots downstream of a panel.

## Batch and export

| Format | Grouped widgets | Bars |
|---|---|---|
| `notebook watch` HTML | Live panel | Live bar + drives link |
| Batch HTML | Same card, every control `disabled`, one note under the controls: `default values — open with notebook watch to interact` | Same disabled control and the same sentence, plus the drives link when there is a figure |
| Markdown | `**Label:** \`value\`` per widget, in source order. No card. | Same |
| LaTeX / PDF | `\textbf{Label:} \texttt{value}` per widget. No card. | Same |
| JSON | Unchanged blocks (`name`, `widget_type`, `label`, `value`). No layout. | Same |

`widget()` in batch and export still returns the declared default. Disabled controls are a display of that default, not a second source of truth.

## Alternatives considered

**Mirror one widget next to every plot that reads its name.** A name reference (`widget` fence stays put; each reading cell draws a copy). Rejected for now. Two controls, one value, and a partial update has to keep them in sync. Adjacency gives one obvious panel. Revisit if authors regularly drive several figures from one slider and the flash is not enough.

**A Run button, and per-control run scope** (MATLAB's button, plus current section / to end / all / nothing). Rejected. Decision 4 keeps one scope: first reader through the end. A button wants the other controls to run nothing, which is a second execution model. Phase 5 of the widgets plan already has a different "play" idea (sweep a slider). Don't mix them.

**Page-wide Hide Code.** MATLAB's sharing view. Rejected. `<!-- hide -->` and `<!-- code: collapsed -->` already hide or fold one cell. A page view is a reader preference, not a placement rule.

**An explicit layout block** (`<!-- layout: controls-left -->`, a row fence, a grid container). Rejected. The v1 widgets plan called layout primitives out of scope, and a directive the author must remember is easier to get wrong than "the widgets sitting on top of the cell belong to it." `<!-- grid: N -->` keeps its current job (tile that cell's figures).

## Tests

Headless, in the implementation PR. No new fixtures that need a browser binary.

- Grouping: widgets, blank lines, `rustlab` cell → one section containing `.rl-drive` and no extra widget section. Prose or a heading between them → no `.rl-drive`, one section per widget. `widgets_demo.md` asserts the second shape.
- Second cell, mermaid-after-widget, and a Caution callout in the middle assert the table above.
- `<!-- grid: 2 -->` between a widget and a two-figure cell: `image-grid` is inside `.rl-drive-plot`, and the widget is not a grid cell.
- No-figure cell: `.rl-drive` has no plot column.
- Comments: a `%%` on a grouped widget still produces `.rl-cm-margin` on the code section; the source string of the fence is unchanged. A `%%` between widget and cell prevents `.rl-drive`.
- Batch HTML: `disabled` present, the note string present, no `widget_update` script. Markdown and LaTeX snapshots stay label = value.
- `run = "release"` emits `data-widget-run="release"`. Default slider does not. `run` on an option is a Caution callout.
- Drives: a bar read by the block that emitted `plot-2` links to `#plot-2`. A panel widget does not. A reader with no figure has no link.
- Existing `valid_prefix_widget_aware` tests stay as they are.
- The WS script still passes `node --check`. A string assert covers the release guard (`input` does not send when `data-widget-run="release"`) and the flash class name.

## Implementation phases

Each phase is shippable on its own. Docs (`docs/notebooks.md`, REPL help only if `widget` text changes, `CHANGELOG.md`) and a workspace version bump land with the phase that changes what a reader sees (rule 12). This design doc does not bump the version.

1. **Panel.** Adjacency in the HTML renderer, fold into the code section, CSS, comments carried onto that section, batch `disabled` + note. Markdown / LaTeX / PDF / JSON untouched. `widgets_demo.md` stays bars.
2. **Link and flash.** Plumb read names to `Rendered::Code`. Emit the drives link. Flash on the widget-update round trip only.
3. **`run = "release"`.** Parse, validate, `data-widget-run`, client send-on-change. Slider only.

## Open questions

1. **Downstream of a panel.** A slider in cell 1's panel also read by cell 4 flashes cell 4's plot and does not say "also drives Figure 4" on the panel. Is the flash enough, or do authors need that second link?
2. **Figure vs animation.** N is one counter for `plot-N` and `anim-N`, and the link always says "Figure". Call an animation out only if that reads wrong in practice.
3. **No-plot right column.** This doc uses a single full-width column when the cell has no figure. The alternative is a 16rem column beside the printed output. Prefer the single column unless a real notebook wants the split.
4. **Duplicate names.** Still last-wins, still a duplicate DOM id. A `check` warning could land later. Not required to place widgets.

## Mockup

`dev/plans/mockups/widget_placement/` is a static page, not a renderer. It reuses the notebook stylesheet (including the comments column) and the markdown SVG of `examples/notebooks/widgets_demo.md`. The "today" screenshot is a batch render of that notebook, not a copy of this folder.
