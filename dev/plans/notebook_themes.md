# Notebook color themes / schemes

**Status:** F2 + F1 implemented on `feature/notebook-themes-f2` — remaining features not started.  
**Created:** 2026-09-13  
**Surfaces:** HTML, LaTeX/PDF, `watch` server (not committed `gallery/*.md` — GitHub owns that CSS).


## Progress checklist

Track feature completion here. Check the box when that feature’s PR is merged (or the sub-item is done). Keep each feature’s **Status:** line in sync (`not started` / `in progress` / `complete`).

### Overall
- [x] F2 — Luminance-based dark/light detection
- [x] F1 — Named Catppuccin builtins + string CLI
- [ ] F5 — HTML CSS custom properties
- [ ] F3 — Theme files (`inherits` + palette + roles)
- [ ] F4 — Theme discovery directories
- [ ] F6 — Frontmatter `theme:`
- [ ] F8 — Project / user default theme config
- [ ] F7 — Watch-server theme picker

### F2 — Luminance-based dark/light detection
- [x] Shared luminance / `is_dark_theme` helper (reuse HTML logic)
- [x] LaTeX: remove pointer-equality dark check
- [x] Audit notebook + plot for other Dark-only assumptions
- [x] Tests: mocha/macchiato/frappe → dark; latte → light; custom dark bg
- [x] Docs / CHANGELOG if behavior note needed

### F1 — Named Catppuccin builtins + string CLI
- [x] Scheme registry: name → `ThemeColors`
- [x] Builtins: `mocha`, `macchiato`, `frappe`, `latte`
- [x] Aliases: `dark` → mocha, `light` → latte
- [x] Document Catppuccin → `ThemeColors` mapping
- [x] CLI / help updated
- [x] `docs/notebooks.md` updated
- [x] Tests per builtin (color-scheme + accent contrast ≥ 4.5:1)
- [x] CHANGELOG entry

### F5 — HTML CSS custom properties
- [ ] Emit `--rl-*` tokens on `:root` from resolved theme
- [ ] Stylesheet uses `var(--rl-…)` with safe fallbacks
- [ ] Tests: HTML contains expected `--rl-` declarations
- [ ] Visual output unchanged vs pre-token HTML

### F3 — Theme files
- [ ] TOML schema: `name`, `inherits`, `[palette]`, `[roles]`
- [ ] Resolver → `ThemeColors`
- [ ] `--theme-file PATH`
- [ ] Validation errors (hex, unknown role/inherit, missing file)
- [ ] WCAG warn (error under `--strict` / check) for text/accents vs bg
- [ ] Example theme file in `examples/` or `docs/themes/`
- [ ] Docs + tests (inherit, full custom, failure cases)
- [x] CHANGELOG entry

### F4 — Theme discovery directories
- [ ] Search path: `./.rustlab/themes/`, XDG/`~/.config/rustlab/themes/`, builtins
- [ ] `-t NAME` loads discovered `NAME.toml`
- [ ] Name-collision policy locked + tested (builtins win for reserved names)
- [ ] Docs for search order
- [x] CHANGELOG entry

### F6 — Frontmatter `theme:`
- [ ] Parse `theme:` from notebook YAML
- [ ] Resolve like CLI (builtin / discovered name)
- [ ] Precedence: CLI overrides frontmatter
- [ ] Omit / disable falls through
- [ ] Tests: frontmatter alone; CLI wins
- [ ] Docs + CHANGELOG

### F8 — Project / user default theme config
- [ ] Project config key (e.g. `rustlab.toml` / `.rustlab/config.toml`)
- [ ] User config default theme
- [ ] Precedence locked + tested (CLI > frontmatter > project > user > env > default)
- [ ] Docs + CHANGELOG

### F7 — Watch-server theme picker
- [ ] UI lists builtins (+ discovered themes if F4 present)
- [ ] Theme change re-renders current notebook
- [ ] Ephemeral by default; optional frontmatter write with `--editable`
- [ ] Tests / smoke
- [ ] Docs + CHANGELOG

## Problem

`rustlab-notebook` today only exposes `--theme dark|light` (Catppuccin Mocha / Latte), hardcoded in `rustlab-plot::theme::{Theme, ThemeColors}`. Authors cannot pick another flavor, load a course/brand palette, or set a notebook-local theme. LaTeX also detects “dark” via pointer equality against `Theme::Dark.colors()`, which blocks custom dark schemes.

## Goals

- Named builtin schemes beyond the dark/light aliases.
- Author-facing config that follows industry practice: **palette + semantic roles**, optional **inherits**, partial overrides.
- Clear precedence across CLI / frontmatter / config / env.
- Each feature below is **separately shippable** (own PR). Dependencies are listed so work can be ordered without bundling.

