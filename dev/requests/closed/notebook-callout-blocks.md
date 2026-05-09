# Feature Request: Callout/admonition blocks

> **STATUS: RESOLVED.** Implemented via `dev/plans/notebook_obsidian_alignment.md`
> Phase A. The renderer accepts both the legacy `<!-- note -->` form and
> the GitHub/Obsidian-native `> [!NOTE]` (and TIP, IMPORTANT, WARNING,
> CAUTION) syntax. See `docs/notebooks.md` § "Callouts: `> [!NOTE]`".

## Problem

Tutorial notebooks need a way to visually distinguish pedagogical annotations — definitions, tips, warnings, and cross-references — from the main narrative. Currently all prose renders identically, making it easy for readers to miss important caveats or key insights buried in a paragraph.

## Proposed directives

`<!-- note -->`, `<!-- tip -->`, and `<!-- warning -->` directives that style the following paragraph (up to the next blank line or heading) as a visually distinct aside box.

### Usage

```markdown
<!-- note -->
Do not confuse neutral Ba (553.5 nm green) with Ba$^+$ (493 nm blue).
The ion is used in trapped-ion quantum computers; neutral Ba is studied here.

<!-- tip -->
The pattern $n - l - 1$ gives the number of radial nodes for any orbital.
Count the zero crossings in your plot to verify.

<!-- warning -->
The rotating wave approximation drops terms oscillating at $2\omega_L$.
This fails when $\Omega$ approaches $\omega_0$ (ultrastrong coupling regime).
```

### Rendered output

Each callout renders as a bordered box with a colored left border or background tint and an icon/label:

- **Note** (blue): factual clarifications, definitions, cross-references
- **Tip** (green): practical advice, shortcuts, verification strategies
- **Warning** (amber/red): common mistakes, approximation limits, edge cases

## Output format mapping

| Format | Rendering |
|---|---|
| HTML | `<div class="callout callout-note">` with CSS for border, background, and icon. Adapts to dark/light theme. |
| LaTeX | `\begin{tcolorbox}[colback=blue!5, colframe=blue!50, title=Note]` or similar. Falls back to `\begin{quote}` with a bold label if tcolorbox is unavailable. |
| PDF | Same as LaTeX |

## Scope rules

The callout captures all content from the directive to the next blank line (paragraph break) or heading. Multi-paragraph callouts could use a closing `<!-- /note -->` tag, but single-paragraph is the common case and doesn't need an explicit close.

Multi-paragraph form (optional, lower priority):
```markdown
<!-- note -->
First paragraph of the note.

Still part of the note.
<!-- /note -->
```

## Motivation from quantum_lab

| Lesson | Content | Type |
|---|---|---|
| 01 | Ba vs Ba$^+$ clarification | note |
| 01 | Radial node counting pattern | tip |
| 02 | HXH = Z identity as Bloch sphere swap | note |
| 03 | RWA validity condition | warning |
| 03 | Fluorescence readout explanation | note |
| 04 | Correspondence principle at high n | tip |
| 05 | Perturbative formula accuracy limit | warning |
| 06 | Tsirelson bound is quantum maximum | note |
