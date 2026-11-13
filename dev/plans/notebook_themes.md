# Notebook themes and website embedding

**Status:** planning (v2 of this plan). Shipped earlier under v1: F1 (named
Catppuccin builtins), F2 (luminance dark/light), F5 (`--rl-*` CSS custom
properties), the user half of F8 (`~/.rustlabrc` `[notebook] theme`), and
the font-role contrast pass (PR #57). Nothing in this revision is
implemented.
**Current phase:** Phase 1 (not started; waiting on Michael's answers to
the open questions at the end).
**Created:** 2026-09-13. **Revised:** 2026-11-13 against `main` at 0.3.65.
**Surfaces:** notebook HTML (`render`), `notebook watch`, LaTeX/PDF,
Plotly and SVG plots inside notebooks, and a new embeddable HTML output.
The committed `gallery/*.md` stays GitHub-styled; GitHub owns that CSS.

## Goal

Michael asked for two things:

1. **More, and more professional, themes.** Today there are four
   Catppuccin palettes. They are good for a dark editor and less good for
   a course handout, a lab report, or a docs site.
2. **Notebook output that can live inside an existing website** and read
   as part of that site. Today a rendered notebook is always a whole page
   with its own top bar, sidebar, fonts, and body styles.

This document is the design. It splits the work into PR-sized phases;
each phase is a separate PR with its own version bump (AGENTS.md Rule 12).

## Contents

- [What the code does today](#what-the-code-does-today)
- [Principles](#principles)
- [1. Theme model](#1-theme-model)
- [2. New built-in themes](#2-new-built-in-themes)
- [3. Website integration](#3-website-integration)
- [4. Plot theming](#4-plot-theming)
- [5. PDF parity](#5-pdf-parity)
- [6. Testing](#6-testing)
- [7. Security](#7-security)
- [8. Phases](#8-phases)
- [Open questions for Michael](#open-questions-for-michael)
- [Appendix A: v1 feature map](#appendix-a-v1-feature-map)
- [Appendix B: references](#appendix-b-references)

---

## What the code does today

A survey of `main` at 0.3.65. File paths are where to look before
changing anything.

### Palettes

- `crates/rustlab-plot/src/theme.rs` defines `ThemeColors`, a `Copy`
  struct of 33 `&'static str` colour roles: page (`bg`, `bg_secondary`,
  `text`, `text_dim`, `border`, `border_subtle`), accents (three), code
  panels (`code_bg`, `output_bg`, `inline_code_bg`), error, plot
  (`plot_bg`, `plot_grid`), six syntax roles, footer, comment marks, and
  eleven git-diff roles.
- There are four static palettes (`MOCHA`, `MACCHIATO`, `FRAPPE`,
  `LATTE`) and an enum `Theme` with `Dark` / `Light` aliases.
  `theme_colors(name)` and `parse_theme(name)` resolve names.
- **There is no theme file.** `~/.rustlabrc` `[notebook] theme` and
  `[plot] theme` only *select* one of the four names
  (`rustlab_config::ColorTheme`). The precedence is CLI `-t` > rc >
  `mocha`.
- Colour is the only thing a theme controls. Fonts, sizes, spacing,
  radii, the content width, and the table style are literals in the
  stylesheet.
- Contrast is enforced in unit tests. PR #57 nudged font roles that missed
  WCAG AA 4.5:1 on a surface they are painted on, and
  `every_builtin_font_meets_wcag_aa_on_its_surfaces` pins every role
  against every surface. Diff bars are checked at 3:1.

### HTML (`render` and `watch`)

- `crates/rustlab-notebook/src/render.rs::render_html_nonced` emits one
  inline `<style>` block (about 700 lines of CSS). `:root` gets the 33
  `--rl-*` tokens (`ThemeColors::css_custom_properties`), and the rules
  use `var(--rl-x, <literal>)` (`ThemeColors::css_var`), so a missing
  token keeps today's colour.
- The stylesheet is **page-global**. It starts with
  `* { margin: 0; padding: 0; box-sizing: border-box; }`, then styles
  `body`, `main`, `footer`, `[id]`, `nav.sidebar`, `.topbar`, and bare
  class names: `.prose`, `.output`, `.error`, `.source`, `.code-block`,
  `.callout*`, `.exercise`, `.solution`, `.plot-container`,
  `.image-grid`, `.page-nav`, `.missing-figure`. Newer pieces use an
  `rl-` prefix (`.rl-cell`, `.rl-src`, `.rl-table`, `.rl-widget`,
  `.rl-drive`, …). Dropped into another site, the unprefixed rules and
  the `*` reset would restyle that site, and its rules would restyle
  ours.
- Fonts are system stacks: a sans stack for the body
  (`-apple-system, … Arial, sans-serif`) and `"SF Mono", "Fira Code",
  "JetBrains Mono", monospace` for code. No web fonts are shipped except
  KaTeX's.
- Tables (`table.rl-table` and `.prose table`) use `color-mix()` of
  `text` into `bg`: 11% header, 6% zebra stripe, 30% rules.
- Callouts reuse the accent roles: note → `accent_secondary`, tip →
  `accent_tertiary`, warning → `error_text`, all on `bg_secondary`.
- Mode is `color-scheme: dark|light`, picked from background luminance
  (`ThemeColors::is_dark`). There is no `prefers-color-scheme` support: a
  page is one palette.
- `watch` serves the same HTML and adds chrome (`server/page.rs`,
  `server/cell.rs`, `server/ws.rs`, `comments.rs`). That chrome reads
  `--rl-*` tokens too. It has three hardcoded `rgba(0,0,0,…)` shadows.

### Assets and the "self-contained" rule

- The template references `/assets/plotly.min.js`,
  `/assets/katex/katex.min.css`, `katex.min.js`, and
  `contrib/auto-render.min.js`. The bytes are vendored into the binary
  (`server/assets.rs`, `include_bytes!`).
- `watch` serves them at `/assets/`. A static `render` writes them once
  into a sibling `assets/` directory and rewrites the URLs to be
  relative (`lib.rs::html_asset_root_for`, `asset_href_prefix`,
  `relativize_batch_assets`). A directory render shares one `{out}/assets/`.
- So "self-contained" in this repo means **no CDN and no network: the
  output folder works from `file://`**. It does not mean one file. PDF
  is the only truly single-file output. This plan keeps that meaning.

### Plots

- `crates/rustlab-plot/src/html.rs::render_figure_plotly_div` writes a
  `<div>` plus an **inline `<script>`** per figure. The layout bakes in
  hex literals from the theme: `paper_bgcolor` / `plot_bgcolor` =
  `plot_bg`, font and axis colour = `text`, `gridcolor` = `plot_grid`,
  default contour lines = `text`.
- Series colours do not follow the theme. `SeriesColor::cycle`
  (`figure.rs`) is a fixed matplotlib-like cycle (cyan, olive, green,
  purple, red, blue). Its RGB values are baked into committed gallery
  SVGs and the viewer wire protocol.
- SVG and PNG (`file.rs`) take `plot_bg`, `text`, and `plot_grid` from
  `ThemeColors`. Markdown renders (the gallery) pass the render theme,
  so gallery SVGs are Mocha. **Any change to a default plot colour
  rewrites every committed gallery SVG.**
- Mermaid diagrams (`mermaid.rs`) ignore the theme entirely.

### PDF

- `render_latex.rs` throws the requested theme away: printed output is
  always Latte ink on white paper (`let theme = Theme::Light.colors()`).
  A dark `\pagecolor` was removed because it made body text unreadable.
- Colours become `\definecolor` lines. Table header, stripe, and rule
  are `blend_srgb(text, #ffffff, 0.11 / 0.06 / 0.30)` with `\rowcolor`,
  matching the HTML mix on white. Body font is `lmodern`.

### The watch CSP (`server/auth.rs`)

```text
default-src 'self';
script-src 'self' 'nonce-…' 'strict-dynamic' 'unsafe-eval';
worker-src 'self' blob:;
style-src 'self' 'unsafe-inline';
img-src 'self' data: blob:;
font-src 'self';
connect-src 'self' ws://127.0.0.1:* ws://localhost:*;
object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'
```

0.3.65 added `'unsafe-eval'` and `worker-src 'self' blob:` for Plotly
WebGL (`scattergl` compiles its draw command with the `Function`
constructor). The nonce is stamped at render time on scripts the
renderer emits, including every per-figure Plotly script. Static
`render` HTML carries no CSP. `frame-ancestors 'none'` means a watch page
cannot be iframed, which is correct for a localhost dev server.

### Other outputs that matter here

- `--format json` (`render_json.rs`, schema version 1) already emits
  pre-rendered prose HTML, code blocks with inline SVG plots, and
  callouts. A site generator with components (Astro, Docusaurus) can
  build pages from it today.
- `--format markdown` writes GitHub-flavoured Markdown with SVG plots.
- `docs/lesson-site-pattern.md` describes a course repo: sources in
  `notebooks/`, rendered Markdown committed to `site/`, and an optional
  local HTML build. It has no story for a hosted website.

### Gaps, in one list

1. Theme = colours only; no typography, spacing, radii, or table style.
2. No theme files, so no brand or course themes (v1 F3/F4 never landed).
3. No light/dark pair that follows the reader's OS setting.
4. Global, partly unprefixed CSS; no fragment output; HTML ids like
   `plot-1` would collide when two notebooks share a host page.
5. Plot series colours and Mermaid ignore the theme; Plotly colours are
   frozen at render time.
6. Inline per-figure scripts would need `'unsafe-inline'` or a nonce on a
   host site with a strict CSP.
7. PDF ignores the theme by design, with no way for a light theme to
   carry into print.

---

## Principles

1. **Semantic tokens are the contract.** Renderers and stylesheets read
   roles (`text`, `accent-primary`, `table-stripe`), never palette
   swatches. Swatches exist only inside theme files.
2. **The existing 33 `--rl-*` names are stable.** New tokens are added;
   none are renamed. Outside CSS (host sites, user stylesheets) can rely
   on them.
3. **Built-ins keep their bytes until a phase says otherwise.** A refactor
   phase must render the four Catppuccin themes byte-identically (or with
   only additive `:root` lines), proven by golden files. That protects the
   downstream `make notebooks-check` drift guards.
4. **Readable on every surface it is painted on.** The contrast tests
   that exist for the four builtins extend to every theme, variant, and
   computed surface (table stripes, callout fills).
5. **Offline by default.** No CDN, no font download, no network at
   render or view time unless the author asks for it by flag.
6. **Embedding is opt-in and conservative.** The fragment output never
   styles anything outside its own root and needs no inline script.
7. **Pure Rust for the core, as always.** Nothing here is numerics, so
   Rule 10 does not bite, but the plan adds no new runtime crates.
   Browser-based screenshot tests are CI tooling, not shipped code.

---

## 1. Theme model

### 1.1 Recommendation: one resolved `Theme` with token groups

Replace "a theme is a `ThemeColors`" with a resolved theme that has
groups. `ThemeColors` stays as the colour group so existing call sites
change little.

```rust
// crates/rustlab-plot/src/theme.rs (sketch, not final)
pub struct ResolvedTheme {
    pub name: String,            // "mocha", "paper", "course-dark", …
    pub mode: Mode,              // Light | Dark (luminance unless declared)
    pub colors: ThemeColors,     // today's 33 roles (+ new ones below)
    pub typography: Typography,  // body / heading / mono stacks, size, line-height, measure
    pub shape: Shape,            // radius-sm / radius / radius-lg, border width
    pub space: Space,            // density: compact | normal | relaxed → spacing scale
    pub table: TableStyle,       // Zebra | Rules | Plain, plus the three mix amounts
    pub plot: PlotTokens,        // bg, paper-bg, grid, text, font, colorway
    pub print: Option<Box<ResolvedTheme>>, // PDF variant, see §5
}
```

`ThemeColors` fields become owned (`Cow<'static, str>`) so a theme file
can produce them; the four builtins stay `const`. The struct is no longer
`Copy`, so call sites take `&ThemeColors` (most already do). Shared
ownership across `watch` renders is an `Arc<ResolvedTheme>`.

**Alternative considered:** keep `&'static str` and `Box::leak` the
strings from theme files. That is the smallest diff, but `watch` reloads
a theme file on every save and would leak each time. Rejected.

**Alternative considered:** a free-form token map
(`HashMap<String, String>`). Maximally flexible, but renderers would look
roles up by string with no compiler help, and typos in theme files would
pass silently. Rejected; theme files are validated against the fixed
role set.

### 1.2 Token list

New CSS custom properties, all on the notebook root (see §3 for which
element that is). Values for the four Catppuccin themes equal today's
literals, so their pages do not change visually.

| Group | Tokens | Today's literal |
| --- | --- | --- |
| Typography | `--rl-font-body`, `--rl-font-heading`, `--rl-font-mono`, `--rl-font-size`, `--rl-line-height`, `--rl-measure` (content max width) | system sans stack, same, `"SF Mono"…`, UA default, per-rule, `960px` |
| Shape | `--rl-radius-sm`, `--rl-radius`, `--rl-radius-lg`, `--rl-border-width` | `3px` / `6px` / `8px`, `1px` |
| Space | `--rl-space-1` … `--rl-space-6` (density scale) | the rem values in the rules today |
| Tables | `--rl-table-head`, `--rl-table-stripe`, `--rl-table-rule` | `color-mix(text 11% / 6% / 30%, bg)` |
| Callouts | `--rl-callout-{note,tip,important,warning,caution}` (border/title) and `--rl-callout-bg` | accent roles, `error_text`, `bg_secondary` |
| Links / focus | `--rl-link`, `--rl-link-visited`, `--rl-focus-ring` | `accent_secondary`, same, UA default |
| Plot | `--rl-plot-bg`, `--rl-plot-paper`, `--rl-plot-grid`, `--rl-plot-text`, `--rl-plot-font`, `--rl-plot-1` … `--rl-plot-8` | `plot_bg`, same, `plot_grid`, `text`, UA default, `SeriesColor` RGB |
| Shadows | `--rl-shadow` | the three `rgba(0,0,0,…)` literals in watch chrome |

Tokens are **semantic only**. A theme's named palette (`brand = "#…"`)
is resolved away before output; it never appears in CSS.

Table style is an enum, not just colours: `zebra` (today), `rules`
(booktabs-like: heavier top and bottom rules, thin header rule, no
fill), and `plain`. HTML maps it to CSS; PDF maps it to `\rowcolor` or
`booktabs` (§5).

### 1.3 Theme files (TOML)

Picks up v1's F3 design: palette plus roles, `inherits`, partial
override. Additions are the non-colour groups and variants.

```toml
# course.toml
name = "course"
inherits = "paper"            # any builtin or discovered theme

[palette]                     # private swatches, referenced by name below
brand = "#0b5394"
ink   = "#1b1b1f"

[colors]                      # roles; a palette name or #RRGGBB
accent_primary   = "brand"
accent_secondary = "brand"
text             = "ink"

[typography]
body    = ["Charter", "Bitstream Charter", "Georgia", "serif"]
heading = ["Inter", "system-ui", "sans-serif"]
measure = "72ch"

[shape]
radius = "4px"

[table]
style = "rules"

[plot]
colorway = ["brand", "#d1495b", "#00798c", "#edae49", "#66a182", "#2e4057"]

# Optional dark sibling; unset roles inherit from the light set above.
[dark.colors]
bg   = "#14171c"
text = "#e6e6e6"
accent_primary = "#8ab4f8"
```

Rules:

- `inherits` chains are resolved depth-first; a cycle is an error naming
  the chain.
- Unknown keys are errors (`deny_unknown_fields`), with the file path
  and key. This matches how `rustlab-config` treats invalid values.
- Colour values: `#RRGGBB`, `#RRGGBBAA`, or a palette name. Font values:
  a list of family names (quoted on output) and generic families. Lengths:
  a number with `px`, `rem`, `em`, or `ch`. Nothing else is accepted
  (§7 explains why).
- Contrast is computed at load time for every role on every surface it is
  painted on, using the same surface table as the unit tests. Default is
  a warning per failing pair; `--strict` (and `rustlab-notebook check`)
  makes it an error. Builtins must pass with no warnings.
- `rustlab-notebook theme` subcommand: `theme list` (builtins and
  discovered files), `theme show NAME` (resolved tokens), `theme check
  FILE` (validation and a contrast table). This is the authoring loop.

### 1.4 Where themes come from, and precedence

Carried over from v1 (F4, F6, F8) unchanged except the project config
location, which is an open question.

1. `--theme NAME` / `--theme-file PATH` on the command line.
2. Notebook frontmatter: `theme: course` (a name, never a path, so a
   notebook cannot read arbitrary files).
3. Project config: recommended `.rustlab/config.toml` `[notebook] theme`
   (the `.rustlab/` directory already holds the function cache).
4. User rc: `~/.rustlabrc` `[notebook] theme` (shipped).
5. Default: `mocha`.

Name lookup: builtins first for reserved names (`mocha`, `latte`,
`paper`, …), then `./.rustlab/themes/NAME.toml`, then
`$XDG_CONFIG_HOME/rustlab/themes/NAME.toml`. A user file cannot shadow a
builtin; use a different name or `--theme-file`.

### 1.5 Light, dark, and `prefers-color-scheme`

A theme is either **single** (one colour set) or a **pair** (a light set
and a dark set). Pairs render as:

```css
.rl-notebook { color-scheme: light dark; /* light tokens */ }
@media (prefers-color-scheme: dark) {
  .rl-notebook:not([data-rl-scheme="light"]) { /* dark tokens */ }
}
.rl-notebook[data-rl-scheme="dark"] { /* dark tokens */ }
```

- `--scheme auto|light|dark` (default `auto` for a pair) pins one side.
  The `data-rl-scheme` attribute lets a host site's own dark-mode toggle
  drive the notebook (§3.5).
- Only the colour group varies between sides. Fonts and spacing do not.
- The four Catppuccin themes stay single by default. A new pair name,
  `catppuccin`, is Latte in light and Mocha in dark.
- SVG plots in Markdown output cannot switch, so they use the light side
  of a pair (or the side `--scheme` names). Plotly plots switch (§4).

---

## 2. New built-in themes

Four additions, each chosen for a context Catppuccin does not cover.
All use system font stacks only (no font files shipped; see open
question 2). Exact hex values are chosen during Phase 3 and must pass the
contrast tests before merge; the descriptions below are the brief.

| Name | Kind | For | Look |
| --- | --- | --- | --- |
| `clean` | light/dark pair | General default candidate; blog posts, internal reports | Neutral grays (white / near-black page, `#1f2328`-ish ink), one blue accent, system sans, zebra tables, 6px radii. Low-chroma syntax colours. |
| `paper` | light, with a print variant | Course handouts, lab reports, anything also printed | Off-white page, serif body (`Charter, "Bitstream Charter", "Iowan Old Style", Georgia, serif`), sans headings, 68–72ch measure, `rules` tables, deep-blue accent, small radii, no card shadows. Its PDF variant is the same design on white paper (§5). |
| `docs` | light/dark pair | Notebooks published inside a documentation site | Compact spacing, system sans, indigo accent, card-style callouts with a left rule, rounded code panels. Designed to sit beside MkDocs Material, Docusaurus, or Starlight without looking foreign. |
| `contrast` | light/dark pair | Accessibility, projectors, low vision | Body text at least 7:1 (WCAG AAA), every accent at least 7:1 on the page, links always underlined, visible 3px focus ring, table rules instead of faint stripes, nothing signalled by colour alone (diff bars keep their `+`/`−` gutter glyph). |

Why these four: they cover the three places notebooks go that Catppuccin
serves poorly (print, docs sites, plain professional pages) plus an
accessibility theme, which is also a test of the token model: if
`contrast` cannot be expressed as a theme file, the model is missing a
token.

**Not proposed:** many brand palettes (Solarized, Nord, GitHub, …). They
are easy to add later as theme files under `docs/themes/` once Phase 2
lands, without growing the binary or the test matrix.

**Default theme:** recommendation is to keep `mocha` as the default.
Switching to `clean` is a visible change to every downstream render
(open question 1).

---

## 3. Website integration

### 3.1 Two ways to put a notebook in a site

| | Inline fragment (recommended default) | `<iframe>` of a notebook page |
| --- | --- | --- |
| Looks like the host | Yes: can inherit host fonts and colours | No: own document; theming needs a query parameter |
| Style isolation | Our CSS is scoped; host CSS can still leak in (we harden against common cases) | Complete |
| Host CSP | Host must allow our three scripts (§7) | Host only needs `frame-src` |
| Height | Natural document flow | Fixed, or a resize script with `postMessage` |
| Search engines, find-in-page, print | Content is part of the page | Separate document |
| Untrusted notebook content | Runs in the host origin, so our sanitiser is the only guard | Can be `sandbox`ed |
| Works today | No | Yes: `render --no-toc` gives a page without the sidebar |

Recommendation: build the inline fragment (Phases 5–7). Document the
iframe path now as the fallback for untrusted notebooks, with
`sandbox="allow-scripts"` and no `allow-same-origin`. An optional resize
helper for iframes is a later item (open question 11).

### 3.2 The fragment output

New flag on the HTML format: `rustlab-notebook render nb.md --embed`
(works for one file and for a directory). Output per notebook:

```html
<div class="rl-notebook" id="rl-nb-intro" data-rl-theme="paper" data-rl-scheme="auto">
  <!-- the notebook body: same blocks as today's <main>, minus page chrome -->
  <section class="rl-block" id="rl-nb-intro-b-1a2b">…</section>
  …
  <div class="rl-plot" id="rl-nb-intro-plot-1"></div>
  <script type="application/json" class="rl-plot-data" data-for="rl-nb-intro-plot-1">{…}</script>
</div>
```

- No `<html>`, `<head>`, `<body>`, top bar, file browser, sidebar, page
  nav, or "Generated by" footer. A `--embed-toc` flag emits the heading
  list as a separate `<nav class="rl-toc">` for hosts that want it.
- No `<style>` and no executable inline `<script>`. Styles live in
  `assets/rustlab-notebook.css`. One external `assets/rustlab-notebook.js`
  finds every `.rl-notebook` on the page, runs KaTeX on each root (not
  `document.body`), and draws each plot from its JSON block. That makes
  the fragment work under a host CSP with no `'unsafe-inline'` for
  scripts.
- **Ids are prefixed** with `rl-nb-<slug>-` (overridable with
  `--id-prefix`), so two notebooks on one page, or a notebook and the
  host, cannot collide. In-notebook anchor links are rewritten to match.
- `--heading-offset N` demotes headings (the transclusion code already
  does this), because the host page normally owns the `<h1>`.
  `--strip-title` drops the notebook's first `# H1`.
- A sidecar `<stem>.meta.json` holds title, frontmatter, the heading list
  with ids, figure count, and the theme, so a site generator can build its
  own navigation without parsing HTML. Alternative: tell generators to use
  `--format json` instead. Both are cheap; the sidecar is simpler to
  consume next to a fragment.

### 3.3 Scoping the CSS

Today's stylesheet becomes two layers that share one source:

1. **Notebook CSS** (`rustlab-notebook.css`): everything that styles
   notebook content. Every selector is under `:where(.rl-notebook)`.
   `:where()` has zero specificity, so a host rule like
   `.content table { … }` still wins if the host wants it to, while our
   rules never reach outside the root.
2. **Page CSS** (standalone `render` and `watch` only): `html`, `body`,
   top bar, sidebar, file browser, footer, and the box-sizing reset.

Supporting changes:

- Rename the unprefixed classes to `rl-*`: `.prose` → `.rl-prose`,
  `.output` → `.rl-output`, `.error` → `.rl-error`, `.callout*` →
  `.rl-callout*`, and so on, including the watch client JavaScript and
  gitdiff selectors that read them. This is the one breaking part
  (open question 4). Standalone pages look the same afterwards.
- Replace the global `*` reset with a reset scoped to our own components
  (`:where(.rl-notebook) :where(.rl-cell, .rl-output, …)`), so embedded
  prose keeps the host's paragraph and list spacing.
- Wrap notebook CSS in `@layer rustlab`. Unlayered host styles then beat
  ours by default, which is what "looks like part of the site" needs. A
  host that wants the reverse declares its layer order.
- Harden against the common hostile host rules (`table { width: 100% }`,
  `pre { white-space: pre }`, `img { max-width: 100% }`, `h2::before`)
  by setting those properties explicitly on our own elements, and test
  against a deliberately aggressive host stylesheet (§6).

**Alternative considered: Shadow DOM** (a `<rl-notebook>` custom element
with an attached shadow root). Full two-way isolation, but host fonts and
dark mode stop flowing in except through custom properties, KaTeX's
auto-render and Plotly's style injection both assume the light DOM, and
content inside a shadow root is invisible to the host's own CSS and some
search tooling. It fights the "looks like part of the site" goal.
Rejected for the default; could be a later `--embed=isolated` mode.

**Alternative considered: CSS `@scope`.** It gives the same result as the
`:where()` prefix with nicer syntax, and is now in all current
browsers, but older Safari and Firefox ESR builds in school labs would
render unstyled notebooks. The prefix works everywhere. Revisit later.

### 3.4 Assets

The offline rule stays the default. Embed output writes
`assets/` like a normal render, now with two more files:

```text
out/
├── intro.html                 # fragment
├── intro.meta.json
├── plots/intro/…              # GIF animations, prose images (as today)
└── assets/
    ├── rustlab-notebook.css
    ├── rustlab-notebook.js
    ├── plotly.min.js
    └── katex/…                # css, js, fonts
```

Flags for site builds:

- `--asset-base URL` sets the URL prefix the host serves `assets/` from
  (for example `/static/rustlab/`), so fragments copied into templates
  still resolve. Accepted: a relative path or an `https:` URL; anything
  else is refused. When it is an absolute URL, `<script>` and `<link>`
  get `integrity` hashes computed from the vendored bytes (SRI), since we
  know exactly what we shipped.
- `--no-assets` skips writing `assets/` (the host already provides them).
- `--no-katex` / `--no-plotly` skip the library and its tags when the host
  page already loads that library. `rustlab-notebook.js` checks
  `window.Plotly` and `window.katex` and logs a console warning on a
  version mismatch.

A CDN is never the default. Pointing `--asset-base` at a CDN is the
author's explicit choice (open question 12).

### 3.5 Inherit mode: mapping to the host's variables

A theme role may be a reference to a host CSS variable instead of a
fixed colour. This is how a notebook adopts the site's palette and fonts:

```toml
# docs/themes/hosts/mkdocs-material.toml
name = "host-mkdocs-material"
inherits = "docs"

[colors]
bg             = "transparent"
text           = "var(--md-typeset-color, #1f2328)"
text_dim       = "var(--md-default-fg-color--light, #57606a)"
accent_primary = "var(--md-primary-fg-color, #3f51b5)"
accent_secondary = "var(--md-accent-fg-color, #526cfe)"
code_bg        = "var(--md-code-bg-color, #f5f5f5)"

[typography]
body = "inherit"
mono = "var(--md-code-font-family, monospace)"
```

- Allowed reference forms: `var(--name)` or `var(--name, <literal>)`,
  plus the keywords `inherit`, `currentColor`, and `transparent`.
  Anything else is rejected (§7).
- Contrast cannot be checked at render time for a reference.
  `theme check` reports those roles as "unchecked: depends on the host"
  and checks the fallback literal instead. The Playwright test (§6)
  checks real values against fixture host pages.
- Proposed presets, shipped as example files under `docs/themes/hosts/`
  rather than as builtins: Material for MkDocs (`--md-*`), Docusaurus
  (`--ifm-*`), Astro Starlight (`--sl-color-*`), and Bootstrap 5
  (`--bs-*`). Which hosts matter is open question 9.
- Host dark mode: if the host toggles a class or attribute on `<html>`,
  the preset sets `data-rl-scheme` by a selector mapping in
  `rustlab-notebook.js` (for example, Material's
  `[data-md-color-scheme="slate"]` means dark). Hosts whose variables
  already flip in dark mode need nothing: the references follow.

### 3.6 How a site generator consumes it

All recipes assume `rustlab-notebook render notebooks/ --embed
--asset-base /rustlab/ -o build/rustlab` and that the build copies
`build/rustlab/assets/` to the site's static root. Each will be a short
section in a new `docs/embedding.md` and a section in
`docs/lesson-site-pattern.md`.

- **Hugo:** copy assets to `static/rustlab/`. A shortcode
  `{{< notebook "intro" >}}` does `{{ readFile (printf
  "build/rustlab/%s.html" .Get 0) | safeHTML }}`. CSS and JS go in the
  base template once.
- **Jekyll:** fragments into `_includes/notebooks/`, assets into
  `assets/rustlab/`; `{% include notebooks/intro.html %}` in a page.
- **Astro:** `import nb from '../build/rustlab/intro.html?raw'` and
  `<Fragment set:html={nb} />`. Astro's scoped styles do not apply to
  `set:html` content, which is fine: our CSS is already scoped. Sites
  that want components per block can read `--format json` instead.
- **MkDocs / Docusaurus:** the snippets extension (`--8<--`) or an MDX
  component that injects the fragment; or the zero-JavaScript path below.
- **Zero-JavaScript path:** `--format markdown` output (SVG plots, no
  Plotly) dropped into any Markdown-based generator, plus
  `rustlab-notebook.css` for tables and callouts. No interactivity, no
  CSP questions. Worth documenting because it already almost works.

---

## 4. Plot theming

### 4.1 What changes

1. **Colour roles, not literals, in the figure JSON.** In themed HTML
   output, a figure's layout carries structure (axes, domains, titles,
   ranges) but no theme colours. Colours the user picked (`plot(x, y,
   "r")`) stay literal.
2. **A runtime Plotly template.** `rustlab-notebook.js` builds one
   template per notebook root from its CSS variables: `paper_bgcolor`,
   `plot_bgcolor`, font colour and family, `gridcolor`, axis colour, and
   `colorway` from `--rl-plot-1…8`. Each figure is drawn with
   `Plotly.newPlot(div, data, {template, ...layout})`.
3. **Resolving variables to real colours.** `getPropertyValue('--rl-x')`
   returns the token text, not a colour, and Plotly does not understand
   `var()` or `color-mix()`. The script sets the variable on a hidden
   probe element (`probe.style.color = 'var(--rl-plot-text)'`) and reads
   back `getComputedStyle(probe).color`, which is always `rgb(…)`. This
   is what makes inherit mode work for plots.
4. **Live switching.** A `matchMedia('(prefers-color-scheme: dark)')`
   listener and a `MutationObserver` on `data-rl-scheme` rebuild the
   template and call `Plotly.relayout` on each figure in that root.
5. **One hydration path everywhere.** Static `render` and `watch` also
   switch from inline per-figure scripts to the JSON block plus the
   shared script. Watch no longer needs to stamp a nonce on figure code;
   only `rustlab-notebook.js` is loaded, from `'self'`.

### 4.2 Series colours and parity

- `SeriesColor::cycle` stays an index. The colour for index *k* comes from
  the theme's colorway at render time. **The default colorway equals
  today's RGB values**, so the four Catppuccin themes and every committed
  gallery SVG stay byte-identical. New themes bring their own colorway.
- SVG and PNG (`file.rs`) take the same colorway from the resolved theme,
  so notebook HTML, Markdown, and PDF agree (Rule 9).
- The terminal (ANSI named colours) and the live viewer keep today's
  cycle. They are not notebook output and have their own colour models;
  the code comment and `docs/notebooks.md` will say so explicitly, as
  Rule 9 asks.
- The default contour line colour moves from `text` to `--rl-plot-text`.
- Colormaps for heatmaps and filled contours (viridis and friends) do not
  follow the theme: they are perceptual scales, and changing them changes
  how data reads.
- Static GIF and HTML animations (`saveanim`) are scripts' own output,
  not notebook pages; they keep using the process default theme.
- **Mermaid:** map a small set of roles (node fill, stroke, text, edge)
  into the renderer's config if `mermaid-rs-renderer` exposes one; check
  in Phase 4 and otherwise record it as a known gap (open question 10).

### 4.3 Alternatives considered

- **Keep baking literals, re-render on scheme change.** Simple, but a
  static file cannot re-render, and inherit mode has no literals to bake.
- **CSS-only theming of the SVG Plotly draws.** Plotly sets most colours
  as inline attributes, so CSS would need `!important` overrides on
  internal class names that change between Plotly versions. Fragile.

---

## 5. PDF parity

Constraint that stays: **PDF is always light ink on white paper.** A dark
page wastes toner and was unreadable when tried.

Recommendation:

- A theme may declare a `[print]` section (colours, body font, table
  style). If it does not:
  - a light theme, or the light side of a pair, is its own print
    variant, with surfaces re-checked against `#ffffff`;
  - a dark-only theme (Mocha, Macchiato, Frappé) falls back to Latte,
    which is exactly today's behaviour, so existing PDFs do not change.
- **Fonts.** pdflatex cannot use arbitrary system fonts, so print fonts
  are a closed list mapped to TeX Live packages:
  `lmodern` (today), `charter` (XCharter), `palatino` (newpxtext),
  `times` (newtxtext), `sans` (Helvetica via helvet). `paper` uses
  `charter`. tectonic supports the same packages. The list is checked
  when the theme loads; unknown names are errors.
- **Tables.** `zebra` stays `\rowcolor` with `blend_srgb` on white;
  `rules` becomes `booktabs` (`\toprule`, `\midrule`, `\bottomrule`, no
  fill); `plain` is rules only. Contrast tests cover body text on the
  head and stripe colours for every theme's print variant.
- **Accents.** Heading and link colours must reach 4.5:1 on white in the
  print variant. Builtins are tuned to pass; a theme file that fails gets
  the usual warning, or an error under `--strict`.
- **Plots in PDF** use the print variant's plot tokens and colorway, so
  a `paper` PDF and a `paper` web page show the same series colours.
- `--pdf-theme NAME` lets an author keep a different print look than the
  screen look (for example screen `docs`, print `paper`).

---

## 6. Testing

Every phase is test-first: write the failing test that describes the new
behaviour, then the code.

| Layer | What | Where |
| --- | --- | --- |
| Golden files | Full HTML, LaTeX, and Markdown output for a fixture set (`quick_look.md`, a table, a callout of each kind, a failing cell, a plot, math) under each builtin theme. Phase 1 refactors must leave the four Catppuccin goldens unchanged except for additive `:root` lines. Regenerated with `UPDATE_GOLDEN=1`; reviewed in the PR diff. | `crates/rustlab-notebook/tests/golden/` |
| Contrast matrix | Generalise `every_builtin_font_meets_wcag_aa_on_its_surfaces` to iterate over every registered theme, both sides of a pair, and the print variant. Add computed surfaces: table head and stripe (the same mix the CSS uses, computed with `blend_srgb`), callout fill, link on page, focus ring at 3:1, plot text on plot background, each colorway entry at 3:1 on the plot background (WCAG 1.4.11, graphical objects). `contrast` asserts 7:1 for text and accents. | `rustlab-plot/src/theme.rs` tests |
| Theme files | Parse and resolve: inherit chains, cycle errors, unknown keys, bad hex, unknown palette names, unknown print font, pair resolution, precedence (CLI > frontmatter > project > rc > default), and the value grammar rejecting injection attempts (`red;}`, `</style>`, `url(…)`, `@import`, `expression(`). | `rustlab-plot` and `rustlab-notebook` unit tests |
| Embed structure | Fragment has no `<html>`, `<head>`, `<body>`, or `<style>`; no `<script>` except `type="application/json"`; every id carries the prefix; two notebooks rendered into one host fixture have disjoint ids; plot JSON escapes `<`, `>`, `&`, U+2028, U+2029. | `crates/rustlab-notebook/tests/embed.rs` |
| CSS scope lint | Parse the emitted `rustlab-notebook.css` (we generate it, so a small rule splitter is enough) and assert every selector starts with `:where(.rl-notebook)` and that no rule targets `html`, `body`, or `*` at top level. | same |
| Plot JSON | Themed layouts contain no theme hex literals; explicit user colours survive; default colorway equals `SeriesColor::to_rgb` for the Catppuccin themes; SVG and Plotly agree on the colorway (cross-backend test, Rule 9). | `rustlab-plot` tests |
| PDF | Preamble golden per print variant (`\definecolor` lines, font package, `booktabs` vs `\rowcolor`); compile each builtin's print variant in `rustlab-notebook validate -f pdf`. | `render_latex.rs` tests, `make validate-notebooks` |
| Screenshots | Playwright with headless Chromium: each builtin theme × {standalone page, fragment inside three fixture host pages (plain, MkDocs-like, deliberately hostile CSS)} × {light, dark} × {1280px, 390px}. Pixel diff against committed baselines with a small tolerance. A scheme-switch test reads `gd._fullLayout.paper_bgcolor` before and after toggling `data-rl-scheme`. axe-core runs on each page, which catches contrast on real rendered surfaces, including inherit mode against the fixture hosts. | `dev/visual/` harness (pattern from `dev/plans/assets/notebook_git_diff_ux/mockup.mjs`), separate CI workflow |

The screenshot suite needs Node and a browser, so it runs in its own
GitHub workflow on PRs that touch notebook rendering, not in
`cargo test --workspace`. Baselines are committed PNGs, regenerated
deliberately. Open question 7 asks whether that tooling is acceptable.

---

## 7. Security

### 7.1 Theme files are a CSS injection surface

Theme values end up inside a `<style>` block or a `.css` file. A value
like `red; } body { display: none } .x {`, or `</style><script>`, would
break out. Defence is a closed grammar, not escaping:

- colours: `#RGB`, `#RRGGBB`, `#RRGGBBAA`, a palette name;
- references: `var(--[a-z0-9-]+)` with an optional fallback that itself
  passes the grammar; keywords `inherit`, `currentColor`, `transparent`;
- fonts: family names matching `[A-Za-z0-9 _-]+`, emitted quoted, plus
  generic families;
- lengths: a number and one of `px`, `rem`, `em`, `ch`, `%`.

No `url()`, so a theme cannot fetch remote resources or leak a page view
through a background image. No `@import`. Anything else is a load error
naming the file and key. Frontmatter can only name a theme, never give a
path, so a notebook cannot make the renderer read an arbitrary file;
theme files are read from the discovery directories or the CLI path.

### 7.2 `notebook watch`

- The theme picker (Phase 9) changes `data-rl-scheme` and swaps
  token blocks. That needs no CSP change.
- Moving figures to JSON blocks plus `rustlab-notebook.js` *removes*
  inline figure scripts, so the only scripts left are the nonced chrome
  scripts and `'self'` assets. `'unsafe-eval'` and `worker-src blob:`
  stay because Plotly WebGL still needs them (0.3.65).
- `font-src 'self'` already covers KaTeX fonts. If shipped theme fonts are
  ever added (open question 2), they are served from `/assets/`, never
  from a theme directory.
- `frame-ancestors 'none'` stays: a watch page must not be framed.

### 7.3 Embedded output on someone else's site

The host owns the CSP. `docs/embedding.md` will give the minimum policy:

```text
script-src  'self';                 # rustlab-notebook.js, plotly, katex (or the --asset-base origin)
style-src   'self' 'unsafe-inline'; # KaTeX writes style attributes; Plotly injects a <style> element
font-src    'self';                 # KaTeX fonts
img-src     'self' data: blob:;     # plot images, Plotly exports
worker-src  'self' blob:;           # only if a figure uses WebGL (scattergl, surface)
# and 'unsafe-eval' in script-src only if a figure uses WebGL
```

`rustlab-notebook.js` will detect a CSP-blocked WebGL figure and print
which directive is missing in the console, instead of Plotly's generic
"WebGL is not supported" overlay.

### 7.4 Trust

An inline fragment puts notebook content in the host's origin. Prose is
already sanitised (attribute-free allow-list, dangerous URLs refused),
and that sanitiser becomes the only barrier between a notebook author and
the host site's cookies. The docs will say plainly: inline embedding is
for notebooks you trust as much as your own site content; for anything
else use a sandboxed iframe. The plot JSON blocks are escaped so data
strings cannot close the `<script>` element.

---

## 8. Phases

Each phase is one PR on its own `feature/` branch, with docs, REPL or CLI
help, `CHANGELOG.md`, and an `AGENTS.md` update in the same PR. Version
numbers assume no other release lands in between; each phase takes the
next patch number at the time it merges.

| Phase | Scope | Visible change | Version |
| --- | --- | --- | --- |
| 0 | This plan | none | none |
| 1 | Token model refactor: `ResolvedTheme`, owned `ThemeColors`, new token groups with values equal to today's literals, stylesheet reads them, golden-file harness. | Additive `:root` lines only | 0.3.66 |
| 2 | Theme files: TOML schema, `inherits`, palette, value grammar, contrast warnings, `--theme-file`, `--strict`, `theme list/show/check`, discovery directories, frontmatter `theme:`, project config. (v1 F3, F4, F6, F8 project half.) | New flags and keys | 0.3.67 |
| 3 | Light/dark pairs, `--scheme`, `data-rl-scheme`, then the four new builtins (`clean`, `paper`, `docs`, `contrast`) and `catppuccin` pair. Screenshot and axe harness lands here, with baselines. | New themes | 0.3.68 |
| 4 | Plot theming: colorway token, JSON figure blocks plus `rustlab-notebook.js` for static and watch, runtime template with probe resolution, scheme-switch relayout, SVG colorway parity, Mermaid check. | New themes' plots change colour; Catppuccin unchanged | 0.3.69 |
| 5 | CSS split and scoping: notebook vs page CSS, `:where(.rl-notebook)`, `@layer rustlab`, `rl-` class renames, scoped reset, hostile-host hardening. | Standalone looks the same; class names change (breaking for custom CSS) | 0.3.70 |
| 6 | `--embed` output: fragment, id prefixing, `--heading-offset`, `--strip-title`, `--embed-toc`, `.meta.json`, `--asset-base` with SRI, `--no-assets` / `--no-katex` / `--no-plotly`, `docs/embedding.md` with Hugo / Jekyll / Astro / MkDocs recipes and the CSP snippet, lesson-site-pattern update. | New output mode | 0.3.71 |
| 7 | Inherit mode: `var()` references in theme files, host presets in `docs/themes/hosts/`, host dark-mode mapping, fixture-host screenshot tests. | New theme capability | 0.3.72 |
| 8 | PDF parity: `[print]` sections, print font list, `rules` and `plain` tables via `booktabs`, `--pdf-theme`, print-variant contrast tests. | `paper` and light themes now theme the PDF; dark themes keep Latte | 0.3.73 |
| 9 | Watch theme picker (v1 F7): menu of builtins and discovered themes, scheme toggle, optional `theme:` write to frontmatter under `--editable`. | New watch UI | 0.3.74 |

Ordering notes:

- Phase 1 first because everything else reads tokens.
- Phase 4 (plot hydration) comes before Phase 6 (embed) because inline
  figure scripts are what would force a host to allow `'unsafe-inline'`.
- Phase 5 before 6 because the fragment is unusable until the CSS is
  scoped.
- Phases 2, 3, and 4 are independent of 5–7 after Phase 1; if website
  embedding is more urgent than new themes, the order can be 1 → 4 → 5 →
  6 → 7 → 2 → 3 → 8 → 9 with no rework.
- Phase 8 needs Phase 2 (print sections live in theme files) and is most
  useful after Phase 3 (`paper`).

## Progress checklist

Check a box when that phase's PR is merged. Keep the **Current phase**
line at the top in sync.

- [ ] Phase 1: token model refactor (status: not started)
- [ ] Phase 2: theme files (status: not started)
- [ ] Phase 3: pairs and new builtins (status: not started)
- [ ] Phase 4: plot theming (status: not started)
- [ ] Phase 5: scoped CSS and class prefix (status: not started)
- [ ] Phase 6: embed output (status: not started)
- [ ] Phase 7: inherit mode (status: not started)
- [ ] Phase 8: PDF parity (status: not started)
- [ ] Phase 9: watch theme picker (status: not started)

---

## Open questions for Michael

1. **Default theme.** Keep `mocha` (recommended; no downstream render
   changes) or switch the default to `clean` (auto light/dark)?
2. **Fonts.** System font stacks only (recommended for v1: zero bytes,
   no licence notice), or vendor one OFL serif and one sans (for example
   Source Serif 4 and Inter, roughly 0.3–1 MB in `assets/` and the
   binary) so `paper` and `docs` look the same on every machine?
3. **Embed flag shape.** `render --embed` on the HTML format
   (recommended), or a separate `--format html-fragment`?
4. **Breaking class renames.** Is it acceptable to rename the unprefixed
   classes (`.prose`, `.output`, `.callout`, …) to `rl-*` in one release,
   with a CHANGELOG migration note? The alternative is emitting both names
   for a release, which doubles some selectors.
5. **Catppuccin colorway.** Keep the matplotlib-like series cycle for the
   four Catppuccin themes (recommended; no gallery SVG churn), or give
   them Catppuccin series colours and regenerate the gallery once?
6. **PDF.** Is "light themes theme the PDF, dark themes keep Latte" the
   right rule? Is the five-font print list enough?
7. **Test tooling.** Hand-rolled golden files (recommended, no new
   dependency) or the `insta` crate? Is a Node + Playwright workflow in CI
   acceptable for screenshots and axe checks?
8. **Project config location.** `.rustlab/config.toml` (recommended,
   next to the cache) or a top-level `rustlab.toml`?
9. **Which host sites matter?** What does the `rustlab_em` site (or the
   site you have in mind) use: Hugo, Jekyll, Astro, MkDocs Material,
   Docusaurus, or something hand-built? That decides which inherit
   presets and recipes come first.
10. **Mermaid theming.** In scope for Phase 4, or leave diagrams with the
    renderer's default colours?
11. **iframe resize helper.** Worth shipping a small `postMessage` script
    so iframed notebooks size to their content, or document fixed heights
    only?
12. **CDN via `--asset-base`.** Allow an `https:` CDN prefix (with SRI
    hashes), or restrict `--asset-base` to the host's own paths?

---

## Appendix A: v1 feature map

The v1 plan listed F1–F8. Where each one went:

| v1 | Status | In v2 |
| --- | --- | --- |
| F1 named Catppuccin builtins | shipped | unchanged |
| F2 luminance dark/light | shipped | unchanged; also decides `mode` for theme files |
| F5 `--rl-*` custom properties | shipped | the 33 names are now a stable contract (§1.2) |
| F8 user default (`~/.rustlabrc`) | shipped | unchanged |
| F3 theme files | not started | Phase 2, extended with typography, shape, table, plot, pairs |
| F4 discovery directories | not started | Phase 2 |
| F6 frontmatter `theme:` | not started | Phase 2 |
| F8 project default | not started | Phase 2 (location is open question 8) |
| F7 watch picker | not started | Phase 9 |

v1 decisions that still hold: palette is separate from roles; `inherits`
with partial override; builtins win name collisions; contrast warns by
default and errors under `--strict` / `check`; no environment variable
for the theme.

## Appendix B: references

- Quarto `brand.yml`: palette plus semantic colours, light and dark
  variants, typography. https://quarto.org/docs/authoring/brand.html
- Catppuccin palette. https://github.com/catppuccin/palette
- Material for MkDocs colour and font variables (`--md-*`).
- Infima / Docusaurus variables (`--ifm-*`); Astro Starlight (`--sl-*`).
- WCAG 2.2: 1.4.3 (text, 4.5:1), 1.4.6 (enhanced, 7:1), 1.4.11 (non-text,
  3:1).
- MDN: `:where()`, `@layer`, `prefers-color-scheme`, `color-scheme`,
  Subresource Integrity.
- Plotly `layout.template` and `colorway`.
- In-repo: `crates/rustlab-plot/src/theme.rs`, `crates/rustlab-plot/src/html.rs`,
  `crates/rustlab-notebook/src/render.rs`, `render_latex.rs`,
  `server/auth.rs`, `server/assets.rs`, `lib.rs` (asset layout),
  `docs/notebooks.md` § Color themes, `docs/lesson-site-pattern.md`,
  `docs/security.md`.