## Non-goals

- Theming committed gallery Markdown on GitHub.
- REPL / terminal color schemes (can share names later).
- Full brand.yml / typography / logos (colors only for now).
- Runtime CSS editing without re-render for SVG/PDF plots.

## Design principles (from Quarto brand.yml, Catppuccin, MkDocs, JupyterLab, VS Code)

1. **Palette ≠ roles** — named swatches, then map roles (`bg`, `text`, `accent_primary`, …).
2. **Scheme identity vs mode** — `mocha` is a scheme; light/dark is derived from background luminance (or an explicit `mode` field).
3. **Inherits + partial override** — don’t require dumping all ~25 `ThemeColors` fields.
4. **Validate early** — hex `#RRGGBB`, unknown keys error, WCAG contrast for text/accents vs `bg`.
5. **HTML runtime tokens** — emit CSS variables (`--rl-*`) from the resolved palette so chrome can share one contract.
6. **Precedence:** CLI (`--theme` / `--theme-file`) > notebook frontmatter > project/user config > env `RUSTLAB_NOTEBOOK_THEME` > default `dark` (= mocha).

## Internal model (shared by all features)

Keep `ThemeColors` in `rustlab-plot` as the **resolved** palette consumed by renderers.

Author-facing (features F2+):

```toml
name = "course-dark"
inherits = "mocha"          # optional builtin or previously loaded name

[palette]
brand = "#ffb454"           # optional extra named swatches

[roles]
# values are palette names or "#RRGGBB"
# unset roles inherit from `inherits` (or builtin defaults)
accent_primary = "brand"
```

Public role names should match today’s `ThemeColors` fields (`bg`, `text`, `accent_primary`, …). Document a **minimum brand set**: `bg`, `text`, `accent_primary`, `accent_secondary`, `code_bg`, `error_text`; the rest inherit.

Builtin aliases: `dark` → `mocha`, `light` → `latte`.

---

## Features (each independently shippable)

> Implement one feature per PR unless Michael asks to combine.  
> **Status** values: `not started` | `in progress` | `complete`.

### F1 — Named Catppuccin builtins + string CLI

**Status:** complete  
**Depends on:** nothing  
**Ships alone as:** yes

- Replace / extend `Theme::{Dark,Light}` with a **scheme registry**: name → `&'static ThemeColors`.
- Builtins: `mocha`, `macchiato`, `frappe`, `latte` (official Catppuccin flavors).
- Keep CLI `--theme` / `-t`; accept the four names plus aliases `dark`/`light`.
- Map Catppuccin tokens → existing `ThemeColors` fields (document the mapping in this plan’s appendix or in `docs/notebooks.md`).
- Update help text, `docs/notebooks.md`, tests for each builtin (`color-scheme` + accent contrast ≥ 4.5:1 vs `bg`).
- **Do not** require theme files, frontmatter, or CSS variables to land.

**Done when:** `rustlab-notebook render note.md -t macchiato` (and `frappe` / `latte` / `mocha` / aliases) produces correctly themed HTML/PDF/LaTeX; old `-t dark|light` still works.

---

### F2 — Luminance-based dark/light detection

**Status:** complete  
**Depends on:** nothing (can land before or after F1; **required before** custom dark schemes in F3)  
**Ships alone as:** yes

- Replace LaTeX `theme as *const _ == Theme::Dark.colors()` with the same luminance helper HTML already uses (`css_color_scheme` / `relative_luminance`).
- Audit for other pointer-equality or `matches!(Theme::Dark)` assumptions in notebook + plot paths; fix or gate behind the helper.
- Add unit tests: mocha/macchiato/frappe → dark; latte → light; synthetic custom bg `#111111` → dark.

**Done when:** any `ThemeColors` with a dark background gets dark LaTeX pagecolor behavior without being the Mocha static.

---

### F3 — Theme files (`inherits` + palette + roles)

**Status:** not started  
**Depends on:** F1 (registry to inherit from), F2 (custom dark LaTeX)  
**Ships alone as:** yes (after F1+F2)

- TOML (preferred; already used elsewhere in rustlab) theme files.
- Support `inherits`, `[palette]`, `[roles]`; resolve to owned or leaked `ThemeColors`.
- CLI `--theme-file PATH` (absolute/relative).
- Hard errors: invalid hex, unknown role, unknown inherit target, missing file.
- Optional WCAG warnings (or errors under `--strict`) for `text` / `accent_primary` / `accent_secondary` vs `bg`.
- Example file under `examples/` or `docs/themes/`.
- Docs + tests (inherit mocha + override one accent; full custom; failure cases).

**Done when:** `rustlab-notebook render note.md --theme-file course.toml` matches fixtures; invalid files fail loudly.

