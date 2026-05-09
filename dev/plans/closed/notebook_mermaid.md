# Plan: Mermaid diagrams in notebooks (pure Rust)

**Status:** Complete — Phase 1 shipped (parser, executor passthrough,
unified SVG renderer with hashed cache, directives). Behind the
`mermaid` Cargo feature on `rustlab-notebook`. Demo:
`examples/notebooks/mermaid_demo.md`.

Implementation contract: see `dev/requests/notebook-mermaid-diagrams.md`.
This plan covers phasing, file-by-file changes, tests, and integration
points. Decisions already made in the request are not re-litigated.

## Pure-Rust correction (supersedes parts of the request)

The request describes a hybrid path: CDN-hosted `mermaid.min.js` for
HTML, `mermaid-rs-renderer` for LaTeX/PDF. **This plan unifies both on
the Rust crate** — render SVG server-side once, embed inline in HTML and
reference from LaTeX. Consequences:

- No CDN dependency for Mermaid in HTML output. Notebooks render
  offline immediately.
- Single renderer means HTML and PDF are pixel-identical (same engine,
  same fonts, same layout). The "HTML / PDF parity" caveat in the
  request becomes moot for Mermaid.
- HTML diagrams are static SVG — **not** interactive. Mermaid.js would
  have offered click/zoom/pan. Acceptable for the documentary use cases
  the request lists; revisit if interactivity becomes a hard requirement.
- The `mermaid-pdf` Cargo feature now gates HTML rendering too. With
  the feature off, Mermaid blocks emit verbatim source in both HTML
  and LaTeX. Rename to `mermaid` (drop `-pdf` suffix) since it no
  longer means "PDF only."
- Crate maturity risk now affects all output formats, not just PDF.
  If a diagram type isn't supported, both HTML and PDF fall back to
  verbatim. Phase 1 changes accordingly: it depends on the crate from
  day one (no JS-fallback safety net).

The rest of this plan reflects the pure-Rust path.

---

## Phasing

Two phases (down from three — HTML and PDF are now the same pipeline,
so no point in splitting).

### Phase 1 — Parser, executor, and unified SVG renderer (HTML + LaTeX/PDF)

Goal: end-to-end Mermaid support across all output formats from one
renderer. Caching from day one so re-renders are cheap.

Tasks (in order):

1. **`Cargo.toml`** — feature + optional deps.
   ```toml
   [features]
   default = ["mermaid"]
   mermaid = ["dep:mermaid-rs-renderer", "dep:blake3"]

   [dependencies]
   mermaid-rs-renderer = { version = "0.2", optional = true }
   blake3              = { version = "1",   optional = true }
   ```
   Hashing decision: **BLAKE3** over SHA-256. ~5× faster on small inputs
   (cache lookups stay off the critical path), pure Rust, ~70 KB
   compiled. Either works.

2. **`src/parse.rs`** — recognize ` ```mermaid ` info-string and the
   `<!-- caption: ... -->` directive.
   - Mirror the existing rustlab branch around `parse.rs:79`
     (`trimmed == "```rustlab" || trimmed.starts_with("```rustlab ")`).
     Add a parallel branch for `"```mermaid"` / `"```mermaid "`.
   - Capture body lines verbatim into `mermaid_buf: String` exactly as
     the rustlab branch does (lines 73–77).
   - On closing fence push
     `Block::Mermaid { source: mermaid_buf, directives: MermaidDirectives::default() }`.
   - Add `Block::Mermaid` to the `Block` enum (line 22).
   - Reuse `extract_code_directives` so `<!-- hide -->` and
     `<!-- details: -->` apply to Mermaid the same way they apply to
     `Code`. Add `MermaidDirectives { hidden, details, caption }`.
   - Add `<!-- caption: ... -->` parsing alongside `<!-- details: -->`
     in the directive walker. Populate both `MermaidDirectives.caption`
     and `CodeDirectives.caption` (forward-compat for code-block
     captions later — Code rendering ignores it for now, no behavior
     change for existing notebooks).

3. **`src/execute.rs`** — pass through, no execution.
   - Add
     ```rust
     Rendered::Mermaid {
         source: String,
         hidden: bool,
         details: Option<String>,
         caption: Option<String>,
     }
     ```
     to the `Rendered` enum (line 10).
   - In `execute_notebook`, add a `Block::Mermaid { source, directives }`
     arm that produces `Rendered::Mermaid` directly. No evaluator
     interaction, no `set_plot_context` work.

4. **`src/mermaid.rs`** — new module, `#[cfg(feature = "mermaid")]`.
   ```rust
   pub fn render_to_svg_cached(
       source: &str,
       plot_dir: &Path,           // plots/<notebook>/
       diagram_idx: usize,
       theme: &ThemeColors,
   ) -> Result<PathBuf, MermaidRenderError>;

   pub fn render_to_svg_string_cached(
       source: &str,
       plot_dir: &Path,           // for cache lookup; SVG returned, not copied
       theme: &ThemeColors,
   ) -> Result<String, MermaidRenderError>;
   ```
   - File-returning variant powers LaTeX (needs a `.svg` on disk).
   - String-returning variant powers HTML (inline `<svg>…</svg>` in the
     page; no CDN, no extra HTTP request).
   - Both share the same cache: `plot_dir/.cache/<blake3-hex>.svg`.
     The string variant reads the cache file when there's a hit.
   - Renderer call wrapped in `std::panic::catch_unwind` defensively
     (mermaid-rs-renderer is 0.2.x). Errors and panics both convert to
     `MermaidRenderError` and become verbatim fallback at the call site
     — never propagate to the render pipeline.
   - Theme mapping spike: characterize `mermaid_rs_renderer::Theme` API
     during implementation (docs.rs shows `Theme::modern()`; need to
     check for dark/light knobs). If no theme knob exists, ship with
     one hardcoded modern theme matching the rustlab dark default and
     file an upstream issue. Linked from this plan.

5. **`src/render.rs`** — emit inline SVG.
   - Pattern after the `Rendered::Code` arm at line 76. For each
     `Rendered::Mermaid`:
     ```rust
     #[cfg(feature = "mermaid")]
     match crate::mermaid::render_to_svg_string_cached(source, plot_dir, theme) {
         Ok(svg) => emit_inline_svg(&mut body, &svg, hidden, details.as_deref()),
         Err(e)  => { warn_once(format!("mermaid block #{idx}: {e}")); emit_verbatim(&mut body, source); }
     }
     #[cfg(not(feature = "mermaid"))]
     { warn_feature_disabled_once(); emit_verbatim(&mut body, source); }
     ```
   - `emit_inline_svg` wraps the SVG in `<figure class="mermaid">` (or
     a `<div>` — `<figure>` is more semantic and lets us add a
     `<figcaption>` from the caption directive). Apply `<details>`
     wrapping when `details` is set. Apply `display:none` when
     `hidden` (or omit entirely — Mermaid sources have no separate
     "source listing" the way `Code` blocks do, so `hidden` becomes
     "don't render this diagram in HTML at all" and the result is
     equivalent to commenting the block out; document that semantics).
   - **No CDN script tag.** No `<script type="module">` injection. No
     `mermaid.initialize`. Remove all browser-side Mermaid concerns.
   - Sanitize the inline SVG: `mermaid-rs-renderer` should produce
     well-formed SVG, but verify in tests that no `<script>` or
     `onclick=` attributes leak through (defense in depth; trust but
     test).