---

### F4 — Theme discovery directories

**Status:** not started  
**Depends on:** F3  
**Ships alone as:** yes

- `--theme NAME` also loads `NAME.toml` from search path:
  1. `./.rustlab/themes/`
  2. `$XDG_CONFIG_HOME/rustlab/themes/` (or `~/.config/rustlab/themes/`)
  3. builtins (F1)
- Document search order; no network.
- Conflict: builtin name wins unless `--theme-file` is used (or document “user themes shadow builtins” — pick one and test it; **recommendation: builtins win for `mocha|…|dark|light`, user names otherwise**).

**Done when:** placing `course.toml` in config themes and running `-t course` works without `--theme-file`.

---

### F5 — HTML CSS custom properties

**Status:** not started  
**Depends on:** nothing strong; nicest after F1  
**Ships alone as:** yes

- When emitting HTML, define `:root { --rl-bg: …; --rl-text: …; … }` from resolved `ThemeColors` and prefer `var(--rl-*)` in the page stylesheet where practical.
- Keeps today’s colors working if vars missing (fallback literals OK during migration).
- Enables F7 watch picker / future chrome without string-rewriting the whole CSS blob.
- Tests: rendered HTML contains expected `--rl-` declarations for the selected theme.

**Done when:** one builtin theme’s HTML exposes the token set; visual output unchanged.

---

### F6 — Frontmatter `theme:`

**Status:** not started  
**Depends on:** F1 (names); F3/F4 if values may be file-based names  
**Ships alone as:** yes (names-only first is fine)

- Notebook YAML frontmatter: `theme: macchiato` or `theme: course` (resolved like CLI).
- Precedence: CLI overrides frontmatter (document clearly).
- `theme: false` or omit → fall through to config/env/default.
- Tests: frontmatter alone; CLI overrides frontmatter.

**Done when:** rendering without `-t` picks frontmatter theme; `-t` wins when both set.

---

### F7 — Watch-server theme picker (optional UX)

**Status:** not started  
**Depends on:** F1, F5 (tokens make live switch saner); F6 optional for persistence  
**Ships alone as:** yes

- UI control listing builtins (+ discovered themes if F4 landed).
- Changing theme triggers re-render of the current notebook (SVG/PDF sidecars as applicable).
- Ephemeral per server process by default; optional “write `theme:` into frontmatter” only with `--editable`.
- Do not block F1–F6 on this.

**Done when:** switching theme in `watch` updates the page without restarting the server.

---

### F8 — Project / user default theme config

**Status:** not started  
**Depends on:** F1; F3/F4 if defaults point at files  
**Ships alone as:** yes

- Optional `theme = "mocha"` in a small project config (e.g. `rustlab.toml` or `.rustlab/config.toml`) and/or user config.
- Fits precedence chain above (below frontmatter, above env — **or** document env above config if that matches other rustlab tooling; lock one order in implementation and tests).
- **Recommendation:** CLI > frontmatter > project config > user config > env > default.

**Done when:** project config changes default render theme without CLI flags.

---

## Suggested implementation order

Safe independent sequence (each PR green on its own):

1. **F2** (luminance) — unblocks custom dark; tiny.  
2. **F1** (builtins) — user-visible win.  
3. **F5** (CSS vars) — HTML hygiene.  
4. **F3** then **F4** — author themes.  
5. **F6** then **F8** — defaults / notebook override.  
6. **F7** — polish.

F1 and F2 can also be one PR if desired; still keep commits separable.

## Testing requirements (every feature)

- Unit tests for resolution / validation belonging to that feature.
- At least one HTML render smoke per new builtin or file path.
- No gallery commit churn unless explicitly regenerating for a documented reason.
- Follow AGENTS.md: feature branch + PR; version bump only if observable behavior changes (new CLI values count — document in CHANGELOG).

## Open decisions (resolve when implementing F1/F3)

1. Exact Catppuccin → `ThemeColors` field mapping table (freeze in docs when F1 lands).  
2. User theme vs builtin name collision policy (recommendation above).  
3. WCAG: warn vs hard-error (recommend warn by default, error with `--strict` / `check`).

## References

- Quarto brand.yml — palette + semantic colors, light/dark variants, precedence: https://quarto.org/docs/authoring/brand.html  
- Catppuccin palette flavors — https://github.com/catppuccin/palette  
- MkDocs Material — scheme + primary/accent; CSS variables for custom  
- JupyterLab — named theme + CSS variable overrides  
- Existing code — `crates/rustlab-plot/src/theme.rs`, `crates/rustlab-notebook/src/render.rs` (`css_color_scheme`), `render_latex.rs` (pointer dark check)