6. **`src/render_markdown.rs`** — emit ` ```mermaid ` fence verbatim.
   Obsidian, GitHub, and other Mermaid-aware viewers render it
   themselves. When `hidden`, omit. When `details`, wrap in
   `<details><summary>…</summary>…</details>` (already used for code
   blocks; mirror the same approach).

7. **`src/render_latex.rs`** — call the file-returning renderer, emit
   a float.
   ```rust
   #[cfg(feature = "mermaid")]
   match crate::mermaid::render_to_svg_cached(source, plot_dir, idx, theme) {
       Ok(_) => emit_includesvg_figure(&mut body, &href_prefix, idx, caption.as_deref()),
       Err(e) => { warn_once(format!("mermaid block #{idx}: {e}")); emit_verbatim(&mut body, source); }
   }
   #[cfg(not(feature = "mermaid"))]
   { warn_feature_disabled_once(); emit_verbatim(&mut body, source); }
   ```
   Exact LaTeX:
   ```latex
   \begin{figure}[htbp]
     \centering
     \includesvg[width=0.8\linewidth]{<href_prefix>/diagram-<N>}
     \caption{<escape_latex(caption)>}    % only when caption.is_some()
   \end{figure}
   ```
   `\usepackage{svg}` and `\usepackage{graphicx}` are already in the
   preamble (lines 155–156). No preamble change.

8. **One-time warning plumbing.** Pass a small `warn_state` struct
   through each renderer (or use a `&Cell<bool>` per-warning-kind).
   Local to one render call so directory-mode renders don't share state
   across notebooks. Two distinct warnings:
   - "feature disabled" — once per render
   - "block N failed: <reason>" — one per failing block

Tests gating Phase 1 done:

Parser:
- `parse::tests::mermaid_block_basic` — `flowchart LR; A-->B` parses to `Block::Mermaid` with source intact.
- `parse::tests::mermaid_with_hide` — `<!-- hide -->` sets `directives.hidden`.
- `parse::tests::mermaid_with_details` — `<!-- details: Architecture -->` sets `directives.details`.
- `parse::tests::mermaid_with_caption` — `<!-- caption: My Diagram -->` sets `directives.caption`.
- `parse::tests::caption_then_rustlab` — caption flows into `CodeDirectives.caption` (forward-compat).
- `parse::tests::mermaid_after_rustlab` — sequential `rustlab` then `mermaid`, both parsed cleanly.
- `parse::tests::mermaid_special_chars` — source with `&`, `<`, `>`, `-->` round-trips byte-exact.

Executor:
- `execute::tests::notebook_mermaid_passthrough` — `Block::Mermaid` produces `Rendered::Mermaid`, no evaluator interaction.

Mermaid module (gated `#[cfg(all(test, feature = "mermaid"))]`):
- `mermaid::tests::renders_simple_flowchart` — basic flowchart returns SVG file with non-zero size.
- `mermaid::tests::cache_hit_skips_rerender` — render twice with same source; second call reads cached file (verify via a counter or mtime).
- `mermaid::tests::cache_miss_on_source_change` — change one byte; cache miss.
- `mermaid::tests::renderer_error_returns_err_no_panic` — malformed source returns `Err`, no panic.
- `mermaid::tests::svg_output_has_no_script_tags` — sanitize check.

HTML render:
- `render::tests::render_html_mermaid_inline_svg` — output contains inline `<svg` from the renderer (gated on the feature).
- `render::tests::render_html_mermaid_no_cdn_script` — output does NOT contain `cdn.jsdelivr.net` or `mermaid.initialize` (regression guard against re-introducing CDN dep).
- `render::tests::render_html_mermaid_details_wrap` — `<details>` wraps the figure when directive set.
- `render::tests::render_html_mermaid_hidden_omits` — hidden Mermaid produces no `<svg>` in output.
- `render::tests::render_html_mermaid_feature_disabled` — `#[cfg(not(feature = "mermaid"))]` test confirming verbatim fallback.
- `render::tests::render_html_multiple_mermaid_blocks` — two blocks both render (independent cache keys, both SVGs inlined).

Markdown render:
- `render_markdown::tests::mermaid_md_passthrough` — emits ` ```mermaid ` fence in the markdown output.

LaTeX render:
- `render_latex::tests::mermaid_emits_figure` — output contains `\begin{figure}[htbp]`, `\includesvg[width=0.8\linewidth]`, `\end{figure}`.
- `render_latex::tests::mermaid_caption_present` — `\caption{…}` in output when caption set.
- `render_latex::tests::mermaid_no_caption_omits_command` — no caption directive → no `\caption{}`.
- `render_latex::tests::mermaid_render_error_falls_back_to_verbatim` — synthesize a Mermaid block the renderer rejects; LaTeX contains `\begin{verbatim}`.
- `render_latex::tests::mermaid_feature_disabled_emits_verbatim` — gated `#[cfg(not(feature = "mermaid"))]`.

Integration:
- `lib::tests::dir_render_mermaid_per_notebook` — two-notebook directory render, each gets its own `plots/<stem>/diagram-1.svg` and its own `.cache/`.

Estimated diff: ~700–900 lines (most of `mermaid.rs`, error type, caching, ~20 tests).

### Phase 2 — Theme mapping, polish, docs/AGENTS update

1. **Theme mapping (`src/mermaid.rs`).** Finalize the spike from Phase 1.
   Map `theme.is_dark()` → upstream dark equivalent if available, else
   manually-overridden background colors on `Theme::modern()`.
2. **`docs/notebooks.md`** — add a "Mermaid diagrams" section after the
   directives section (line ~218). Include: basic syntax, theme note,
   caption directive example, feature-flag note (how to disable),
   note that HTML renders are static SVG (no interactivity).
3. **`AGENTS.md`** — update the "Notebook System" row in the table at
   line 156; add a brief paragraph in the `rustlab-notebook` section
   (around line 753) listing Mermaid as a supported block type
   alongside Markdown / Code. No `HelpEntry` work — Mermaid is not a
   REPL builtin.
4. **`examples/notebooks/mermaid_demo.md`** — small demo with one
   flowchart, one sequence diagram, one with caption + `<!-- details: -->`.
5. **CHANGELOG / release note** if the repo keeps one (check during
   implementation).
6. **Bench script (optional, recommended).** Time a 10-block Mermaid
   notebook cold (no cache) and warm (full cache hit). Use to validate
   the crate's "100–600× faster than mmdc" claim and confirm caching
   pays off.

Tests:
- Manual: render `mermaid_demo.md` to HTML, LaTeX, PDF, Markdown — verify all four.

Estimated diff: ~150 lines code + docs.

---

## Block / Rendered shapes (final)

```rust
// parse.rs
#[derive(Debug, Clone, Default)]
pub struct MermaidDirectives {
    pub hidden: bool,
    pub details: Option<String>,
    pub caption: Option<String>,
}

pub enum Block {
    // existing variants...
    Mermaid { source: String, directives: MermaidDirectives },
}

// execute.rs
pub enum Rendered {
    // existing variants...
    Mermaid {
        source: String,
        hidden: bool,
        details: Option<String>,
        caption: Option<String>,
    },
}
```

`CodeDirectives` gains a `pub caption: Option<String>` field too, so
the directive walker is fully generic. Code-block rendering ignores it
in v1.

## Caching design

- Hash: BLAKE3 of `source.as_bytes()`, hex-encoded.
- Layout: `plots/<notebook>/.cache/<hash>.svg`. Hidden (dotfile) so
  Obsidian's plot pickers and `make clean` rules ignore it by default.
- HTML hit: read cache file as `String`, embed inline.
- LaTeX hit: copy cache file → `plots/<notebook>/diagram-<N>.svg`. Don't
  symlink (Windows + Obsidian compat).
- Miss: render → atomic write to `<hash>.svg.tmp` → rename to
  `<hash>.svg` → copy/read as needed.
- Corruption: empty/unreadable cache file → treat as miss, re-render.
  Never propagate cache errors as failures.
- Invalidation: implicit via hash mismatch. No explicit cache-clear
  command in v1 — users delete `.cache/` manually if needed.
- Multi-notebook: each notebook has its own `.cache/` because `plot_dir`
  is per-notebook (`lib.rs:364`). Cross-notebook sharing would need a
  workspace-level cache; not in scope.

## LaTeX float wrapping — exact emission

```latex
\begin{figure}[htbp]
  \centering
  \includesvg[width=0.8\linewidth]{<href_prefix>/diagram-<N>}
  \caption{<escape_latex(caption)>}    % only when caption.is_some()
\end{figure}
```

Width fixed at `0.8\linewidth` in v1. No per-block override.

## Multi-notebook directory render

`lib.rs::plot_layout_for` (lines 357–366) returns
`(parent/plots/<stem>/, "plots/<stem>")`. Same path passed to all
renderers (`render_html`, `render_markdown`, `render_latex` at lines
294, 300, 322). Mermaid SVGs and `.cache/` land under `plot_dir`
automatically — no extra plumbing for directory mode.

## Feature flag policy

- Name: `mermaid` (no `-pdf` suffix; gates all formats now).
- Default state: **on** (per request).
- Gated:
  - `mermaid-rs-renderer` and `blake3` deps.
  - `src/mermaid.rs` module.
  - The renderer-call arms in `render.rs` and `render_latex.rs`.
- With feature off, all formats emit verbatim source plus a one-time
  stderr warning:
  `"warning: rustlab-notebook built without 'mermaid' feature. Mermaid blocks rendered as verbatim source. Re-build with --features mermaid for diagram rendering."`

## Risks and mitigations

1. **Crate maturity (load-bearing, broader scope now).**
   `mermaid-rs-renderer` 0.2.2, single maintainer, ~3% documented. Now
   affects HTML and PDF, not just PDF. Mitigation: defensive
   `catch_unwind`, per-block verbatim fallback, one-time warning. The
   notebook never panics on Mermaid input. If the crate proves
   unsuitable during Phase 1 implementation, the escape hatch is
   "ship with feature off by default and document Mermaid as
   experimental" — no rewrite required, just a default flip.
2. **Speed claim unverified.** Mitigation: caching from day one. Phase
   2 includes a bench script to validate.
3. **Theme API may not exist.** Mitigation: Phase 1 starts with a
   30-min spike on the crate's `Theme` struct. If no theme knob, ship
   with one hardcoded theme; file upstream issue.
4. **No interactivity in HTML.** Static SVGs lose Mermaid.js's
   click/zoom. Mitigation: documented in Phase 2 user docs. Revisit if
   interactivity becomes a hard requirement (could be done as a future
   `--mermaid-interactive` flag that switches HTML back to the CDN
   path; not in v1).

## Files touched

| File | Phase | Change |
|---|---|---|
| `crates/rustlab-notebook/Cargo.toml` | 1 | `mermaid` feature, optional `mermaid-rs-renderer` + `blake3` deps. |
| `crates/rustlab-notebook/src/parse.rs` | 1 | `Block::Mermaid`, `MermaidDirectives`, mermaid fence handler, `<!-- caption: -->` directive. |
| `crates/rustlab-notebook/src/execute.rs` | 1 | `Rendered::Mermaid`, passthrough arm. |
| `crates/rustlab-notebook/src/mermaid.rs` | 1 | New module: `render_to_svg_cached`, `render_to_svg_string_cached`, BLAKE3 hashing, error type, theme mapping. Behind `#[cfg(feature = "mermaid")]`. |
| `crates/rustlab-notebook/src/render.rs` | 1 | `Rendered::Mermaid` arm; inline SVG emission. **No CDN injection.** |
| `crates/rustlab-notebook/src/render_markdown.rs` | 1 | `Rendered::Mermaid` arm — ` ```mermaid ` fence. |
| `crates/rustlab-notebook/src/render_latex.rs` | 1 | `Rendered::Mermaid` arm — `\includesvg` figure with optional `\caption`. |
| `docs/notebooks.md` | 2 | Mermaid section. |
| `AGENTS.md` | 2 | Notebook section update. |
| `examples/notebooks/mermaid_demo.md` | 2 | New demo. |

No REPL `HelpEntry` change — Mermaid is not a REPL builtin. Workflow
Rule 3 (HelpEntry) does not apply. Workflow Rule 7 (update AGENTS.md)
does — handled in Phase 2.

## Sequencing & estimated diff size

| Phase | LoC est. | Tests | Dependencies added |
|---|---|---|---|
| 1 | 700–900 | ~20 | `mermaid-rs-renderer`, `blake3` (both behind `mermaid` feature) |
| 2 | 150 + docs | manual | none |

Total: ~850–1050 LoC plus ~20 tests across two phases.

## Updates the request file needs

`dev/requests/notebook-mermaid-diagrams.md` reflects the original hybrid
direction. Update before or alongside Phase 1:

- Drop "HTML uses CDN `mermaid.min.js`" — replace with "HTML embeds
  inline SVG produced by the same crate."
- Drop the "HTML / PDF rendering parity" caveat — single renderer
  means parity is automatic.
- Drop the "CDN dependency / offline rendering" open question for
  Mermaid (still applies to KaTeX/Plotly; track separately).
- Rename feature flag throughout: `mermaid-pdf` → `mermaid`.
- Add note: HTML diagrams are static SVG, not interactive. Future
  optional `--mermaid-interactive` flag could re-introduce the CDN
  path if needed.
